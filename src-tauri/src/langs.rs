//! 通用的语言服务器（docs/LANGPACK.md，docs/TAKEOVER.md R4）：Java 以外的语言，由「语言包」（一份 JSON）说怎么起、认哪些文件。
//!
//! - **包**：内置的编进应用（`src-tauri/packs/*.json`），你自己的放应用数据目录 `packs/*.json`，同 id 的盖过内置的。带 `schema` 版本号，认不得的版本不认、说一句
//! - **找服务器**：包里写的是命令名（`rust-analyzer`）。从访达起的应用没有你 shell 里的 PATH，所以起一次交互式登录 shell、按标记取 `command -v` 和 `$PATH`
//!   （同 `acpsvc::find_npx`：rc 文件自己往 stdout 打的字不算）；服务器本身**直接起**、不经过 shell —— 它的 stdout 是说话的管道，shell 打一个字就把协议搅乱了。
//!   包里写绝对路径的照用（测试构建的 shell 不读你的 rc，验收用它）
//! - **装语言服务器**（Python / TS 那两个是 Node 程序）：包里写 `"npm": "<套>"`，那一套的 `package.json` + `package-lock.json` 编进应用（`packs/npm/<套>/`，
//!   每个包钉版本、带 sha512）。点「安装」时 `npm ci --ignore-scripts` 装进应用数据目录 `lsp/<套>/` —— npm 照锁文件核每个包的 sha512、对不上就失败；
//!   `--ignore-scripts`：哪个包的安装脚本都不跑。先装进临时目录、成了再改名过去。找服务器时先找这里装的，再找你自己全局装的
//! - **一个服务器管一个根**：从文件往上找包里 `roots` 列的文件（`Cargo.toml`），找到的第一层就是根，到项目根还没有就用项目根。按 `窗口 × 根 × 包` 一个，`Owned` 登记，关窗口收；
//!   没有开着的文档、闲了 [`IDLE`] 就收掉（同 jdtls）
//! - 请求和解析照 Java 那边（`java::locations` / `completion_items` / `hover_markdown` / `workspace_edit` 都是纯 LSP 的），DTO 也用同一套，前端画法共用；
//!   改名、改到别的文件的快速修复照跨文件替换那套落盘（`java::apply_rename`：撤销日志、开着的标签跟上）

use crate::owned::Owned;
use lspsvc::server::{Server, Spec};
use lspsvc::uri;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

pub const IDLE: Duration = Duration::from_secs(10 * 60);
const ASK: Duration = Duration::from_secs(20);
const INIT: Duration = Duration::from_secs(60);

const BUILTIN: &[&str] = &[include_str!("../packs/rust.json"), include_str!("../packs/python.json"), include_str!("../packs/typescript.json")];

/// 能一键装的那几套：（套名, package.json, package-lock.json, 装完大概多大）。版本、sha512 在锁文件里；挑版本的理由在 docs/LANGPACK.md 第 6 节
const NPM: &[(&str, &str, &str, &str)] = &[
    ("typescript", include_str!("../packs/npm/typescript/package.json"), include_str!("../packs/npm/typescript/package-lock.json"), "约 26 MB"),
    ("python", include_str!("../packs/npm/python/package.json"), include_str!("../packs/npm/python/package-lock.json"), "约 42 MB"),
];

