//! LSP 的线：stdin / stdout 上的 JSON-RPC，`Content-Length` 分帧。和进程无关 —— 读写两头是参数，测试拿一对 socket 冒充服务器。
//!
//! 三条线程：
//!
//! - **读线程**：分帧、解析、分发。响应交给等它的那个请求；通知当场交给 `on_notify`（要快：产品里只是转发给前端）。
//! - **答话线程**：服务器问我们的（`workspace/configuration`、`client/registerCapability`…）**不在读线程里答**。
//!   读线程要是卡在写回话上（服务器那头没在读 stdin、因为它正忙着往 stdout 写给我们），就没人排空它的 stdout，两边互相等住 ——
//!   rust.md「两个管道要并发读」那个死锁的同一个形状。答话挪到另一条线程，读线程永远只读。
//! - 调用方自己的线程：`request` 写完就在一个通道上等，带超时。
//!
//! 一条消息多大由服务器说了算（查用法几万处、一个大 class 的反编译），所以有上限 [`MAX_MESSAGE`]：超了不解析、读掉丢弃，
//! 但**要说出来** —— 从开头那一小段里找出 id，把「太大」交给等它的那个请求，而不是让它干等到超时、以为服务器卡了。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 一条消息最多多大。jdtls 在 petclinic 上查一个常用类的用法几十 KB；64MB 是「不正常」的那一档
pub const MAX_MESSAGE: usize = 64 << 20;
/// 一行头最多多长：头只有 `Content-Length` / `Content-Type` 两种，几十字节；读不到换行就一直读是另一种没有上限
const MAX_HEADER_LINE: usize = 1024;
/// 超大消息里留开头多少字节去找 id
const ID_PEEK: usize = 512;

#[derive(Debug)]
pub enum Error {
    /// 等满了还没回。已经发过 `$/cancelRequest`
    Timeout,
    /// 线断了（服务器退了、管道关了）
    Closed,
    /// 回来的那条消息比 [`MAX_MESSAGE`] 大，读掉丢了。带真字节数
    TooLarge(usize),
    /// 服务器回的是 error
    Rpc { code: i64, message: String },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Timeout => write!(f, "语言服务器没有及时回答"),
            Error::Closed => write!(f, "语言服务器已经退出"),
            Error::TooLarge(n) => write!(f, "回答太大（{} MB，上限 {} MB），没有读", n >> 20, MAX_MESSAGE >> 20),
            Error::Rpc { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for Error {}

type Reply = Result<Value, Error>;

/// 服务器问我们的，怎么答：`Ok(结果)` 或 `Err((错误码, 说明))`
pub type OnRequest = Box<dyn Fn(&str, &Value) -> Result<Value, (i64, String)> + Send>;
/// 服务器发来的通知（`textDocument/publishDiagnostics`、`language/status`…）。在读线程上调，要快
pub type OnNotify = Box<dyn Fn(&str, Value) + Send>;

struct Inner {
    out: Mutex<Box<dyn Write + Send>>,
    /// 在等回话的请求。`closed` 和它在**同一把锁**下改：断线时「全部判失败」和「新请求登记」不会错开 ——
    /// 错开的话，断线之后才登记的那个永远没人回，只能等满超时
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
    /// 接上一条线：起读线程和答话线程。`on_close` 在读线程发现断线时调一次（带原因）
    pub fn start(
        read: impl Read + Send + 'static,
        write: impl Write + Send + 'static,
        on_request: OnRequest,
        on_notify: OnNotify,
        on_close: Box<dyn FnOnce(String) + Send>,
    ) -> io::Result<Conn> {
        let inner = Arc::new(Inner {
            out: Mutex::new(Box::new(write)),
            pending: Mutex::new(Pending { waiting: HashMap::new(), closed: false }),
            next: AtomicU64::new(1),
        });
        let (ask_tx, ask_rx) = mpsc::channel::<(Value, String, Value)>();
        let answer = Arc::clone(&inner);
        std::thread::Builder::new().name("lsp-answer".into()).spawn(move || {
            for (id, method, params) in ask_rx {
                let msg = match on_request(&method, &params) {
                    Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                    Err((code, message)) => json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } }),
                };
                if write_msg(&answer, &msg).is_err() {
                    return;
                }
            }
        })?;
        let reader = Arc::clone(&inner);
        std::thread::Builder::new().name("lsp-read".into()).spawn(move || {
            let why = read_loop(BufReader::new(read), &reader, &ask_tx, &on_notify);
            drop(ask_tx);
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

    /// 发一个请求、等回话，最多等 `timeout`。超时会给服务器发 `$/cancelRequest`（它可能还在算，让它别算了）
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Reply {
        let id = self.0.next.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = mpsc::channel();
        {
            let mut p = self.0.pending.lock().unwrap_or_else(|e| e.into_inner());
            if p.closed {
                return Err(Error::Closed);
            }
            p.waiting.insert(id, tx);
        }
        if write_msg(&self.0, &json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })).is_err() {
            self.forget(id);
            return Err(Error::Closed);
        }
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                self.forget(id);
                let _ = self.notify("$/cancelRequest", json!({ "id": id }));
                Err(Error::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(Error::Closed),
        }
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        write_msg(&self.0, &json!({ "jsonrpc": "2.0", "method": method, "params": params })).map_err(|_| Error::Closed)
    }

