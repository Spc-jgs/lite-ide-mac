//! Java 智能（docs/IDEA.md 第 2 步）：一个项目一个 jdtls，按需起、闲了收、崩了说。线和进程在 `lspsvc`，这里管的是「应用里的一份」。
//!
//! 第 3 节本心的几条在这里落成代码：
//!
//! - **不等**：打开 Java 文件时 `open` 只是登记 + 后台起，立刻返回；起好之前编辑器照常（Lezer 的跳转、着色都在）。
//!   状态（起着 / 导入中 43% / 能用了 / 出错了）走 `java-status` 事件到状态栏那格，不弹窗。
//! - **独立进程、可以关**：jdtls 在自己的进程组里；这个项目一个 Java 标签都没开着满 [`IDLE`] 就收掉（热启 1.2 秒，收掉代价很小）；
//!   窗口关了收它名下的（`AppState::release_window`）；退出时先按协议说再见（`stop_all`），`RunEvent::Exit` 时还没走的强杀（`kill_all`）。
//! - **不往项目里写东西**：参数在 `lspsvc::jdtls::spec`（第 0 步实测：不带那个系统属性 jdtls 往每个模块写 Eclipse 文件）。
//!
//! 文档同步用**全文**（`TextDocumentSyncKind.Full`）：一个 Java 文件几十 KB，每次改动全发也就几十 KB 的本机管道；增量同步要我们自己
//! 把 CM6 的改动换算成 UTF-16 的行列，错一个字符服务器那份就和编辑器分叉 —— 那种错不报错、只是报错位置越来越偏。先用全文，量出慢再换。

use crate::owned::Owned;
use lspsvc::{jdtls, uri, Error, Server};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, OnceLock};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager};

/// jdtls 的堆上限默认 1 GB（`java.jdtlsMemory`，docs/SETTINGS.md 13.4）。petclinic（9 个 Maven 项目）实测 RSS 1.1–1.5 GB、没有 OOM（IDEA.md 第 7 节）；
/// 更大的项目在设置页里调
/// 一个 Java 标签都没开着多久就收掉（第 3 节第 2 条）
pub const IDLE: Duration = Duration::from_secs(10 * 60);
/// 一个查询最多等多久。第一次查用法实测最慢 6 秒（冷、要搜全部索引），之后 65 ms
const ASK: Duration = Duration::from_secs(30);
/// initialize 最多等多久。实测 1–2 秒；冷缓存 + 要上网核对 Maven 元数据的那次也在 initialize 之后，不在这里
const INIT: Duration = Duration::from_secs(120);
/// 收尾时 shutdown / exit 各等多久。导入到一半收尾会卡在 JDT 的索引线程上（实测 20 秒不退，IDEA.md 第 7 节），到点就杀
const GRACE: Duration = Duration::from_secs(3);
/// 一次查用法最多回多少条（再多就截断、说出真条数）
pub const REFS_MAX: usize = 2000;
/// ⇧⇧ 的「类」一次最多要多少个：项目里的、连 jar 一起的各一份（找够 jdtls 自己停，见 [`Instance::types`]）
const TYPES_SRC: u32 = 100;
const TYPES_ALL: u32 = 200;
/// 内嵌提示一次最多要多少条（问的是看得见的那几十行，正常几条到几十条）
const INLAY_MAX: usize = 2000;
/// 层级里一个节点最多列多少个孩子（一个工具方法被几千处调用）
const HIER_MAX: usize = 500;
/// 同名高亮一次最多标多少处（一个几千行的生成文件里的 `i`）
const HL_MAX: usize = 1000;
/// 一个文件最多报多少条诊断（导入时整个项目的警告一起来，一个生成的文件可能几千条）
const DIAG_MAX: usize = 500;
/// 一次补全最多回多少项（jdtls 默认自己就截在 50 左右、标 isIncomplete；这里是防别的服务器 / 设置改大了一次回几千项）
pub const COMPL_MAX: usize = 200;
/// 补全项的说明文字最多留多少字（JDK 类的 Javadoc 动辄几千字，框里只看得下开头几段）
const DOC_MAX: usize = 3000;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// 进程起了，initialize 还没回
    Starting,
    /// initialize 回了，在导入（m2e 解析依赖、建索引）。带 jdtls 自己说的进度那句
    Importing(String),
    /// ServiceReady：能答语义问题了
    Ready,
    /// 起不来 / 崩了。带原因（崩了的话带 stderr 最后几行）
    Failed(String),
    /// 我们收掉的（闲了、窗口关了）
    Stopped,
}

/// 状态栏那格要的东西
#[derive(Clone, Debug)]
pub struct Status {
    pub root: String,
    pub phase: Phase,
    /// 用的哪个 JDK、Lombok 挂没挂上：出问题时人要知道这两样
    pub detail: String,
}

/// 一处位置。`path` 是工作区里的文件；jar 里的类 `path` 为空、`uri` 是 `jdt://…`、`label` 是类的全名
#[derive(Clone, Debug)]
pub struct Location {
    pub uri: String,
    pub path: Option<String>,
    pub label: String,
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// ⇧⇧ 的「类」找到的一个类型（docs/TAKEOVER.md G2）。`container` 是包名（内部类是外面那个类的全名）
#[derive(Clone, Debug)]
pub struct TypeHit {
    pub name: String,
    pub container: String,
    /// LSP 的 SymbolKind，jdtls 只回三种（1.61 的 `WorkspaceSymbolTypeRequestor.mapKind`）：11 接口（**注解也是**：先判的 `isInterface`，
    /// 注解类型带着接口标记）、10 枚举、5 类（record 也是）
    pub kind: u8,
    pub loc: Location,
}

/// 同名高亮的一处（docs/TAKEOVER.md G11）。`write`：这一处是写（赋值、声明时带初值），IDEA 用另一种底色
#[derive(Clone, Debug, PartialEq)]
pub struct Highlight {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub write: bool,
}

/// 一条内嵌提示（docs/TAKEOVER.md G11）：`label` 画在 `(line, character)` 那个位置前面。`param`：参数名（`cents:`）；否则是类型（`: long`）
#[derive(Clone, Debug, PartialEq)]
pub struct Inlay {
    pub line: u32,
    pub character: u32,
    pub label: String,
    pub param: bool,
}

/// 层级里的一个节点（docs/TAKEOVER.md G10）：调用层级的一个方法、类型层级的一个类型。
/// `raw` 是 jdtls 给的那个节点原样的 JSON —— 问它的下一层要原样送回去（里面有 jdtls 自己的 `data`），我们不拆它。
/// `site`：调用方那一层里「在哪儿调的」（`fromRanges` 的第一处，在 `at` 那个文件里）；别的方向没有
#[derive(Clone, Debug)]
pub struct HierNode {
    pub name: String,
    pub detail: String,
    pub kind: u8,
    pub at: Location,
    pub site: Option<(u32, u32)>,
    pub raw: String,
}

/// ⌘N 生成（docs/TAKEOVER.md G5）的第一步问回来的：这个类里能选的那几样（字段、可覆盖的方法）。
/// `raw` 是 jdtls 给的那一项原样的 JSON，第二步原样交回去；`note` 是要说给人听的一句（已经有 toString 了、会替换）
#[derive(Clone, Debug, PartialEq)]
pub struct GenChoices {
    pub type_name: String,
    pub items: Vec<GenItem>,
    pub note: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct GenItem {
    pub label: String,
    pub detail: String,
    pub checked: bool,
    pub raw: String,
}

#[derive(Clone, Debug)]
pub struct Diagnostic {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    /// 1 错 2 警告 3 信息 4 提示（LSP 的 DiagnosticSeverity）
    pub severity: u8,
    pub message: String,
}

/// 一处改动：`[line:character, end_line:end_character)` 换成 `text`。行列从 0 数、列按 UTF-16（LSP 的默认，正好是 JS 字符串下标）
#[derive(Clone, Debug, PartialEq)]
pub struct Edit {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
    pub text: String,
}

/// 一个补全项。**插什么看 `edit`，不看 `insertText`**：jdtls 的 `insertText` 是给不认 textEdit 的客户端的退路（方法只有名字），
/// 真要插的在 `textEdit.newText`（`add(${0});`、带括号和光标位置）—— 第 3 步第一次用探针看的时候看错了字段，以为要自己补括号
#[derive(Clone, Debug)]
pub struct CompletionItem {
    pub label: String,
    /// LSP 的 CompletionItemKind（2 方法、5 字段、7 类……），界面上换成图标
    pub kind: u32,
    /// 跟在名字后面的灰字：方法的参数表 `(String name, int count)`
    pub detail: String,
    /// 靠右的灰字：返回类型 / 包名
    pub description: String,
    /// 没有 textEdit 的（别的服务器）：`edit` 为空，前端拿光标前那个词的范围、插 `label`
    pub edit: Option<Edit>,
    /// `newText` 是 snippet（`${1:name}`、`$0` 这种占位）
    pub snippet: bool,
    pub sort: String,
    /// 原样的这一项（JSON）：选中时整个交回去 resolve —— jdtls 认 `data` 里它自己记的编号，别的字段也可能要
    pub raw: String,
}

/// 改名要做的事（jdtls 的 WorkspaceEdit 拆开）：哪些文件改哪几处、哪些文件改名。**先改全部内容、最后改名**（[`workspace_edit`]）
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RenamePlan {
    /// 工作区里的文件（绝对路径）→ 这个文件里的改动，按服务器给的次序
    pub files: Vec<(String, Vec<Edit>)>,
    /// （原路径, 新路径）
    pub renames: Vec<(String, String)>,
}

/// 光标底下能不能改名、改的是哪一段（prepareRename）
#[derive(Clone, Debug, PartialEq)]
pub struct RenameTarget {
    pub range: Edit,
    /// 输入框里预填的：服务器给了就用它的，没给就是那一段的原文（前端取）
    pub placeholder: Option<String>,
}

/// 一条修复（⌥↵）。`raw` 原样交回 `apply_action`（选中了再 resolve 要改动）
#[derive(Clone, Debug)]
pub struct Action {
    pub title: String,
    /// `quickfix`（修报错）/ `quickassist`（顺手的改法）/ `source…`（整理 import 这类整个文件的）
    pub kind: String,
    pub raw: String,
}

/// 一次最多列多少条修复：jdtls 对一个找不到的类名会列二三十条「改成某某」，列表再长没人看
pub const ACTIONS_MAX: usize = 40;

/// resolve 回来的：要一起做的改动（自动 import 就在这里，**只在 resolve 时才给**，补全列表里没有）+ 说明文字
#[derive(Clone, Debug, Default)]
pub struct Resolved {
    pub edits: Vec<Edit>,
    pub doc: String,
}

struct Shared {
    root: String,
    owner: String,
    app: AppHandle,
    phase: Mutex<Phase>,
    changed: Condvar,
    detail: String,
    /// 开着的 Java 文档：路径 → 版本号。`synced` = initialize 回过了、开着的都 didOpen 过了；在这之前开的先记着，回了一起补发
    docs: Mutex<Docs>,
    last_used: Mutex<Instant>,
    /// 每个文件最近一轮的报错，**原样**（带 `code`）：⌥↵ 问修复时要把光标那儿的报错交回去，jdtls 按 `code` 里的问题编号找修复。
    /// 前端画波浪线那份没带 `code`。上限同 `DIAG_MAX`
    raw_diags: Mutex<HashMap<String, Vec<Value>>>,
    /// 起的时候带没带调试插件（开发构建没有）
    debuggable: bool,
}

struct Docs {
    open: HashMap<String, i64>,
    /// synced 之前每个文档最新的全文：补发 didOpen 用它，**不重读盘** —— 编辑器里没保存的才是真相（IDEA.md 第 4 节），
    /// 读盘补发的话 jdtls 分析的和屏幕上的不是同一份。synced 之后不再留（每次改动直接发出去了）
    pending: HashMap<String, String>,
    synced: bool,
}

pub struct Instance {
    shared: Arc<Shared>,
    srv: Server,
    /// 有几个调试会话挂在它上面（`debug.rs`）：java-debug 跑在 jdtls 进程里，调试的时候不能因为「没开着的 Java 文件、闲了十分钟」被收掉
    holds: std::sync::atomic::AtomicUsize,
    /// 项目里的 `.java` / 构建文件在盘上变了（改名、查找替换、撤销、切分支、别的编辑器）→ 告诉 jdtls。随实例 drop 停
    watch: Mutex<Option<fsservice::watch::PathWatch>>,
}

/// 一个调试会话攥着的 jdtls（[`Instance::hold`]）
pub struct Hold(Arc<Instance>);

impl Drop for Hold {
    fn drop(&mut self) {
        self.0.holds.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
    }
}

/// `resolveMainClass` 的回答里，这个主类在哪个项目（m2e 的项目名，一般是 artifactId）
/// 调试要等到 ServiceReady：下断点要按源文件算出类名，导入没完的模块算不出来（断点会一直是「没对上」）。
/// 第一次导入一个大项目要下依赖，几分钟也正常 —— 被调试的 JVM 这时停在入口等着，等得起。
/// **但人可能等不及、把程序停了**：原来只认截止时间（10 分钟），停了之后调试窗还转着「等 Java 语言服务器导入完…」最多 10 分钟（2026-10-20 code review）。
/// 现在最多隔 [`ALIVE_POLL`] 问一次 `alive`
fn wait_ready(phase: &Mutex<Phase>, changed: &Condvar, limit: Duration, alive: &dyn Fn() -> bool) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    let mut p = phase.lock().unwrap_or_else(|e| e.into_inner());
    loop {
        match &*p {
            Phase::Ready => return Ok(()),
            Phase::Failed(why) => return Err(why.clone()),
            Phase::Stopped => return Err("Java 语言服务器已经收掉了".into()),
            Phase::Starting | Phase::Importing(_) => {}
        }
        if !alive() {
            return Err("程序已经停了，不再等 Java 语言服务器".into());
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!("Java 语言服务器 {} 分钟还没导入完", limit.as_secs() / 60));
        }
        p = changed.wait_timeout(p, left.min(ALIVE_POLL)).unwrap_or_else(|e| e.into_inner()).0;
    }
}