/// 一键装的那一套：（package.json, 锁文件, 大小）
pub fn npm_set(id: &str) -> Option<(&'static str, &'static str, &'static str)> {
    NPM.iter().find(|x| x.0 == id).map(|x| (x.1, x.2, x.3))
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Pack {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub files: Vec<String>,
    pub language_id: String,
    /// 扩展名 → 语言标识（TS 包里 `.js` 是 javascript、`.tsx` 是 typescriptreact）；没写的用 `language_id`
    #[serde(default)]
    pub language_ids: HashMap<String, String>,
    #[serde(default)]
    pub roots: Vec<String>,
    /// 能一键装：`NPM` 里那一套的名字（只认内置的那几套：锁文件编在应用里，你自己的包写了也不认）
    #[serde(default)]
    pub npm: Option<String>,
    pub server: PackServer,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PackServer {
    pub command: Vec<String>,
    #[serde(default)]
    pub initialization_options: Value,
    /// 没装时给人看的一句「怎么装」
    #[serde(default)]
    pub install: String,
}

impl Pack {
    fn lang_of(&self, path: &str) -> String {
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        self.language_ids.get(ext).cloned().unwrap_or_else(|| self.language_id.clone())
    }
}

/// `*.rs` 这种（只认 `*.扩展名` 和完整的文件名，够包用；不是完整的 glob）
fn name_matches(pat: &str, name: &str) -> bool {
    match pat.strip_prefix('*') {
        Some(suffix) => name.len() > suffix.len() && name.ends_with(suffix),
        None => pat == name,
    }
}

/// 解析一份包；不认的版本、缺字段的回错误（一句人话）
pub fn parse_pack(text: &str) -> Result<Pack, String> {
    let p: Pack = serde_json::from_str(text).map_err(|e| format!("格式无效：{e}"))?;
    if p.schema != 1 {
        return Err(format!("schema 版本 {} 不受支持（当前支持 1）", p.schema));
    }
    if p.id.is_empty() || p.files.is_empty() || p.server.command.is_empty() {
        return Err("缺少 id、files 或 server.command".into());
    }
    Ok(p)
}

/// 把你自己的包合进去：同 id 的换掉内置的（**换在原地**：`pack_for` 按次序找第一个认这个文件的，排在后面的盖不住前面的），新的加在后面
fn merge(out: &mut Vec<Pack>, p: Pack) {
    match out.iter_mut().find(|x| x.id == p.id) {
        Some(x) => *x = p,
        None => out.push(p),
    }
}

/// 内置的 + 你自己的（同 id 盖过内置的）。第二个是认不得的包文件，一条一句
pub fn packs(app: &AppHandle) -> (Vec<Pack>, Vec<String>) {
    let mut out: Vec<Pack> = BUILTIN.iter().filter_map(|t| parse_pack(t).ok()).collect();
    let mut problems = Vec::new();
    if let Ok(dir) = app.path().app_data_dir().map(|d| d.join("packs")) {
        let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|x| x == "json")).collect()).unwrap_or_default();
        files.sort();
        for f in files {
            match std::fs::read_to_string(&f).map_err(|e| e.to_string()).and_then(|t| parse_pack(&t)) {
                Ok(p) => merge(&mut out, p),
                Err(e) => problems.push(format!("{}：{e}", f.display())),
            }
        }
    }
    (out, problems)
}

pub fn pack_for<'a>(packs: &'a [Pack], path: &str) -> Option<&'a Pack> {
    let name = Path::new(path).file_name()?.to_str()?;
    packs.iter().find(|p| p.files.iter().any(|pat| name_matches(pat, name)))
}

/// 这个文件归哪个根：从它所在的目录往上，第一层有包里 `roots` 列的文件的；到项目根还没有就是项目根
pub fn root_for(pack: &Pack, project_root: &Path, path: &Path) -> PathBuf {
    let mut dir = path.parent();
    while let Some(d) = dir {
        if pack.roots.iter().any(|r| d.join(r).exists()) {
            return d.to_path_buf();
        }
        if d == project_root || !d.starts_with(project_root) {
            break;
        }
        dir = d.parent();
    }
    project_root.to_path_buf()
}

/// 命令名 → 绝对路径和该带的 PATH。写了绝对路径的照用（PATH 只加它所在的目录）。
/// 测试构建可以直接给一个 PATH（`LITE_LOGIN_PATH`）：测试应用的 shell 不读你的 rc，node、rust-analyzer 它都找不到
fn locate(shell: &str, program: &str) -> Result<(PathBuf, String), String> {
    #[cfg(feature = "test-bridge")]
    if let Ok(path) = std::env::var("LITE_LOGIN_PATH") {
        if !program.starts_with('/') {
            return path.split(':').map(|d| Path::new(d).join(program)).find(|p| p.is_file()).map(|p| (p, path.clone())).ok_or_else(|| format!("未找到 {program}"));
        }
    }
    if program.starts_with('/') {
        let p = PathBuf::from(program);
        let dir = p.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
        return if p.is_file() { Ok((p, format!("{dir}:/usr/bin:/bin:/usr/sbin:/sbin"))) } else { Err(format!("{program} 不存在")) };
    }
    if !program.chars().all(|c| c.is_ascii_alphanumeric() || "-_.+".contains(c)) {
        return Err(format!("命令名「{program}」包含不允许的字符"));
    }
    let mut cmd = std::process::Command::new(shell);
    cmd.args(["-ilc", &format!("printf '\\n__LITE_BIN__%s\\n__LITE_PATH__%s\\n' \"$(command -v {program})\" \"$PATH\"")]);
    let out = procutil::run_capped(&mut cmd, 64 << 10, 16 << 10, procutil::Overflow::Drain).map_err(|e| format!("无法启动 {shell}：{e}"))?;
    parse_locate(&String::from_utf8_lossy(&out.stdout)).ok_or_else(|| format!("未找到 {program}"))
}

/// 按标记取最后那两行（rc 文件自己打的字不算）。没找到（`command -v` 空、或者找到的是 shell 函数不是文件）是 None
fn parse_locate(out: &str) -> Option<(PathBuf, String)> {
    let bin = out.lines().rev().find_map(|l| l.strip_prefix("__LITE_BIN__"))?.trim();
    let path = out.lines().rev().find_map(|l| l.strip_prefix("__LITE_PATH__")).unwrap_or("").trim();
    bin.starts_with('/').then(|| (PathBuf::from(bin), path.to_string()))
}