    pub fn is_closed(&self) -> bool {
        self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).closed
    }

    fn forget(&self, id: u64) {
        self.0.pending.lock().unwrap_or_else(|e| e.into_inner()).waiting.remove(&id);
    }
}

fn write_msg(inner: &Inner, msg: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(msg)?;
    let mut out = inner.out.lock().unwrap_or_else(|e| e.into_inner());
    // 头和正文一次写出去：分两次写，中间另一条线程插进来一条，服务器那头的分帧就错位了（锁保证了这个，一次写省一个系统调用）
    let mut buf = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    buf.extend_from_slice(&body);
    out.write_all(&buf)?;
    out.flush()
}

/// 读到断线为止；返回断的原因（给 `on_close`，最后会出现在状态栏的提示里）
fn read_loop(mut r: impl BufRead, inner: &Inner, ask: &Sender<(Value, String, Value)>, on_notify: &OnNotify) -> String {
    loop {
        let len = match read_header(&mut r) {
            Ok(Some(n)) => n,
            Ok(None) => return "服务器关掉了输出".into(),
            Err(e) => return format!("读消息头出错：{e}"),
        };
        if len > MAX_MESSAGE {
            let mut head = vec![0u8; ID_PEEK.min(len)];
            if let Err(e) = r.read_exact(&mut head) {
                return format!("读超大消息出错：{e}");
            }
            if let Err(e) = io::copy(&mut (&mut r).take((len - head.len()) as u64), &mut io::sink()) {
                return format!("读超大消息出错：{e}");
            }
            if let Some(id) = peek_id(&head) {
                if let Some(tx) = inner.pending.lock().unwrap_or_else(|e| e.into_inner()).waiting.remove(&id) {
                    let _ = tx.send(Err(Error::TooLarge(len)));
                }
            }
            continue;
        }
        let mut body = vec![0u8; len];
        if let Err(e) = r.read_exact(&mut body) {
            return format!("读消息出错：{e}");
        }
        let Ok(msg) = serde_json::from_slice::<Value>(&body) else {
            // 一条坏的不断线：分帧还是对的（长度是头给的），下一条照读
            continue;
        };
        let method = msg.get("method").and_then(Value::as_str).map(str::to_owned);
        let id = msg.get("id").cloned();
        match (method, id) {
            (Some(m), Some(id)) => {
                let params = msg.get("params").cloned().unwrap_or(Value::Null);
                let _ = ask.send((id, m, params));
            }
            (Some(m), None) => on_notify(&m, msg.get("params").cloned().unwrap_or(Value::Null)),
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

/// 读一组头，返回 `Content-Length`。`None` = 干净的 EOF（在两条消息之间断的）
fn read_header(r: &mut impl BufRead) -> io::Result<Option<usize>> {
    let mut len = None;
    let mut first = true;
    loop {
        let mut line = Vec::new();
        let n = (&mut *r).take(MAX_HEADER_LINE as u64).read_until(b'\n', &mut line)?;
        if n == 0 {
            return if first { Ok(None) } else { Err(io::ErrorKind::UnexpectedEof.into()) };
        }
        if line.last() != Some(&b'\n') {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "消息头一行太长"));
        }
        first = false;
        let line = String::from_utf8_lossy(&line);
        let line = line.trim_end();
        if line.is_empty() {
            return len.map(Some).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "消息头里没有 Content-Length"));
        }
        if let Some((k, v)) = line.split_once(':') {
            if k.trim().eq_ignore_ascii_case("content-length") {
                len = Some(v.trim().parse().map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "Content-Length 不是数"))?);
            }
        }
    }
}

