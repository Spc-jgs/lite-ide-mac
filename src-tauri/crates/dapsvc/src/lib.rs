//! 调试适配器协议（DAP）的客户端：TCP 上 `Content-Length` 分帧的 JSON。和 LSP 同一种分帧，消息形状不同（`seq` / `request_seq` / `type: event`），
//! 所以不硬套 lspsvc 的连接层，自己写一个小的（docs/IDEA.md 第 18 节）。
//!
//! 适配器是 java-debug（jdtls 的插件）：jdtls 的 `vscode.java.startDebugSession` 命令起一个、回一个端口，我们连上去说 DAP。
//!
//! 三条线程，同 lspsvc 的理由：
//! - **读线程**：分帧、解析、分发。响应交给等它的那个请求；事件当场交给 `on_event`（产品里只是转发给前端，要快）。
//! - **答话线程**：适配器反过来问我们的（`runInTerminal` 这类，我们走 attach 用不上）**不在读线程里答**，一律回「不支持」——
//!   读线程卡在写回话上的话没人读它发来的，两边互相等住（rust.md「两个管道要并发读」那个死锁的形状）。
//! - 调用方自己的线程：`request` 写完在一个通道上等，带超时。
//!
//! **线断了，在等的请求当场收到 `Closed`**，不干等到超时 —— 被调试的程序退了、适配器跟着退，界面上的「单步」按下去要立刻说「会话结束了」。

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// 一条消息最多多大。变量展开一个几万元素的集合是几 MB；16MB 是「不正常」那一档 —— 超了读掉丢弃，等它的请求收到 `TooLarge`
pub const MAX_MESSAGE: usize = 16 << 20;
/// 一行头最多多长（只有 `Content-Length` 一种，几十字节）
const MAX_HEADER_LINE: usize = 1024;

#[derive(Debug, PartialEq)]
pub enum Error {
    /// 等满了还没回
    Timeout,
    /// 线断了（适配器退了、被调试的程序退了）
    Closed,
    /// 回来的那条比 [`MAX_MESSAGE`] 大，读掉丢了。带真字节数
    TooLarge(usize),
    /// 适配器回的是 `success: false`，带它说的原因
    Failed(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Timeout => write!(f, "调试器没回答"),
            Error::Closed => write!(f, "调试会话已经结束了"),
            Error::TooLarge(n) => write!(f, "调试器回的太大（{} MB），没读", n >> 20),
            Error::Failed(m) => write!(f, "{m}"),
        }
    }
}

type Waiters = Arc<Mutex<HashMap<i64, Sender<Result<Value, Error>>>>>;

/// 一个活着的调试会话（到适配器的一条连接）。**drop 即断**：关掉 socket，读线程退出，在等的请求收到 `Closed`
pub struct Session {
    out: Arc<Mutex<TcpStream>>,
    seq: Arc<AtomicI64>,
    waiters: Waiters,
    closed: Arc<AtomicBool>,
}

fn write_msg(w: &mut impl Write, v: &Value) -> io::Result<()> {
    let body = v.to_string();
    write!(w, "Content-Length: {}\r\n\r\n", body.len())?;
    w.write_all(body.as_bytes())?;
    w.flush()
}

/// 读一条：头（只认 `Content-Length`）+ 正文。超过 [`MAX_MESSAGE`] 的读掉丢弃、返回 `Err(大小)`；线断了返回 None
fn read_msg(r: &mut impl BufRead) -> Option<Result<Vec<u8>, usize>> {
    let mut len: Option<usize> = None;
    loop {
        let mut line = String::new();
        let n = r.by_ref().take(MAX_HEADER_LINE as u64).read_line(&mut line).ok()?;
        if n == 0 {
            return None;
        }
        let l = line.trim();
        if l.is_empty() {
            if len.is_some() {
                break;
            }
            continue;
        }
        if let Some(v) = l.strip_prefix("Content-Length:") {
            len = v.trim().parse().ok();
        }
    }
    let len = len?;
    if len > MAX_MESSAGE {
        io::copy(&mut r.by_ref().take(len as u64), &mut io::sink()).ok()?;
        return Some(Err(len));
    }
    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf).ok()?;
    Some(Ok(buf))
}

impl Session {
    /// 连上 `127.0.0.1:port` 的适配器。`on_event`：适配器发来的事件（`stopped` / `output` / `terminated` …），整条原样；
    /// `on_close`：线断了（只调一次）
    pub fn connect(port: u16, on_event: Box<dyn Fn(Value) + Send>, on_close: Box<dyn FnOnce() + Send>) -> io::Result<Session> {
        let sock = TcpStream::connect(("127.0.0.1", port))?;
        sock.set_nodelay(true).ok();
        Self::start(sock, on_event, on_close)
    }