struct Inst {
    srv: Server,
    pack: Pack,
    /// 服务器报的诊断原样（快速修复要把和这几行重叠的原样带回去当上下文）
    raw_diags: Arc<Mutex<HashMap<String, Vec<Value>>>>,
    /// 打开的文档 → 版本号
    docs: Mutex<HashMap<String, i64>>,
    last_used: Mutex<Instant>,
}

impl Inst {
    fn touch(&self) {
        *self.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    fn open(&self, path: &str, text: &str) {
        self.touch();
        let mut d = self.docs.lock().unwrap_or_else(|e| e.into_inner());
        match d.get_mut(path) {
            Some(v) => {
                *v += 1;
                let _ = self.srv.notify("textDocument/didChange", json!({ "textDocument": { "uri": uri::from_path(path), "version": *v }, "contentChanges": [{ "text": text }] }));
            }
            None => {
                d.insert(path.to_string(), 1);
                let _ = self.srv.notify("textDocument/didOpen", json!({ "textDocument": { "uri": uri::from_path(path), "languageId": self.pack.lang_of(path), "version": 1, "text": text } }));
            }
        }
    }

    fn close(&self, path: &str) {
        self.touch();
        if self.docs.lock().unwrap_or_else(|e| e.into_inner()).remove(path).is_some() {
            let _ = self.srv.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri::from_path(path) } }));
        }
    }

    fn ask(&self, method: &str, params: Value) -> Result<Value, String> {
        self.touch();
        self.srv.request(method, params, ASK).map_err(|e| format!("{}：{e}", self.pack.name))
    }

    fn at(path: &str, line: u32, character: u32) -> Value {
        json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } })
    }

    fn idle(&self) -> bool {
        self.docs.lock().unwrap_or_else(|e| e.into_inner()).is_empty() && self.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() >= IDLE
    }
}

/// 服务器问我们的：`workspace/configuration` 每一项答 null（用它自己的默认），别的（登记能力、进度条）答 null
fn answer(method: &str, p: &Value) -> Result<Value, (i64, String)> {
    match method {
        "workspace/configuration" => Ok(Value::Array(p["items"].as_array().map(|a| a.iter().map(|_| Value::Null).collect()).unwrap_or_default())),
        _ => Ok(Value::Null),
    }
}

#[derive(Default)]
pub struct Langs {
    /// 键是 `窗口\n根\n包 id`
    all: Owned<String, Arc<Inst>>,
    /// 起不来的（没装、崩了）：同一个只说一次，键同上 → 那句话
    failed: Mutex<HashMap<String, String>>,
}

impl Langs {
    /// 这个文件该归的那个服务器（没起的起）。不是任何包认的文件是 Ok(None)
    fn inst(&self, app: &AppHandle, owner: &str, project_root: &str, path: &str) -> Result<Option<Arc<Inst>>, String> {
        let (packs, _) = packs(app);
        let Some(pack) = pack_for(&packs, path) else { return Ok(None) };
        let root = root_for(pack, Path::new(project_root), Path::new(path));
        let key = format!("{owner}\n{}\n{}", root.display(), pack.id);
        if let Some(i) = self.all.get(&key, Arc::clone) {
            if i.srv.exit().is_none() {
                return Ok(Some(i));
            }
            self.all.remove(&key);
        }
        if let Some(why) = self.failed.lock().unwrap_or_else(|e| e.into_inner()).get(&key) {
            return Err(why.clone());
        }
        match self.start(app, owner, pack, &root) {
            Ok(i) => {
                let i = Arc::new(i);
                self.all.insert(owner, key, Arc::clone(&i));
                Ok(Some(i))
            }
            Err(e) => {
                self.failed.lock().unwrap_or_else(|e| e.into_inner()).insert(key, e.clone());
                Err(e)
            }
        }
    }