/// 从一条 JSON 的开头那段里找 `"id":<数>`。只用在超大、不解析的那种消息上；gson / Jackson 都把 id 写在前面
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

    /// 冒充服务器的那一头：读一条、写一条，按协议分帧
    struct Fake {
        r: BufReader<UnixStream>,
        w: UnixStream,
    }

    impl Fake {
        fn read(&mut self) -> Value {
            let n = read_header(&mut self.r).unwrap().unwrap();
            let mut b = vec![0; n];
            self.r.read_exact(&mut b).unwrap();
            serde_json::from_slice(&b).unwrap()
        }
        fn write_raw(&mut self, body: &[u8]) {
            self.w.write_all(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes()).unwrap();
            self.w.write_all(body).unwrap();
        }
        fn write(&mut self, v: Value) {
            self.write_raw(&serde_json::to_vec(&v).unwrap());
        }
    }

    fn pair(on_request: OnRequest, on_notify: OnNotify) -> (Conn, Fake, mpsc::Receiver<String>) {
        let (a, b) = UnixStream::pair().unwrap();
        let (tx, rx) = mpsc::channel();
        let conn = Conn::start(a.try_clone().unwrap(), a, on_request, on_notify, Box::new(move |why| tx.send(why).unwrap())).unwrap();
        (conn, Fake { r: BufReader::new(b.try_clone().unwrap()), w: b }, rx)
    }

    fn quiet() -> (OnRequest, OnNotify) {
        (Box::new(|_, _| Ok(Value::Null)), Box::new(|_, _| {}))
    }

    #[test]
    fn 回话按_id_配给发它的那个请求_顺序反过来也对() {
        let (q, n) = quiet();
        let (conn, mut srv, _) = pair(q, n);
        let c1 = conn.clone();
        let c2 = conn.clone();
        let a = std::thread::spawn(move || c1.request("a", json!(1), Duration::from_secs(5)));
        let first = srv.read();
        let b = std::thread::spawn(move || c2.request("b", json!(2), Duration::from_secs(5)));
        let second = srv.read();
        // 先回后来的那个
        srv.write(json!({ "jsonrpc": "2.0", "id": second["id"], "result": format!("回 {}", second["method"]) }));
        srv.write(json!({ "jsonrpc": "2.0", "id": first["id"], "result": format!("回 {}", first["method"]) }));
        let ra = a.join().unwrap().unwrap();
        let rb = b.join().unwrap().unwrap();
        assert_eq!(ra, json!(format!("回 {}", first["method"])));
        assert_eq!(rb, json!(format!("回 {}", second["method"])));
        assert_ne!(ra, rb);
    }

    #[test]
    fn 服务器回错误_原样交回() {
        let (q, n) = quiet();
        let (conn, mut srv, _) = pair(q, n);
        let t = std::thread::spawn(move || conn.request("x", Value::Null, Duration::from_secs(5)));
        let m = srv.read();
        srv.write(json!({ "jsonrpc": "2.0", "id": m["id"], "error": { "code": -32601, "message": "没这个方法" } }));
        match t.join().unwrap() {
            Err(Error::Rpc { code, message }) => assert_eq!((code, message.as_str()), (-32601, "没这个方法")),
            r => panic!("{r:?}"),
        }
    }

    #[test]
    fn 等满超时_发取消_不留在等待表里() {
        let (q, n) = quiet();
        let (conn, mut srv, _) = pair(q, n);
        let t0 = Instant::now();
        let r = conn.request("slow", Value::Null, Duration::from_millis(150));
        assert!(matches!(r, Err(Error::Timeout)), "{r:?}");
        assert!(t0.elapsed() < Duration::from_secs(2));
        let req = srv.read();
        let cancel = srv.read();
        assert_eq!(cancel["method"], "$/cancelRequest");
        assert_eq!(cancel["params"]["id"], req["id"]);
        assert!(conn.0.pending.lock().unwrap().waiting.is_empty());
    }

    #[test]
    fn 断线_在等的请求立刻失败_之后的请求也立刻失败() {
        let (q, n) = quiet();
        let (conn, srv, closed) = pair(q, n);
        let c = conn.clone();
        let t0 = Instant::now();
        let t = std::thread::spawn(move || c.request("x", Value::Null, Duration::from_secs(30)));
        std::thread::sleep(Duration::from_millis(100));
        drop(srv);
        assert!(matches!(t.join().unwrap(), Err(Error::Closed)));
        assert!(t0.elapsed() < Duration::from_secs(5), "断线要立刻判失败，不是等满 30 秒");
        assert!(!closed.recv_timeout(Duration::from_secs(5)).unwrap().is_empty());
        assert!(matches!(conn.request("y", Value::Null, Duration::from_secs(30)), Err(Error::Closed)));
        assert!(conn.is_closed());
    }

    #[test]
    fn 服务器问我们的_在另一条线程上答_答话期间照样能收() {
        // 答话的处理函数卡住 400ms：要是在读线程上答，这段时间里服务器发来的通知就收不到
        let (got_tx, got_rx) = mpsc::channel();
        let (conn, mut srv, _) = pair(
            Box::new(|m, _| {
                std::thread::sleep(Duration::from_millis(400));
                Ok(json!(format!("答 {m}")))
            }),
            Box::new(move |m, _| got_tx.send(m.to_owned()).unwrap()),
        );
        srv.write(json!({ "jsonrpc": "2.0", "id": 7, "method": "workspace/configuration", "params": {} }));
        srv.write(json!({ "jsonrpc": "2.0", "method": "language/status", "params": {} }));
        assert_eq!(got_rx.recv_timeout(Duration::from_millis(200)).unwrap(), "language/status", "答话卡住时通知被堵住了");
        let ans = srv.read();
        assert_eq!((ans["id"].clone(), ans["result"].clone()), (json!(7), json!("答 workspace/configuration")));
        drop(conn);
    }

    #[test]
    fn 超大消息读掉不解析_等它的请求拿到太大和真字节数_之后的消息照读() {
        let (q, n) = quiet();
        let (conn, mut srv, _) = pair(q, n);
        let c = conn.clone();
        let t = std::thread::spawn(move || c.request("big", Value::Null, Duration::from_secs(20)));
        let m = srv.read();
        let mut big = format!("{{\"jsonrpc\":\"2.0\",\"id\":{},\"result\":\"", m["id"]).into_bytes();
        big.resize(MAX_MESSAGE + 10, b'x');
        big.extend_from_slice(b"\"}");
        let n = big.len();
        let w = std::thread::spawn(move || {
            srv.write_raw(&big);
            srv
        });
        match t.join().unwrap() {
            Err(Error::TooLarge(got)) => assert_eq!(got, n),
            r => panic!("{r:?}"),
        }
        let mut srv = w.join().unwrap();
        let c = conn.clone();
        let t = std::thread::spawn(move || c.request("small", Value::Null, Duration::from_secs(5)));
        let m = srv.read();
        srv.write(json!({ "jsonrpc": "2.0", "id": m["id"], "result": "ok" }));
        assert_eq!(t.join().unwrap().unwrap(), json!("ok"), "超大那条之后分帧要还是对的");
    }

    #[test]
    fn 一条写坏的_json_不断线() {
        let (q, n) = quiet();
        let (conn, mut srv, _) = pair(q, n);
        let c = conn.clone();
        let t = std::thread::spawn(move || c.request("x", Value::Null, Duration::from_secs(5)));
        let m = srv.read();
        srv.write_raw(b"{not json");
        srv.write(json!({ "jsonrpc": "2.0", "id": m["id"], "result": 1 }));
        assert_eq!(t.join().unwrap().unwrap(), json!(1));
    }

    #[test]
    fn 找超大消息的_id() {
        assert_eq!(peek_id(br#"{"jsonrpc":"2.0","id":42,"result":["#), Some(42));
        assert_eq!(peek_id(br#"{"id" : 7,"#), Some(7));
        assert_eq!(peek_id(br#"{"jsonrpc":"2.0","method":"x"#), None);
    }
}