/// 等导入完时隔多久问一次「被调试的程序还在不在」
const ALIVE_POLL: Duration = Duration::from_millis(500);

fn project_of(mains: &Value, main_class: &str) -> Option<String> {
    mains.as_array()?.iter().find(|m| m["mainClass"] == main_class)?["projectName"].as_str().map(str::to_owned)
}

/// 一批里最多告诉 jdtls 多少个文件变了。切一次大分支能动几万个文件：超了只说前这么多、记一句 WARN（jdtls 的索引那时可能不全，重起它就好）
const WATCH_CAP: usize = 2000;

/// jdtls 关心哪些文件在盘上的变化：Java 源码和构建文件（改了 pom 要重新导入）
fn java_relevant(p: &Path) -> bool {
    p.extension().is_some_and(|e| e == "java")
        || p.file_name().is_some_and(|n| matches!(n.to_str(), Some("pom.xml" | "build.gradle" | "build.gradle.kts" | "settings.gradle" | "settings.gradle.kts")))
}

impl Instance {
    pub fn status(&self) -> Status {
        self.shared.status()
    }

    fn alive(&self) -> bool {
        !matches!(*self.shared.phase.lock().unwrap_or_else(|e| e.into_inner()), Phase::Failed(_) | Phase::Stopped)
    }

    fn touch(&self) {
        *self.shared.last_used.lock().unwrap_or_else(|e| e.into_inner()) = Instant::now();
    }

    /// 开着的文档没了、又闲了够久
    fn idle(&self) -> bool {
        self.holds.load(std::sync::atomic::Ordering::SeqCst) == 0
            && self.shared.docs.lock().unwrap_or_else(|e| e.into_inner()).open.is_empty()
            && self.shared.last_used.lock().unwrap_or_else(|e| e.into_inner()).elapsed() >= IDLE
    }

    pub fn open(&self, path: &str, text: &str) {
        self.touch();
        let mut d = self.shared.docs.lock().unwrap_or_else(|e| e.into_inner());
        if d.open.contains_key(path) {
            drop(d);
            return self.change(path, text);
        }
        d.open.insert(path.to_string(), 1);
        if d.synced {
            let _ = self.srv.notify("textDocument/didOpen", did_open(path, 1, text));
        } else {
            d.pending.insert(path.to_string(), text.to_string());
        }
    }

    pub fn change(&self, path: &str, text: &str) {
        self.touch();
        let mut d = self.shared.docs.lock().unwrap_or_else(|e| e.into_inner());
        let Some(v) = d.open.get_mut(path) else {
            drop(d);
            return self.open(path, text);
        };
        *v += 1;
        let v = *v;
        if !d.synced {
            d.pending.insert(path.to_string(), text.to_string());
        } else {
            let _ = self.srv.notify(
                "textDocument/didChange",
                json!({ "textDocument": { "uri": uri::from_path(path), "version": v }, "contentChanges": [{ "text": text }] }),
            );
        }
    }

    pub fn save(&self, path: &str) {
        self.touch();
        let d = self.shared.docs.lock().unwrap_or_else(|e| e.into_inner());
        if d.synced && d.open.contains_key(path) {
            let _ = self.srv.notify("textDocument/didSave", json!({ "textDocument": { "uri": uri::from_path(path) } }));
        }
    }

    pub fn close(&self, path: &str) {
        self.touch();
        let mut d = self.shared.docs.lock().unwrap_or_else(|e| e.into_inner());
        d.pending.remove(path);
        if d.open.remove(path).is_some() && d.synced {
            let _ = self.srv.notify("textDocument/didClose", json!({ "textDocument": { "uri": uri::from_path(path) } }));
        }
    }