    fn start(&self, app: &AppHandle, owner: &str, pack: &Pack, root: &Path) -> Result<Inst, String> {
        let shell = crate::settingsctl::terminal_shell(&app.state::<crate::state::AppState>());
        let (program, path_env) = match managed_bin(app, pack) {
            // 应用装的那份（Node 程序）：脚本头是 `#!/usr/bin/env node`，PATH 里要有 node
            Some(bin) => {
                let (node, path) = locate(&shell, "node").map_err(|_| format!("{} 的语言服务器需要 Node.js，但未找到 node", pack.name))?;
                let dir = node.parent().map(|d| d.to_string_lossy().into_owned()).unwrap_or_default();
                (bin, format!("{dir}:{path}"))
            }
            None => locate(&shell, &pack.server.command[0]).map_err(|e| {
                let how = if pack.npm.as_deref().and_then(npm_set).is_some() {
                    String::from("（可一键安装）")
                } else if pack.server.install.is_empty() {
                    String::new()
                } else {
                    format!("（安装：{}）", pack.server.install)
                };
                format!("{e}：{} 的语言服务器未安装{how}", pack.name)
            })?,
        };
        let spec = Spec { program, args: pack.server.command[1..].to_vec(), cwd: root.to_path_buf(), env: vec![("PATH".into(), path_env)] };
        let (app2, owner2, root2) = (app.clone(), owner.to_string(), root.to_string_lossy().into_owned());
        let raw_diags: Arc<Mutex<HashMap<String, Vec<Value>>>> = Arc::default();
        let raw2 = Arc::clone(&raw_diags);
        let on_notify: lspsvc::conn::OnNotify = Box::new(move |method, p| {
            if method != "textDocument/publishDiagnostics" {
                return;
            }
            let Some(path) = p["uri"].as_str().and_then(uri::to_path) else { return };
            raw2.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), p["diagnostics"].as_array().map(|a| a.iter().take(500).cloned().collect()).unwrap_or_default());
            let (items, total) = crate::java::diagnostics(&p["diagnostics"]);
            let _ = app2.emit_to(
                owner2.as_str(),
                "lsp-diagnostics",
                crate::commands::JavaDiagnosticsDto { root: root2.clone(), path, total: total as u32, items: items.into_iter().map(Into::into).collect() },
            );
        });
        let name = pack.name.clone();
        let srv = Server::start(&spec, Box::new(answer), on_notify, Box::new(move |e| {
            if !e.stopped {
                applog::write(applog::Level::Warn, "lsp", &format!("{name} 的语言服务器退出了（{}）：{}", e.status, e.stderr_tail.lines().last().unwrap_or("")));
            }
        }))
        .map_err(|e| format!("无法启动 {} 的语言服务器：{e}", pack.name))?;
        let caps = json!({
            "textDocument": {
                "synchronization": { "didSave": false },
                "completion": { "completionItem": { "snippetSupport": true, "labelDetailsSupport": true } },
                "hover": { "contentFormat": ["markdown", "plaintext"] },
                "definition": { "linkSupport": true },
                "references": {},
                "rename": { "prepareSupport": true },
                "formatting": {},
                "codeAction": { "codeActionLiteralSupport": { "codeActionKind": { "valueSet": ["quickfix", "refactor", "refactor.extract", "refactor.inline", "refactor.rewrite", "source"] } }, "resolveSupport": { "properties": ["edit"] } },
                "publishDiagnostics": {}
            },
            "workspace": { "configuration": true, "workspaceFolders": true }
        });
        let root_uri = uri::from_path(&root.to_string_lossy());
        let base = root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        srv.initialize(&root_uri, &base, caps, pack.server.initialization_options.clone(), INIT).map_err(|e| {
            let tail = srv.stderr_tail();
            format!("{} 的语言服务器初始化失败：{e}{}", pack.name, tail.lines().last().map(|l| format!("（{l}）")).unwrap_or_default())
        })?;
        applog::write(applog::Level::Info, "lsp", &format!("{} 起了（{}）", pack.name, root.display()));
        spawn_reaper(app.clone());
        Ok(Inst { srv, pack: pack.clone(), raw_diags, docs: Mutex::new(HashMap::new()), last_used: Mutex::new(Instant::now()) })
    }

    /// 打开或更新一个文档（全文）。不是任何包认的文件什么都不做；回这个文件归哪个包（`None` = 没有）
    pub fn open(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str) -> Result<Option<String>, String> {
        let Some(i) = self.inst(app, owner, root, path)? else { return Ok(None) };
        i.open(path, text);
        Ok(Some(i.pack.name.clone()))
    }

    pub fn close(&self, app: &AppHandle, owner: &str, root: &str, path: &str) {
        if let Ok(Some(i)) = self.inst_if_running(app, owner, root, path) {
            i.close(path);
        }
    }

    /// 只找已经起着的（关文档不该为了关它去起一个服务器）
    fn inst_if_running(&self, app: &AppHandle, owner: &str, root: &str, path: &str) -> Result<Option<Arc<Inst>>, String> {
        let (packs, _) = packs(app);
        let Some(pack) = pack_for(&packs, path) else { return Ok(None) };
        let key = format!("{owner}\n{}\n{}", root_for(pack, Path::new(root), Path::new(path)).display(), pack.id);
        Ok(self.all.get(&key, Arc::clone))
    }

    fn need(&self, app: &AppHandle, owner: &str, root: &str, path: &str) -> Result<Arc<Inst>, String> {
        self.inst(app, owner, root, path)?.ok_or_else(|| "此文件类型没有语言服务器".into())
    }

    pub fn definition(&self, app: &AppHandle, owner: &str, root: &str, path: &str, line: u32, character: u32) -> Result<Vec<crate::java::Location>, String> {
        let i = self.need(app, owner, root, path)?;
        Ok(crate::java::locations(&i.ask("textDocument/definition", Inst::at(path, line, character))?))
    }

    pub fn references(&self, app: &AppHandle, owner: &str, root: &str, path: &str, line: u32, character: u32) -> Result<(Vec<crate::java::Location>, usize), String> {
        let i = self.need(app, owner, root, path)?;
        let mut p = Inst::at(path, line, character);
        p["context"] = json!({ "includeDeclaration": false });
        let mut all = crate::java::locations(&i.ask("textDocument/references", p)?);
        let total = all.len();
        all.truncate(crate::java::REFS_MAX);
        Ok((all, total))
    }

    pub fn hover(&self, app: &AppHandle, owner: &str, root: &str, path: &str, line: u32, character: u32) -> Result<String, String> {
        let i = self.need(app, owner, root, path)?;
        Ok(crate::java::hover_markdown(&i.ask("textDocument/hover", Inst::at(path, line, character))?))
    }

    /// 补全。先把编辑器此刻的全文发过去（同 Java：改动是去抖了才发的）再在同一根管道上问
    pub fn complete(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str, line: u32, character: u32) -> Result<(Vec<crate::java::CompletionItem>, bool, usize), String> {
        let i = self.need(app, owner, root, path)?;
        i.open(path, text);
        let mut p = Inst::at(path, line, character);
        p["context"] = json!({ "triggerKind": 1 });
        Ok(crate::java::completion_items(&i.ask("textDocument/completion", p)?))
    }

    /// 光标底下能不能改名（先同步此刻的全文）。null = 这里不能
    pub fn prepare_rename(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str, line: u32, character: u32) -> Result<Option<crate::java::RenameTarget>, String> {
        let i = self.need(app, owner, root, path)?;
        i.open(path, text);
        Ok(crate::java::rename_target(&i.ask("textDocument/prepareRename", Inst::at(path, line, character))?))
    }

    /// 改名要改哪些（只问不改，落盘在命令那层走 `java::apply_rename`）
    pub fn rename(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str, line: u32, character: u32, new_name: &str) -> Result<crate::java::RenamePlan, String> {
        let i = self.need(app, owner, root, path)?;
        i.open(path, text);
        let mut p = Inst::at(path, line, character);
        p["newName"] = json!(new_name);
        let r = i.ask("textDocument/rename", p)?;
        if r.is_null() {
            return Err(format!("{} 语言服务器未返回更改（光标处可能无法重命名）", i.pack.name));
        }
        crate::java::workspace_edit(&r)
    }

    /// 格式化整份（缩进照编辑器对这个文件的判断给）。回改动，前端一笔做
    pub fn format(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str, tab_size: u32, spaces: bool) -> Result<Vec<crate::java::Edit>, String> {
        let i = self.need(app, owner, root, path)?;
        i.open(path, text);
        let r = i.ask("textDocument/formatting", json!({ "textDocument": { "uri": uri::from_path(path) }, "options": { "tabSize": tab_size, "insertSpaces": spaces } }))?;
        Ok(r.as_array().map(|a| a.iter().filter_map(|e| e["newText"].as_str().map(|t| crate::java::edit_of(&e["range"], t))).collect()).unwrap_or_default())
    }

    /// 光标那儿（或选中的那几行）能做的修复：把和这几行重叠的诊断原样带上
    pub fn code_actions(&self, app: &AppHandle, owner: &str, root: &str, path: &str, text: &str, range: crate::java::Edit) -> Result<Vec<crate::java::Action>, String> {
        let i = self.need(app, owner, root, path)?;
        i.open(path, text);
        let diags: Vec<Value> = i.raw_diags.lock().unwrap_or_else(|e| e.into_inner()).get(path).map(|v| v.iter().filter(|d| crate::java::overlaps(d, &range)).cloned().collect()).unwrap_or_default();
        let r = i.ask(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri::from_path(path) },
                "range": { "start": { "line": range.line, "character": range.character }, "end": { "line": range.end_line, "character": range.end_character } },
                "context": { "diagnostics": diags, "triggerKind": 1 },
            }),
        )?;
        Ok(crate::java::actions_of(&r))
    }

    /// 选中的那条修复要改什么：带着改动的直接用，没带的先 `codeAction/resolve`；还没有改动、只有命令的（要服务器再做一步的那种）这一版不做，说清楚
    pub fn resolve_action(&self, app: &AppHandle, owner: &str, root: &str, path: &str, raw: &str) -> Result<crate::java::RenamePlan, String> {
        let i = self.need(app, owner, root, path)?;
        let item: Value = serde_json::from_str(raw).map_err(|e| format!("快速修复格式无效：{e}"))?;
        let resolved = if item.get("edit").is_some() { item } else { i.ask("codeAction/resolve", item)? };
        if resolved.get("edit").is_none() {
            let cmd = if resolved["command"].is_string() { &resolved["command"] } else { &resolved["command"]["command"] };
            return Err(format!("此修复需要{} 语言服务器执行命令{}，当前版本暂不支持", i.pack.name, cmd.as_str().map(|c| format!("（{c}）")).unwrap_or_default()));
        }
        crate::java::workspace_edit(&resolved["edit"])
    }

    /// 一键装（`npm ci --ignore-scripts`，照编进应用的锁文件）。装好了把「起不来」的记录清掉，下次打开那种文件就起。回（装在哪, 多少字节）
    pub fn install(&self, app: &AppHandle, pack_id: &str) -> Result<(PathBuf, u64), String> {
        let (packs, _) = packs(app);
        let pack = packs.iter().find(|p| p.id == pack_id).ok_or_else(|| format!("没有语言包 {pack_id}"))?;
        let set = pack.npm.as_deref().ok_or_else(|| format!("{} 的语言服务器不支持一键安装", pack.name))?;
        let (manifest, lock, _) = npm_set(set).ok_or_else(|| format!("{} 的语言服务器不支持一键安装", pack.name))?;
        let shell = crate::settingsctl::terminal_shell(&app.state::<crate::state::AppState>());
        let (npm, path) = locate(&shell, "npm").map_err(|_| "安装需要 Node.js 和 npm，但未找到 npm（可先安装 Node.js：brew install node）".to_string())?;
        let base = app.path().app_data_dir().map_err(|e| format!("无法获取应用数据目录：{e}"))?.join("lsp");
        let dest = base.join(set);
        let tmp = base.join(format!(".{set}.part-{}", std::process::id()));
        // 上一次装到一半的临时目录（应用自己建的、名字卡死在 `.<套>.part-*`）：先清掉
        #[allow(clippy::disallowed_methods)] // 应用自己生成的临时目录，范围卡死在 lsp/.<套>.part-<pid>
        let _ = std::fs::remove_dir_all(&tmp);
        std::fs::create_dir_all(&tmp).map_err(|e| format!("无法创建 {}：{e}", tmp.display()))?;
        fsservice::write_text(&tmp.join("package.json"), manifest).map_err(|e| format!("无法写入 package.json：{e}"))?;
        fsservice::write_text(&tmp.join("package-lock.json"), lock).map_err(|e| format!("无法写入 package-lock.json：{e}"))?;
        let mut cmd = std::process::Command::new(&npm);
        cmd.args(["ci", "--ignore-scripts", "--no-audit", "--no-fund", "--prefer-offline", "--loglevel=error"]).current_dir(&tmp).env("PATH", &path);
        let out = procutil::run_capped(&mut cmd, 256 << 10, 64 << 10, procutil::Overflow::Drain).map_err(|e| format!("无法启动 npm：{e}"))?;
        if !out.status.success() {
            #[allow(clippy::disallowed_methods)] // 同上：装失败的临时目录
            let _ = std::fs::remove_dir_all(&tmp);
            return Err(format!("npm 安装失败：{}", npm_reason(&String::from_utf8_lossy(&out.stderr))));
        }
        // 装好的换上去：原来有一份（重装）先挪开再改名，改名失败就把原来那份放回去
        let old = base.join(format!(".{set}.old-{}", std::process::id()));
        let had = dest.exists();
        #[allow(clippy::disallowed_methods)] // 应用自己装的那份，挪到旁边的临时名
        if had {
            std::fs::rename(&dest, &old).map_err(|e| format!("无法替换原来的安装：{e}"))?;
        }
        #[allow(clippy::disallowed_methods)] // 刚装好的临时目录改名成正式的；目标此刻不存在（原来那份刚挪开）
        if let Err(e) = std::fs::rename(&tmp, &dest) {
            #[allow(clippy::disallowed_methods)] // 放回原来那份
            if had {
                let _ = std::fs::rename(&old, &dest);
            }
            return Err(format!("无法完成安装：{e}"));
        }
        #[allow(clippy::disallowed_methods)] // 被换下来的旧安装（应用自己装的）
        let _ = std::fs::remove_dir_all(&old);
        self.failed.lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| !k.ends_with(&format!("\n{pack_id}")));
        applog::write(applog::Level::Info, "lsp", &format!("装好了 {}（{}）", pack.name, dest.display()));
        Ok((dest.clone(), dir_size(&dest)))
    }

    /// 关窗口：它起的服务器都停掉（在别的线程上，shutdown 要等一会儿）
    pub fn release(&self, owner: &str) {
        let gone = self.all.take_owner(owner);
        self.failed.lock().unwrap_or_else(|e| e.into_inner()).retain(|k, _| !k.starts_with(&format!("{owner}\n")));
        if gone.is_empty() {
            return;
        }
        std::thread::spawn(move || {
            for (_, i) in gone {
                i.srv.shutdown(Duration::from_secs(2));
            }
        });
    }

    /// 没有开着的文档、闲了够久的收掉
    pub fn reap_idle(&self) {
        let idle: Vec<String> = self.all.with_map(|m| m.iter().filter(|(_, (_, i))| i.idle()).map(|(k, _)| k.clone()).collect());
        for k in idle {
            if let Some(i) = self.all.remove(&k) {
                applog::write(applog::Level::Info, "lsp", &format!("{} 闲了 {} 分钟，收掉", i.pack.name, IDLE.as_secs() / 60));
                i.srv.shutdown(Duration::from_secs(2));
            }
        }
    }
}

