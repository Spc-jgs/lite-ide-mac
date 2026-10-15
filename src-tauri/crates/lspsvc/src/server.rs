//! 语言服务器这个**进程**：起、排空 stderr、收尸、停。线上的事在 [`crate::conn`]。
//!
//! 为什么不走 procutil（rust.md「起子进程时的三条硬纪律」）：procutil 管的是「起、读完、收尸」的短命令 —— stdin 接空、stdout 有上限。
//! 语言服务器一直开着、**stdin 是我们说话的那根管道**、stdout 是一条一条的消息（上限在 `conn::MAX_MESSAGE`，按条算，不按总量）。
//! 纪律照守，只是答案不同：
//!
//! - **两个管道并发读**：stdout 由 conn 的读线程读，stderr 另一条线程一直排空，只留最后 [`STDERR_TAIL`] 字节（报崩溃时给人看）。
//! - **收尸**：一条等待线程攥着 `Child`，`wait()` 一返回就知道它退了（崩了的话当场报，带 stderr 尾巴），不留僵尸。
//! - **停整组**：自己的进程组。jdtls 是一个 JVM，但 m2e 导入时可能起子进程；停的时候 `killpg`，同 tasksvc。
//! - **丢掉就停**：`Drop` 发 SIGKILL 给整组。正常收尾走 [`Server::shutdown`]（先按协议说再见，到点再杀）。

use crate::conn::{Conn, Error, OnNotify, OnRequest};
use serde_json::{json, Value};
use std::io::{self, Read};
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// stderr 只留最后这么多：够看清崩溃前最后几句
pub const STDERR_TAIL: usize = 16 << 10;

pub struct Spec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: Vec<(String, String)>,
}

/// 退出了。`stopped` = 是我们停的（`shutdown` / `kill`），不是它自己崩的
#[derive(Clone, Debug)]
pub struct Exit {
    pub status: String,
    pub stopped: bool,
    pub stderr_tail: String,
}

struct Shared {
    exit: Mutex<Option<Exit>>,
    exited: Condvar,
    stopped: std::sync::atomic::AtomicBool,
    tail: Mutex<Vec<u8>>,
}

pub struct Server {
    pub conn: Conn,
    pid: i32,
    shared: Arc<Shared>,
}

