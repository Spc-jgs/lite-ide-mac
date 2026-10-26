//! AI 助手的会话（docs/TAKEOVER.md MVP-6）：一个 ACP agent 进程 + 它的一个会话，按窗口登记。和前端之间的那些事：
//!
//! - agent 推来的 `session/update`（回答的一块块字、想法、工具调用、计划）原样转成 `agent-event` 事件给开着它的那个窗口
//! - agent **反过来问**宿主的三种请求（DIRECTION.md 第 6 节：问人的不超时、问机器的才超时）：
//!   - `session/request_permission`（要不要允许跑这条命令 / 改这个文件）：交给前端，人点了才回 —— **不设超时**，超时等于替人拒绝
//!   - `fs/read_text_file`：先问前端编辑器里有没有没保存的那份（问机器的，2 秒没回就读盘）—— agent 读到的是人眼前那一份
//!   - `fs/write_text_file`：只许写项目里的；交给前端写进编辑器（先不存盘，审阅了才存，DIRECTION.md「agent 写文件要走编辑器」）
//! - 会话、进程的生命周期：窗口关了就整组停（`release`），agent 自己退了说一句 stderr 的最后几行
//!
//! 业务都在这里和 acpsvc，命令层（`commands/agent.rs`）只解包、转错误

use crate::owned::Owned;
use acpsvc::conn::{Conn, Incoming};
use acpsvc::process::Agent;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter};

/// 初始化最多等多久：第一次要 npx 下适配器（实测 18–24 秒），网慢的话更久
const INIT_WAIT: Duration = Duration::from_secs(180);
/// 「编辑器里有没有没保存的那份」等前端多久（问机器的：前端没回就当没有、读盘）
const READ_ASK: Duration = Duration::from_secs(2);

/// 哪个 agent
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Kind {
    Claude,
    Codex,
}

impl Kind {
    pub fn parse(s: &str) -> Option<Kind> {
        match s {
            "claude" => Some(Kind::Claude),
            "codex" => Some(Kind::Codex),
            _ => None,
        }
    }
    fn adapter(self) -> &'static str {
        match self {
            Kind::Claude => acpsvc::CLAUDE_ADAPTER,
            Kind::Codex => acpsvc::CODEX_ADAPTER,
        }
    }
}

/// 一个会话
pub struct Session {
    conn: Conn,
    agent: Agent,
    pub session_id: String,
    /// 问前端「编辑器里有没有没保存的那份」、还在等回话的：agent 那条请求的 id（JSON 文本）→ 交回答案的通道
    asks: Arc<Mutex<HashMap<String, Sender<Option<String>>>>>,
    /// 还没回的权限请求：id（JSON 文本）→ 问的是什么。人点了之后往 app.log 记一行（DIRECTION.md「每次授权留一行」）
    perms: Arc<Mutex<HashMap<String, String>>>,
}

/// 起好了交给前端的
pub struct Started {
    pub id: u32,
    pub session_id: String,
    pub name: String,
    pub version: String,
    /// 会话的几种模式（Claude：手动 / 自动接受编辑 / 计划…）和当前那个，原样给前端
    pub modes: Value,
}

#[derive(Default)]
pub struct Agents {
    all: Owned<u32, Arc<Session>>,
    next: AtomicU32,
}

/// 事件负载：哪个会话、什么事、内容
fn event(app: &AppHandle, owner: &str, id: u32, kind: &str, data: Value) {
    let _ = app.emit_to(owner, "agent-event", json!({ "id": id, "kind": kind, "data": data }));
}

/// `path` 在不在项目根里（写文件只许写这里面的）。不碰盘：agent 要写的文件可能还不存在，按词法比（`..` 一律不认）
pub fn inside(root: &Path, path: &str) -> bool {
    let p = Path::new(path);
    p.is_absolute() && p.starts_with(root) && !p.components().any(|c| matches!(c, std::path::Component::ParentDir))
}