    /// 等到能问为止（initialize 回过了），最多等 `ASK`。导入中也能问：jdtls 答得出已经导入的那部分
    fn wait_usable(&self) -> Result<(), String> {
        let deadline = Instant::now() + ASK;
        let mut p = self.shared.phase.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            match &*p {
                Phase::Importing(_) | Phase::Ready => return Ok(()),
                Phase::Failed(why) => return Err(why.clone()),
                Phase::Stopped => return Err("Java 语言服务器已经收掉了".into()),
                Phase::Starting => {}
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err("Java 语言服务器还没起好".into());
            }
            p = self.shared.changed.wait_timeout(p, left).unwrap_or_else(|e| e.into_inner()).0;
        }
    }

    /// 起一个调试会话（docs/IDEA.md 第 18 节）：java-debug 插件在 jdtls 进程里开一个端口说 DAP，回给我们。
    /// 给了主类就顺带问它属于哪个 jdtls 项目 —— attach 时不带 `projectName` 的话求值报「please specify projectName」（探针实测）；
    /// 问不到不算错，只是求值用不了。没给（单测：测试类没有 main）由调用方自己定。
    /// 等导入完的时候 `alive` 说被调试的程序还在不在：人在等的时候把它停了，就不再等（见 [`wait_ready`]）
    pub fn debug_session(&self, main_class: Option<&str>, limit: Duration, alive: &dyn Fn() -> bool) -> Result<(u16, Option<String>), String> {
        if !self.shared.debuggable {
            return Err("这个构建没带调试插件（开发构建）：用 pnpm app:bundle 打的包".into());
        }
        self.touch();
        wait_ready(&self.shared.phase, &self.shared.changed, limit, alive)?;
        let r = self
            .srv
            .request("workspace/executeCommand", json!({ "command": "vscode.java.startDebugSession", "arguments": [] }), ASK)
            .map_err(|e| format!("调试插件没答（{e}）"))?;
        let port = r.as_u64().and_then(|p| u16::try_from(p).ok()).ok_or_else(|| format!("调试插件回的不是端口：{r}"))?;
        let Some(main_class) = main_class else { return Ok((port, None)) };
        let mains = self
            .srv
            .request("workspace/executeCommand", json!({ "command": "vscode.java.resolveMainClass", "arguments": [uri::from_path(&self.shared.root)] }), ASK)
            .unwrap_or(Value::Null);
        Ok((port, project_of(&mains, main_class)))
    }

    /// 调试期间攥着它（见 `holds`）。guard 丢了就放开
    pub fn hold(self: &Arc<Self>) -> Hold {
        self.holds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Hold(Arc::clone(self))
    }

    fn ask(&self, method: &str, params: Value) -> Result<Value, String> {
        self.touch();
        self.wait_usable()?;
        let r = self.srv.request(method, params, ASK).map_err(|e| match e {
            Error::Timeout => format!("Java 语言服务器 {} 秒没回答（可能还在导入）", ASK.as_secs()),
            e => e.to_string(),
        });
        // 记下来：补全失败时前端是悄悄退回空列表的（不为一次补全弹红字），不记的话「补全怎么没东西」查无可查（2026-10-19 验收撞见）
        if let Err(e) = &r {
            applog::write(applog::Level::Warn, "java", &format!("{method} 失败：{e}"));
        }
        r
    }

    pub fn definition(&self, path: &str, line: u32, character: u32) -> Result<Vec<Location>, String> {
        let r = self.ask("textDocument/definition", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(locations(&r))
    }

    /// 跳到实现（⌥⌘B，docs/TAKEOVER.md G1）：接口方法 / 抽象方法的实现，类的子类。超过 [`REFS_MAX`] 截断，返回 `(前 REFS_MAX 条, 真条数)`
    pub fn implementation(&self, path: &str, line: u32, character: u32) -> Result<(Vec<Location>, usize), String> {
        let r = self.ask("textDocument/implementation", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        let mut all = locations(&r);
        let total = all.len();
        all.truncate(REFS_MAX);
        Ok((all, total))
    }

    /// 跳到父方法（⌘U）：覆盖的那个方法 / 父类。LSP 没有这一条，jdtls 自己的 `java/findLinks`（type = superImplementation，
    /// VS Code 的 Java 插件也是它）—— 读 jdtls 1.61 的 `FindLinksHandler` 确认：回的是 `LinkLocation`（Location 多两个字段），`locations` 照读
    pub fn super_impl(&self, path: &str, line: u32, character: u32) -> Result<Vec<Location>, String> {
        let r = self.ask(
            "java/findLinks",
            json!({ "type": "superImplementation", "position": { "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } } }),
        )?;
        Ok(locations(&r))
    }

    /// 快速文档（F1，docs/TAKEOVER.md G3）：jdtls 的 hover —— 签名 + Javadoc，统一成一段 Markdown（[`hover_markdown`]）；没有就是空串
    pub fn hover(&self, path: &str, line: u32, character: u32) -> Result<String, String> {
        let r = self.ask("textDocument/hover", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(hover_markdown(&r))
    }

    /// 按名字找类型（⇧⇧ 的「类」，docs/TAKEOVER.md G2），返回 `(找到的, 是不是没找全)`。
    ///
    /// 走 jdtls 自己的 `java/searchSymbols`，不走 LSP 的 `workspace/symbol`：两条进的是同一个 `WorkspaceSymbolHandler.search`
    /// （读 1.61 的字节码确认），但标准那条把上限写死成 0 = 不设上限 —— 打一个 `S` 就把 JDK 和每个 jar 里名字带 S 的类型全回来，
    /// 一条消息几 MB。这条能传 `maxResults`（找够了让搜索自己停）和 `sourceOnly`。名字怎么匹配是 jdtls 定的：
    /// 不带 `*` `?` 时按「包含」和驼峰（`OC` 找得到 `OrderController`），带 `.` 时点前面那段当包名。
    ///
    /// 项目里的单独先问一次：回来的次序 jdtls 不保证（存在一个 HashSet 里），只问一次「连 jar 一起、200 个」的话，
    /// JDK 里同名的能把项目自己的挤出去。只找类型 —— 方法要 jdtls 开 `includeSourceMethodDeclarations`，开了类型和方法共用一个上限
    pub fn types(&self, query: &str, libs: bool) -> Result<(Vec<TypeHit>, bool), String> {
        let ask = |max: u32, source_only: bool| -> Result<Vec<TypeHit>, String> {
            let r = self.ask("java/searchSymbols", json!({ "query": query, "maxResults": max, "sourceOnly": source_only, "projectName": null }))?;
            Ok(type_hits(&r))
        };
        let mut out = ask(TYPES_SRC, true)?;
        let mut cut = out.len() >= TYPES_SRC as usize;
        if libs {
            let more = ask(TYPES_ALL, false)?;
            cut |= more.len() >= TYPES_ALL as usize;
            for t in more {
                if !out.iter().any(|o| o.loc.uri == t.loc.uri && o.name == t.name) {
                    out.push(t);
                }
            }
        }
        Ok((out, cut))
    }

    /// 同名高亮（docs/TAKEOVER.md G11，IDEA 的「光标处元素的用法」）：光标底下那个名字在这个文件里出现的地方。
    /// 是语义的：同名的局部变量、别的类的同名字段不算 —— 这就是它和「选中的字在文中别处」（CM6 的 selectionMatch）不一样的地方
    pub fn highlights(&self, path: &str, line: u32, character: u32) -> Result<Vec<Highlight>, String> {
        let r = self.ask("textDocument/documentHighlight", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(highlights_of(&r))
    }

    /// 内嵌提示（docs/TAKEOVER.md G11，IDEA 的 Parameter Name Hints）：从 `from` 行开头到 `(end_line, end_char)` 的。行列从 0 数。
    /// **终点必须是文件里真有的位置**：写成「最后一行的下一行、第 0 列」jdtls 换不成偏移，整段回空（2026-10-22 探针：12 行的文件问到第 12 行是 `[]`、问到第 11 行有两条）。
    /// 给哪些是 jdtls 的设置定的：`java.inlayHints.parameterNames.enabled` 默认 `literals`（读 1.61 的 `Preferences` 确认）——
    /// 只给字面量参数（`apply(100)` 的 `cents:`），变量名本身就说清了的不给，同 IDEA 的默认
    pub fn inlay_hints(&self, path: &str, from: u32, end_line: u32, end_char: u32) -> Result<Vec<Inlay>, String> {
        let r = self.ask(
            "textDocument/inlayHint",
            json!({ "textDocument": { "uri": uri::from_path(path) }, "range": { "start": { "line": from, "character": 0 }, "end": { "line": end_line, "character": end_char } } }),
        )?;
        Ok(inlays_of(&r))
    }

    /// 调用层级 / 类型层级的根（docs/TAKEOVER.md G10，⌃⌥H / ⌃H）：光标底下那个方法 / 类型。`types`：类型层级。
    /// LSP 3.17 那几条（jdtls 1.61 的 `JDTLanguageServer` 里都登记了，读字节码确认）。光标不在方法 / 类型上是空的
    pub fn hier_prepare(&self, path: &str, line: u32, character: u32, types: bool) -> Result<Vec<HierNode>, String> {
        let method = if types { "textDocument/prepareTypeHierarchy" } else { "textDocument/prepareCallHierarchy" };
        let r = self.ask(method, json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(r.as_array().map(|a| a.iter().filter_map(|i| hier_node(i, None)).collect()).unwrap_or_default())
    }

    /// 一个节点的下一层，返回 `(前 HIER_MAX 个, 真个数)`。`dir`：`in` 谁调了它 / `out` 它调了谁 / `super` 父类型 / `sub` 子类型。`raw` 是那个节点原样的 JSON
    pub fn hier_children(&self, raw: &str, dir: &str) -> Result<(Vec<HierNode>, usize), String> {
        let item: Value = serde_json::from_str(raw).map_err(|e| format!("层级节点读不懂：{e}"))?;
        let method = match dir {
            "in" => "callHierarchy/incomingCalls",
            "out" => "callHierarchy/outgoingCalls",
            "super" => "typeHierarchy/supertypes",
            "sub" => "typeHierarchy/subtypes",
            _ => return Err(format!("不认识的方向：{dir}")),
        };
        let r = self.ask(method, json!({ "item": item }))?;
        let all = hier_children_of(&r, dir);
        let total = all.len();
        Ok((all.into_iter().take(HIER_MAX).collect(), total))
    }

    /// ⌘N 生成（docs/TAKEOVER.md G5）第一步：光标所在的类里能选哪些。`what`：`constructor` / `getter` / `setter` / `accessors` / `equals` / `tostring` / `override`。
    ///
    /// 直接调 jdtls 自己的 `java/check*Status` 这几条，不走 code action：toString、equals/hashCode、覆盖方法这几样，
    /// 客户端不声明「会弹框让人选」jdtls 根本不给（1.61 `SourceAssistProcessor` 里先判 `is*PromptSupported`，否则空）；
    /// 声明了的话 ⌥↵ 的列表里又会多出一堆只带命令、我们执行不了的条目。所以弹框是我们自己的，问和改都直接调它背后那两条请求
    pub fn gen_status(&self, path: &str, text: &str, line: u32, character: u32, what: &str) -> Result<GenChoices, String> {
        self.change(path, text);
        let ctx = gen_ctx(path, line, character);
        let r = match what {
            "tostring" => self.ask("java/checkToStringStatus", ctx)?,
            "equals" => self.ask("java/checkHashCodeEqualsStatus", ctx)?,
            "constructor" => self.ask("java/checkConstructorsStatus", ctx)?,
            "getter" | "setter" | "accessors" => {
                // `kind` 不能省：jdtls 拿它挑字段，空着就是 Internal error（2026-10-22 真 jdtls 上撞的）。lsp4j 按序号收这个枚举（GETTER 0 / SETTER 1 / BOTH 2），
                // VS Code 的 Java 插件传的也是数字
                let mut p = ctx;
                p["kind"] = json!(match what { "getter" => 0, "setter" => 1, _ => 2 });
                self.ask("java/resolveUnimplementedAccessors", p)?
            }
            "override" => self.ask("java/listOverridableMethods", ctx)?,
            _ => return Err(format!("不认识的生成：{what}")),
        };
        Ok(gen_choices_of(what, &r))
    }

    /// 第二步：人勾好了（`picked` 是第一步那几项的 `raw`），要改动。回来的是一份 WorkspaceEdit，同改名那条路落地
    pub fn gen_apply(&self, path: &str, text: &str, line: u32, character: u32, what: &str, picked: &[String]) -> Result<RenamePlan, String> {
        self.change(path, text);
        let ctx = gen_ctx(path, line, character);
        let picked: Vec<Value> = picked.iter().map(|s| serde_json::from_str(s).map_err(|e| format!("选中的那一项读不懂：{e}"))).collect::<Result<_, _>>()?;
        // 构造器要带「调父类的哪个构造器」：第一步没让人选（几乎总是只有一个），这里再问一次、用第一个
        let supers = if what == "constructor" { self.ask("java/checkConstructorsStatus", ctx.clone())?["constructors"].as_array().and_then(|a| a.first().cloned()) } else { None };
        let (method, params) = gen_params(what, ctx, picked, supers)?;
        let r = self.ask(method, params)?;
        if r.is_null() {
            return Err("jdtls 没有给出改动（光标可能不在类里）".into());
        }
        workspace_edit(&r)
    }

    /// 查用法。超过 [`REFS_MAX`] 截断，返回 `(前 REFS_MAX 条, 真条数)`
    pub fn references(&self, path: &str, line: u32, character: u32) -> Result<(Vec<Location>, usize), String> {
        let r = self.ask(
            "textDocument/references",
            json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character }, "context": { "includeDeclaration": false } }),
        )?;
        let mut all = locations(&r);
        let total = all.len();
        all.truncate(REFS_MAX);
        Ok((all, total))
    }

    /// jar 里一个类的内容：有源码 jar 就是源码，没有就是 jdtls 反编译的
    pub fn class_contents(&self, jdt_uri: &str) -> Result<String, String> {
        let r = self.ask("java/classFileContents", json!({ "uri": jdt_uri }))?;
        r.as_str().map(str::to_owned).ok_or_else(|| "这个类拿不到内容（jdtls 没有回源码，也没有反编译）".into())
    }

    /// 补全。带着编辑器里**此刻**的全文：前端改动是去抖 250ms 才发的，刚敲的那个点 jdtls 还没看到 ——
    /// 先把这份文本作为一次改动发过去，再在同一根管道上问（jdtls 按顺序处理，问的时候已经是新文本）
    pub fn complete(&self, path: &str, text: &str, line: u32, character: u32) -> Result<(Vec<CompletionItem>, bool, usize), String> {
        self.change(path, text);
        let r = self.ask(
            "textDocument/completion",
            json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character }, "context": { "triggerKind": 1 } }),
        )?;
        Ok(completion_items(&r))
    }

    /// 参数提示（敲 `(` / `,` 时）。同补全：先把编辑器此刻的全文发过去再问
    pub fn signature_help(&self, path: &str, text: &str, line: u32, character: u32) -> Result<Signatures, String> {
        self.change(path, text);
        let r = self.ask("textDocument/signatureHelp", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(signatures(&r))
    }

    /// 选中一项之后要的：自动 import（`additionalTextEdits`）和说明文字。`raw` 是补全时那一项的原样
    pub fn resolve(&self, raw: &str) -> Result<Resolved, String> {
        let item: Value = serde_json::from_str(raw).map_err(|e| format!("补全项不是 JSON：{e}"))?;
        let r = self.ask("completionItem/resolve", item)?;
        Ok(resolved(&r))
    }

    /// 格式化（⌥⌘L）：整份，或者 `range` 那几行。带着此刻的全文（同 `complete`）。缩进照编辑器此刻的设置给（`tab_size` 个空格 / Tab）——
    /// 不给的话 jdtls 用 Eclipse 的默认（Tab 缩进），和文件现在的缩进对不上就是满屏改动（探针实测：给了它就照着排）
    pub fn format(&self, path: &str, text: &str, range: Option<Edit>, tab_size: u32, spaces: bool) -> Result<Vec<Edit>, String> {
        self.change(path, text);
        let doc = json!({ "uri": uri::from_path(path) });
        let options = json!({ "tabSize": tab_size, "insertSpaces": spaces });
        let r = match range {
            Some(r) => self.ask(
                "textDocument/rangeFormatting",
                json!({ "textDocument": doc, "options": options, "range": { "start": { "line": r.line, "character": r.character }, "end": { "line": r.end_line, "character": r.end_character } } }),
            )?,
            None => self.ask("textDocument/formatting", json!({ "textDocument": doc, "options": options }))?,
        };
        Ok(r.as_array().map(|a| a.iter().filter_map(|e| e["newText"].as_str().map(|t| edit_of(&e["range"], t))).collect()).unwrap_or_default())
    }

    /// 光标那儿（或选中的那几行）能做的修复（⌥↵）。带着全文；把和这几行重叠的报错原样带上（[`Shared::raw_diags`]）
    pub fn code_actions(&self, path: &str, text: &str, range: Edit) -> Result<Vec<Action>, String> {
        self.change(path, text);
        let diags: Vec<Value> = self
            .shared
            .raw_diags
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(path)
            .map(|v| v.iter().filter(|d| overlaps(d, &range)).cloned().collect())
            .unwrap_or_default();
        let r = self.ask(
            "textDocument/codeAction",
            json!({
                "textDocument": { "uri": uri::from_path(path) },
                "range": { "start": { "line": range.line, "character": range.character }, "end": { "line": range.end_line, "character": range.end_character } },
                "context": { "diagnostics": diags, "triggerKind": 1 },
            }),
        )?;
        Ok(actions_of(&r))
    }

    /// 选中的那条修复要改什么（没带改动的先 `codeAction/resolve`）。没有改动、只有命令的（要弹交互框的那种）这一版不做，说清楚
    pub fn resolve_action(&self, raw: &str) -> Result<RenamePlan, String> {
        let item: Value = serde_json::from_str(raw).map_err(|e| format!("修复不是 JSON：{e}"))?;
        let resolved = if item.get("edit").is_some() { item } else { self.ask("codeAction/resolve", item)? };
        if resolved.get("edit").is_none() {
            let cmd = resolved["command"]["command"].as_str().unwrap_or("");
            return Err(format!("这条修复要执行命令{}，这一版还不支持", if cmd.is_empty() { String::new() } else { format!("（{cmd}）") }));
        }
        workspace_edit(&resolved["edit"])
    }

    /// 光标底下能不能改名。带着此刻的全文（同 `complete`）。null / 不认 = 这里不能改名（关键字、注释里）
    pub fn prepare_rename(&self, path: &str, text: &str, line: u32, character: u32) -> Result<Option<RenameTarget>, String> {
        self.change(path, text);
        let r = self.ask("textDocument/prepareRename", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(rename_target(&r))
    }

    /// 改名：问 jdtls 要改哪些（第一次要搜全部索引，探针实测 4.3 秒，之后一两百毫秒），拆成 [`RenamePlan`]。只问不改
    pub fn rename(&self, path: &str, text: &str, line: u32, character: u32, new_name: &str) -> Result<RenamePlan, String> {
        self.change(path, text);
        let r = self
            .ask(
                "textDocument/rename",
                json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character }, "newName": new_name }),
            )
            .map_err(Self::rename_refusal)?;
        workspace_edit(&r)
    }

    /// jdtls 自己拒绝改名时的原话是英文的（`Compilation unit 'Customer.java' already exists`），常见的换成中文，别的原样
    fn rename_refusal(e: String) -> String {
        match e.split_once("Compilation unit '").and_then(|(_, rest)| rest.split_once("' already exists")) {
            Some((name, _)) => format!("{name} 已经有了，不覆盖"),
            None => e,
        }
    }

    /// 盘上的文件变了（[`java_relevant`] 那些）：`workspace/didChangeWatchedFiles`。**不分是谁改的** —— 改名、查找替换、撤销、切分支、
    /// 别的编辑器写的都从文件监听这一处来，不在每条写盘的路上各补一句通知（改名第一版就是只在改名里补了一句，撤销之后 jdtls 还以为类叫新名字）。
    /// 开着的文件 jdtls 以编辑器里那份为准（didChange），这里说了也不会盖掉它。initialize 之前不说（说了它也不认）
    fn on_disk_changes(&self, b: fsservice::watch::PathBatch) {
        if !self.shared.docs.lock().unwrap_or_else(|e| e.into_inner()).synced {
            return;
        }
        if b.overflow {
            applog::write(applog::Level::Warn, "java", &format!("一次变了超过 {WATCH_CAP} 个文件，只告诉了 jdtls 前 {WATCH_CAP} 个：索引可能不全，状态栏「重起」可以补"));
        }
        let changes: Vec<Value> = b
            .changes
            .iter()
            .map(|(p, c)| {
                let t = match c {
                    fsservice::watch::PathChange::Created => 1,
                    fsservice::watch::PathChange::Changed => 2,
                    fsservice::watch::PathChange::Removed => 3,
                };
                json!({ "uri": uri::from_path(&p.to_string_lossy()), "type": t })
            })
            .collect();
        if !changes.is_empty() {
            let head: Vec<String> = b.changes.iter().take(3).map(|(p, c)| format!("{c:?} {}", p.file_name().map(|n| n.to_string_lossy()).unwrap_or_default())).collect();
            applog::write(applog::Level::Info, "java", &format!("盘上变了 {} 个文件，告诉 jdtls：{}{}", changes.len(), head.join("、"), if changes.len() > 3 { " …" } else { "" }));
            let _ = self.srv.notify("workspace/didChangeWatchedFiles", json!({ "changes": changes }));
        }
    }

    fn stop(&self) {
        self.shared.set(Phase::Stopped);
        self.srv.shutdown(GRACE);
    }
}

impl Shared {
    fn status(&self) -> Status {
        Status { root: self.root.clone(), phase: self.phase.lock().unwrap_or_else(|e| e.into_inner()).clone(), detail: self.detail.clone() }
    }

    /// 换状态、通知前端。已经是 Failed / Stopped 的不再被后来的进度覆盖（崩了之后迟到的一条「导入中」不能把错误盖掉）
    fn set(&self, next: Phase) {
        {
            let mut p = self.phase.lock().unwrap_or_else(|e| e.into_inner());
            if matches!(*p, Phase::Failed(_) | Phase::Stopped) || *p == next {
                return;
            }
            *p = next;
        }
        self.changed.notify_all();
        emit_status(&self.app, &self.owner, &self.status());
    }

    fn on_notify(&self, method: &str, p: Value) {
        match method {
            "language/status" => match p["type"].as_str().unwrap_or("") {
                "ServiceReady" => self.set(Phase::Ready),
                // 「Starting」的那句是进度（`32% Starting Java Language Server - https://maven.aliyun.com/…`）：导入中要上网核对 Maven 元数据，
                // 第 0 步那次 127 秒全花在这里 —— 状态栏得说在干什么，不能只转圈
                "Starting" => {
                    let ready = matches!(*self.phase.lock().unwrap_or_else(|e| e.into_inner()), Phase::Ready);
                    if !ready {
                        self.set(Phase::Importing(progress_text(p["message"].as_str().unwrap_or(""))));
                    }
                }
                _ => {}
            },
            "textDocument/publishDiagnostics" => {
                let Some(path) = p["uri"].as_str().and_then(uri::to_path) else { return };
                let raw: Vec<Value> = p["diagnostics"].as_array().map(|a| a.iter().take(DIAG_MAX).cloned().collect()).unwrap_or_default();
                self.raw_diags.lock().unwrap_or_else(|e| e.into_inner()).insert(path.clone(), raw);
                let (items, total) = diagnostics(&p["diagnostics"]);
                let _ = self.app.emit_to(
                    self.owner.as_str(),
                    "java-diagnostics",
                    crate::commands::JavaDiagnosticsDto { root: self.root.clone(), path, total: total as u32, items: items.into_iter().map(Into::into).collect() },
                );
            }
            _ => {}
        }
    }
}