impl Server {
    /// 起服务器、接上线。`on_exit` 在它退出时调一次（等待线程上）—— 崩了、被停了都调，`Exit::stopped` 区分
    pub fn start(spec: &Spec, on_request: OnRequest, on_notify: OnNotify, on_exit: Box<dyn FnOnce(Exit) + Send>) -> io::Result<Server> {
        let mut cmd = Command::new(&spec.program);
        cmd.args(&spec.args)
            .current_dir(&spec.cwd)
            .envs(spec.env.iter().map(|(k, v)| (k, v)))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        #[allow(clippy::disallowed_methods)] // 语言服务器：stdin 是说话的管道、一直开着，不是 procutil 管的短命令（理由在本文件头）
        let mut child = cmd.spawn()?;
        let pid = child.id() as i32;
        let (stdin, stdout, mut stderr) = match (child.stdin.take(), child.stdout.take(), child.stderr.take()) {
            (Some(a), Some(b), Some(c)) => (a, b, c),
            _ => {
                kill_group(pid);
                let _ = child.wait();
                return Err(io::Error::other("拿不到语言服务器的管道"));
            }
        };
        let shared = Arc::new(Shared {
            exit: Mutex::new(None),
            exited: Condvar::new(),
            stopped: std::sync::atomic::AtomicBool::new(false),
            tail: Mutex::new(Vec::new()),
        });
        let (err_done_tx, err_done_rx) = std::sync::mpsc::channel::<()>();
        let sh = Arc::clone(&shared);
        let drained = std::thread::Builder::new().name("lsp-stderr".into()).spawn(move || {
            let mut buf = [0u8; 8192];
            // 超过尾巴长度也一直读、只是不存：停下来不读，服务器写满 stderr 就卡住（rust.md 那个死锁）
            while let Ok(n) = stderr.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let mut t = sh.tail.lock().unwrap_or_else(|e| e.into_inner());
                t.extend_from_slice(&buf[..n]);
                if t.len() > STDERR_TAIL {
                    let cut = t.len() - STDERR_TAIL;
                    t.drain(..cut);
                }
            }
            let _ = err_done_tx.send(());
        });
        if let Err(e) = drained {
            kill_group(pid);
            let _ = child.wait();
            return Err(e);
        }
        let conn = match Conn::start(stdout, stdin, on_request, on_notify, Box::new(|_| {})) {
            Ok(c) => c,
            Err(e) => {
                kill_group(pid);
                let _ = child.wait();
                return Err(e);
            }
        };
        let sh = Arc::clone(&shared);
        let waited = std::thread::Builder::new().name("lsp-wait".into()).spawn(move || {
            let status = child.wait();
            // 退了之后 stderr 最多再等 2 秒（同 procutil::PIPE_GRACE：它留下的孙进程可能还攥着 stderr）
            let _ = err_done_rx.recv_timeout(Duration::from_secs(2));
            let tail = sh.tail.lock().unwrap_or_else(|e| e.into_inner());
            let exit = Exit {
                status: match status {
                    Ok(s) => describe(s),
                    Err(e) => format!("等不到退出状态：{e}"),
                },
                stopped: sh.stopped.load(std::sync::atomic::Ordering::SeqCst),
                stderr_tail: String::from_utf8_lossy(&tail).into_owned(),
            };
            drop(tail);
            *sh.exit.lock().unwrap_or_else(|e| e.into_inner()) = Some(exit.clone());
            sh.exited.notify_all();
            on_exit(exit);
        });
        if let Err(e) = waited {
            // 等待线程起不来：没人收尸也没人报退出，这个服务器不能用。`child` 已经被移进那个没起来的闭包里、跟着丢了 ——
            // 杀掉整组，它退了之后留成僵尸，直到应用退出（同 procutil::reap_later 起不来时的说法）
            kill_group(pid);
            return Err(e);
        }
        Ok(Server { conn, pid, shared })
    }

    pub fn pid(&self) -> i32 {
        self.pid
    }

    /// 退了没有；退了带原因
    pub fn exit(&self) -> Option<Exit> {
        self.shared.exit.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn stderr_tail(&self) -> String {
        String::from_utf8_lossy(&self.shared.tail.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }

    /// 按协议收尾：`shutdown` 请求（最多等 `grace`）→ `exit` 通知 → 等它自己退，到 `grace` 还没退就杀整组。最多阻塞约两个 `grace`
    pub fn shutdown(&self, grace: Duration) {
        self.shared.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        if self.exit().is_none() {
            let _ = self.conn.request("shutdown", Value::Null, grace);
            let _ = self.conn.notify("exit", Value::Null);
        }
        if !self.wait_exit(grace) {
            kill_group(self.pid);
            self.wait_exit(Duration::from_millis(500));
        }
    }

    /// 不说再见，直接杀整组。不等
    pub fn kill(&self) {
        self.shared.stopped.store(true, std::sync::atomic::Ordering::SeqCst);
        kill_group(self.pid);
    }

    fn wait_exit(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut g = self.shared.exit.lock().unwrap_or_else(|e| e.into_inner());
        while g.is_none() {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            g = self.shared.exited.wait_timeout(g, left).unwrap_or_else(|e| e.into_inner()).0;
        }
        true
    }

    /// 发请求的便捷写法：结果里的 `null` 也算 Ok
    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, Error> {
        self.conn.request(method, params, timeout)
    }

    pub fn notify(&self, method: &str, params: Value) -> Result<(), Error> {
        self.conn.notify(method, params)
    }

    /// LSP 的 `initialize` + `initialized`。`options` 是服务器自己的 `initializationOptions`
    pub fn initialize(&self, root_uri: &str, name: &str, capabilities: Value, options: Value, timeout: Duration) -> Result<Value, Error> {
        let r = self.request(
            "initialize",
            json!({
                "processId": std::process::id(),
                "rootUri": root_uri,
                "workspaceFolders": [{ "uri": root_uri, "name": name }],
                "capabilities": capabilities,
                "initializationOptions": options,
            }),
            timeout,
        )?;
        self.notify("initialized", json!({}))?;
        Ok(r)
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        if self.exit().is_none() {
            self.kill();
        }
    }
}

fn kill_group(pid: i32) {
    // 组长的 pid 就是组号（process_group(0)）。组没了（ESRCH）不算错
    unsafe {
        libc::killpg(pid, libc::SIGKILL);
    }
}

fn describe(s: ExitStatus) -> String {
    use std::os::unix::process::ExitStatusExt;
    match (s.code(), s.signal()) {
        (Some(c), _) => format!("退出码 {c}"),
        (_, Some(sig)) => format!("被信号 {sig} 结束"),
        _ => "退出了".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    fn sh(script: &str) -> Spec {
        Spec { program: "/bin/sh".into(), args: vec!["-c".into(), script.into()], cwd: std::env::temp_dir(), env: vec![] }
    }

    fn quiet() -> (OnRequest, OnNotify) {
        (Box::new(|_, _| Ok(Value::Null)), Box::new(|_, _| {}))
    }

    #[test]
    fn 自己崩了_当场报_带_stderr_尾巴_不算我们停的() {
        let (q, n) = quiet();
        let (tx, rx) = mpsc::channel();
        // 最后那句由孙进程在组长退出 0.3 秒后才写（它攥着 stderr）：报退出之前要等 stderr 收完，否则崩溃前的遗言丢一截
        let script = "echo 'Error: Could not find or load main class' >&2; (sleep 0.3; echo 'late words' >&2) & exit 3";
        let s = Server::start(&sh(script), q, n, Box::new(move |e| tx.send(e).unwrap())).unwrap();
        let e = rx.recv_timeout(Duration::from_secs(5)).expect("崩了要报");
        assert_eq!(e.status, "退出码 3");
        assert!(!e.stopped);
        assert!(e.stderr_tail.contains("Could not find or load main class"), "{:?}", e.stderr_tail);
        assert!(e.stderr_tail.contains("late words"), "{:?}", e.stderr_tail);
        assert!(s.exit().is_some());
        assert!(s.conn.is_closed() || s.request("x", Value::Null, Duration::from_secs(2)).is_err());
    }

    #[test]
    fn stderr_话多不卡住_只留尾巴() {
        // 往 stderr 写 1MB（远超管道缓冲）再往 stdout 说话：stderr 没人排空的话它卡在写上，后面那句永远到不了
        let (q, _) = quiet();
        let (got_tx, got_rx) = mpsc::channel();
        let body = r#"{"jsonrpc":"2.0","method":"hello","params":{}}"#;
        let script = format!(
            "head -c 1048576 /dev/zero | tr '\\0' 'e' >&2; echo tail-marker >&2; printf 'Content-Length: {}\\r\\n\\r\\n{}'; sleep 5",
            body.len(),
            body
        );
        let s = Server::start(&sh(&script), q, Box::new(move |m, _| got_tx.send(m.to_owned()).unwrap()), Box::new(|_| {})).unwrap();
        assert_eq!(got_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "hello");
        std::thread::sleep(Duration::from_millis(100));
        let tail = s.stderr_tail();
        assert!(tail.len() <= STDERR_TAIL);
        assert!(tail.trim_end().ends_with("tail-marker"), "尾巴要是最后那段");
    }

    #[test]
    fn 停整组_孙进程也没了_退出算我们停的() {
        let (q, n) = quiet();
        let (tx, rx) = mpsc::channel();
        let s = Server::start(&sh("sleep 30 & echo $! >&2; wait"), q, n, Box::new(move |e| tx.send(e).unwrap())).unwrap();
        let t0 = Instant::now();
        let grand: i32 = loop {
            if let Ok(p) = s.stderr_tail().trim().parse() {
                break p;
            }
            assert!(t0.elapsed() < Duration::from_secs(5));
            std::thread::sleep(Duration::from_millis(20));
        };
        // 不说 LSP 的 shell：shutdown 请求等满 grace、exit 没人理，到点杀整组
        s.shutdown(Duration::from_millis(300));
        let e = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(e.stopped);
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(unsafe { libc::kill(grand, 0) }, -1, "孙进程 {grand} 还活着：只杀了组长");
    }

    #[test]
    fn 丢掉就杀() {
        let (q, n) = quiet();
        let (tx, rx) = mpsc::channel();
        let s = Server::start(&sh("sleep 30"), q, n, Box::new(move |e| tx.send(e).unwrap())).unwrap();
        drop(s);
        let e = rx.recv_timeout(Duration::from_secs(5)).expect("丢掉之后要退");
        assert!(e.stopped);
    }
}