/// 照 `fs/read_text_file` 的 `line`（从 1 数）/ `limit` 截一段
pub fn slice_lines(text: &str, line: Option<u64>, limit: Option<u64>) -> String {
    if line.is_none() && limit.is_none() {
        return text.to_string();
    }
    let start = line.unwrap_or(1).saturating_sub(1) as usize;
    let take = limit.map_or(usize::MAX, |l| l as usize);
    text.split_inclusive('\n').skip(start).take(take).collect()
}

impl Agents {
    /// 起一个 agent、初始化、建会话（阻塞：第一次要下适配器，几十秒）。`program` 给了就用它（测试通道里换成按剧本回话的假 agent），
    /// 不然经 npx 起钉死版本的适配器
    pub fn start(&self, app: &AppHandle, owner: &str, root: &str, kind: Kind, shell: &str, program: Option<(PathBuf, Vec<String>)>) -> Result<Started, String> {
        let (prog, args, path_env) = match program {
            Some((p, a)) => (p, a, std::env::var("PATH").unwrap_or_default()),
            None => {
                // 设置里没写 shell 就用 $SHELL（同终端、任务）
                let shell = if shell.is_empty() { std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into()) } else { shell.to_string() };
                let (npx, path) = acpsvc::process::find_npx(&shell)?;
                (npx, vec!["-y".to_string(), kind.adapter().to_string()], path)
            }
        };
        let id = self.next.fetch_add(1, Ordering::Relaxed) + 1;
        let root_path = PathBuf::from(root);
        let (agent, pipes) = {
            let (app, owner) = (app.clone(), owner.to_string());
            Agent::spawn(&prog, &args, &root_path, &path_env, Box::new(move |code| event(&app, &owner, id, "exit", json!({ "code": code }))))
                .map_err(|e| format!("起不来 agent（{}）：{e}", prog.display()))?
        };
        let asks: Arc<Mutex<HashMap<String, Sender<Option<String>>>>> = Arc::default();
        let perms: Arc<Mutex<HashMap<String, String>>> = Arc::default();
        // 读线程上要回话的那几种需要 conn 本身：先放一个空格子，start 之后填上
        let slot: Arc<Mutex<Option<Conn>>> = Arc::default();
        let on_incoming = {
            let (app, owner, slot, asks, perms, root_path) = (app.clone(), owner.to_string(), Arc::clone(&slot), Arc::clone(&asks), Arc::clone(&perms), root_path.clone());
            Box::new(move |m: Incoming| on_incoming(&app, &owner, id, &root_path, &slot, &asks, &perms, m))
        };
        let conn = Conn::start(pipes.stdout, pipes.stdin, on_incoming, Box::new(|_| {})).map_err(|e| format!("接不上 agent：{e}"))?;
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(conn.clone());