    fn start(sock: TcpStream, on_event: Box<dyn Fn(Value) + Send>, on_close: Box<dyn FnOnce() + Send>) -> io::Result<Session> {
        let out = Arc::new(Mutex::new(sock.try_clone()?));
        let waiters: Waiters = Arc::default();
        let closed = Arc::new(AtomicBool::new(false));
        let seq = Arc::new(AtomicI64::new(1));
        let (w2, c2, o2, s2) = (Arc::clone(&waiters), Arc::clone(&closed), Arc::clone(&out), Arc::clone(&seq));
        // 答话线程：适配器反过来问的，一律「不支持」
        let (ask_tx, ask_rx) = mpsc::channel::<Value>();
        std::thread::Builder::new().name("dap-answer".into()).spawn(move || {
            for req in ask_rx {
                let reply = json!({
                    "seq": s2.fetch_add(1, Ordering::SeqCst), "type": "response", "request_seq": req["seq"], "command": req["command"],
                    "success": false, "message": "lite-ide 不支持这个反向请求",
                });
                if write_msg(&mut *o2.lock().unwrap_or_else(|e| e.into_inner()), &reply).is_err() {
                    break;
                }
            }
        })?;
        std::thread::Builder::new().name("dap-read".into()).spawn(move || {
            let mut r = BufReader::new(sock);
            while let Some(msg) = read_msg(&mut r) {
                let v: Value = match msg {
                    Ok(b) => match serde_json::from_slice(&b) {
                        Ok(v) => v,
                        Err(_) => continue,
                    },
                    Err(n) => {
                        // 太大：不知道是哪个请求的（正文没读），交给最早那个在等的 —— 适配器按次序回，最早的那个最可能是它
                        let first = w2.lock().unwrap_or_else(|e| e.into_inner()).keys().min().copied();
                        if let Some(tx) = first.and_then(|k| w2.lock().unwrap_or_else(|e| e.into_inner()).remove(&k)) {
                            let _ = tx.send(Err(Error::TooLarge(n)));
                        }
                        continue;
                    }
                };
                match v["type"].as_str() {
                    Some("response") => {
                        let id = v["request_seq"].as_i64().unwrap_or(-1);
                        if let Some(tx) = w2.lock().unwrap_or_else(|e| e.into_inner()).remove(&id) {
                            let r = if v["success"].as_bool() == Some(true) {
                                Ok(v.get("body").cloned().unwrap_or(Value::Null))
                            } else {
                                Err(Error::Failed(v["message"].as_str().unwrap_or("调试器说失败了").to_string()))
                            };
                            let _ = tx.send(r);
                        }
                    }
                    Some("event") => on_event(v),
                    Some("request") => {
                        let _ = ask_tx.send(v);
                    }
                    _ => {}
                }
            }
            c2.store(true, Ordering::SeqCst);
            // 线断了：在等的全部当场收到 Closed
            for (_, tx) in w2.lock().unwrap_or_else(|e| e.into_inner()).drain() {
                let _ = tx.send(Err(Error::Closed));
            }
            on_close();
        })?;
        Ok(Session { out, seq, waiters, closed })
    }

    /// 发一个请求、等它的响应（`body`）。`success: false` 是 `Failed`，带适配器说的原因
    pub fn request(&self, command: &str, arguments: Value, timeout: Duration) -> Result<Value, Error> {
        if self.closed.load(Ordering::SeqCst) {
            return Err(Error::Closed);
        }
        let seq = self.seq.fetch_add(1, Ordering::SeqCst);
        let (tx, rx) = mpsc::channel();
        self.waiters.lock().unwrap_or_else(|e| e.into_inner()).insert(seq, tx);
        let msg = json!({ "seq": seq, "type": "request", "command": command, "arguments": arguments });
        if write_msg(&mut *self.out.lock().unwrap_or_else(|e| e.into_inner()), &msg).is_err() {
            self.waiters.lock().unwrap_or_else(|e| e.into_inner()).remove(&seq);
            return Err(Error::Closed);
        }
        match rx.recv_timeout(timeout) {
            Ok(r) => r,
            Err(RecvTimeoutError::Timeout) => {
                self.waiters.lock().unwrap_or_else(|e| e.into_inner()).remove(&seq);
                Err(Error::Timeout)
            }
            Err(RecvTimeoutError::Disconnected) => Err(Error::Closed),
        }
    }

    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.out.lock().unwrap_or_else(|e| e.into_inner()).shutdown(Shutdown::Both);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;
    use std::time::Instant;