/// jdtls 的进度那句去掉套话：`32% Starting Java Language Server - Project 'x'` → `32% Project 'x'`
fn progress_text(m: &str) -> String {
    let m = m.replace("Starting Java Language Server - ", "").replace("Starting Java Language Server", "");
    let m = m.trim();
    let mut out: String = m.chars().take(120).collect();
    if m.chars().count() > 120 {
        out.push('…');
    }
    out
}

fn did_open(path: &str, version: i64, text: &str) -> Value {
    json!({ "textDocument": { "uri": uri::from_path(path), "languageId": "java", "version": version, "text": text } })
}

fn num(v: &Value) -> u32 {
    v.as_u64().unwrap_or(0).min(u32::MAX as u64) as u32
}

/// `Location | Location[] | LocationLink[] | null` → 我们的位置列表（规范允许这几种，jdtls 回的是数组，别的服务器不一定）
/// hover 回的 `contents` 有三种样子（LSP 规范）：`MarkupContent { kind, value }`、`MarkedString`（字符串，或 `{ language, value }`）、
/// 以及它们的数组。jdtls 回的是数组：第一项 `{ language: "java", value: 签名 }`、第二项 Javadoc 转成的 Markdown 字符串。
/// 统一成一段 Markdown：带 language 的包成代码块，空的项丢掉
pub fn hover_markdown(r: &Value) -> String {
    fn one(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            Value::Object(o) => {
                let value = o.get("value").and_then(Value::as_str).unwrap_or("");
                match o.get("language").and_then(Value::as_str) {
                    Some(lang) if !value.trim().is_empty() => format!("```{lang}\n{value}\n```"),
                    _ => value.to_string(),
                }
            }
            _ => String::new(),
        }
    }
    let parts: Vec<String> = match r.get("contents") {
        Some(Value::Array(a)) => a.iter().map(one).collect(),
        Some(c) => vec![one(c)],
        None => Vec::new(),
    };
    parts.into_iter().filter(|s| !s.trim().is_empty()).collect::<Vec<_>>().join("\n\n")
}

/// 生成那几条请求都吃一个 `CodeActionParams`：jdtls 按这个位置找「光标在哪个类里」
fn gen_ctx(path: &str, line: u32, character: u32) -> Value {
    let at = json!({ "line": line, "character": character });
    json!({ "textDocument": { "uri": uri::from_path(path) }, "range": { "start": at, "end": at }, "context": { "diagnostics": [] } })
}

/// 第一步的回话 → 能选的几项。字段：名字 + 类型；可覆盖的方法：`名字(参数)` + 声明它的类，抽象的（没实现的）默认勾上（同 IDEA 的 Implement）；
/// getter / setter：只列还缺的那一种（jdtls 每个字段给两个标记：缺不缺 getter、缺不缺 setter）
pub fn gen_choices_of(what: &str, r: &Value) -> GenChoices {
    let arr = |v: &Value| v.as_array().cloned().unwrap_or_default();
    let field = |f: &Value| GenItem { label: f["name"].as_str().unwrap_or("").into(), detail: f["type"].as_str().unwrap_or("").into(), checked: true, raw: f.to_string() };
    match what {
        "tostring" | "equals" | "constructor" => {
            let note = if what == "tostring" && r["exists"].as_bool() == Some(true) {
                "这个类已经有 toString() 了：生成会替换掉它".to_string()
            } else if what == "equals" && !arr(&r["existingMethods"]).is_empty() {
                let m: Vec<String> = arr(&r["existingMethods"]).iter().filter_map(|x| x.as_str().map(str::to_owned)).collect();
                format!("这个类已经有 {} 了：生成会替换掉", m.join("、"))
            } else {
                String::new()
            };
            GenChoices { type_name: r["type"].as_str().unwrap_or("").into(), items: arr(&r["fields"]).iter().map(field).collect(), note }
        }
        "getter" | "setter" | "accessors" => {
            let items = arr(r)
                .iter()
                .filter(|f| match what {
                    "getter" => f["generateGetter"].as_bool() == Some(true),
                    "setter" => f["generateSetter"].as_bool() == Some(true),
                    _ => f["generateGetter"].as_bool() == Some(true) || f["generateSetter"].as_bool() == Some(true),
                })
                .map(|f| GenItem { label: f["fieldName"].as_str().unwrap_or("").into(), detail: f["typeName"].as_str().unwrap_or("").into(), checked: true, raw: f.to_string() })
                .collect();
            GenChoices { type_name: String::new(), items, note: String::new() }
        }
        "override" => {
            let items = arr(&r["methods"])
                .iter()
                .map(|m| {
                    let ps: Vec<String> = arr(&m["parameters"]).iter().filter_map(|p| p.as_str().map(str::to_owned)).collect();
                    GenItem {
                        label: format!("{}({})", m["name"].as_str().unwrap_or(""), ps.join(", ")),
                        detail: m["declaringClass"].as_str().unwrap_or("").into(),
                        checked: m["unimplemented"].as_bool() == Some(true),
                        raw: m.to_string(),
                    }
                })
                .collect();
            GenChoices { type_name: r["type"].as_str().unwrap_or("").into(), items, note: String::new() }
        }
        _ => GenChoices { type_name: String::new(), items: Vec::new(), note: String::new() },
    }
}

/// 第二步的请求和参数。getter / setter：把每一项的两个标记改成「只生成这一种」—— jdtls 照标记生成，不改的话选了 getter 也会顺带生成 setter
pub fn gen_params(what: &str, ctx: Value, picked: Vec<Value>, supers: Option<Value>) -> Result<(&'static str, Value), String> {
    Ok(match what {
        "tostring" => ("java/generateToString", json!({ "context": ctx, "fields": picked })),
        "equals" => ("java/generateHashCodeEquals", json!({ "context": ctx, "fields": picked, "regenerate": true })),
        "constructor" => ("java/generateConstructors", json!({ "context": ctx, "constructors": supers.into_iter().collect::<Vec<_>>(), "fields": picked })),
        "getter" | "setter" | "accessors" => {
            let accessors: Vec<Value> = picked
                .into_iter()
                .map(|mut f| {
                    if what == "getter" {
                        f["generateSetter"] = json!(false);
                    }
                    if what == "setter" {
                        f["generateGetter"] = json!(false);
                    }
                    f
                })
                .collect();
            ("java/generateAccessors", json!({ "context": ctx, "accessors": accessors }))
        }
        "override" => ("java/addOverridableMethods", json!({ "context": ctx, "overridableMethods": picked })),
        _ => return Err(format!("不认识的生成：{what}")),
    })
}

/// `CallHierarchyItem` / `TypeHierarchyItem` → 节点。`site` 由调用方给（调用方那一层的 `fromRanges`）。位置用 `selectionRange`（名字那一段）
fn hier_node(i: &Value, site: Option<(u32, u32)>) -> Option<HierNode> {
    let name = i["name"].as_str()?.to_string();
    let range = if i["selectionRange"].is_object() { &i["selectionRange"] } else { &i["range"] };
    let at = locations(&json!({ "uri": i["uri"], "range": range })).into_iter().next()?;
    Some(HierNode { name, detail: i["detail"].as_str().unwrap_or("").to_string(), kind: i["kind"].as_u64().unwrap_or(0).min(255) as u8, at, site, raw: i.to_string() })
}

/// 下一层的回话 → 节点。调用层级回的是包了一层的（`{from, fromRanges}` / `{to, fromRanges}`），类型层级直接是节点。
/// 「谁调了它」那一层带上第一处调用的位置（`fromRanges` 在 `from` 那个文件里）—— 双击要跳到调用那一行，不是那个方法的开头（同 IDEA）。
/// 「它调了谁」的 `fromRanges` 在**我们自己**的文件里，跳的是被调的那个方法，不带
pub fn hier_children_of(r: &Value, dir: &str) -> Vec<HierNode> {
    let Some(a) = r.as_array() else { return Vec::new() };
    a.iter()
        .filter_map(|c| match dir {
            "in" => {
                let s = &c["fromRanges"][0]["start"];
                hier_node(&c["from"], s.is_object().then(|| (num(&s["line"]), num(&s["character"]))))
            }
            "out" => hier_node(&c["to"], None),
            _ => hier_node(c, None),
        })
        .collect()
}

/// `InlayHint[] | null` → 几条。`label` 可以是字符串、也可以是几段（`InlayHintLabelPart[]`，拼起来）；空的丢掉。
/// 是不是参数名：`kind` 2，或者没给 `kind`、标签以 `:` 结尾 —— jdtls 1.61 给参数名提示时**不带 kind**（同一次探针看到的）
pub fn inlays_of(r: &Value) -> Vec<Inlay> {
    r.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|h| {
                    let label = match &h["label"] {
                        Value::String(s) => s.clone(),
                        Value::Array(parts) => parts.iter().filter_map(|p| p["value"].as_str()).collect(),
                        _ => return None,
                    };
                    let label = label.trim().to_string();
                    let param = match h["kind"].as_u64() {
                        Some(k) => k == 2,
                        None => label.ends_with(':'),
                    };
                    (!label.is_empty()).then(|| Inlay { line: num(&h["position"]["line"]), character: num(&h["position"]["character"]), label, param })
                })
                .take(INLAY_MAX)
                .collect()
        })
        .unwrap_or_default()
}