        let fail = |what: &str, e: acpsvc::conn::Error| -> String {
            let tail = agent.stderr_tail();
            let last: Vec<&str> = tail.lines().rev().filter(|l| !l.trim().is_empty()).take(4).collect();
            let hint = if last.is_empty() { String::new() } else { format!("\n{}", last.into_iter().rev().collect::<Vec<_>>().join("\n")) };
            format!("{what}：{e}{hint}")
        };
        let init = conn
            .request(
                "initialize",
                json!({ "protocolVersion": 1, "clientCapabilities": acpsvc::client_capabilities(), "clientInfo": { "name": "lite-ide", "version": env!("CARGO_PKG_VERSION") } }),
                Some(INIT_WAIT),
            )
            .map_err(|e| fail("agent 初始化失败", e))?;
        let created = conn
            .request("session/new", json!({ "cwd": root, "mcpServers": [] }), Some(Duration::from_secs(60)))
            .map_err(|e| fail("建不了会话", e))?;
        let session_id = created["sessionId"].as_str().ok_or("agent 没给会话 id")?.to_string();
        applog::write(applog::Level::Info, "agent", &format!("{} 起了 {}（{root}）", init["agentInfo"]["name"].as_str().unwrap_or("?"), session_id));
        let started = Started {
            id,
            session_id: session_id.clone(),
            name: init["agentInfo"]["title"].as_str().or(init["agentInfo"]["name"].as_str()).unwrap_or("agent").to_string(),
            version: init["agentInfo"]["version"].as_str().unwrap_or("").to_string(),
            modes: created.get("modes").cloned().unwrap_or(Value::Null),
        };
        self.all.insert(owner, id, Arc::new(Session { conn, agent, session_id, asks, perms }));
        Ok(started)
    }

    fn get(&self, id: u32) -> Result<Arc<Session>, String> {
        self.all.get(&id, Arc::clone).ok_or_else(|| "这个 AI 会话已经结束了".to_string())
    }

    /// 发一轮话（阻塞到这一轮结束：agent 回 `stopReason`）。`prompt` 是 ACP 的内容块数组（文字、带上下文的资源）
    pub fn prompt(&self, id: u32, prompt: Value) -> Result<String, String> {
        let s = self.get(id)?;
        let r = s
            .conn
            .request("session/prompt", json!({ "sessionId": s.session_id, "prompt": prompt }), None)
            .map_err(|e| format!("{e}"))?;
        Ok(r["stopReason"].as_str().unwrap_or("end_turn").to_string())
    }

    /// 停这一轮（agent 照协议收尾、`session/prompt` 回 `cancelled`）
    pub fn cancel(&self, id: u32) -> Result<(), String> {
        let s = self.get(id)?;
        s.conn.notify("session/cancel", json!({ "sessionId": s.session_id })).map_err(|e| format!("{e}"))
    }

    /// 换会话模式（Claude 的「改文件先问我」/「自动接受编辑」/「计划」）
    pub fn set_mode(&self, id: u32, mode: &str) -> Result<(), String> {
        let s = self.get(id)?;
        s.conn
            .request("session/set_mode", json!({ "sessionId": s.session_id, "modeId": mode }), Some(Duration::from_secs(10)))
            .map(|_| ())
            .map_err(|e| format!("{e}"))
    }

    /// 前端回 agent 问的那个请求。`req` 是 agent 那条请求的 id（原样）。问编辑器内容的那种交给等着的线程（`content` 字段，null = 编辑器里没有）；
    /// 别的（权限、写文件）直接回给 agent：`error` 非空就回错误
    pub fn reply(&self, id: u32, req: Value, result: Value, error: Option<String>) -> Result<(), String> {
        let s = self.get(id)?;
        if let Some(tx) = s.asks.lock().unwrap_or_else(|e| e.into_inner()).remove(&req.to_string()) {
            let _ = tx.send(result["content"].as_str().map(str::to_owned));
            return Ok(());
        }
        if let Some(what) = s.perms.lock().unwrap_or_else(|e| e.into_inner()).remove(&req.to_string()) {
            let chose = result["outcome"]["optionId"].as_str().or(result["outcome"]["outcome"].as_str()).unwrap_or("?");
            applog::write(applog::Level::Info, "agent", &format!("权限：{what} → {chose}"));
        }
        let r = match error {
            Some(m) => Err((-32000, m)),
            None => Ok(result),
        };
        s.conn.respond(&req, r).map_err(|e| format!("{e}"))
    }

    /// 停掉这个会话（整组停 agent 进程）
    pub fn stop(&self, id: u32) {
        if let Some(s) = self.all.remove(&id) {
            std::thread::spawn(move || s.agent.kill());
        }
    }

    /// 窗口关了：它的会话全停（后台停，窗口不用等）
    pub fn release(&self, owner: &str) {
        let gone = self.all.take_owner(owner);
        if !gone.is_empty() {
            std::thread::spawn(move || {
                for (_, s) in gone {
                    s.agent.kill();
                }
            });
        }
    }
}

/// 回 agent 的请求，**另起一条线程回**（rust.md「服务器问我们的另起一条线程答」）：在读线程上写回话，agent 那边要是正卡在往 stdout 写、
/// 不读 stdin，我们卡在写、没人读它的 stdout，两边互相等住
fn answer_later(conn: Option<Conn>, req: Value, r: Result<Value, (i64, String)>) {
    if let Some(c) = conn {
        std::thread::spawn(move || {
            let _ = c.respond(&req, r);
        });
    }
}