/// 应用装的那份（`lsp/<套>/node_modules/.bin/<命令>`），没装是 None
fn managed_bin(app: &AppHandle, pack: &Pack) -> Option<PathBuf> {
    let set = pack.npm.as_deref()?;
    let bin = app.path().app_data_dir().ok()?.join("lsp").join(set).join("node_modules").join(".bin").join(&pack.server.command[0]);
    bin.is_file().then_some(bin)
}

/// npm 失败时给人看的原因：前两条有内容的 `npm error`（错误码、原话）。最后一行是「完整日志在 …」，拿它当原因等于什么都没说
fn npm_reason(stderr: &str) -> String {
    let lines: Vec<&str> = stderr
        .lines()
        .filter_map(|l| l.trim().strip_prefix("npm error").or_else(|| l.trim().strip_prefix("npm ERR!")))
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.contains("A complete log of this run"))
        .take(2)
        .collect();
    let s = if lines.is_empty() { stderr.lines().find(|l| !l.trim().is_empty()).unwrap_or("（无输出）").trim().to_string() } else { lines.join("；") };
    s.chars().take(300).collect()
}

fn dir_size(p: &Path) -> u64 {
    let Ok(rd) = std::fs::read_dir(p) else { return 0 };
    rd.flatten()
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(t) if t.is_file() => e.metadata().map_or(0, |m| m.len()),
            _ => 0,
        })
        .sum()
}

