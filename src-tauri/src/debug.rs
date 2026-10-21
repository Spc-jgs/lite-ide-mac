//! 断点调试（docs/IDEA.md 第 18 节）：Spring Boot 服务和单测照 IDEA 的「Debug」起 —— 同一条命令，`java` 多带一个 JDWP 代理参数
//! （单测的 JVM 是 surefire fork 的，参数交给 `-Dmaven.surefire.debug`）；认出「调试端口开好了」，让 jdtls 里的 java-debug 插件挂上去，
//! 我们和插件之间说 DAP（`crates/dapsvc`）。
//!
//! ```text
//! 任务（tasksvc）  java -agentlib:jdwp=…suspend=y,address=127.0.0.1:0 …   ← 起来先停在入口等调试器
//!      │ Spring：输出里一行 "Listening for transport dt_socket at address: 56050"（[`wait_port`]）
//!      │ 单测：  surefire 不转这一行（JVM 停着它就不转），问系统进程组里谁在听（[`wait_jdwp`]）
//!      ▼
//! 这里  等 jdtls 导入完 → startDebugSession（插件开一个 DAP 端口）→ attach 56050 → 断点 → configurationDone（JVM 接着跑）
//!      │ stopped / continued / terminated 事件 → 推给前端（"debug-state"）；前端要变量、单步、求值走命令
//! ```
//!
//! 几条为什么：
//! - **`suspend=y`**：不等调试器就跑的话，`main` 里、启动阶段（`@PostConstruct`、`CommandLineRunner`）的断点永远停不住 —— 挂上去之前它们早跑过了。
//!   IDEA 的 Debug 也是这么起的。代价：调试器挂不上的话 JVM 一直停在入口，所以挂不上就把任务停掉、照实说为什么（[`fail`]）
//! - **端口 0**：系统挑一个空的，JVM 把真端口打在输出里（JDK 17 实测）。自己先找一个空端口再交给它，找到和用上之间会被别人占
//! - **事件不在读线程里处理**：`stopped` 之后要问调用栈 —— 在 dapsvc 的读线程里问的话，等的那个回答要读线程自己去读，永远等不到。
//!   事件进一个通道，会话自己的线程（[`run_events`]）一条一条处理
//! - **调试期间攥着 jdtls**（`java::Hold`）：插件跑在 jdtls 进程里，「没开着的 Java 文件、闲了十分钟」的回收不能把它收掉

use crate::owned::Owned;
use dapsvc::Session;
use serde_json::{json, Value};
use std::collections::{BTreeMap, HashMap};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// 加在 `java` 后面的那一段。`server=y`：JVM 开端口等我们连；`suspend=y` / 端口 0 见文件头
pub const JDWP: &str = "-agentlib:jdwp=transport=dt_socket,server=y,suspend=y,address=127.0.0.1:0";
/// JVM 开好端口时打的那句（JDK 9+ 都是这个样子）
const LISTEN: &str = "Listening for transport dt_socket at address: ";
/// 等 jdtls 导入完最多多久：第一次打开一个大项目要下依赖
const READY_WAIT: Duration = Duration::from_secs(10 * 60);
/// 一次 DAP 请求最多等多久。变量展开要调目标 JVM 里的 `toString()`，慢的几秒
const REQ: Duration = Duration::from_secs(15);
/// 调用栈最多拿多少帧：Spring 的栈一百多帧是常事，再深就是递归了
pub const FRAMES_MAX: usize = 200;
/// 一层变量最多列多少个（一个几万元素的集合展开，剩下的说「还有 N 个」）
pub const VARS_MAX: usize = 300;
/// 一层里最多替多少个变量先算好 `toString()`（见 [`resolve_lazy`]）
const LAZY_MAX: usize = 50;
/// 任务输出里认端口时，一行最多留多长（没有换行的超长输出不让它攒满内存）
const LINE_MAX: usize = 4096;

#[derive(Clone, Debug, PartialEq)]
pub enum Phase {
    /// 还没挂上：等端口 / 等 jdtls / 正在挂。带一句给人看的
    Waiting(String),
    Running,
    Paused,
    /// 结束了。带原因（正常结束是空的）
    Ended(String),
}

