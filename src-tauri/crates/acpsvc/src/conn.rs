//! ACP 的线：stdin / stdout 上的 JSON-RPC 2.0，**一行一条**（不是 LSP 那种 `Content-Length` 头）。和进程无关 —— 读写两头是参数，测试拿一对 socket 冒充 agent。
//!
//! 和 `lspsvc::conn` 同一副骨架（读线程只读、调用方在通道上等回话、断线时同一把锁下把所有在等的判失败），差在一处：
//! **agent 反过来问宿主的请求不在这里同步答**。ACP 里有「要不要允许跑这条命令」这种问人的，人可能几分钟才点；
//! 同步答的话，后面 agent 要读文件的请求全排在它后面。所以收到就交给 `on_incoming`（要快：产品里只是转给前端或另起一条线程），
//! 谁有答案了谁调 [`Conn::respond`] 按 id 回。
//!
//! 一行多长由 agent 说了算（读一个大文件、一大段回答），所以有上限 [`MAX_LINE`]：超了这一行读掉丢弃、不断线，
//! 从开头那段里找出 id 把「太大」交给等它的请求（同 lspsvc 的理由：不能让它干等）。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 一行最多多长。Claude 读一个几 MB 的文件也到不了；64 MB 是「不正常」的那一档
pub const MAX_LINE: usize = 64 << 20;
/// 超长那一行里留开头多少字节去找 id
const ID_PEEK: usize = 512;