/// 收闲着的服务器：每分钟看一眼。整个进程一条，第一次起服务器时才起（同 jdtls 那条）
fn spawn_reaper(app: AppHandle) {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| {
        let _ = std::thread::Builder::new().name("lsp-idle".into()).spawn(move || loop {
            std::thread::sleep(Duration::from_secs(60));
            app.state::<crate::state::AppState>().langs.reap_idle();
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 内置的包都读得进来_认文件() {
        let packs: Vec<Pack> = BUILTIN.iter().map(|t| parse_pack(t).unwrap()).collect();
        assert_eq!(pack_for(&packs, "/p/src/main.rs").map(|p| p.id.as_str()), Some("rust"));
        assert_eq!(pack_for(&packs, "/p/app/views.py").map(|p| p.id.as_str()), Some("python"));
        let ts = pack_for(&packs, "/p/web/App.tsx").unwrap();
        assert_eq!((ts.id.as_str(), ts.lang_of("/p/web/App.tsx"), ts.lang_of("/p/a.ts"), ts.lang_of("/p/a.js")), ("typescript", "typescriptreact".into(), "typescript".into(), "javascript".into()));
        assert!(pack_for(&packs, "/p/README.md").is_none());
        assert!(pack_for(&packs, "/p/.rs").is_none(), "只有扩展名、没有名字的不算");
    }

    #[test]
    fn 你自己的包_同_id_盖过内置的_新的加在后面() {
        let mut packs: Vec<Pack> = BUILTIN.iter().map(|t| parse_pack(t).unwrap()).collect();
        let mine = parse_pack(r#"{"schema":1,"id":"rust","name":"我的 Rust","files":["*.rs"],"languageId":"rust","server":{"command":["/opt/ra"]}}"#).unwrap();
        let go = parse_pack(r#"{"schema":1,"id":"go","name":"Go","files":["*.go"],"languageId":"go","server":{"command":["gopls"]}}"#).unwrap();
        merge(&mut packs, mine);
        merge(&mut packs, go);
        assert_eq!(pack_for(&packs, "/p/main.rs").map(|p| p.server.command[0].as_str()), Some("/opt/ra"), "认 .rs 的是你那份");
        assert_eq!(packs.iter().filter(|p| p.id == "rust").count(), 1, "不留两份");
        assert_eq!(pack_for(&packs, "/p/main.go").map(|p| p.name.as_str()), Some("Go"));
    }

    #[test]
    fn 一键装的那几套_锁文件里每个包都钉了版本_带_sha512_只从官方源下() {
        for (id, manifest, lock, _) in NPM {
            let m: Value = serde_json::from_str(manifest).unwrap();
            let l: Value = serde_json::from_str(lock).unwrap();
            let pkgs = l["packages"].as_object().unwrap();
            for (name, want) in m["dependencies"].as_object().unwrap() {
                let got = &pkgs[&format!("node_modules/{name}")];
                assert_eq!(got["version"], *want, "{id}：{name} 在锁文件里的版本和 package.json 钉的不一样");
            }
            for (k, v) in pkgs.iter().filter(|(k, _)| !k.is_empty()) {
                assert!(v["integrity"].as_str().is_some_and(|s| s.starts_with("sha512-")), "{id}：{k} 没有 sha512");
                assert!(v["resolved"].as_str().is_some_and(|s| s.starts_with("https://registry.npmjs.org/")), "{id}：{k} 不是从 npm 官方源下的");
            }
        }
        // 内置包里写的 npm 套都在表里
        for t in BUILTIN {
            let p = parse_pack(t).unwrap();
            if let Some(s) = p.npm.as_deref() {
                assert!(npm_set(s).is_some(), "{} 写的 npm 套 {s} 没编进应用", p.id);
            }
        }
    }

    #[test]
    fn npm_失败的原因取错误码和原话_不取最后那句日志在哪() {
        let err = "npm error code EINTEGRITY\nnpm error sha512-AAA== integrity checksum failed when using sha512\nnpm error A complete log of this run can be found in: /Users/x/.npm/_logs/1.log\n";
        assert_eq!(npm_reason(err), "code EINTEGRITY；sha512-AAA== integrity checksum failed when using sha512");
        assert_eq!(npm_reason("npm ERR! code ENOTFOUND\n"), "code ENOTFOUND", "老版本 npm 的写法");
        assert_eq!(npm_reason("something odd\n"), "something odd");
    }

    #[test]
    fn 认不得的版本和缺字段的不认() {
        assert!(parse_pack(r#"{"schema":2,"id":"x","name":"X","files":["*.x"],"languageId":"x","server":{"command":["x"]}}"#).unwrap_err().contains("schema 版本 2"));
        assert!(parse_pack(r#"{"schema":1,"id":"x","name":"X","files":[],"languageId":"x","server":{"command":["x"]}}"#).is_err());
        assert!(parse_pack("{").unwrap_err().contains("格式无效"));
    }

    #[test]
    #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
    fn 根_往上找到第一层有_cargo_toml_的_找不到就是项目根() {
        let d = std::env::temp_dir().join(format!("langs-root-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("crates/a/src")).unwrap();
        std::fs::write(d.join("crates/a/Cargo.toml"), "").unwrap();
        std::fs::create_dir_all(d.join("scripts")).unwrap();
        let rust = parse_pack(BUILTIN[0]).unwrap();
        assert_eq!(root_for(&rust, &d, &d.join("crates/a/src/lib.rs")), d.join("crates/a"));
        assert_eq!(root_for(&rust, &d, &d.join("scripts/x.rs")), d, "到项目根还没有：项目根");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn 找服务器_按标记取_rc_打的字不算_找到的是函数不算() {
        let out = "Welcome!\n__LITE_BIN__ fake\n__LITE_BIN__/Users/me/.cargo/bin/rust-analyzer\n__LITE_PATH__/Users/me/.cargo/bin:/usr/bin\n";
        assert_eq!(parse_locate(out), Some((PathBuf::from("/Users/me/.cargo/bin/rust-analyzer"), "/Users/me/.cargo/bin:/usr/bin".into())));
        assert_eq!(parse_locate("__LITE_BIN__\n__LITE_PATH__/usr/bin\n"), None, "没装");
        assert_eq!(parse_locate("__LITE_BIN__pylsp\n__LITE_PATH__/usr/bin\n"), None, "command -v 回的是别名 / 函数名（不是路径）");
        assert!(locate("/bin/zsh", "rust analyzer; rm -rf /").unwrap_err().contains("不允许的字符"), "命令名拼进 shell 之前先卡字符");
    }

    #[test]
    fn 服务器问配置_每项答_null() {
        assert_eq!(answer("workspace/configuration", &json!({ "items": [{}, {}] })).unwrap(), json!([null, null]));
        assert_eq!(answer("client/registerCapability", &json!({})).unwrap(), Value::Null);
    }
}