/// `DocumentHighlight[] | null` → 几处。`kind` 3 = 写，1（文本）/ 2（读）/ 没给都算读；超过 [`HL_MAX`] 的不要（画几千个底色没有意义）
pub fn highlights_of(r: &Value) -> Vec<Highlight> {
    r.as_array()
        .map(|a| {
            a.iter()
                .take(HL_MAX)
                .map(|h| Highlight {
                    line: num(&h["range"]["start"]["line"]),
                    character: num(&h["range"]["start"]["character"]),
                    end_line: num(&h["range"]["end"]["line"]),
                    end_character: num(&h["range"]["end"]["character"]),
                    write: h["kind"].as_u64() == Some(3),
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `SymbolInformation[]` → 类型。位置照 [`locations`] 读（jar 里的是 `jdt://`、`path` 为空）；名字是空的丢掉（匿名类）
pub fn type_hits(r: &Value) -> Vec<TypeHit> {
    r.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|s| {
                    let name = s["name"].as_str().filter(|n| !n.is_empty())?.to_string();
                    let loc = locations(&s["location"]).into_iter().next()?;
                    Some(TypeHit { name, container: s["containerName"].as_str().unwrap_or("").to_string(), kind: s["kind"].as_u64().unwrap_or(5).min(255) as u8, loc })
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn locations(r: &Value) -> Vec<Location> {
    let items: Vec<&Value> = match r {
        Value::Array(a) => a.iter().collect(),
        Value::Object(_) => vec![r],
        _ => vec![],
    };
    items
        .into_iter()
        .filter_map(|l| {
            let (u, range) = match l.get("targetUri") {
                Some(u) => (u.as_str()?, l.get("targetSelectionRange").or_else(|| l.get("targetRange"))?),
                None => (l["uri"].as_str()?, &l["range"]),
            };
            let path = uri::to_path(u);
            let label = match &path {
                Some(p) => p.rsplit('/').next().unwrap_or(p).to_string(),
                None => uri::jdt_label(u).map(|(name, _)| name).unwrap_or_else(|| u.to_string()),
            };
            Some(Location {
                uri: u.to_string(),
                path,
                label,
                line: num(&range["start"]["line"]),
                character: num(&range["start"]["character"]),
                end_line: num(&range["end"]["line"]),
                end_character: num(&range["end"]["character"]),
            })
        })
        .collect()
}

/// 诊断列表，最多 [`DIAG_MAX`] 条（先错误、再警告，截掉的是最不要紧的那些），返回 `(留下的, 真条数)`
fn diagnostics(v: &Value) -> (Vec<Diagnostic>, usize) {
    let mut all: Vec<Diagnostic> = v
        .as_array()
        .map(|a| {
            a.iter()
                .map(|d| Diagnostic {
                    line: num(&d["range"]["start"]["line"]),
                    character: num(&d["range"]["start"]["character"]),
                    end_line: num(&d["range"]["end"]["line"]),
                    end_character: num(&d["range"]["end"]["character"]),
                    severity: d["severity"].as_u64().unwrap_or(1).clamp(1, 4) as u8,
                    message: d["message"].as_str().unwrap_or("").to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    let total = all.len();
    all.sort_by_key(|d| (d.severity, d.line, d.character));
    all.truncate(DIAG_MAX);
    (all, total)
}

fn edit_of(range: &Value, text: &str) -> Edit {
    Edit {
        line: num(&range["start"]["line"]),
        character: num(&range["start"]["character"]),
        end_line: num(&range["end"]["line"]),
        end_character: num(&range["end"]["character"]),
        text: text.to_string(),
    }
}

/// `CompletionItem[] | CompletionList | null` → `(前 COMPL_MAX 项, 是不是不完整, 真项数)`。不完整 = 服务器自己截了，接着敲要重新问
pub fn completion_items(r: &Value) -> (Vec<CompletionItem>, bool, usize) {
    let (arr, incomplete) = match r {
        Value::Array(a) => (a.as_slice(), false),
        Value::Object(_) => (r["items"].as_array().map(Vec::as_slice).unwrap_or(&[]), r["isIncomplete"].as_bool().unwrap_or(false)),
        _ => (&[][..], false),
    };
    let total = arr.len();
    let items = arr
        .iter()
        .take(COMPL_MAX)
        .map(|it| {
            let te = &it["textEdit"];
            // InsertReplaceEdit（`insert` / `replace` 两个范围）：取 `replace` —— 光标在词中间时把后半截也换掉，IDEA 回车补全就是这样
            let edit = te["newText"].as_str().map(|t| edit_of(if te["range"].is_object() { &te["range"] } else { &te["replace"] }, t));
            CompletionItem {
                label: it["label"].as_str().unwrap_or("").to_string(),
                kind: it["kind"].as_u64().unwrap_or(1) as u32,
                detail: it["labelDetails"]["detail"].as_str().unwrap_or("").to_string(),
                description: it["labelDetails"]["description"].as_str().or(it["detail"].as_str()).unwrap_or("").to_string(),
                snippet: it["insertTextFormat"].as_u64() == Some(2),
                edit,
                sort: it["sortText"].as_str().unwrap_or("").to_string(),
                raw: it.to_string(),
            }
        })
        .collect();
    (items, incomplete || total > COMPL_MAX, total)
}

/// 参数提示的一个重载：整句签名，每个参数在签名里的位置（字符下标，从 0 数、左闭右开）
#[derive(Debug, PartialEq)]
pub struct Signature {
    pub label: String,
    pub params: Vec<(u32, u32)>,
    pub doc: String,
}

#[derive(Debug, PartialEq)]
pub struct Signatures {
    pub list: Vec<Signature>,
    /// jdtls 认为最可能是哪个重载
    pub active: u32,
    /// 光标在第几个参数上
    pub param: u32,
    /// 真有几个重载（`list` 最多 [`SIG_MAX`] 个）
    pub total: u32,
}

/// 一次最多给几个重载：`StringBuilder.insert` 有 13 个，再多就该接着敲字让它筛了
pub const SIG_MAX: usize = 20;

/// `SignatureHelp | null` → 前端要的。参数位置：给了 `[开始, 结束]` 就用（UTF-16 下标，换成字符下标），
/// 给的是文字就**从括号后面按顺序往后找** —— 声明了 `labelOffsetSupport` 它照样给文字（探针实测）；不按顺序找的话，
/// `copy(int from, int to)` 里两个 `int` 会都亮第一个
pub fn signatures(r: &Value) -> Signatures {
    let all = r["signatures"].as_array().cloned().unwrap_or_default();
    let list = all
        .iter()
        .take(SIG_MAX)
        .map(|s| {
            let label = s["label"].as_str().unwrap_or("").to_string();
            let chars: Vec<char> = label.chars().collect();
            let mut from = label.find('(').map_or(0, |b| label[..b].chars().count() + 1);
            let params = s["parameters"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|p| match &p["label"] {
                    Value::Array(se) => {
                        let u = |i: usize| se.get(i).and_then(Value::as_u64).unwrap_or(0) as usize;
                        let to_char = |n: usize| String::from_utf16_lossy(&label.encode_utf16().take(n).collect::<Vec<_>>()).chars().count() as u32;
                        Some((to_char(u(0)), to_char(u(1))))
                    }
                    Value::String(t) => {
                        let needle: Vec<char> = t.chars().collect();
                        let at = (from..chars.len().saturating_sub(needle.len()) + 1).find(|&i| chars[i..].starts_with(&needle))?;
                        from = at + needle.len();
                        Some((at as u32, from as u32))
                    }
                    _ => None,
                })
                .collect();
            let doc = s["documentation"].as_str().or(s["documentation"]["value"].as_str()).unwrap_or("");
            let doc = match doc.char_indices().nth(DOC_MAX) {
                Some((i, _)) => format!("{}…", &doc[..i]),
                None => doc.to_string(),
            };
            Signature { label, params, doc }
        })
        .collect::<Vec<_>>();
    let active = r["activeSignature"].as_u64().unwrap_or(0) as usize;
    let active = if active < list.len() { active } else { 0 };
    // 每个重载自己可以带 activeParameter（LSP 3.16），没带用整体的
    let param = all.get(active).and_then(|s| s["activeParameter"].as_u64()).or(r["activeParameter"].as_u64()).unwrap_or(0) as u32;
    Signatures { list, active: active as u32, param, total: all.len() as u32 }
}

/// resolve 的回包 → 要一起做的改动 + 说明文字（`string | MarkupContent`，截在 [`DOC_MAX`] 字、切在字符边界上）
pub fn resolved(r: &Value) -> Resolved {
    let edits = r["additionalTextEdits"]
        .as_array()
        .map(|a| a.iter().filter_map(|e| e["newText"].as_str().map(|t| edit_of(&e["range"], t))).collect())
        .unwrap_or_default();
    let doc = r["documentation"].as_str().or(r["documentation"]["value"].as_str()).unwrap_or("");
    let doc = match doc.char_indices().nth(DOC_MAX) {
        Some((i, _)) => format!("{}…", &doc[..i]),
        None => doc.to_string(),
    };
    Resolved { edits, doc }
}

/// 一条报错和这一段（按行）有没有重叠。按行不按列：工作区构建出来的那批报错只有行号、列是 0（探针实测），按列判会漏
fn overlaps(d: &Value, r: &Edit) -> bool {
    let (a, b) = (num(&d["range"]["start"]["line"]), num(&d["range"]["end"]["line"]));
    a <= r.end_line && b >= r.line
}

/// import 候选的包排在第几（越小越前）：`java.*` / `javax.*` 先，项目和第三方居中，最后是 JDK 内部的（`com.sun.*`、`sun.*`、`jdk.*`）和
/// 桌面图形界面的（`java.awt*`、`javax.swing*`）。jdtls 按字母排：`List` 的候选是 `com.sun.tools.javac.util`、`java.awt`、`java.util`（探针实测），
/// 真 .app 验收里第一条是 `java.awt.List` —— 写后端的人几乎不会要它。IDEA 靠使用统计把它们压下去，我们没有统计，按包分三档
fn import_rank(title: &str) -> u8 {
    let Some(pkg) = title.strip_prefix("Import '").and_then(|t| t.split_once("' (")).map(|(_, p)| p.trim_end_matches(')')) else { return 1 };
    if ["com.sun.", "sun.", "jdk.", "java.awt", "javax.swing", "java.applet"].iter().any(|p| pkg.starts_with(p)) {
        2
    } else if pkg.starts_with("java.") || pkg.starts_with("javax.") {
        0
    } else {
        1
    }
}

/// codeAction 的回包 → 列给人看的那些。去掉要弹交互框的（`source.generate.*`：VS Code 里是让人勾字段的对话框，这一版没有）；
/// 修报错的（quickfix）排前面，其中 import 按 [`import_rank`] 排；最多 [`ACTIONS_MAX`] 条
pub fn actions_of(r: &Value) -> Vec<Action> {
    let mut out: Vec<(u8, u8, usize, Action)> = r
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .filter_map(|(i, x)| {
            let title = x["title"].as_str()?.to_string();
            let kind = x["kind"].as_str().unwrap_or("").to_string();
            if kind.starts_with("source.generate") {
                return None;
            }
            let group = if kind == "quickfix" { 0 } else if kind.starts_with("source") { 2 } else { 1 };
            Some((group, import_rank(&title), i, Action { title, kind, raw: x.to_string() }))
        })
        .collect();
    out.sort_by_key(|(g, r, i, _)| (*g, *r, *i));
    out.into_iter().take(ACTIONS_MAX).map(|(_, _, _, a)| a).collect()
}

/// prepareRename 的回包：`Range | { range, placeholder } | { defaultBehavior } | null`。
/// `defaultBehavior`（让客户端自己按光标底下的词算）这里当「不能改」：jdtls 不回这种，回了也说明它没把握
pub fn rename_target(r: &Value) -> Option<RenameTarget> {
    let (range, placeholder) = if r["start"].is_object() {
        (r, None)
    } else if r["range"].is_object() {
        (&r["range"], r["placeholder"].as_str().map(str::to_owned))
    } else {
        return None;
    };
    Some(RenameTarget { range: edit_of(range, ""), placeholder })
}

/// jdtls 的 WorkspaceEdit → [`RenamePlan`]。认 `changes`（uri → 改动）和 `documentChanges`（按次序的改动 + 改名），两种可能同时给（实测 `changes` 是空的）。
/// 拒绝的（整个报错，一处都不改）：新建 / 删除文件（这一版不做；我们向它声明了支持，是因为不声明它连改名都不给）、改到 jar 里、不是 file:// 的地址。
/// **先改名后又改它**的（改动指向改名后的新路径）换回原路径：执行时是先改全部内容、最后改名
pub fn workspace_edit(r: &Value) -> Result<RenamePlan, String> {
    let mut plan = RenamePlan::default();
    let path_of = |u: &str| -> Result<String, String> {
        uri::to_path(u).ok_or_else(|| format!("改动落在 {u}（不在工作区的文件里，比如 jar），这一版不做"))
    };
    let mut new_to_old: HashMap<String, String> = HashMap::new();
    let add = |plan: &mut RenamePlan, path: String, edits: Vec<Edit>| {
        match plan.files.iter_mut().find(|(p, _)| *p == path) {
            Some((_, v)) => v.extend(edits),
            None => plan.files.push((path, edits)),
        }
    };
    let edits_of = |a: &Value| -> Vec<Edit> {
        a.as_array().map(|a| a.iter().filter_map(|e| e["newText"].as_str().map(|t| edit_of(&e["range"], t))).collect()).unwrap_or_default()
    };
    if let Some(m) = r["changes"].as_object() {
        for (u, es) in m {
            let p = path_of(u)?;
            add(&mut plan, p, edits_of(es));
        }
    }
    for c in r["documentChanges"].as_array().map(Vec::as_slice).unwrap_or(&[]) {
        match c["kind"].as_str() {
            None => {
                let p = path_of(c["textDocument"]["uri"].as_str().unwrap_or(""))?;
                let p = new_to_old.get(&p).cloned().unwrap_or(p);
                add(&mut plan, p, edits_of(&c["edits"]));
            }
            Some("rename") => {
                let from = path_of(c["oldUri"].as_str().unwrap_or(""))?;
                let to = path_of(c["newUri"].as_str().unwrap_or(""))?;
                new_to_old.insert(to.clone(), from.clone());
                plan.renames.push((from, to));
            }
            Some(k) => return Err(format!("这次改动要{}文件，这一版还不支持（只改已有文件的内容、给文件改名）", if k == "create" { "新建" } else if k == "delete" { "删除" } else { k })),
        }
    }
    plan.files.retain(|(_, e)| !e.is_empty());
    Ok(plan)
}

/// 改名的执行（docs/IDEA.md 第 14 节）。复用跨文件替换的落盘（重读核对指纹、写撤销日志、两段提交、开着的改编辑器），次序是这个函数存在的理由：
/// 1. 先查改名做不做得了（新名字被占、跨目录）—— 内容改完了名字却改不了，类名和文件名对不上就编译不过
/// 2. 按 jdtls 给的位置建扫描结果。**有一个文件改不了就整个不做**：查找替换跳过一个文件是常态，改名少改一处就是编译错误
/// 3. 两段提交改内容；执行时被跳过的（建完扫描到执行之间被改了）→ 撤回去、报错
/// 4. 最后改名；改名失败 → 内容也撤回去
///
/// `dirty`：开着、有未保存改动的标签（它们只改编辑器，同查找替换）。成了的话这一次就是「最近一次替换」，撤销卡片照样能撤（名字一起改回去）
pub fn apply_rename(plan: &RenamePlan, root: &Path, journal: &Path, dirty: &HashMap<PathBuf, String>) -> Result<replacesvc::Outcome, String> {
    if plan.files.is_empty() && plan.renames.is_empty() {
        return Err("语言服务器说这里没有要改的".into());
    }
    let renames: Vec<(PathBuf, PathBuf)> = plan.renames.iter().map(|(f, t)| (PathBuf::from(f), PathBuf::from(t))).collect();
    replacesvc::check_renames(&renames)?;
    let files = plan
        .files
        .iter()
        .map(|(p, es)| {
            let es = es
                .iter()
                .map(|e| replacesvc::LineEdit { line: e.line, character: e.character, end_line: e.end_line, end_character: e.end_character, text: e.text.clone() })
                .collect();
            (PathBuf::from(p), es)
        })
        .collect();
    let scan = replacesvc::scan_edits(root, files, dirty)?;
    if !scan.skipped.is_empty() {
        let head: Vec<String> = scan.skipped.iter().take(3).map(|k| format!("{}：{}", k.rel, k.why.text())).collect();
        return Err(format!("有 {} 个文件改不了，整个没改（少改一处就编译不过）—— {}", scan.skipped.len(), head.join("；")));
    }
    let picks: Vec<replacesvc::Pick> = scan.files.iter().map(|f| replacesvc::Pick { rel: f.rel.clone(), hits: (0..f.hits.len()).collect() }).collect();
    let o = replacesvc::ApplyOpts { journal, replacement: "", picks: &picks, dirty_now: dirty, allow_no_undo: false, crash_after: None, journal_cap: None };
    let mut out = replacesvc::apply(&scan, &o).map_err(|e| match e {
        replacesvc::ApplyError::Pending => "上一次替换中断了还没处理：先在启动时的卡片上处理它".to_string(),
        replacesvc::ApplyError::TooBigForUndo { bytes } => format!("这次改名要改的文件太大（{} MB），撤销不了，没改", bytes >> 20),
        replacesvc::ApplyError::Truncated => "要改的地方太多，没改".to_string(),
        replacesvc::ApplyError::Failed { rel, msg } => if rel.is_empty() { msg } else { format!("{rel}：{msg}") },
    })?;
    // 撤回去只碰盘上的：只改在编辑器里的那些，改动还没交给前端（这里报错就不交），本来就没生效
    let roll_back = |why: String| -> String {
        match replacesvc::undo(journal, &HashMap::new()) {
            Ok(_) => format!("{why}，改了的已经撤回去了"),
            Err(e) => format!("{why}；撤回去也失败了：{e}（替换卡片上还能再撤一次）"),
        }
    };
    if !out.skipped.is_empty() {
        return Err(roll_back(format!("{} 刚刚被改过", out.skipped[0].rel)));
    }
    out.renamed = replacesvc::rename_files(journal, &renames).map_err(roll_back)?;
    Ok(out)
}

/// 服务器问我们的。只答真问得到的几种；`workspace/applyEdit` 第 2 步不接（只读智能），答「没改」
fn answer(method: &str, p: &Value) -> Result<Value, (i64, String)> {
    match method {
        "workspace/configuration" => Ok(Value::Array(vec![Value::Null; p["items"].as_array().map_or(0, Vec::len)])),
        "client/registerCapability" | "client/unregisterCapability" | "window/workDoneProgress/create" => Ok(Value::Null),
        "workspace/applyEdit" => Ok(json!({ "applied": false, "failureReason": "这一版只读，不改代码" })),
        "window/showMessageRequest" => Ok(Value::Null),
        _ => Err((-32601, format!("lite-ide 不支持 {method}"))),
    }
}

fn emit_status(app: &AppHandle, owner: &str, s: &Status) {
    let _ = app.emit_to(owner, "java-status", crate::commands::JavaStatusDto::from(s));
}

/// 是不是 Maven / Gradle 项目：根上或者第一层子目录里有构建文件（同 Maven 工具窗的判据，IDEA.md 第 1 步）。
/// 不是的话打开一个 .java 也不起 jdtls：随手看一眼别处的 Java 文件不该起一个 1 GB 的 JVM（第 3 节「不打断」）
pub fn is_build_root(root: &Path) -> bool {
    const FILES: [&str; 5] = ["pom.xml", "build.gradle", "build.gradle.kts", "settings.gradle", "settings.gradle.kts"];
    let has = |d: &Path| FILES.iter().any(|f| d.join(f).is_file());
    if has(root) {
        return true;
    }
    std::fs::read_dir(root)
        .map(|rd| rd.flatten().any(|e| e.file_type().is_ok_and(|t| t.is_dir()) && !e.file_name().to_string_lossy().starts_with('.') && has(&e.path())))
        .unwrap_or(false)
}

/// 起 jdtls 要的东西都找齐。缺哪样就说缺哪样、怎么补 —— 这条会原样出现在状态栏的提示里
struct Found {
    java: PathBuf,
    /// 给人看的：用的是哪个 Java（内置的、还是机器上哪个 JDK）
    java_label: String,
    layout: jdtls::Layout,
    lombok: Option<PathBuf>,
    workspace: PathBuf,
    /// 调试插件（java-debug，内置在 `java/java-debug/`）：作为 bundle 交给 jdtls。开发构建没有 —— 那时调试照实说「没带」，别的照常
    debug_jar: Option<PathBuf>,
}

/// 内置的那份在哪：`.app/Contents/Resources/java/`（scripts/java-bundle.sh 准备、src-tauri/tauri.java.json 打进包）。
/// 开发构建（`pnpm app`、`cargo test`）没有它
fn bundled(app: &AppHandle) -> Option<PathBuf> {
    let d = app.path().resource_dir().ok()?.join("java");
    d.is_dir().then_some(d)
}

/// 热替换的代理（docs/IDEA.md 第 26 节，scripts/java-bundle.sh 编的）。开发构建没有 —— 那时调试照常、只是不能热替换
pub fn hotswap_jar(app: &AppHandle) -> Option<PathBuf> {
    bundled(app).map(|d| d.join("hotswap/lite-hotswap.jar")).filter(|p| p.is_file())
}

/// 内置运行时的目录名：一个架构一份（universal 包里两份都有，起的时候挑这台机器的）
fn runtime_dir_name() -> String {
    format!("runtime-{}", std::env::consts::ARCH)
}

/// 目录里那一个 java-debug 的 jar（scripts/java-bundle.sh 钉的版本，文件名带版本号，不写死）
fn debug_jar(dir: &Path) -> Option<PathBuf> {
    std::fs::read_dir(dir).ok()?.flatten().map(|e| e.path()).find(|p| p.extension().is_some_and(|x| x == "jar"))
}

fn find(app: &AppHandle, root: &Path) -> Result<Found, String> {
    let user = std::env::var_os("HOME").map(PathBuf::from).ok_or("找不到用户目录（没有 HOME）")?;
    let data = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?;
    let inside = bundled(app);
    // jdtls：设置里指的（想用自己那份的人）→ 内置的 → 都没有（开发构建）就照实说
    let set = app.state::<crate::state::AppState>().settings.view().0.java_jdtls_home;
    let layout = if !set.trim().is_empty() {
        jdtls::layout(Path::new(set.trim())).map_err(|e| format!("settings.json 的 java.jdtlsHome 指的不是 jdtls（{e}）"))?
    } else if let Some(d) = &inside {
        jdtls::layout(&d.join("jdtls")).map_err(|e| format!("内置的 jdtls 坏了（{e}）：重装 lite-ide"))?
    } else {
        return Err("这个构建没带内置的 jdtls（开发构建）：用 pnpm app:bundle 打的包，或者在 settings.json 写 java.jdtlsHome".into());
    };
    // 跑 jdtls 的 Java：内置的 → 机器上的 JDK 21+（开发构建、或者内置的那份被删了）
    let rt = inside.as_ref().map(|d| d.join(runtime_dir_name())).filter(|r| r.join("bin/java").is_file());
    let (java, java_label) = match rt {
        Some(r) => (r, "Java 21（内置）".to_string()),
        None => {
            let (v, home) = jdtls::find_jdk(&jdtls::default_jdk_dirs(&user), jdtls::MIN_JAVA)
                .ok_or("没有能跑 jdtls 的 Java：这个构建没带内置的 Java 21，机器上也没有 JDK 21+（brew install --cask temurin@21）")?;
            let label = format!("JDK {v}（{}）", home.display());
            (home, label)
        }
    };
    // 本地仓库跟着设置走（maven.localRepository / maven.settingsFile，SETTINGS.md 13.4）：和跑 mvn 的是同一个
    let lombok = jdtls::lombok_jar(&crate::maven::local_repo(&user));
    let workspace = jdtls::workspace_dir(&data.join("jdtls-ws"), root);
    jdtls::prepare_config(&layout, &workspace).map_err(|e| format!("建不了 jdtls 的缓存目录 {}：{e}", workspace.display()))?;
    let debug_jar = inside.as_ref().and_then(|d| debug_jar(&d.join("java-debug")));
    Ok(Found { java, java_label, layout, lombok, workspace, debug_jar })
}

/// 应用里全部的 jdtls：项目根 → 实例，记在打开它的窗口名下
#[derive(Default)]
pub struct Java {
    all: Owned<String, Arc<Instance>>,
    janitor: OnceLock<()>,
    /// 没起起来的项目最近一次的状态（设置指错了、开发构建没带 jdtls…）：它们在 `all` 里没有实例，`status` 要从这儿答 ——
    /// 不然晚挂上的前端问一句「现在什么状态」拿到的是「什么都没有」（验收撞上的）
    last: std::sync::Mutex<HashMap<String, Status>>,
}

impl Java {
    pub fn get(&self, root: &str) -> Option<Arc<Instance>> {
        self.all.get(&root.to_string(), Arc::clone).filter(|i| i.alive())
    }

    pub fn status(&self, root: &str) -> Option<Status> {
        self.all
            .get(&root.to_string(), |i| i.status())
            .or_else(|| self.last.lock().unwrap_or_else(|e| e.into_inner()).get(root).cloned())
    }

    /// 这个项目的 jdtls，没有（或者上一个崩了 / 收掉了）就起一个。**不等它起好**：进程起来、登记、后台 initialize，立刻返回。
    /// 起不来给 `Err(状态)`（`Failed`，带原因），同时发给前端
    pub fn ensure(&self, app: &AppHandle, owner: &str, root: &str) -> Result<Arc<Instance>, Status> {
        self.janitor.get_or_init(|| spawn_janitor(app.clone()));
        if let Some(i) = self.get(root) {
            return Ok(i);
        }
        let say = |phase: Phase| {
            let s = Status { root: root.to_string(), phase, detail: String::new() };
            emit_status(app, owner, &s);
            self.last.lock().unwrap_or_else(|e| e.into_inner()).insert(root.to_string(), s.clone());
            s
        };
        let fail = |why: String| say(Phase::Failed(why));
        let f = find(app, Path::new(root)).map_err(fail)?;
        let detail = format!(
            "{} · {}",
            f.java_label,
            match &f.lombok {
                Some(j) => format!("Lombok {}", j.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                None => "没找到 Lombok（用 Lombok 的项目会报假错）".into(),
            }
        );
        let xmx = format!("{}m", app.state::<crate::state::AppState>().settings.current().java_jdtls_memory);
        let spec = jdtls::spec(&jdtls::Launch {
            java_home: &f.java,
            layout: &f.layout,
            workspace: &f.workspace,
            lombok: f.lombok.as_deref(),
            xmx: &xmx,
            root: Path::new(root),
        });
        let shared = Arc::new(Shared {
            root: root.to_string(),
            owner: owner.to_string(),
            app: app.clone(),
            phase: Mutex::new(Phase::Starting),
            changed: Condvar::new(),
            detail,
            docs: Mutex::new(Docs { open: HashMap::new(), pending: HashMap::new(), synced: false }),
            last_used: Mutex::new(Instant::now()),
            raw_diags: Mutex::new(HashMap::new()),
            debuggable: f.debug_jar.is_some(),
        });
        let on_notify = Arc::clone(&shared);
        let on_exit = Arc::clone(&shared);
        let srv = Server::start(
            &spec,
            Box::new(answer),
            Box::new(move |m, p| on_notify.on_notify(m, p)),
            Box::new(move |e| {
                if !e.stopped {
                    let tail: Vec<&str> = e.stderr_tail.lines().rev().take(6).collect();
                    let tail: Vec<&str> = tail.into_iter().rev().collect();
                    applog::write(applog::Level::Warn, "java", &format!("jdtls 自己退了（{}）：{}", e.status, tail.join(" | ")));
                    on_exit.set(Phase::Failed(format!("Java 语言服务器退出了（{}）{}", e.status, tail.last().map(|l| format!("：{l}")).unwrap_or_default())));
                }
            }),
        )
        .map_err(|e| fail(format!("起不了 jdtls：{e}")))?;
        let inst = Arc::new(Instance { shared: Arc::clone(&shared), srv, watch: Mutex::new(None), holds: Default::default() });
        // 盘上的变化告诉 jdtls（`on_disk_changes`）。拿弱引用：监听活在实例里，强引用就是互相攥着、谁也收不掉。起不来只记一句，Java 智能照常
        let weak = Arc::downgrade(&inst);
        match fsservice::watch::watch_paths(root, java_relevant, WATCH_CAP, move |b| {
            if let Some(i) = weak.upgrade() {
                i.on_disk_changes(b);
            }
        }) {
            Ok(w) => *inst.watch.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
            Err(e) => applog::write(applog::Level::Warn, "java", &format!("起不了文件监听（盘上的改动 jdtls 不会知道）：{e}")),
        }
        self.last.lock().unwrap_or_else(|e| e.into_inner()).remove(root);
        if let Some(old) = self.all.insert(owner, root.to_string(), Arc::clone(&inst)) {
            std::thread::spawn(move || old.stop());
        }
        emit_status(app, owner, &shared.status());
        let init = Arc::clone(&inst);
        let mvn_settings = crate::maven::cfg().settings_file;
        let mut opts = jdtls::init_options(&crate::jdks::all(), Some(&mvn_settings));
        if let Some(j) = &f.debug_jar {
            opts["bundles"] = json!([j]);
        }
        std::thread::Builder::new()
            .name("jdtls-init".into())
            .spawn(move || {
                let root_uri = uri::from_path(&init.shared.root);
                let name = Path::new(&init.shared.root).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match init.srv.initialize(&root_uri, &name, jdtls::capabilities(), opts, INIT) {
                    Ok(_) => {
                        init.shared.set(Phase::Importing(String::new()));
                        // initialize 之前开的那些现在补发。在 docs 锁里补、补完置 synced：这期间来的 open / change 要么在补之前登记（会被补到），
                        // 要么等锁、看到 synced 自己发 —— 不会漏，也不会发两遍
                        let mut d = init.shared.docs.lock().unwrap_or_else(|e| e.into_inner());
                        let pending = std::mem::take(&mut d.pending);
                        for (path, v) in &d.open {
                            if let Some(text) = pending.get(path) {
                                let _ = init.srv.notify("textDocument/didOpen", did_open(path, *v, text));
                            }
                        }
                        d.synced = true;
                    }
                    Err(e) => init.shared.set(Phase::Failed(format!("jdtls 没有完成初始化：{e}"))),
                }
            })
            .map_err(|e| fail(format!("起不了线程：{e}")))?;
        Ok(inst)
    }

    /// 重起（状态栏「重起」按钮）：停掉旧的、起新的。开着的文档由前端重新 `open` 一遍（它手上的才是没保存的那份，这里不读盘）
    pub fn restart(&self, app: &AppHandle, owner: &str, root: &str) -> Result<(), String> {
        if let Some(old) = self.all.remove(&root.to_string()) {
            old.stop();
        }
        self.ensure(app, owner, root).map(|_| ()).map_err(|s| match s.phase {
            Phase::Failed(m) => m,
            _ => String::new(),
        })
    }

    /// 窗口关了：它名下的在后台按协议收尾（关窗口不用等它们）
    pub fn release(&self, owner: &str) {
        for (_, i) in self.all.take_owner(owner) {
            std::thread::spawn(move || i.stop());
        }
    }

    /// 退出流程（`winctl::quit`）：全部按协议收尾，**并行**，最多两个 `GRACE`。还没走的在 [`Java::kill_all`] 里强杀
    pub fn stop_all(&self) -> Vec<std::thread::JoinHandle<()>> {
        self.all.map(Arc::clone).into_iter().map(|i| std::thread::spawn(move || i.stop())).collect()
    }

    /// 进程真要结束了（`RunEvent::Exit`）。jdtls 在自己的进程组里，不会跟着我们死；`initialize` 里给了我们的 pid，
    /// jdtls 自己也会盯着、我们没了它就退 —— 但那是它的行为，这里不指望
    pub fn kill_all(&self) {
        for i in self.all.drain() {
            i.srv.kill();
        }
    }

    fn reap_idle(&self) {
        let idle: Vec<String> = self.all.map(|i| (i.shared.root.clone(), i.idle())).into_iter().filter(|(_, idle)| *idle).map(|(r, _)| r).collect();
        for root in idle {
            if let Some(i) = self.all.remove(&root) {
                applog::write(applog::Level::Info, "java", &format!("{root} 闲了 {} 分钟，收掉 jdtls", IDLE.as_secs() / 60));
                i.stop();
            }
        }
    }
}

/// 每处那一行的文字（查用法的列表里给人看）。每个文件读一次；读不了、太大（> 4MB，多半是生成的）、jar 里的给空串。
/// 读的是盘上那份：没保存的改动不在里面，行号可能偏一两行 —— 列表只是预览，点过去跳的位置是 jdtls 给的，对的是它那份
pub fn line_texts(locs: &[Location]) -> Vec<String> {
    let mut files: HashMap<&str, Option<Vec<String>>> = HashMap::new();
    locs.iter()
        .map(|l| {
            let Some(p) = l.path.as_deref() else { return String::new() };
            let lines = files.entry(p).or_insert_with(|| {
                let small = std::fs::metadata(p).map(|m| m.len() <= 4 << 20).unwrap_or(false);
                small.then(|| std::fs::read_to_string(p).ok()).flatten().map(|t| t.lines().map(str::to_owned).collect())
            });
            let line = lines.as_ref().and_then(|ls| ls.get(l.line as usize)).map(|s| s.trim()).unwrap_or("");
            line.chars().take(240).collect()
        })
        .collect()
}

/// 一分钟看一次谁闲够了。整个进程一条，第一次起 jdtls 时才起
fn spawn_janitor(app: AppHandle) {
    let _ = std::thread::Builder::new().name("jdtls-idle".into()).spawn(move || loop {
        std::thread::sleep(Duration::from_secs(60));
        app.state::<crate::state::AppState>().java.reap_idle();
    });
}

#[cfg(test)]
mod tests {
    #[test]
    fn 参数提示_同类型的参数按顺序找_给偏移的按_utf16_换算_重载的当前参数优先() {
        // 探针实测的形状：参数给的是文字
        let r = serde_json::json!({ "activeSignature": 1, "activeParameter": 0, "signatures": [
            { "label": "copy(int from, int to) : void", "parameters": [{ "label": "int from" }, { "label": "int to" }] },
            { "label": "copy(int a, int b, int c) : void", "parameters": [{ "label": "int a" }, { "label": "int b" }, { "label": "int c" }], "activeParameter": 2 },
        ] });
        let s = super::signatures(&r);
        assert_eq!(s.list[0].params, vec![(5, 13), (15, 21)], "两个 int 各亮各的");
        assert_eq!((s.active, s.param, s.total), (1, 2, 2), "重载自己带的 activeParameter 优先");
        // 给偏移的（UTF-16 下标）：前面有一个占两个 UTF-16 单位的字（emoji）也不错位。中文在 UTF-16 里也是一个单位，拿它测的话不换算照样过
        let r = serde_json::json!({ "signatures": [{ "label": "f(String 😀, int n)", "parameters": [{ "label": [2, 11] }, { "label": [13, 18] }] }] });
        let s = super::signatures(&r);
        let l: Vec<char> = s.list[0].label.chars().collect();
        let p = |i: usize| l[s.list[0].params[i].0 as usize..s.list[0].params[i].1 as usize].iter().collect::<String>();
        assert_eq!((p(0), p(1)), ("String 😀".to_string(), "int n".to_string()));
        // 越界的 activeSignature 回 0，不让前端去取一个不存在的
        assert_eq!(super::signatures(&serde_json::json!({ "activeSignature": 9, "signatures": [{ "label": "g()" }] })).active, 0);
        assert_eq!(super::signatures(&serde_json::Value::Null).list.len(), 0);
    }

    use super::*;

    #[test]
    fn 等导入完时程序被停了_不等满截止时间() {
        let phase = Mutex::new(Phase::Importing("导入中".into()));
        let changed = Condvar::new();
        let alive = std::sync::atomic::AtomicBool::new(true);
        let t0 = Instant::now();
        std::thread::scope(|sc| {
            sc.spawn(|| {
                std::thread::sleep(Duration::from_millis(300));
                alive.store(false, std::sync::atomic::Ordering::SeqCst);
            });
            let r = wait_ready(&phase, &changed, Duration::from_secs(30), &|| alive.load(std::sync::atomic::Ordering::SeqCst));
            assert!(r.unwrap_err().contains("程序已经停了"));
        });
        assert!(t0.elapsed() < Duration::from_secs(2), "停了之后最多再等一个 ALIVE_POLL：{:?}", t0.elapsed());
        // 导入完了照常回
        *phase.lock().unwrap() = Phase::Ready;
        assert!(wait_ready(&phase, &changed, Duration::from_secs(1), &|| true).is_ok());
    }

    #[test]
    fn 补全_插的是_textedit_不是_inserttext_带范围_认_snippet() {
        // jdtls 实测的一项（guessMethodArguments 关着）：insertText 只有名字，真要插的在 textEdit
        let r = json!({ "isIncomplete": false, "items": [
            { "label": "add", "kind": 2, "insertText": "add", "insertTextFormat": 2, "sortText": "999999035",
              "labelDetails": { "detail": "(String name, int count)", "description": "void" },
              "textEdit": { "newText": "add(${0});", "range": { "start": { "line": 10, "character": 13 }, "end": { "line": 10, "character": 15 } } },
              "data": { "pid": "0", "rid": "0" } },
            { "label": "ArrayList", "kind": 7, "detail": "java.util.ArrayList", "insertTextFormat": 1,
              "textEdit": { "newText": "ArrayList", "insert": { "start": { "line": 4, "character": 8 }, "end": { "line": 4, "character": 15 } },
                            "replace": { "start": { "line": 4, "character": 8 }, "end": { "line": 4, "character": 20 } } } },
            { "label": "foo" }
        ]});
        let (items, incomplete, total) = completion_items(&r);
        assert_eq!((items.len(), incomplete, total), (3, false, 3));
        let add = &items[0];
        assert_eq!(add.edit, Some(Edit { line: 10, character: 13, end_line: 10, end_character: 15, text: "add(${0});".into() }));
        assert!(add.snippet);
        assert_eq!((add.detail.as_str(), add.description.as_str(), add.kind), ("(String name, int count)", "void", 2));
        assert!(add.raw.contains("\"pid\""), "原样带着 data，resolve 要用：{}", add.raw);
        // InsertReplaceEdit 取 replace；没有 labelDetails 的说明退回 detail
        let al = &items[1];
        assert_eq!(al.edit.as_ref().map(|e| e.end_character), Some(20));
        assert!(!al.snippet);
        assert_eq!(al.description, "java.util.ArrayList");
        assert!(items[2].edit.is_none(), "没有 textEdit 的交给前端按光标前的词插 label");
        // 数组形式（不是 CompletionList）也认
        assert_eq!(completion_items(&json!([{ "label": "x" }])).0.len(), 1);
        assert_eq!(completion_items(&Value::Null).0.len(), 0);
    }

    #[test]
    fn 改名_两种回包都认_先改名后改的换回原路径_新建删除和jar拒绝() {
        // jdtls 实测的类改名（声明了 documentChanges + 三种文件操作之后）：changes 空、documentChanges 里先改两个文件、最后一条改名
        let r = json!({ "changes": {}, "documentChanges": [
            { "textDocument": { "uri": "file:///p/Shop.java", "version": null }, "edits": [{ "newText": "Store", "range": { "start": { "line": 2, "character": 13 }, "end": { "line": 2, "character": 17 } } }] },
            { "textDocument": { "uri": "file:///p/Use.java", "version": null }, "edits": [{ "newText": "Store s = new Store", "range": { "start": { "line": 4, "character": 8 }, "end": { "line": 4, "character": 25 } } }] },
            { "kind": "rename", "oldUri": "file:///p/Shop.java", "newUri": "file:///p/Store.java" },
            // 别的服务器可能这样：改名之后再改新路径上的文件 —— 我们先改内容后改名，要换回原路径
            { "textDocument": { "uri": "file:///p/Store.java", "version": null }, "edits": [{ "newText": "x", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } } }] }
        ]});
        let p = workspace_edit(&r).unwrap();
        assert_eq!(p.renames, vec![("/p/Shop.java".to_string(), "/p/Store.java".to_string())]);
        assert_eq!(p.files.iter().map(|(f, e)| (f.as_str(), e.len())).collect::<Vec<_>>(), vec![("/p/Shop.java", 2), ("/p/Use.java", 1)]);
        // 方法改名（没声明 documentChanges 时的形状）：只有 changes
        let r = json!({ "changes": { "file:///p/A.java": [{ "newText": "put", "range": { "start": { "line": 5, "character": 16 }, "end": { "line": 5, "character": 19 } } }] } });
        let p = workspace_edit(&r).unwrap();
        assert_eq!(p.files.len(), 1);
        assert_eq!(p.files[0].1[0], Edit { line: 5, character: 16, end_line: 5, end_character: 19, text: "put".into() });
        assert!(workspace_edit(&json!({ "documentChanges": [{ "kind": "create", "uri": "file:///p/N.java" }] })).unwrap_err().contains("新建"));
        assert!(workspace_edit(&json!({ "documentChanges": [{ "kind": "delete", "uri": "file:///p/N.java" }] })).unwrap_err().contains("删除"));
        assert!(workspace_edit(&json!({ "changes": { "jdt://contents/x.jar/A.class": [] } })).unwrap_err().contains("jar"));
        assert_eq!(workspace_edit(&Value::Null).unwrap(), RenamePlan::default());
    }

    #[test]
    fn 改名_jdtls_拒绝的英文原话换成中文_别的原样() {
        assert_eq!(Instance::rename_refusal("Compilation unit 'Customer.java' already exists".into()), "Customer.java 已经有了，不覆盖");
        assert_eq!(Instance::rename_refusal("别的错".into()), "别的错");
    }

    #[test]
    fn 修复_去掉要弹框的_修报错的在前_import_java_包排前面_jdk内部的最后() {
        // jdtls 实测的形状（按字母排：com.sun 在 java.util 前面）
        let r = json!([
            { "kind": "quickassist", "title": "Add Javadoc comment", "data": {} },
            { "kind": "quickfix", "title": "Import 'List' (com.sun.tools.javac.util)", "data": {} },
            { "kind": "quickfix", "title": "Import 'List' (java.awt)", "data": {} },
            { "kind": "quickfix", "title": "Import 'List' (java.util)", "data": {} },
            { "kind": "quickfix", "title": "Import 'List' (org.example.util)", "data": {} },
            { "kind": "quickfix", "title": "Create class 'List<T>'", "data": {} },
            { "kind": "source.organizeImports", "title": "Organize imports", "data": {} },
            { "kind": "source.generate.accessors", "title": "Generate Getters and Setters", "data": {} }
        ]);
        let t: Vec<String> = actions_of(&r).into_iter().map(|a| a.title).collect();
        assert_eq!(
            t,
            vec![
                "Import 'List' (java.util)",
                "Import 'List' (org.example.util)",
                "Create class 'List<T>'",
                "Import 'List' (com.sun.tools.javac.util)",
                "Import 'List' (java.awt)",
                "Add Javadoc comment",
                "Organize imports",
            ],
            "java.* 在前、项目和第三方居中、JDK 内部的和桌面图形界面的（java.awt）最后；同一档里照 jdtls 原来的次序；要弹框的生成 getter 不列"
        );
        let many: Vec<Value> = (0..ACTIONS_MAX + 5).map(|i| json!({ "kind": "quickfix", "title": format!("Change to 'X{i}'") })).collect();
        assert_eq!(actions_of(&Value::Array(many)).len(), ACTIONS_MAX);
    }

    #[test]
    fn 修复_报错按行判重叠_构建出来的那批列是0也算() {
        let d = json!({ "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 0 } } });
        let on = |l: u32, c: u32| Edit { line: l, character: c, end_line: l, end_character: c, text: String::new() };
        assert!(overlaps(&d, &on(3, 6)), "光标在第 3 行第 6 列，报错只有行号：算重叠");
        assert!(!overlaps(&d, &on(4, 0)));
        assert!(overlaps(&d, &Edit { line: 1, character: 0, end_line: 5, end_character: 0, text: String::new() }), "选中好几行");
    }

    #[test]
    fn 改名_prepare_回包三种形状() {
        let range = json!({ "start": { "line": 2, "character": 13 }, "end": { "line": 2, "character": 17 } });
        assert_eq!(rename_target(&range).map(|t| (t.range.character, t.range.end_character, t.placeholder)), Some((13, 17, None)));
        let with = json!({ "range": range, "placeholder": "Shop" });
        assert_eq!(rename_target(&with).and_then(|t| t.placeholder), Some("Shop".into()));
        assert!(rename_target(&json!({ "defaultBehavior": true })).is_none());
        assert!(rename_target(&Value::Null).is_none());
    }

    #[test]
    fn 补全_超过上限截断_报不完整和真项数() {
        let many: Vec<Value> = (0..COMPL_MAX + 7).map(|i| json!({ "label": format!("x{i}") })).collect();
        let (items, incomplete, total) = completion_items(&json!({ "isIncomplete": false, "items": many }));
        assert_eq!((items.len(), incomplete, total), (COMPL_MAX, true, COMPL_MAX + 7));
    }

    #[test]
    fn resolve_自动_import_在_additionaltextedits_说明截断在字符边界() {
        let r = json!({
            "additionalTextEdits": [{ "newText": "\n\nimport java.util.ArrayList;\n\n", "range": { "start": { "line": 0, "character": 13 }, "end": { "line": 2, "character": 0 } } }],
            "documentation": { "kind": "markdown", "value": "数".repeat(DOC_MAX + 5) }
        });
        let got = resolved(&r);
        assert_eq!(got.edits, vec![Edit { line: 0, character: 13, end_line: 2, end_character: 0, text: "\n\nimport java.util.ArrayList;\n\n".into() }]);
        assert_eq!(got.doc.chars().count(), DOC_MAX + 1, "截到 DOC_MAX 个字再加一个省略号");
        assert!(got.doc.ends_with('…'));
        assert_eq!(resolved(&json!({ "documentation": "纯文本" })).doc, "纯文本");
        assert!(resolved(&json!({})).edits.is_empty());
    }

    #[test]
    fn 位置_几种回法都认_jar_里的类给全名() {
        let r = json!([
            { "uri": "file:///p/a/Owner.java", "range": { "start": { "line": 5, "character": 13 }, "end": { "line": 5, "character": 18 } } },
            { "uri": "jdt://contents/spring-data-commons-4.0.1.jar/org.springframework.data.repository/CrudRepository.class?=x/%2Fa", "range": { "start": { "line": 40, "character": 1 }, "end": { "line": 40, "character": 9 } } },
        ]);
        let l = locations(&r);
        assert_eq!(l.len(), 2);
        assert_eq!((l[0].path.as_deref(), l[0].label.as_str(), l[0].line, l[0].character), (Some("/p/a/Owner.java"), "Owner.java", 5, 13));
        assert_eq!((l[1].path.as_deref(), l[1].label.as_str()), (None, "org.springframework.data.repository.CrudRepository"));
        // 单个 Location、LocationLink、null
        assert_eq!(locations(&json!({ "uri": "file:///x.java", "range": { "start": { "line": 1, "character": 2 }, "end": { "line": 1, "character": 3 } } })).len(), 1);
        let link = json!([{ "targetUri": "file:///y.java", "targetRange": { "start": { "line": 0, "character": 0 }, "end": { "line": 9, "character": 0 } },
                            "targetSelectionRange": { "start": { "line": 3, "character": 4 }, "end": { "line": 3, "character": 8 } } }]);
        assert_eq!(locations(&link)[0].line, 3, "LocationLink 跳到 selectionRange（名字那里），不是整个范围的开头");
        assert!(locations(&Value::Null).is_empty());
    }

    #[test]
    fn 诊断_超过上限先留错误_报真条数() {
        let mut items: Vec<Value> = (0..DIAG_MAX + 30)
            .map(|i| json!({ "range": { "start": { "line": i, "character": 0 }, "end": { "line": i, "character": 1 } }, "severity": 2, "message": "warn" }))
            .collect();
        items.push(json!({ "range": { "start": { "line": 9999, "character": 0 }, "end": { "line": 9999, "character": 3 } }, "severity": 1, "message": "真错误" }));
        let (kept, total) = diagnostics(&Value::Array(items));
        assert_eq!(total, DIAG_MAX + 31);
        assert_eq!(kept.len(), DIAG_MAX);
        assert!(kept.iter().any(|d| d.message == "真错误"), "截断时把错误截掉了：最要紧的一条没了");
    }

    #[test]
    fn 构建项目_根上或第一层有构建文件才算_点开头的目录不看() {
        let d = std::env::temp_dir().join(format!("java-root-{}-{:?}", std::process::id(), Instant::now()));
        std::fs::create_dir_all(d.join("svc")).unwrap();
        std::fs::create_dir_all(d.join(".idea")).unwrap();
        assert!(!is_build_root(&d), "什么都没有");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时目录
        std::fs::write(d.join(".idea/pom.xml"), "").unwrap();
        assert!(!is_build_root(&d), "点开头的目录里的不算");
        #[allow(clippy::disallowed_methods)] // 同上
        std::fs::write(d.join("svc/build.gradle.kts"), "").unwrap();
        assert!(is_build_root(&d), "第一层子目录里的 Gradle 项目");
        assert!(!is_build_root(&d.join("nope")));
    }

    #[test]
    fn 进度那句去掉套话() {
        assert_eq!(progress_text("32% Starting Java Language Server - https://maven.aliyun.com/repository/public/org"), "32% https://maven.aliyun.com/repository/public/org");
        assert_eq!(progress_text("0% Starting Java Language Server"), "0%");
    }

    #[test]
    fn 服务器问的_配置按条数回空_改代码一律不接() {
        assert_eq!(answer("workspace/configuration", &json!({ "items": [{}, {}, {}] })).unwrap(), json!([null, null, null]));
        assert_eq!(answer("workspace/applyEdit", &json!({})).unwrap()["applied"], json!(false));
        assert!(answer("something/else", &json!({})).is_err());
    }

    /// hover 的三种样子都认（jdtls 回的是数组那种）；空的项丢掉；没有 contents（光标在空白上）是空串
    #[test]
    fn 快速文档_三种样子统一成一段_markdown() {
        let jdtls = json!({ "contents": [{ "language": "java", "value": "long com.demo.PriceRule.apply(long cents)" }, "按规则算出折后价（分）。\n\n* **Parameters:**\n  * **cents** 原价"] });
        assert_eq!(hover_markdown(&jdtls), "```java\nlong com.demo.PriceRule.apply(long cents)\n```\n\n按规则算出折后价（分）。\n\n* **Parameters:**\n  * **cents** 原价");
        assert_eq!(hover_markdown(&json!({ "contents": { "kind": "markdown", "value": "**粗**" } })), "**粗**");
        assert_eq!(hover_markdown(&json!({ "contents": "纯字符串" })), "纯字符串");
        assert_eq!(hover_markdown(&json!({ "contents": [{ "language": "java", "value": "" }, ""] })), "", "空的项丢掉");
        assert_eq!(hover_markdown(&Value::Null), "");
    }

    #[test]
    fn 生成_字段默认全勾_已有的说出来_getter只列缺的_覆盖方法抽象的默认勾() {
        let ts = json!({ "type": "Tally", "exists": true, "fields": [{ "bindingKey": "k1", "name": "total", "type": "int", "isField": true, "isSelected": false }] });
        let c = gen_choices_of("tostring", &ts);
        assert_eq!((c.type_name.as_str(), c.items[0].label.as_str(), c.items[0].detail.as_str(), c.items[0].checked), ("Tally", "total", "int", true));
        assert!(c.note.contains("已经有 toString()"), "{}", c.note);
        assert!(c.items[0].raw.contains("\"bindingKey\":\"k1\""), "原样留着，第二步交回去");
        let acc = json!([
            { "fieldName": "a", "isStatic": false, "generateGetter": true, "generateSetter": false, "typeName": "int" },
            { "fieldName": "b", "isStatic": false, "generateGetter": false, "generateSetter": true, "typeName": "String" },
        ]);
        assert_eq!(gen_choices_of("getter", &acc).items.iter().map(|i| i.label.as_str()).collect::<Vec<_>>(), vec!["a"], "已经有 getter 的 b 不列");
        assert_eq!(gen_choices_of("accessors", &acc).items.len(), 2);
        let ov = json!({ "type": "VipRule", "methods": [
            { "bindingKey": "m1", "name": "apply", "parameters": ["long"], "unimplemented": true, "declaringClass": "PriceRule", "declaringClassType": "interface" },
            { "bindingKey": "m2", "name": "equals", "parameters": ["Object"], "unimplemented": false, "declaringClass": "Object", "declaringClassType": "class" },
        ] });
        let o = gen_choices_of("override", &ov);
        assert_eq!((o.items[0].label.as_str(), o.items[0].checked, o.items[1].checked), ("apply(long)", true, false), "接口里没实现的默认勾，Object 的不勾");
        let eq = json!({ "type": "T", "fields": [], "existingMethods": ["equals", "hashCode"] });
        assert!(gen_choices_of("equals", &eq).note.contains("equals、hashCode"));
    }

    #[test]
    fn 生成_只要getter时把setter的标记关掉_构造器带父类构造器() {
        let f = json!({ "fieldName": "b", "isStatic": false, "generateGetter": true, "generateSetter": true, "typeName": "String" });
        let (m, p) = gen_params("getter", json!({}), vec![f.clone()], None).unwrap();
        assert_eq!(m, "java/generateAccessors");
        assert_eq!((p["accessors"][0]["generateGetter"].as_bool(), p["accessors"][0]["generateSetter"].as_bool()), (Some(true), Some(false)), "选了 getter 不顺带生成 setter");
        let (_, p) = gen_params("setter", json!({}), vec![f], None).unwrap();
        assert_eq!(p["accessors"][0]["generateGetter"].as_bool(), Some(false));
        let (m, p) = gen_params("constructor", json!({}), vec![], Some(json!({ "name": "Object" }))).unwrap();
        assert_eq!((m, p["constructors"][0]["name"].as_str()), ("java/generateConstructors", Some("Object")));
        assert!(gen_params("nope", json!({}), vec![], None).is_err());
    }

    #[test]
    fn 层级_调用方带调用那一行_被调的不带_类型层级直接是节点_原样留着送回去() {
        let item = |name: &str, uri: &str, line: u64| json!({ "name": name, "detail": "com.demo.common.Tally", "kind": 6, "uri": uri, "range": { "start": { "line": line, "character": 4 }, "end": { "line": line + 2, "character": 5 } }, "selectionRange": { "start": { "line": line, "character": 15 }, "end": { "line": line, "character": 20 } }, "data": { "k": 1 } });
        let incoming = json!([{ "from": item("twice", "file:///p/Tally.java", 9), "fromRanges": [{ "start": { "line": 9, "character": 32 }, "end": { "line": 9, "character": 35 } }] }]);
        let n = hier_children_of(&incoming, "in");
        assert_eq!((n[0].name.as_str(), n[0].at.line, n[0].at.character, n[0].site), ("twice", 9, 15, Some((9, 32))), "位置是名字那一段，site 是调用的那一处");
        assert!(n[0].raw.contains("\"data\""), "jdtls 自己的 data 留在 raw 里，问下一层原样送回去");
        let outgoing = json!([{ "to": item("add", "file:///p/Tally.java", 4), "fromRanges": [{ "start": { "line": 9, "character": 32 }, "end": { "line": 9, "character": 35 } }] }]);
        assert_eq!(hier_children_of(&outgoing, "out")[0].site, None, "被调的：fromRanges 在我们自己的文件里，不当成它的位置");
        let types = json!([item("PriceRule", "file:///p/PriceRule.java", 2)]);
        assert_eq!(hier_children_of(&types, "super")[0].name, "PriceRule");
        assert!(hier_children_of(&Value::Null, "in").is_empty());
    }

    #[test]
    fn 内嵌提示_字符串和几段的标签都认_空的丢掉() {
        let r = json!([
            { "position": { "line": 6, "character": 22 }, "label": "cents:", "kind": 2, "paddingRight": true },
            { "position": { "line": 7, "character": 9 }, "label": [{ "value": ": " }, { "value": "long" }], "kind": 1 },
            { "position": { "line": 8, "character": 0 }, "label": "  " },
            // jdtls 1.61 真回的样子：参数名不带 kind
            { "position": { "line": 9, "character": 36 }, "label": "n:", "paddingRight": true },
        ]);
        let h = inlays_of(&r);
        assert_eq!(
            h,
            vec![
                Inlay { line: 6, character: 22, label: "cents:".into(), param: true },
                Inlay { line: 7, character: 9, label: ": long".into(), param: false },
                Inlay { line: 9, character: 36, label: "n:".into(), param: true },
            ]
        );
        assert!(inlays_of(&Value::Null).is_empty());
    }

    #[test]
    fn 同名高亮_写的单独标_没给种类的算读() {
        let r = json!([
            { "range": { "start": { "line": 3, "character": 8 }, "end": { "line": 3, "character": 13 } }, "kind": 3 },
            { "range": { "start": { "line": 5, "character": 15 }, "end": { "line": 5, "character": 20 } }, "kind": 2 },
            { "range": { "start": { "line": 6, "character": 4 }, "end": { "line": 6, "character": 9 } } },
        ]);
        let h = highlights_of(&r);
        assert_eq!(h.iter().map(|x| x.write).collect::<Vec<_>>(), vec![true, false, false]);
        assert_eq!((h[1].line, h[1].character, h[1].end_line, h[1].end_character), (5, 15, 5, 20));
        assert!(highlights_of(&Value::Null).is_empty(), "jdtls 对空白处回 null");
    }

    #[test]
    fn 找类_项目里的带路径_jar_里的是_jdt_地址_匿名的丢掉() {
        // jdtls 1.61 `java/searchSymbols` 回的形状（SymbolInformation[]）
        let r = json!([
            { "name": "OrderService", "kind": 5, "containerName": "com.demo.order", "location": { "uri": "file:///p/src/main/java/com/demo/order/OrderService.java", "range": { "start": { "line": 6, "character": 13 }, "end": { "line": 6, "character": 25 } } } },
            { "name": "Ordered", "kind": 11, "containerName": "org.springframework.core", "location": { "uri": "jdt://contents/spring-core.jar/org.springframework.core/Ordered.class", "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 0 } } } },
            { "name": "", "kind": 5, "containerName": "com.demo", "location": { "uri": "file:///p/A.java", "range": { "start": { "line": 1, "character": 0 }, "end": { "line": 1, "character": 0 } } } },
        ]);
        let t = type_hits(&r);
        assert_eq!(t.len(), 2, "匿名类（名字是空的）不列");
        assert_eq!((t[0].name.as_str(), t[0].container.as_str(), t[0].kind), ("OrderService", "com.demo.order", 5));
        assert_eq!((t[0].loc.path.as_deref(), t[0].loc.line, t[0].loc.character), (Some("/p/src/main/java/com/demo/order/OrderService.java"), 6, 13));
        assert!(t[1].loc.path.is_none() && t[1].loc.uri.starts_with("jdt://"), "jar 里的：没有路径，地址留着给 ⌘B 那条路开");
        assert_eq!(t[1].kind, 11);
        assert!(type_hits(&Value::Null).is_empty());
    }
}