#[derive(Debug, PartialEq)]
pub enum Error {
    /// 等满了还没回
    Timeout,
    /// 线断了（agent 退了、管道关了）
    Closed,
    /// 回来的那一行比 [`MAX_LINE`] 长，读掉丢了。带真字节数
    TooLarge(usize),
    /// agent 回的是 error
    Rpc { code: i64, message: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Timeout => write!(f, "agent 没有及时回答"),
            Error::Closed => write!(f, "agent 已经退出"),
            Error::TooLarge(n) => write!(f, "回答太大（{} MB，上限 {} MB），没有读", n >> 20, MAX_LINE >> 20),
            Error::Rpc { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Reply = Result<Value, Error>;

/// agent 发来的：通知（`session/update`…），或者问宿主的请求（`session/request_permission`、`fs/read_text_file`…）。
/// 请求带着 id，答案用 [`Conn::respond`] 回
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Notify { method: String, params: Value },
    Request { id: Value, method: String, params: Value },
}

/// 在读线程上调，要快
pub type OnIncoming = Box<dyn Fn(Incoming) + Send>;

struct Inner {
    out: Mutex<Box<dyn Write + Send>>,
    /// 在等回话的请求。`closed` 和它在同一把锁下改（同 lspsvc：断线之后才登记的那个不会没人回）
    pending: Mutex<Pending>,
    next: AtomicU64,
}

struct Pending {
    waiting: HashMap<u64, Sender<Reply>>,
    closed: bool,
}

#[derive(Clone)]
pub struct Conn(Arc<Inner>);

impl Conn {
    /// 接上一条线、起读线程。`on_close` 在读线程发现断线时调一次（带原因）
    pub fn start(
        read: impl Read + Send + 'static,
        write: impl Write + Send + 'static,
        on_incoming: OnIncoming,
        on_close: Box<dyn FnOnce(String) + Send>,
    ) -> io::Result<Conn> {
        let inner = Arc::new(Inner {
            out: Mutex::new(Box::new(write)),
            pending: Mutex::new(Pending { waiting: HashMap::new(), closed: false }),
            next: AtomicU64::new(1),
        });
        let reader = Arc::clone(&inner);
        std::thread::Builder::new().name("acp-read".into()).spawn(move || {
            let why = read_loop(BufReader::new(read), &reader, &on_incoming);
            let mut p = reader.pending.lock().unwrap_or_else(|e| e.into_inner());
            p.closed = true;
            for (_, tx) in p.waiting.drain() {
                let _ = tx.send(Err(Error::Closed));
            }
            drop(p);
            on_close(why);
        })?;
        Ok(Conn(inner))
    }

    /// 发一个请求、等回话。`timeout` 为 None 就一直等到回话或断线 —— `session/prompt` 一轮可能跑好几分钟，
    /// 停它靠 `session/cancel` 通知（agent 收到后照协议回 `stopReason: cancelled`），不靠这里超时
    pub fn request(&self, method: &str, params: Value, timeout: Option<Duration>) -> Reply {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            let mut p = self.0.pending.lock().unwrap_or_else(|e| e.into_inner());
            if p.closed {
                return Err(Error::Closed);
            }
            p.waiting.insert(id, tx);
        }
        if write_line(&self.0, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).is_err() {
            self.forget(id);
            return Err(Error::Closed);
        }
        match timeout {
            None => rx.recv().unwrap_or(Err(Error::Closed)),
            Some(t) => match rx.recv_timeout(t) {
                Ok(r) => r,
                Err(RecvTimeoutError::Timeout) => {
                    self.forget(id);
                    Err(Error::Timeout)
                }
                Err(RecvTimeoutError::Disconnected) => Err(Error::Closed),
            },
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        write_line(&self.0, &json!({ "jsonrpc": "2.0", "method": method, "params": params })).map_err(|_| Error::Closed)
    }

    /// 回 agent 问的那个请求：`Ok(结果)` 或 `Err((错误码, 说明))`
    pub fn respond(&self, id: &Value, result: Result<Value, (i64, String)>) -> Result<(), Error> {
        let msg = match result {
            Ok(r) => json!({ "jsonrpc": "2.0", "id": id, "result": r }),
            Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
        };
        write_line(&self.0, &msg).map_err(|_| Error::Closed)
    }

    pub fn is_closed(&self) -> bool {
        self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    fn forget(&self, id: u64) {
        self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).waiting.remove(&id);
    }
}

fn write_line(inner: &Inner, msg: &Value) -> io::Result<()> {
    // serde_json 的紧凑输出里不会有裸换行（字符串里的换行写成 \n），一行一条成立
    let mut buf = serde_json::to_vec(msg)?;
    buf.push(b'\n');
    let mut out = inner.out.lock().unwrap_or_else(|e| e.into_inner());
    out.write_all(&buf)?;
    out.flush()
}

/// 读到断线为止；返回断的原因
fn read_loop(mut r: impl BufRead, inner: &Inner, on_incoming: &OnIncoming) -> String {
    loop {
        let mut line = Vec::new();
        let n = match (&mut r).take(MAX_LINE as u64 + 1).read_until(b'\n', &mut line) {
            Ok(n) => n,
            Err(e) => return format!("读出错：{e}"),
        };
        if n == 0 {
            return "agent 关掉了输出".into();
        }
        if line.len() > MAX_LINE && line.last() != Some(&b'\n') {
            // 超长：剩下的读到换行为止扔掉，按开头那段找 id 告诉等它的请求
            let mut total = line.len();
            let mut sink = Vec::new();
            loop {
                sink.clear();
                match (&mut r).take(1 << 20).read_until(b'\n', &mut sink) {
                    Ok(0) => return "agent 关掉了输出".into(),
                    Ok(k) => {
                        total += k;
                        if sink.last() == Some(&b'\n') {
                            break;
                        }
                    }
                    Err(e) => return format!("读出错：{e}"),
                }
            }
            if let Some(id) = peek_id(&line[..ID_PEEK.min(line.len())]) {
                if let Some(tx) = inner.pending.lock().unwrap_or_else(|e| e.into_inner()).waiting.remove(&id) {
                    let _ = tx.send(Err(Error::TooLarge(total)));
                }
            }
            continue;
        }
        let Ok(msg) = serde_json::from_slice::<Value>(&line) else {
            // 不是 JSON 的一行（适配器往 stdout 打了句日志）不断线，下一行照读
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = msg.get("id").cloned();
        match (method, id) {
            (Some(method), Some(id)) => on_incoming(Incoming::Request { id, method, params: msg.get("params").cloned().unwrap_or(Value::Null) }),
            (Some(method), None) => on_incoming(Incoming::Notify { method, params: msg.get("params").cloned().unwrap_or(Value::Null) }),
            (None, Some(id)) => {
                let Some(id) = id.as_u64() else { continue };
                let Some(tx) = inner.pending.lock().unwrap_or_else(|e| e.into_inner()).waiting.remove(&id) else { continue };
                let reply = match msg.get("error") {
                    Some(e) => Err(Error::Rpc {
                        code: e.get("code").and_then(Value::as_i64).unwrap_or(0),
                        message: e.get("message").and_then(Value::as_str).unwrap_or("").to_owned(),
                    }),
                    None => Ok(msg.get("result").cloned().unwrap_or(Value::Null)),
                };
                let _ = tx.send(reply);
            }
            (None, None) => {}
        }
    }
}

/// 从一条 JSON 的开头那段里找 `"id":<数>`（只用在超长、不解析的那种上）
fn peek_id(head: &[u8]) -> Option<u64> {
    let s = String::from_utf8_lossy(head);
    let at = s.find("\"id\"")? + 4;
    let rest = s[at..].trim_start().strip_prefix(':')?.trim_start();
    let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;
    use std::time::Instant;

    /// 冒充 agent 的那一头：一行一条
    struct Fake {
        r: BufReader<UnixStream>,
        w: UnixStream,
    }

    impl Fake {
        fn read(&mut self) -> Value {
            let mut l = String::new();
            self.r.read_line(&mut l).unwrap();
            serde_json::from_str(&l).unwrap()
        }
        fn write(&mut self, v: Value) {
            self.w.write_all(format!("{v}\n").as_bytes()).unwrap();
        }
    }

    fn pair(on: OnIncoming) -> (Conn, Fake, mpsc::Receiver<String>) {
        let (a, b) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        let conn = Conn::start(a.try_clone().unwrap(), a, on, Box::new(move |why| tx.send(why).unwrap())).unwrap();
        (conn, Fake { r: BufReader::new(b.try_clone().unwrap()), w: b }, rx)
    }

    #[test]
    fn 回话按_id_配给发它的那个请求_不是_json_的行跳过() {
        let (conn, mut fake, _) = pair(Box::new(|_| {}));
        let c2 = conn.clone();
        let h = std::thread::spawn(move || c2.request("initialize", json!({}), Some(Duration::from_secs(5))));
        let req = fake.read();
        assert_eq!(req["method"], "initialize");
        assert!(req["id"].is_u64());
        fake.w.write_all(b"npm WARN deprecated something\n").unwrap();
        fake.write(json!({ "jsonrpc": "2.0", "id": req["id"], "result": { "protocolVersion": 1 } }));
        assert_eq!(h.join().unwrap().unwrap()["protocolVersion"], 1);
    }

    /// agent 反过来问的请求：交出去、晚点再回（问人的那种可能等几分钟）；等它的这段时间里，别的通知照样到
    #[test]
    fn 反向请求延后回答_期间通知照样到() {
        let (tx, rx) = mpsc::channel::<Incoming>();
        let (conn, mut fake, _) = pair(Box::new(move |m| tx.send(m).unwrap()));
        fake.write(json!({ "jsonrpc": "2.0", "id": 7, "method": "session/request_permission", "params": { "toolCall": {} } }));
        fake.write(json!({ "jsonrpc": "2.0", "method": "session/update", "params": { "update": { "sessionUpdate": "agent_message_chunk" } } }));
        let first = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let Incoming::Request { id, method, .. } = first else { panic!("{first:?}") };
        assert_eq!(method, "session/request_permission");
        let second = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert!(matches!(second, Incoming::Notify { ref method, .. } if method == "session/update"), "问人的还没答，通知照样到：{second:?}");
        conn.respond(&id, Ok(json!({ "outcome": { "outcome": "selected", "optionId": "allow" } }))).unwrap();
        let back = fake.read();
        assert_eq!((back["id"].clone(), back["result"]["outcome"]["optionId"].clone()), (json!(7), json!("allow")));
    }

    #[test]
    fn 断线时在等的请求马上失败_不等超时() {
        let (conn, fake, closed) = pair(Box::new(|_| {}));
        let c2 = conn.clone();
        let t0 = Instant::now();
        let h = std::thread::spawn(move || c2.request("session/prompt", json!({}), None));
        std::thread::sleep(Duration::from_millis(50));
        drop(fake);
        assert_eq!(h.join().unwrap(), Err(Error::Closed));
        assert!(t0.elapsed() < Duration::from_secs(2), "没设超时的请求，断线时也要马上回：{:?}", t0.elapsed());
        assert!(closed.recv_timeout(Duration::from_secs(2)).is_ok());
        assert!(conn.is_closed());
    }

    #[test]
    fn 超时只放弃这一个() {
        let (conn, mut fake, _) = pair(Box::new(|_| {}));
        assert_eq!(conn.request("x", json!({}), Some(Duration::from_millis(80))), Err(Error::Timeout));
        let _ = fake.read();
        let c2 = conn.clone();
        let h = std::thread::spawn(move || c2.request("y", json!({}), Some(Duration::from_secs(5))));
        let req = fake.read();
        fake.write(json!({ "jsonrpc": "2.0", "id": req["id"], "result": "ok" }));
        assert_eq!(h.join().unwrap().unwrap(), json!("ok"));
    }

    #[test]
    fn 找_id_只认开头那段() {
        assert_eq!(peek_id(br#"{"jsonrpc":"2.0","id": 42,"result":"#), Some(42));
        assert_eq!(peek_id(br#"{"jsonrpc":"2.0","method":"x"}"#), None);
    }
}