/// agent 发来的东西（在 conn 的读线程上：要快，慢的另起线程）
#[allow(clippy::too_many_arguments)]
fn on_incoming(
    app: &AppHandle,
    owner: &str,
    id: u32,
    root: &Path,
    slot: &Arc<Mutex<Option<Conn>>>,
    asks: &Arc<Mutex<HashMap<String, Sender<Option<String>>>>>,
    perms: &Arc<Mutex<HashMap<String, String>>>,
    m: Incoming,
) {
    let conn = || slot.lock().unwrap_or_else(|e| e.into_inner()).clone();
    match m {
        Incoming::Notify { method, params } if method == "session/update" => event(app, owner, id, "update", params["update"].clone()),
        // 适配器的私有通知（`_auth/status_update` 之类）：不认的不管
        Incoming::Notify { .. } => {}
        Incoming::Request { id: req, method, params } => match method.as_str() {
            "session/request_permission" => {
                let what = params["toolCall"]["title"].as_str().unwrap_or("?").to_string();
                perms.lock().unwrap_or_else(|e| e.into_inner()).insert(req.to_string(), what);
                event(app, owner, id, "permission", json!({ "req": req, "params": params }));
            }
            "fs/write_text_file" => {
                let path = params["path"].as_str().unwrap_or("");
                if !inside(root, path) {
                    applog::write(applog::Level::Warn, "agent", &format!("拒绝写项目外面的文件：{path}"));
                    let why = format!("只许改项目里的文件（{} 底下），{path} 不在里面", root.display());
                    answer_later(conn(), req, Err((-32000, why)));
                    return;
                }
                event(app, owner, id, "write", json!({ "req": req, "path": path, "content": params["content"] }));
            }
            "fs/read_text_file" => {
                // 问机器的：另起线程等前端（别堵住读线程），2 秒没回就读盘
                let (app, owner, asks, conn) = (app.clone(), owner.to_string(), Arc::clone(asks), conn());
                std::thread::spawn(move || {
                    let Some(conn) = conn else { return };
                    let path = params["path"].as_str().unwrap_or("").to_string();
                    let (tx, rx) = mpsc::channel();
                    asks.lock().unwrap_or_else(|e| e.into_inner()).insert(req.to_string(), tx);
                    event(&app, &owner, id, "read", json!({ "req": req, "path": path }));
                    let live = rx.recv_timeout(READ_ASK).ok().flatten();
                    asks.lock().unwrap_or_else(|e| e.into_inner()).remove(&req.to_string());
                    let text = match live {
                        Some(t) => Ok(t),
                        None => fsservice::read_text_detect(&path, "").map(|d| d.content).map_err(|e| (-32000, format!("读不了 {path}：{e}"))),
                    };
                    let r = text.map(|t| json!({ "content": slice_lines(&t, params["line"].as_u64(), params["limit"].as_u64()) }));
                    let _ = conn.respond(&req, r);
                });
            }
            other => answer_later(conn(), req, Err((-32601, format!("lite-ide 不支持 {other}")))),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 写文件只许写项目里面的() {
        let root = Path::new("/p/proj");
        assert!(inside(root, "/p/proj/src/A.java"));
        assert!(!inside(root, "/p/proj/../other/A.java"), "带 .. 的不认");
        assert!(!inside(root, "/p/project2/A.java"), "名字前缀相同的别的目录不算");
        assert!(!inside(root, "src/A.java"), "相对路径不认");
        assert!(!inside(root, "/etc/hosts"));
    }

    #[test]
    fn 读文件照_line_和_limit_截() {
        let t = "a\nb\nc\nd\n";
        assert_eq!(slice_lines(t, None, None), t);
        assert_eq!(slice_lines(t, Some(2), Some(2)), "b\nc\n");
        assert_eq!(slice_lines(t, Some(3), None), "c\nd\n");
        assert_eq!(slice_lines(t, None, Some(1)), "a\n");
    }
}