    /// 一个假的适配器：收到请求按 `script` 回（先回事件、再乱序回响应这类），测客户端那一半
    fn fake(script: impl FnOnce(&mut BufReader<TcpStream>, &mut TcpStream) + Send + 'static) -> u16 {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let (s, _) = l.accept().unwrap();
            let mut w = s.try_clone().unwrap();
            let mut r = BufReader::new(s);
            script(&mut r, &mut w);
        });
        port
    }

    fn recv(r: &mut BufReader<TcpStream>) -> Value {
        serde_json::from_slice(&read_msg(r).unwrap().unwrap()).unwrap()
    }

    #[test]
    fn 响应按_request_seq_配对_乱序也对_事件当场交出_失败带原因() {
        let port = fake(|r, w| {
            let a = recv(r);
            let b = recv(r);
            write_msg(w, &json!({ "seq": 1, "type": "event", "event": "stopped", "body": { "threadId": 7 } })).unwrap();
            // 先回第二个、再回第一个
            write_msg(w, &json!({ "seq": 2, "type": "response", "request_seq": b["seq"], "success": false, "message": "没有这个线程" })).unwrap();
            write_msg(w, &json!({ "seq": 3, "type": "response", "request_seq": a["seq"], "success": true, "body": { "threads": [] } })).unwrap();
            std::thread::sleep(Duration::from_millis(300));
        });
        let (tx, rx) = mpsc::channel();
        let s = Arc::new(Session::connect(port, Box::new(move |e| tx.send(e).unwrap()), Box::new(|| {})).unwrap());
        let s2 = Arc::clone(&s);
        let first = std::thread::spawn(move || s2.request("threads", json!({}), Duration::from_secs(5)));
        std::thread::sleep(Duration::from_millis(50));
        let second = s.request("stackTrace", json!({ "threadId": 9 }), Duration::from_secs(5));
        assert_eq!(second, Err(Error::Failed("没有这个线程".into())));
        assert_eq!(first.join().unwrap(), Ok(json!({ "threads": [] })));
        let ev = rx.recv_timeout(Duration::from_secs(2)).unwrap();
        assert_eq!(ev["body"]["threadId"], 7);
    }

    #[test]
    fn 线断了_在等的当场收到_closed_不干等超时() {
        let port = fake(|r, _w| {
            let _ = recv(r);
            // 不回，直接断
        });
        let (ctx, crx) = mpsc::channel();
        let s = Session::connect(port, Box::new(|_| {}), Box::new(move || ctx.send(()).unwrap())).unwrap();
        let t = Instant::now();
        assert_eq!(s.request("continue", json!({}), Duration::from_secs(20)), Err(Error::Closed));
        assert!(t.elapsed() < Duration::from_secs(3), "断了要立刻知道：{:?}", t.elapsed());
        crx.recv_timeout(Duration::from_secs(2)).expect("on_close 要调");
        assert!(s.is_closed());
        assert_eq!(s.request("next", json!({}), Duration::from_secs(1)), Err(Error::Closed), "断了之后再问不等");
    }

    #[test]
    fn 适配器反过来问的_回不支持_读线程不被卡住() {
        let port = fake(|r, w| {
            write_msg(w, &json!({ "seq": 1, "type": "request", "command": "runInTerminal", "arguments": {} })).unwrap();
            let reply = recv(r);
            assert_eq!((reply["type"].as_str(), reply["success"].as_bool(), reply["request_seq"].as_i64()), (Some("response"), Some(false), Some(1)));
            let q = recv(r);
            write_msg(w, &json!({ "seq": 2, "type": "response", "request_seq": q["seq"], "success": true })).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let s = Session::connect(port, Box::new(|_| {}), Box::new(|| {})).unwrap();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(s.request("threads", json!({}), Duration::from_secs(5)), Ok(Value::Null), "答完反向请求之后照常能问");
    }

    #[test]
    fn 太大的读掉丢弃_交给在等的那个() {
        let port = fake(|r, w| {
            let _ = recv(r);
            let big = "x".repeat(MAX_MESSAGE + 10);
            write!(w, "Content-Length: {}\r\n\r\n{}", big.len(), big).unwrap();
            w.flush().unwrap();
            let q = recv(r);
            write_msg(w, &json!({ "seq": 2, "type": "response", "request_seq": q["seq"], "success": true, "body": 1 })).unwrap();
            std::thread::sleep(Duration::from_millis(200));
        });
        let s = Session::connect(port, Box::new(|_| {}), Box::new(|| {})).unwrap();
        assert_eq!(s.request("variables", json!({}), Duration::from_secs(10)), Err(Error::TooLarge(MAX_MESSAGE + 10)));
        assert_eq!(s.request("threads", json!({}), Duration::from_secs(5)), Ok(json!(1)), "丢掉那条之后线还是好的");
    }
}
