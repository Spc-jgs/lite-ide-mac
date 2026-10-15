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

/// jdtls 的堆上限。petclinic（9 个 Maven 项目）实测 RSS 1.1–1.5 GB、没有 OOM（IDEA.md 第 7 节）
pub const XMX: &str = "1G";
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
/// 一个文件最多报多少条诊断（导入时整个项目的警告一起来，一个生成的文件可能几千条）
const DIAG_MAX: usize = 500;

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
        self.shared.docs.lock().unwrap_or_else(|e| e.into_inner()).open.is_empty()
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

    fn ask(&self, method: &str, params: Value) -> Result<Value, String> {
        self.touch();
        self.wait_usable()?;
        self.srv.request(method, params, ASK).map_err(|e| match e {
            Error::Timeout => format!("Java 语言服务器 {} 秒没回答（可能还在导入）", ASK.as_secs()),
            e => e.to_string(),
        })
    }

    pub fn definition(&self, path: &str, line: u32, character: u32) -> Result<Vec<Location>, String> {
        let r = self.ask("textDocument/definition", json!({ "textDocument": { "uri": uri::from_path(path) }, "position": { "line": line, "character": character } }))?;
        Ok(locations(&r))
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
    java_version: u32,
    layout: jdtls::Layout,
    lombok: Option<PathBuf>,
    workspace: PathBuf,
}

fn find(app: &AppHandle, root: &Path) -> Result<Found, String> {
    let user = std::env::var_os("HOME").map(PathBuf::from).ok_or("找不到用户目录（没有 HOME）")?;
    let (java_version, java) = jdtls::find_jdk(&jdtls::default_jdk_dirs(&user), jdtls::MIN_JAVA)
        .ok_or("没有 JDK 21 或更新的：jdtls 自己要 21 才能跑（你的项目可以是 8–25）。装一个：brew install --cask temurin@21")?;
    let data = app.path().app_data_dir().map_err(|e| format!("找不到应用数据目录：{e}"))?;
    let set = app.state::<crate::state::AppState>().settings.view().0.java_jdtls_home;
    let home = if set.trim().is_empty() { data.join("jdtls") } else { PathBuf::from(set.trim()) };
    let layout = jdtls::layout(&home).map_err(|e| format!("没有 jdtls（{e}）。在 settings.json 的 java.jdtlsHome 写上它解开后的目录"))?;
    let settings = std::fs::read_to_string(user.join(".m2/settings.xml")).ok();
    let lombok = jdtls::lombok_jar(&jdtls::local_repo(&user, settings.as_deref()));
    let workspace = jdtls::workspace_dir(&data.join("jdtls-ws"), root);
    jdtls::prepare_config(&layout, &workspace).map_err(|e| format!("建不了 jdtls 的缓存目录 {}：{e}", workspace.display()))?;
    Ok(Found { java, java_version, layout, lombok, workspace })
}

/// 应用里全部的 jdtls：项目根 → 实例，记在打开它的窗口名下
#[derive(Default)]
pub struct Java {
    all: Owned<String, Arc<Instance>>,
    janitor: OnceLock<()>,
}

impl Java {
    pub fn get(&self, root: &str) -> Option<Arc<Instance>> {
        self.all.get(&root.to_string(), Arc::clone).filter(|i| i.alive())
    }

    pub fn status(&self, root: &str) -> Option<Status> {
        self.all.get(&root.to_string(), |i| i.status())
    }

    /// 这个项目的 jdtls，没有（或者上一个崩了 / 收掉了）就起一个。**不等它起好**：进程起来、登记、后台 initialize，立刻返回。
    /// 起不来（缺 JDK 21、缺 jdtls）给 `Err`，原因同时作为状态发给前端
    pub fn ensure(&self, app: &AppHandle, owner: &str, root: &str) -> Result<Arc<Instance>, String> {
        self.janitor.get_or_init(|| spawn_janitor(app.clone()));
        if let Some(i) = self.get(root) {
            return Ok(i);
        }
        let fail = |why: String| {
            emit_status(app, owner, &Status { root: root.to_string(), phase: Phase::Failed(why.clone()), detail: String::new() });
            why
        };
        let f = find(app, Path::new(root)).map_err(fail)?;
        let detail = format!(
            "JDK {}（{}）· {}",
            f.java_version,
            f.java.display(),
            match &f.lombok {
                Some(j) => format!("Lombok {}", j.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
                None => "没找到 Lombok（用 Lombok 的项目会报假错）".into(),
            }
        );
        let spec = jdtls::spec(&jdtls::Launch {
            java_home: &f.java,
            layout: &f.layout,
            workspace: &f.workspace,
            lombok: f.lombok.as_deref(),
            xmx: XMX,
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
        let inst = Arc::new(Instance { shared: Arc::clone(&shared), srv });
        if let Some(old) = self.all.insert(owner, root.to_string(), Arc::clone(&inst)) {
            std::thread::spawn(move || old.stop());
        }
        emit_status(app, owner, &shared.status());
        let init = Arc::clone(&inst);
        std::thread::Builder::new()
            .name("jdtls-init".into())
            .spawn(move || {
                let root_uri = uri::from_path(&init.shared.root);
                let name = Path::new(&init.shared.root).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                match init.srv.initialize(&root_uri, &name, jdtls::capabilities(), jdtls::init_options(), INIT) {
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
            .map_err(|e| format!("起不了线程：{e}"))?;
        Ok(inst)
    }

    /// 重起（状态栏「重起」按钮）：停掉旧的、起新的。开着的文档由前端重新 `open` 一遍（它手上的才是没保存的那份，这里不读盘）
    pub fn restart(&self, app: &AppHandle, owner: &str, root: &str) -> Result<(), String> {
        if let Some(old) = self.all.remove(&root.to_string()) {
            old.stop();
        }
        self.ensure(app, owner, root).map(|_| ())
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
    use super::*;

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
}