#[derive(Clone, Debug)]
pub struct Frame {
    pub id: i64,
    pub name: String,
    /// 源文件的绝对路径，或者 jar / JDK 里的类的 `jdt://` 地址（编辑器认得，⌘B 进 jar 用的同一种），没有源码（lambda 生成的类）是空的
    pub path: String,
    /// jar 里的类：类的全名（打开成只读标签时的标题）；源文件是空的
    pub label: String,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub struct Thread {
    pub id: i64,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Var {
    pub name: String,
    pub value: String,
    pub ty: String,
    /// 能展开就是正数（再问一次 `variables` 拿下一层）
    pub reference: i64,
    /// 数组 / 集合有几个元素（展开时按它分页）
    pub indexed: u32,
}

/// 界面要画的那一份：每次变了整份推给前端（几十帧，不值得做增量）
#[derive(Clone, Debug)]
pub struct Snapshot {
    pub phase: Phase,
    /// 停在哪个线程（`Paused` 时）
    pub thread: Option<i64>,
    /// 为什么停：breakpoint / step / exception / pause
    pub reason: String,
    pub frames: Vec<Frame>,
    /// 一共几帧（超过 [`FRAMES_MAX`] 时比 `frames` 多）
    pub total_frames: usize,
    /// 停着的线程（不止一个线程撞上断点时可以切）
    pub threads: Vec<Thread>,
    /// 这一次停下时插件说的话（条件断点的条件算不了：它照样停下，不说的话看着和「条件成立」一模一样）
    pub warn: String,
}

impl Snapshot {
    fn waiting(why: &str) -> Snapshot {
        Snapshot { phase: Phase::Waiting(why.into()), thread: None, reason: String::new(), frames: Vec::new(), total_frames: 0, threads: Vec::new(), warn: String::new() }
    }
}

/// 一个调试会话：一次运行（运行 id）对一个
pub struct Dbg {
    pub run: u32,
    pub root: String,
    owner: String,
    app: AppHandle,
    dap: Mutex<Option<Arc<Session>>>,
    snap: Mutex<Snapshot>,
    /// 停着的线程，最近停的在最后
    stopped: Mutex<Vec<i64>>,
    hold: Mutex<Option<crate::java::Hold>>,
    /// 插件刚说过的提醒，等紧跟着的 `stopped` 一起推（见 [`cond_warning`]）
    warn: Mutex<Option<String>>,
}

/// 应用里全部的调试会话（按窗口登记）+ 每个项目的断点
#[derive(Default)]
pub struct Debugs {
    all: Owned<u32, Arc<Dbg>>,
    /// 项目根 → 文件 → 断点（行从 1 数，带条件的是条件断点）。前端每次改了就整份报一个文件；会话挂上时照这张表下断点
    bps: Mutex<HashMap<String, BTreeMap<String, Vec<Bp>>>>,
}

impl Debugs {
    pub fn get(&self, run: u32) -> Option<Arc<Dbg>> {
        self.all.get(&run, Arc::clone)
    }

    /// 这个文件的断点换成这些；这个项目正在调试的会话当场跟着改。回会话说「对上了」的那些行（没有会话就原样回）
    pub fn set_breakpoints(&self, root: &str, path: &str, bps: Vec<Bp>) -> Vec<u32> {
        {
            let mut b = self.bps.lock().unwrap_or_else(|e| e.into_inner());
            let files = b.entry(root.to_string()).or_default();
            if bps.is_empty() {
                files.remove(path);
            } else {
                files.insert(path.to_string(), bps.clone());
            }
        }
        let live: Vec<Arc<Dbg>> = self.all.map(Arc::clone).into_iter().filter(|d| d.root == root).collect();
        let mut verified = bps.iter().map(|b| b.line).collect();
        for d in live {
            if let Some(v) = d.send_breakpoints(path, &bps) {
                verified = v;
            }
        }
        verified
    }

    fn breakpoints(&self, root: &str) -> Vec<(String, Vec<Bp>)> {
        self.bps.lock().unwrap_or_else(|e| e.into_inner()).get(root).map(|m| m.iter().map(|(k, v)| (k.clone(), v.clone())).collect()).unwrap_or_default()
    }

    /// 窗口关了：它的会话全断开（被调试的任务在 `release_window` 里另外收）
    pub fn release(&self, owner: &str) {
        for (_, d) in self.all.take_owner(owner) {
            d.disconnect();
        }
    }
}

/// 一个断点：行（从 1 数）和条件（Java 表达式，为真才停；空的是普通断点）。条件由 java-debug 在目标 JVM 里算 —— IDEA 的条件断点同一个意思
#[derive(Clone, Debug, PartialEq)]
pub struct Bp {
    pub line: u32,
    pub condition: String,
}

/// DAP 的 setBreakpoints 参数。条件是空串的不带 `condition` 字段（带一个空的，java-debug 会拿它去编译、报「表达式不对」）
fn breakpoints_args(path: &str, bps: &[Bp]) -> Value {
    let list: Vec<Value> = bps
        .iter()
        .map(|b| if b.condition.trim().is_empty() { json!({ "line": b.line }) } else { json!({ "line": b.line, "condition": b.condition.trim() }) })
        .collect();
    json!({ "source": { "path": path }, "breakpoints": list, "lines": bps.iter().map(|b| b.line).collect::<Vec<_>>() })
}

/// 起任务时用的命令：在 `exec java` 后面插上 JDWP。只认 `maven::run_command` 那个形状 —— 别的命令里插哪儿说不准，不猜
pub fn with_jdwp(command: &str) -> Option<String> {
    let at = command.find("exec java ")? + "exec java ".len();
    Some(format!("{}{JDWP} {}", &command[..at], &command[at..]))
}

/// 调试单测（`maven::test_command` 那个形状）：跑测试的是 surefire **另外 fork 的 JVM**，`mvn` 自己不是 —— JDWP 要交给 surefire 的
/// `maven.surefire.debug`（值原样加在 fork 出来的 JVM 的参数后面，不盖掉 pom 里的 `argLine`）。不是 `mvn ` 开头的不猜
pub fn with_surefire_debug(command: &str) -> Option<String> {
    let rest = command.strip_prefix("mvn ")?;
    Some(format!("mvn '-Dmaven.surefire.debug={JDWP}' {rest}"))
}

/// 一段输出里有没有「端口开好了」那一行（JVM 自己打的，没有前缀）
fn port_in(line: &str) -> Option<u16> {
    line.trim_end().strip_prefix(LISTEN)?.trim().parse().ok()
}

/// 命令行里有没有一个参数**本身**是调试代理。mvn 自己的命令行里也带着这串（`-Dmaven.surefire.debug=-agentlib:jdwp=…`），不能只看包不包含
fn jdwp_in(command: &str) -> bool {
    command.split_whitespace().any(|w| w.starts_with("-agentlib:jdwp="))
}

/// 调试单测：在任务的进程组里等一个**还没挂过的**、带调试代理的 JVM 开好端口。
///
/// **为什么不读输出**（2026-10-20 真 JVM 上卡死过）：surefire fork 的 JVM 确实打了「Listening for transport…」，
/// 可 surefire 要等那个 JVM 跑起来才把它的输出转给 mvn —— 而 `suspend=y` 的 JVM 在等调试器，调试器在等那一行，两边互相等。
/// 先前的实验用的是 `suspend=n`，正好绕开了这一步（那时看到的 `[INFO] Listening…` 是 JVM 已经跑起来之后才转出来的）。
/// fork 的 JVM 和 mvn 在同一个进程组里，问系统谁在听，不等它转话
fn wait_jdwp(pgid: i32, seen: &mut std::collections::HashSet<i32>, alive: impl Fn() -> bool, poll: Duration) -> Result<u16, String> {
    loop {
        let running = alive();
        for (pid, ports) in tasksvc::port::listening_by_pid(pgid) {
            if seen.contains(&pid) {
                continue;
            }
            if tasksvc::port::command_of(pid).is_some_and(|c| jdwp_in(&c)) {
                seen.insert(pid);
                return Ok(ports[0]);
            }
        }
        if !running {
            return Err("还没开调试端口就退出了（多半是编译没过，原因在输出里）".into());
        }
        std::thread::sleep(poll);
    }
}

/// 盯着任务的输出文件，从 `from` 往后等那一行出来；认出来时 `from` 停在那一行的末尾（同一个任务里下一个 JVM 从这儿接着等）。
/// `alive` 说任务还在不在 —— 先退了（编译没过）就不等了。一行超过 [`LINE_MAX`] 的丢掉开头（那一行不会是我们要的）
fn wait_port(log: &Path, from: &mut u64, alive: impl Fn() -> bool, poll: Duration) -> Result<u16, String> {
    let mut f: Option<std::fs::File> = None;
    let mut pos = *from;
    let mut line = Vec::<u8>::new();
    loop {
        let running = alive();
        if f.is_none() {
            f = std::fs::File::open(log).ok();
        }
        if let Some(file) = f.as_mut() {
            let mut buf = Vec::new();
            if file.seek(SeekFrom::Start(pos)).is_ok() && file.read_to_end(&mut buf).is_ok() {
                let start = pos;
                pos += buf.len() as u64;
                for (i, &b) in buf.iter().enumerate() {
                    if b == b'\n' {
                        if let Some(p) = port_in(&String::from_utf8_lossy(&line)) {
                            // 同一块里后面可能紧跟着下一个 JVM 的那一行：只算到这一行为止
                            *from = start + i as u64 + 1;
                            return Ok(p);
                        }
                        line.clear();
                    } else if line.len() < LINE_MAX {
                        line.push(b);
                    }
                }
            }
        }
        // 这一轮读之前就已经退了：读完还没有，就是没有（先判活、再读，退出前最后写的那几行也读到了）
        if !running {
            return Err("还没开调试端口就退出了（多半是编译没过，原因在输出里）".into());
        }
        std::thread::sleep(poll);
    }
}

fn s(v: &Value) -> String {
    v.as_str().unwrap_or("").to_string()
}

fn frames_of(body: &Value) -> (Vec<Frame>, usize) {
    let list: Vec<Frame> = body["stackFrames"]
        .as_array()
        .into_iter()
        .flatten()
        .take(FRAMES_MAX)
        .map(|f| Frame {
            id: f["id"].as_i64().unwrap_or(0),
            name: s(&f["name"]),
            path: s(&f["source"]["path"]),
            label: lspsvc::uri::jdt_label(f["source"]["path"].as_str().unwrap_or("")).map(|(n, _)| n).unwrap_or_default(),
            line: f["line"].as_u64().and_then(|l| u32::try_from(l).ok()).unwrap_or(0),
        })
        .collect();
    let total = body["totalFrames"].as_u64().map_or(list.len(), |n| n as usize).max(list.len());
    (list, total)
}

fn var_of(v: &Value) -> Var {
    Var {
        name: s(&v["name"]),
        value: s(&v["value"]),
        ty: s(&v["type"]),
        reference: v["variablesReference"].as_i64().unwrap_or(0),
        indexed: v["indexedVariables"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
    }
}

/// java-debug 对重写了 `toString()` 的对象回 `presentationHint.lazy`：值先空着，要再问一次 `variables` 才拿到
/// （回一个没名字的子项，值是 `Money@21 "Money[cents=12444]"`，子项再展开才是字段 —— 探针实测）。
/// IDEA 是直接把 `toString()` 写在值那一格的，这里替前端先问好：把那个子项的值和引用提上来。最多替 [`LAZY_MAX`] 个
fn resolve_lazy(sess: &Session, raw: &[Value], vars: &mut [Var]) {
    let mut n = 0;
    for (r, v) in raw.iter().zip(vars.iter_mut()) {
        if r["presentationHint"]["lazy"] != true || v.reference <= 0 {
            continue;
        }
        n += 1;
        if n > LAZY_MAX {
            break;
        }
        let Ok(body) = sess.request("variables", json!({ "variablesReference": v.reference }), REQ) else { continue };
        let kids = body["variables"].as_array().cloned().unwrap_or_default();
        if let [one] = &kids[..] {
            if one["name"].as_str().unwrap_or("").is_empty() {
                let k = var_of(one);
                v.value = k.value;
                v.reference = k.reference;
                v.indexed = k.indexed;
            }
        }
    }
}

impl Dbg {
    fn session(&self) -> Result<Arc<Session>, String> {
        self.dap.lock().unwrap_or_else(|e| e.into_inner()).clone().ok_or_else(|| "调试器还没挂上".into())
    }

    pub fn snapshot(&self) -> Snapshot {
        self.snap.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    fn set(&self, f: impl FnOnce(&mut Snapshot)) {
        let snap = {
            let mut g = self.snap.lock().unwrap_or_else(|e| e.into_inner());
            f(&mut g);
            g.clone()
        };
        let _ = self.app.emit_to(&self.owner, "debug-state", crate::commands::DebugStateDto::of(self.run, &snap));
        sync_menu(&self.app);
    }

    /// 只丢这一个 JVM 的连接，jdtls 照旧攥着（同一个任务里还会有下一个 JVM 要挂）
    fn disconnect_dap(&self) {
        self.dap.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn disconnect(&self) {
        // 只丢连接：Session 的 drop 关 socket。插件那边看到断线自己收尾（不替被调试的程序决定生死 —— 那是任务的事）
        self.dap.lock().unwrap_or_else(|e| e.into_inner()).take();
        self.hold.lock().unwrap_or_else(|e| e.into_inner()).take();
    }

    fn send_breakpoints(&self, path: &str, bps: &[Bp]) -> Option<Vec<u32>> {
        let sess = self.session().ok()?;
        let body = sess.request("setBreakpoints", breakpoints_args(path, bps), REQ).ok()?;
        Some(body["breakpoints"].as_array().into_iter().flatten().filter(|b| b["verified"] == true).filter_map(|b| b["line"].as_u64().map(|l| l as u32)).collect())
    }

    /// 停在 `thread` 上：拿调用栈和停着的线程的名字，整份推给前端
    fn show_paused(&self, thread: i64, reason: &str) {
        let Ok(sess) = self.session() else { return };
        let (frames, total) = match sess.request("stackTrace", json!({ "threadId": thread, "startFrame": 0, "levels": FRAMES_MAX }), REQ) {
            Ok(b) => frames_of(&b),
            Err(e) => {
                applog::write(applog::Level::Warn, "debug", &format!("stackTrace 失败：{e}"));
                (Vec::new(), 0)
            }
        };
        let stopped = self.stopped.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let names: HashMap<i64, String> = sess
            .request("threads", json!({}), REQ)
            .ok()
            .and_then(|b| b["threads"].as_array().cloned())
            .unwrap_or_default()
            .iter()
            .map(|t| (t["id"].as_i64().unwrap_or(0), s(&t["name"])))
            .collect();
        let threads = stopped.iter().map(|id| Thread { id: *id, name: names.get(id).cloned().unwrap_or_else(|| format!("线程 {id}")) }).collect();
        let reason = reason.to_string();
        let warn = self.warn.lock().unwrap_or_else(|e| e.into_inner()).take().unwrap_or_default();
        self.set(|s| {
            s.phase = Phase::Paused;
            s.warn = warn;
            s.thread = Some(thread);
            s.reason = reason;
            s.frames = frames;
            s.total_frames = total;
            s.threads = threads;
        });
    }

    /// 这个线程不停了（继续 / 单步出去了）：还有别的线程停着就换成它，没有就是「在跑」
    fn resumed(&self, thread: Option<i64>) {
        let next = {
            let mut st = self.stopped.lock().unwrap_or_else(|e| e.into_inner());
            match thread {
                Some(t) => st.retain(|x| *x != t),
                None => st.clear(),
            }
            st.last().copied()
        };
        match next {
            Some(t) => self.show_paused(t, "breakpoint"),
            None => self.set(|s| {
                s.phase = Phase::Running;
                s.thread = None;
                s.threads.clear();
                s.warn.clear();
            }),
        }
    }

    /// 继续 / 单步 / 暂停。作用在当前停着的那个线程上（暂停是全部）
    pub fn step(&self, action: &str) -> Result<(), String> {
        let sess = self.session()?;
        let snap = self.snapshot();
        let cmd = match action {
            "continue" | "next" | "stepIn" | "stepOut" => action,
            "pause" => {
                if snap.phase != Phase::Running {
                    return Ok(());
                }
                // threadId 0：java-debug 把它当成「整个 JVM」
                return sess.request("pause", json!({ "threadId": 0 }), REQ).map(|_| ()).map_err(|e| e.to_string());
            }
            _ => return Err(format!("不认识的动作 {action}")),
        };
        let Some(t) = snap.thread.filter(|_| snap.phase == Phase::Paused) else { return Ok(()) };
        let body = sess.request(cmd, json!({ "threadId": t }), REQ).map_err(|e| e.to_string())?;
        // 回了就是走了：DAP 不保证还会发 continued 事件。单步的话紧接着会来一个 stopped（reason=step）
        self.resumed(if all_continued(cmd, &body) { None } else { Some(t) });
        Ok(())
    }

    /// 换看另一个停着的线程
    pub fn select_thread(&self, thread: i64) -> Result<Snapshot, String> {
        if !self.stopped.lock().unwrap_or_else(|e| e.into_inner()).contains(&thread) {
            return Err("这个线程没停着".into());
        }
        let reason = self.snapshot().reason;
        self.show_paused(thread, &reason);
        Ok(self.snapshot())
    }

    /// 一帧的局部变量（`frame`），或者一个变量的下一层（`reference`）。回 (变量, 还有几个没列)
    pub fn variables(&self, frame: Option<i64>, reference: Option<i64>, indexed: u32) -> Result<(Vec<Var>, usize), String> {
        let sess = self.session()?;
        let refs: Vec<i64> = match (frame, reference) {
            (_, Some(r)) => vec![r],
            (Some(f), None) => {
                let b = sess.request("scopes", json!({ "frameId": f }), REQ).map_err(|e| e.to_string())?;
                // 「贵的」作用域（静态字段那种）不自动展开，同 VS Code 的约定
                b["scopes"].as_array().into_iter().flatten().filter(|sc| sc["expensive"] != true).filter_map(|sc| sc["variablesReference"].as_i64()).collect()
            }
            (None, None) => return Err("要哪一帧的变量？".into()),
        };
        let mut raw = Vec::new();
        for r in refs {
            let mut args = json!({ "variablesReference": r });
            if indexed > 0 {
                args["start"] = json!(0);
                args["count"] = json!(indexed.min(VARS_MAX as u32));
            }
            let b = sess.request("variables", args, REQ).map_err(|e| e.to_string())?;
            raw.extend(b["variables"].as_array().cloned().unwrap_or_default());
        }
        let total = raw.len().max(indexed as usize);
        raw.truncate(VARS_MAX);
        let mut vars: Vec<Var> = raw.iter().map(var_of).collect();
        resolve_lazy(&sess, &raw, &mut vars);
        let more = total - vars.len();
        Ok((vars, more))
    }

    /// 在这一帧里算一个表达式（IDEA 的 Evaluate，⌥F8）
    pub fn evaluate(&self, frame: i64, expr: &str) -> Result<Var, String> {
        let sess = self.session()?;
        let b = sess.request("evaluate", json!({ "expression": expr, "frameId": frame, "context": "watch" }), REQ).map_err(|e| e.to_string())?;
        Ok(Var {
            name: expr.to_string(),
            value: s(&b["result"]),
            ty: s(&b["type"]),
            reference: b["variablesReference"].as_i64().unwrap_or(0),
            indexed: b["indexedVariables"].as_u64().and_then(|n| u32::try_from(n).ok()).unwrap_or(0),
        })
    }
}

/// 这一下放走的是不是整个 JVM。java-debug 的 `continue` 默认放走全部线程，回答里 `allThreadsContinued` 说清楚（DAP 规定不写就当是 true）；
/// 单步只走那一个线程。原来一律只标那一个：两个请求同时撞上断点时按 F9，另一个线程在界面上还「停着」，其实早跑了（2026-10-20 code review）
fn all_continued(cmd: &str, body: &Value) -> bool {
    cmd == "continue" && body["allThreadsContinued"] != false
}

/// 等端口 / 挂上的时候出了错，任务已经不在了：要不要当成正常结束（不报错）。
/// 人点了停止（`stopped`）不是错 —— 原来编译到一半点停止会报「还没开调试端口就退出了（多半是编译没过）」；挂上过之后任务退了是跑完了
fn quiet_end(attached: usize, st: &tasksvc::State) -> bool {
    matches!(st, tasksvc::State::Exited { stopped: true, .. }) || (attached > 0 && matches!(st, tasksvc::State::Exited { .. }))
}

/// 菜单里单步那几项亮不亮：有会话停着 / 有会话在跑（`menu::sync_debug`）。原生菜单要在主线程上改
fn sync_menu(app: &AppHandle) {
    let st = app.state::<crate::state::AppState>();
    let phases = st.debug.all.map(|d| d.snapshot().phase);
    let paused = phases.iter().any(|p| *p == Phase::Paused);
    let running = phases.iter().any(|p| *p == Phase::Running);
    let a = app.clone();
    let _ = app.run_on_main_thread(move || crate::menu::sync_debug(&a, paused, running));
}

/// 被调试的是什么：求值要告诉插件「按哪个项目编译这个表达式」（attach 的 `projectName`）
pub enum Who {
    /// Spring Boot 应用：项目名问 jdtls（`resolveMainClass`）
    Main(String),
    /// 单测：测试类没有 main，问不出来 —— 直接用模块的 artifactId（m2e 默认拿它当项目名）
    Project(String),
}

/// 起了一个带 JDWP 的任务之后：登记会话、后台去挂。立刻返回。
/// **一个任务里可能不止一个 JVM**（surefire `forkCount > 1`、重跑没过的跨了几个类）：每个都停在入口等调试器，
/// 没人挂上去就永远卡着。所以一个 JVM 结束、任务还在，就接着等下一个（还没挂过的）端口、再挂上去；挂上过一次之后任务退了算正常结束
pub fn start(app: &AppHandle, owner: &str, run: u32, root: &str, log: PathBuf, who: Who, task: Arc<tasksvc::Task>) {
    let d = Arc::new(Dbg {
        run,
        root: root.to_string(),
        owner: owner.to_string(),
        app: app.clone(),
        dap: Mutex::new(None),
        snap: Mutex::new(Snapshot::waiting("等它编译完、开调试端口…")),
        stopped: Mutex::new(Vec::new()),
        hold: Mutex::new(None),
        warn: Mutex::new(None),
    });
    let st = app.state::<crate::state::AppState>();
    if let Some(old) = st.debug.all.insert(owner, run, Arc::clone(&d)) {
        old.disconnect();
    }
    d.set(|_| {});
    let app = app.clone();
    let _ = std::thread::Builder::new().name(format!("debug-{run}")).spawn(move || {
        let mut from = 0u64;
        let mut seen = std::collections::HashSet::new();
        let pgid = task.pgid();
        let t2 = Arc::clone(&task);
        // 下一个 JVM 的调试端口：Spring 是 `exec java`，任务自己就是那个 JVM，那一行当场写进输出；单测的 JVM 是 surefire fork 的，问系统（见 `wait_jdwp`）
        let mut next_port = || match &who {
            Who::Main(_) => wait_port(&log, &mut from, || t2.alive(), Duration::from_millis(150)),
            Who::Project(_) => wait_jdwp(pgid, &mut seen, || t2.alive(), Duration::from_millis(300)),
        };
        let mut attached = 0;
        let why = loop {
            match attach(&app, &d, &mut next_port, &who, &|| task.alive()) {
                Ok(rx) => {
                    attached += 1;
                    run_events(&d, rx);
                    d.disconnect_dap();
                    d.stopped.lock().unwrap_or_else(|e| e.into_inner()).clear();
                    if !task.alive() {
                        break String::new();
                    }
                    d.set(|s| {
                        s.phase = Phase::Waiting("这个 JVM 跑完了，等下一个（surefire 可能还要再起一个）…".into());
                        s.thread = None;
                        s.frames.clear();
                        s.threads.clear();
                    });
                }
                // 人停的、或者挂上过之后任务跑完了（mvn 写完报告退出）：不是出错
                Err(_) if quiet_end(attached, &task.state()) => break String::new(),
                Err(e) => break fail(&d, &task, e),
            }
        };
        d.disconnect();
        d.stopped.lock().unwrap_or_else(|e| e.into_inner()).clear();
        d.set(|s| {
            s.phase = Phase::Ended(why);
            s.thread = None;
            s.frames.clear();
            s.threads.clear();
        });
        // 只摘自己：同一次运行不会再有第二个会话，但重跑换了 id 的那个不能被这里摘掉
        let st = app.state::<crate::state::AppState>();
        if st.debug.get(d.run).is_some_and(|x| Arc::ptr_eq(&x, &d)) {
            st.debug.all.remove(&d.run);
        }
        sync_menu(&app);
    });
}

/// 挂不上：被调试的 JVM 正停在入口等我们（`suspend=y`），不收它的话它就一直挂着、占着端口。停掉任务，原因交给界面
fn fail(d: &Dbg, task: &tasksvc::Task, why: String) -> String {
    applog::write(applog::Level::Warn, "debug", &format!("调试器没挂上（运行 {}）：{why}", d.run));
    if task.alive() {
        task.stop(Duration::from_secs(5));
        return format!("{why} —— 程序停在入口等调试器，已经停掉了");
    }
    why
}

fn attach(app: &AppHandle, d: &Arc<Dbg>, next_port: &mut dyn FnMut() -> Result<u16, String>, who: &Who, alive: &dyn Fn() -> bool) -> Result<mpsc::Receiver<Value>, String> {
    let jvm_port = next_port()?;
    d.set(|s| s.phase = Phase::Waiting("等 Java 语言服务器导入完…".into()));
    let st = app.state::<crate::state::AppState>();
    let java = st.java.ensure(app, &d.owner, &d.root).map_err(|s| match s.phase {
        crate::java::Phase::Failed(w) => w,
        _ => "Java 语言服务器起不来".into(),
    })?;
    *d.hold.lock().unwrap_or_else(|e| e.into_inner()) = Some(java.hold());
    let (dap_port, project) = match who {
        Who::Main(c) => java.debug_session(Some(c), READY_WAIT, alive)?,
        Who::Project(p) => (java.debug_session(None, READY_WAIT, alive)?.0, Some(p.clone())),
    };
    d.set(|s| s.phase = Phase::Waiting("正在挂上调试器…".into()));
    let (tx, rx) = mpsc::channel::<Value>();
    let sess = Arc::new(
        Session::connect(dap_port, Box::new(move |e| { let _ = tx.send(e); }), Box::new(|| {})).map_err(|e| format!("连不上调试插件：{e}"))?,
    );
    sess.request(
        "initialize",
        json!({ "clientID": "lite-ide", "adapterID": "java", "linesStartAt1": true, "columnsStartAt1": true, "pathFormat": "path", "supportsVariableType": true }),
        REQ,
    )
    .map_err(|e| format!("调试插件 initialize 失败：{e}"))?;
    sess.request("attach", json!({ "hostName": "127.0.0.1", "port": jvm_port, "timeout": 10000, "projectName": project }), REQ)
        .map_err(|e| format!("挂不上 JVM（端口 {jvm_port}）：{e}"))?;
    // 等 initialized 再下断点（DAP 的次序）。中间来的别的事件（版本不一致的提醒这类）记日志
    let deadline = std::time::Instant::now() + REQ;
    loop {
        let left = deadline.saturating_duration_since(std::time::Instant::now());
        let e = rx.recv_timeout(left).map_err(|_| "调试插件挂上之后没说「准备好了」".to_string())?;
        if e["event"] == "initialized" {
            break;
        }
        note_output(&e);
    }
    *d.dap.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::clone(&sess));
    for (path, bps) in st.debug.breakpoints(&d.root) {
        d.send_breakpoints(&path, &bps);
    }
    sess.request("configurationDone", json!({}), REQ).map_err(|e| format!("configurationDone 失败：{e}"))?;
    d.set(|s| s.phase = Phase::Running);
    Ok(rx)
}

/// 插件发来的输出（「调试器和被调试的 JVM 版本不一样」这类）：程序自己的输出在任务日志里，这些只是调试器的话，记进 app.log
fn note_output(e: &Value) {
    if e["event"] == "output" {
        let text = s(&e["body"]["output"]);
        if !text.trim().is_empty() {
            applog::write(applog::Level::Info, "debug", text.trim());
        }
    }
}

/// 插件的原话翻成人话。它的两种（0.53.1 的字符串，读字节码拿到的）：
/// `Breakpoint condition '<条件>' error: <原因>`（变量名打错、编译不过）、`Result of breakpoint condition '<条件>' is not a boolean, …`。
/// 原因那半是 JDT 编译器的英文，照原样留着（翻错了比不翻糟），只把它固定的那句开头换成「编译不过」；认不出的整句原样给。
/// 真 JVM 上的原话（spring.sh 撞出来的）：`… error: Cannot evaluate because of compilation error(s): nosuch cannot be resolved to a variable.`
fn cond_warning(msg: &str) -> String {
    if let Some(rest) = msg.strip_prefix("Breakpoint condition '") {
        if let Some((cond, why)) = rest.split_once("' error: ") {
            let why = match why.strip_prefix("Cannot evaluate because of compilation error(s): ") {
                Some(w) => format!("编译不过：{}", w.trim_end_matches('.')),
                None => why.to_string(),
            };
            return format!("条件 {cond} 算不了，照样停下了：{why}");
        }
    }
    if let Some(rest) = msg.strip_prefix("Result of breakpoint condition '") {
        if let Some((cond, _)) = rest.split_once("' is not a boolean") {
            return format!("条件 {cond} 的结果不是 true / false，照样停下了");
        }
    }
    msg.to_string()
}

fn run_events(d: &Dbg, rx: mpsc::Receiver<Value>) {
    // 线断了（插件退了、被调试的 JVM 没了）dapsvc 的读线程退出、丢掉发送端，这里的 recv 就返回 Err
    for e in rx {
        let body = &e["body"];
        match e["event"].as_str().unwrap_or("") {
            "stopped" => {
                let t = body["threadId"].as_i64().unwrap_or(0);
                {
                    let mut st = d.stopped.lock().unwrap_or_else(|e| e.into_inner());
                    st.retain(|x| *x != t);
                    st.push(t);
                }
                d.show_paused(t, body["reason"].as_str().unwrap_or("breakpoint"));
            }
            "continued" => d.resumed(if body["allThreadsContinued"] == true { None } else { body["threadId"].as_i64() }),
            // java-debug 自己的事件（不在 DAP 标准里）：条件算不了时先发它、紧跟着照样发 stopped（读 0.53.1 的 SetBreakpointsRequestHandler 确认的）
            "usernotification" => {
                let msg = s(&body["message"]);
                applog::write(applog::Level::Warn, "debug", &msg);
                *d.warn.lock().unwrap_or_else(|e| e.into_inner()) = Some(cond_warning(&msg));
            }
            "terminated" | "exited" => break,
            _ => note_output(&e),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn 条件断点_有条件的带_condition_空的不带() {
        let v = breakpoints_args("/p/A.java", &[Bp { line: 4, condition: " cents == 1 ".into() }, Bp { line: 7, condition: "  ".into() }]);
        assert_eq!(v["breakpoints"], json!([{ "line": 4, "condition": "cents == 1" }, { "line": 7 }]));
        assert_eq!(v["lines"], json!([4, 7]));
    }

    #[test]
    fn 条件算不了_插件的原话翻成人话_认不出的原样给() {
        assert_eq!(
            cond_warning("Breakpoint condition 'nosuch == 1' error: Cannot evaluate because of compilation error(s): nosuch cannot be resolved to a variable."),
            "条件 nosuch == 1 算不了，照样停下了：编译不过：nosuch cannot be resolved to a variable"
        );
        assert_eq!(cond_warning("Breakpoint condition 'a.b()' error: java.lang.NullPointerException"), "条件 a.b() 算不了，照样停下了：java.lang.NullPointerException");
        assert_eq!(cond_warning("Result of breakpoint condition 'cents' is not a boolean, please correct your expression."), "条件 cents 的结果不是 true / false，照样停下了");
        assert_eq!(cond_warning("[Logpoint] Log message 'x' error: y"), "[Logpoint] Log message 'x' error: y");
    }

    #[test]
    fn jdwp_插在_exec_java_后面_别的命令不猜() {
        let c = r#"mvn -B -ntp -pl a -am compile && cd a && exec java -cp "target/classes:$(cat x)" com.A"#;
        assert_eq!(with_jdwp(c).unwrap(), format!(r#"mvn -B -ntp -pl a -am compile && cd a && exec java {JDWP} -cp "target/classes:$(cat x)" com.A"#));
        assert_eq!(with_jdwp("pnpm run dev"), None);
    }

    #[test]
    fn 从输出里认端口_跨两次写入的半行也认_先退了就不等() {
        let dir = std::env::temp_dir().join(format!("dbg-port-{}-{:?}", std::process::id(), std::time::Instant::now()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("run.log");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        let mut f = std::fs::File::create(&log).unwrap();
        writeln!(f, "[INFO] BUILD SUCCESS").unwrap();
        write!(f, "Listening for transport dt_").unwrap();
        f.flush().unwrap();
        let log2 = log.clone();
        let t = std::thread::spawn(move || wait_port(&log2, &mut 0, || true, Duration::from_millis(20)));
        std::thread::sleep(Duration::from_millis(120));
        writeln!(f, "socket at address: 56050").unwrap();
        f.flush().unwrap();
        assert_eq!(t.join().unwrap(), Ok(56050));

        // 编译没过：没有那一行、任务已经退了 —— 不干等
        let bad = dir.join("bad.log");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        std::fs::write(&bad, "[ERROR] COMPILATION ERROR\n").unwrap();
        let t0 = std::time::Instant::now();
        assert!(wait_port(&bad, &mut 0, || false, Duration::from_millis(20)).is_err());
        assert!(t0.elapsed() < Duration::from_secs(1));
        // 退出前最后写的那一行也要读到（先判活、再读）
        let last = dir.join("last.log");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        std::fs::write(&last, "Listening for transport dt_socket at address: 1234\n").unwrap();
        assert_eq!(wait_port(&last, &mut 0, || false, Duration::from_millis(20)), Ok(1234));

        // 一个任务里两个 JVM 的那两行挨在同一块里：第二次从第一次停下的地方接着认出第二个
        let two = dir.join("two.log");
        #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
        std::fs::write(&two, "Listening for transport dt_socket at address: 52450\n[INFO] Tests run: 1\nListening for transport dt_socket at address: 52451\n").unwrap();
        let mut from = 0;
        assert_eq!(wait_port(&two, &mut from, || false, Duration::from_millis(20)), Ok(52450));
        assert_eq!(wait_port(&two, &mut from, || false, Duration::from_millis(20)), Ok(52451));
        assert!(wait_port(&two, &mut from, || false, Duration::from_millis(20)).is_err(), "两个都认过了、任务退了：不再有");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn 继续放走的是整个_jvm_单步只走那一个线程() {
        assert!(all_continued("continue", &json!({})), "不写就当是全部（DAP 规定）");
        assert!(all_continued("continue", &json!({ "allThreadsContinued": true })));
        assert!(!all_continued("continue", &json!({ "allThreadsContinued": false })));
        assert!(!all_continued("next", &json!({})), "单步只走那一个");
    }

    #[test]
    fn 人停的和挂过之后跑完的_不算出错_没挂上就退了的才算() {
        let st = |stopped| tasksvc::State::Exited { code: Some(1), signal: None, stopped };
        assert!(quiet_end(0, &st(true)), "编译到一半人点了停止");
        assert!(quiet_end(1, &st(false)), "挂上过、跑完了");
        assert!(!quiet_end(0, &st(false)), "没挂上就自己退了（编译没过）：要说出来");
    }

    #[test]
    fn 带调试代理的是参数本身_mvn_命令行里转交的那串不算() {
        assert!(jdwp_in(&format!("/usr/bin/java {JDWP} -jar surefirebooter.jar /tmp/x")));
        assert!(!jdwp_in(&format!("java -classpath plexus.jar org.codehaus.plexus.classworlds.launcher.Launcher -Dmaven.surefire.debug={JDWP} -pl order-service -am test")));
    }

    /// 真起一个带调试代理的 JVM 太重；这里起一个**命令行里带着那个参数**、在听端口的进程（python 的一行服务器，参数照抄），
    /// 和一个命令行里只是转交那串（像 mvn）、也在听的进程：认出来的必须是前一个。两个都在本测试自己的进程组里
    #[test]
    fn 单测的调试端口_在进程组里认带调试代理的那个_不认转交的() {
        use std::os::unix::process::CommandExt;
        let py = |arg: &str| {
            let mut c = std::process::Command::new("python3");
            c.args(["-c", "import socket,time,sys; s=socket.socket(); s.bind(('127.0.0.1',0)); s.listen(); print(s.getsockname()[1], flush=True); time.sleep(30)", arg])
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::piped())
                .process_group(0);
            c
        };
        #[allow(clippy::disallowed_methods)] // 测试里起的探针进程，下面一定收
        let mut fake_mvn = py(&format!("-Dmaven.surefire.debug={JDWP}")).spawn().unwrap();
        let pgid = fake_mvn.id() as i32;
        let mut jvm_cmd = py(JDWP);
        jvm_cmd.process_group(pgid);
        #[allow(clippy::disallowed_methods)] // 同上
        let mut jvm = jvm_cmd.spawn().unwrap();
        let read_port = |c: &mut std::process::Child| {
            let mut line = String::new();
            std::io::BufRead::read_line(&mut std::io::BufReader::new(c.stdout.as_mut().unwrap()), &mut line).unwrap();
            line.trim().parse::<u16>().unwrap()
        };
        let (_, want) = (read_port(&mut fake_mvn), read_port(&mut jvm));
        let mut seen = std::collections::HashSet::new();
        let got = wait_jdwp(pgid, &mut seen, || true, Duration::from_millis(50));
        // 挂过的那个不再认：任务还在、又没有新的 JVM —— 等着（这里让它「任务退了」就返回）
        let again = wait_jdwp(pgid, &mut seen, || false, Duration::from_millis(50));
        let _ = jvm.kill();
        let _ = fake_mvn.kill();
        let _ = (jvm.wait(), fake_mvn.wait());
        assert_eq!(got, Ok(want));
        assert!(again.is_err(), "挂过的 JVM 不再认第二次：{again:?}");
    }

    #[test]
    fn 单测的调试参数交给_surefire_不是_mvn_自己() {
        let c = "mvn -B -ntp -Dtest='com.demo.OrderTest' -Dsurefire.failIfNoSpecifiedTests=false -pl order-service -am test";
        assert_eq!(with_surefire_debug(c).unwrap(), format!("mvn '-Dmaven.surefire.debug={JDWP}' -B -ntp -Dtest='com.demo.OrderTest' -Dsurefire.failIfNoSpecifiedTests=false -pl order-service -am test"));
        assert_eq!(with_surefire_debug("./mvnw test"), None);
    }

    #[test]
    fn 调用栈_jar_里的帧带_jdt_地址_没源码的帧路径是空的_超过上限说总数() {
        let body = json!({ "stackFrames": [
            { "id": 1, "name": "Money.show()", "line": 5, "source": { "name": "Money.java", "path": "/p/Money.java" } },
            { "id": 2, "name": "SpringApplication.callRunner(…)", "line": 792, "source": { "path": "jdt://contents/spring-boot.jar/x" } },
            { "id": 3, "name": "0x000000b001236400.accept(Object)", "line": -1, "source": {} },
        ], "totalFrames": 240 });
        let (f, total) = frames_of(&body);
        assert_eq!(f.iter().map(|x| (x.path.as_str(), x.line)).collect::<Vec<_>>(), vec![("/p/Money.java", 5), ("jdt://contents/spring-boot.jar/x", 792), ("", 0)]);
        assert_eq!(total, 240);
    }
}
