//! 起子进程、读它的输出：rust.md「起子进程时的三条硬纪律」「子进程输出必须设闸」那几条，**收成代码**。
//!
//! 为什么要有这个 crate：这几条原来只写在文档里，每个模块照着各写一遍 —— gitsvc 两份、fsservice（剪贴板）一份、
//! tasksvc（lsof）一份、searchsvc（rg）一份，细节各不相同，**searchsvc 那份把「两个管道要并发读」写反了**：
//! 读完 stdout 才读 stderr，rg 往 stderr 写满 64KB 就和我们互相等住，搜索永远不回来（2026-10-10 整体审核时复现）。
//! git 那边 2026-09-07 在 `git commit` 上真的挂过一次、修过 —— 同一个坑在另一个模块重犯，说明规矩放错了地方。
//!
//! 这里只管「起、读、设闸、收尸」；命令怎么拼（git 的加固参数、rg 的过滤）还是各模块自己的事。
//! 不收的两种：长期跑、输出流式落盘的任务（`tasksvc::Task`，stdout + stderr 合进一根管道）；边读边解析进度、要能取消的
//! git fetch / push（`gitsvc::remote`）—— 硬塞进来比现在更绕。

use std::io::{self, Read};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// stderr 默认留多少：够拼出一句能读的报错就行
pub const ERR_CAP: usize = 8 << 10;

/// 子进程退出之后，管道最多再等多久。
///
/// **管道的 EOF 不等于子进程退出**：命令留下的后台进程继承了 stdout / stderr，子进程退了、管道还被它攥着，读的一方等不到 EOF ——
/// 原来就一直等到那个后台进程退出，不退就永远（`sleep 4 & echo hi` 实测 4.02 秒才回来）。真实的入口是 `git commit`：
/// 它跑仓库带来的钩子，一个 `post-commit` 里没重定向输出的 `… &`，提交就一直转、之后 git 写操作全堵住。
/// 照 Codex（`core/src/exec.rs` 的 `IO_DRAIN_TIMEOUT_MS`，docs/research/codex.md 主题 1 第 1 条）：退出之后宽限这么久，到点拿已经读到的、
/// 读线程撒手不管（后台进程退了它自己会结束）。**不杀那个后台进程** —— 钩子留下的后台活可能是用户想要的。
/// 本机管道上，退出时已经写进去的东西几毫秒就读完了，2 秒是给「退出前最后一刻还在写」留的
pub const PIPE_GRACE: Duration = Duration::from_secs(2);

/// 子进程还没退时，多久问一次它退了没有（`try_wait`，一个系统调用）。只在管道上没东西来的时候才问
const POLL: Duration = Duration::from_millis(50);

/// 在另一条线程里把一路输出读到 EOF，只留前 `cap` 字节。**超过 `cap` 也要接着读**（只是不再存）——
/// 停下来不读，对方写满管道就卡在写上，正是要防的那个死锁。
///
/// 读到的放在共享的缓冲里，不是线程结束时才交出来：等不到 EOF（后台进程攥着管道）的时候，[`Drained::take`] 照样拿得到已经读到的
pub fn drain(mut src: impl Read + Send + 'static, cap: usize) -> Drained {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done) = mpsc::channel::<()>();
    let kept = Arc::clone(&buf);
    std::thread::spawn(move || {
        let _done = done_tx; // 线程结束时跟着丢掉：等的一方收到「断开」就知道读完了
        let mut chunk = [0u8; 8 << 10];
        loop {
            match src.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    let mut k = kept.lock().unwrap_or_else(|e| e.into_inner());
                    if k.len() < cap {
                        let room = cap - k.len();
                        k.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
        }
    });
    Drained { buf, done }
}

/// [`drain`] 的那一路：读到 EOF 了，或者到点了，都能取走已经读到的
pub struct Drained {
    buf: Arc<Mutex<Vec<u8>>>,
    done: Receiver<()>,
}

impl Drained {
    /// 等它读到 EOF，最多等到 `by`。到点还没完（后台进程攥着管道）就拿已经读到的
    pub fn take(self, by: Instant) -> Vec<u8> {
        let _ = self.done.recv_timeout(by.saturating_duration_since(Instant::now()));
        std::mem::take(&mut *self.buf.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// 起来了的子进程：stdin 接空（绝不让子进程卡住等输入）、stdout 由一条线程读进通道、**stderr 已经在另一条线程里排空**。
///
/// **丢掉时还没收尸，就杀掉再 wait**：中途 `?` 提前返回、解析出错 panic，都不会留下一个还在跑、或者成了僵尸的子进程。
/// 杀的是它自己，不是进程组 —— 要整组停的（git fetch 的孙进程）用 `process_group(0)` + `killpg`，那是调用方的事
pub struct Running {
    child: Child,
    /// stdout 的读线程送过来的块。有界：调用方读得慢，读线程就停在 send 上，对方停在写上 —— 和直接读管道一样的背压。
    /// 丢掉它 = 不要了：读线程下一次 send 失败就退出、关掉读端，子进程再写收到 SIGPIPE 自己退
    out: Option<Receiver<Vec<u8>>>,
    pending: Vec<u8>,
    pos: usize,
    err: Option<Drained>,
    /// 读 stdout 时顺手发现它退了（`try_wait` 收了尸）：退出状态、什么时候退的（[`PIPE_GRACE`] 从这一刻算）
    status: Option<ExitStatus>,
    exited_at: Option<Instant>,
    /// 上次 `try_wait` 是什么时候：有数据一直来的时候也要隔一阵问一次（见 [`Stdout`] 的 `read`）
    polled: Instant,
    reaped: bool,
}

/// 收完尸：退出状态 + stderr 的前 `err_cap` 字节
pub struct Finished {
    pub status: ExitStatus,
    pub stderr: Vec<u8>,
}

pub fn spawn(cmd: &mut Command, err_cap: usize) -> io::Result<Running> {
    #[allow(clippy::disallowed_methods)] // 这里就是那个包装本身
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    // **先**把 stderr 挂到自己的线程上，再让调用方去读 stdout —— 次序反了就是死锁
    let err = child.stderr.take().map(|e| drain(e, err_cap));
    let out = child.stdout.take().map(|mut o| {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(4);
        std::thread::spawn(move || {
            let mut chunk = vec![0u8; 16 << 10];
            loop {
                match o.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        if tx.send(chunk[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });
        rx
    });
    Ok(Running { child, out, pending: Vec::new(), pos: 0, err, status: None, exited_at: None, polled: Instant::now(), reaped: false })
}

impl Running {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// stdout。读到 EOF，或者子进程退出后 [`PIPE_GRACE`] 到点，都算读完（返回 0）。
    /// 调用方读够了就可以不读了（[`Running::finish`] 丢掉通道，子进程再写收到 SIGPIPE 自己退），或者 [`Running::kill`]
    pub fn stdout(&mut self) -> Stdout<'_> {
        Stdout(self)
    }

    /// 不要它的输出了：杀掉。**之后它的退出码没有意义**，调用方不能拿来判失败（rust.md「被掐掉的子进程退出码没有意义」）
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// 等它退出、收尸，拿回 stderr。stderr 被后台进程攥着的话，从退出那一刻起最多再等 [`PIPE_GRACE`]
    pub fn finish(mut self) -> io::Result<Finished> {
        // 不要 stdout 了：调用方没读完的话，读线程下一次 send 失败就关掉读端，子进程不会因为没人读卡在写上
        self.out = None;
        let status = match self.status.take() {
            Some(s) => s,
            None => {
                let s = self.child.wait()?;
                self.exited_at = Some(Instant::now());
                s
            }
        };
        self.reaped = true;
        let by = self.exited_at.unwrap_or_else(Instant::now) + PIPE_GRACE;
        let stderr = self.err.take().map(|d| d.take(by)).unwrap_or_default();
        Ok(Finished { status, stderr })
    }
}

/// [`Running::stdout`]：从读线程的通道里取，子进程退出后限时
pub struct Stdout<'a>(&'a mut Running);

impl Read for Stdout<'_> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let r = &mut *self.0;
        loop {
            if r.pos < r.pending.len() {
                let n = buf.len().min(r.pending.len() - r.pos);
                buf[..n].copy_from_slice(&r.pending[r.pos..r.pos + n]);
                r.pos += n;
                return Ok(n);
            }
            // 退了没有：**不能只在没数据时才问** —— 后台进程一直在写（`yes &` 那种）的话，通道永远不空、永远等不到超时，
            // 子进程早退了也发现不了，宽限期就永远不开始算
            if r.exited_at.is_none() && r.polled.elapsed() >= POLL {
                r.polled = Instant::now();
                if let Ok(Some(st)) = r.child.try_wait() {
                    r.status = Some(st);
                    r.exited_at = Some(Instant::now());
                    r.reaped = true;
                }
            }
            let Some(rx) = &r.out else { return Ok(0) };
            let wait = match r.exited_at {
                None => POLL,
                Some(t) => match PIPE_GRACE.checked_sub(t.elapsed()) {
                    Some(left) => left.min(POLL),
                    // 退出之后宽限期过了，管道还没到头：后台进程攥着它。拿已经读到的，读线程撒手
                    None => {
                        r.out = None;
                        return Ok(0);
                    }
                },
            };
            match rx.recv_timeout(wait) {
                Ok(chunk) => {
                    r.pending = chunk;
                    r.pos = 0;
                }
                Err(RecvTimeoutError::Disconnected) => {
                    r.out = None;
                    return Ok(0);
                }
                Err(RecvTimeoutError::Timeout) => {}
            }
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        if !self.reaped {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// stdout 超过上限时怎么办
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Overflow {
    /// 杀掉它：后面的没人要看（差异、剪贴板、lsof）。退出码随之没有意义
    Kill,
    /// 读完、不存：**退出码要算数**的（git commit —— 杀掉的话「提交成功但钩子话多」和「提交失败」分不出来）
    Drain,
}

pub struct Capped {
    /// 最多 `cap` 字节
    pub stdout: Vec<u8>,
    /// 实际输出比 `cap` 多（**正好**等于 `cap` 不算）
    pub truncated: bool,
    /// `Overflow::Kill` 且截断了时是被我们杀掉的那个状态，别拿来判失败
    pub status: ExitStatus,
    pub stderr: Vec<u8>,
}

/// 跑完一条命令，stdout 最多收 `cap` 字节、stderr 最多 `err_cap` 字节（两个并发读）
pub fn run_capped(cmd: &mut Command, cap: usize, err_cap: usize, over: Overflow) -> io::Result<Capped> {
    let mut r = spawn(cmd, err_cap)?;
    let mut out = Vec::new();
    // 数总量而不是看「out 满没满」：正好读满 cap、后面再没有了，那不算截断 —— 差这一个字节就会给一份完整的输出误报截断
    let mut total = 0usize;
    let mut buf = [0u8; 16 << 10];
    loop {
        match r.stdout().read(&mut buf) {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                total += n;
                if out.len() < cap {
                    out.extend_from_slice(&buf[..n.min(cap - out.len())]);
                }
                if total > cap && over == Overflow::Kill {
                    r.kill();
                    break;
                }
            }
        }
    }
    let f = r.finish()?;
    Ok(Capped { stdout: out, truncated: total > cap, status: f.status, stderr: f.stderr })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sh(script: &str) -> Command {
        let mut c = Command::new("/bin/sh");
        c.args(["-c", script]);
        c
    }

    /// 在别的线程里跑，最多等 `secs` 秒：卡死的话这里报失败，不把整个测试挂住
    fn within<T: Send + 'static>(secs: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = tx.send(f());
        });
        rx.recv_timeout(Duration::from_secs(secs)).expect("卡住了：两个管道没有并发读")
    }

    /// 这个 crate 存在的头号理由：stderr 先写满管道缓冲（这里 300KB，缓冲 64KB），stdout 最后才有东西
    #[test]
    fn stderr_写满也不卡死() {
        let c = within(10, || run_capped(&mut sh("head -c 300000 /dev/zero >&2; echo out"), 1024, ERR_CAP, Overflow::Kill).unwrap());
        assert_eq!(c.stdout, b"out\n");
        assert!(!c.truncated && c.status.success());
        assert_eq!(c.stderr.len(), ERR_CAP, "stderr 只留前 ERR_CAP 字节");
    }

    #[test]
    fn 超上限_kill_就杀掉不等它跑完() {
        let c = within(10, || run_capped(&mut Command::new("yes"), 1000, ERR_CAP, Overflow::Kill).unwrap());
        assert!(c.truncated && c.stdout.len() == 1000);
    }

    /// 读完不存：退出码要算数（git commit）
    #[test]
    fn 超上限_drain_就读完_退出码照实() {
        let c = within(10, || run_capped(&mut sh("head -c 200000 /dev/zero; exit 3"), 1000, ERR_CAP, Overflow::Drain).unwrap());
        assert!(c.truncated && c.stdout.len() == 1000);
        assert_eq!(c.status.code(), Some(3), "没被杀，退出码是它自己的");
    }

    #[test]
    fn 正好读满不算截断() {
        let c = run_capped(&mut sh("printf abcd"), 4, ERR_CAP, Overflow::Kill).unwrap();
        assert_eq!(c.stdout, b"abcd");
        assert!(!c.truncated, "正好 4 字节、上限 4：不是截断");
    }

    /// 丢掉就收尸：中途 `?` 返回不留下一个在跑的、也不留僵尸（`ps -p` 连僵尸也列，所以它查不到才算收干净了）
    #[test]
    fn 丢掉就杀掉并收尸() {
        let r = spawn(&mut Command::new("sleep").arg("30"), ERR_CAP).unwrap();
        let pid = r.id().to_string();
        drop(r);
        let still = Command::new("ps").args(["-p", &pid]).stdout(Stdio::null()).status().unwrap().success();
        assert!(!still, "丢掉之后 {pid} 还在（在跑或是僵尸）");
    }

    /// 测试里起的后台进程：命令行里带上这个独一无二的记号，测完（不管红绿）按它收掉
    struct Bg(String);
    impl Drop for Bg {
        fn drop(&mut self) {
            let _ = Command::new("pkill").args(["-f", &self.0]).status();
        }
    }
    fn bg(tag: &str) -> Bg {
        Bg(format!("procutil-bg-{tag}-{}", std::process::id()))
    }

    /// 命令留下一个攥着 stdout / stderr 的后台进程（git 钩子里没重定向的 `… &`）：子进程退了，宽限期一到就回来，
    /// 不等那个后台进程。原来要等到它退出，这里它要活 30 秒（docs/research/codex.md 主题 1 第 1 条）
    #[test]
    fn 后台进程攥着管道_子进程退了宽限期一到就回来() {
        let b = bg("both");
        let t = Instant::now();
        let c = within(10, {
            let m = b.0.clone();
            move || run_capped(&mut sh(&format!("sh -c 'sleep 30' {m} & echo hi; echo err >&2")), 1024, ERR_CAP, Overflow::Drain).unwrap()
        });
        let took = t.elapsed();
        assert_eq!(c.stdout, b"hi\n", "退出前写的要都在");
        assert_eq!(c.stderr, b"err\n");
        assert!(c.status.success() && !c.truncated);
        assert!(took >= PIPE_GRACE && took < PIPE_GRACE + Duration::from_millis(1500), "该在宽限期之后不久回来：{took:?}");
    }

    /// 只攥着 stderr：stdout 当场到头，等的是 `finish` 里 stderr 那一路
    #[test]
    fn 后台进程只攥着_stderr_也一样() {
        let b = bg("err");
        let t = Instant::now();
        let c = within(10, {
            let m = b.0.clone();
            move || run_capped(&mut sh(&format!("sh -c 'sleep 30' {m} >/dev/null & echo hi; echo err >&2")), 1024, ERR_CAP, Overflow::Drain).unwrap()
        });
        assert_eq!((c.stdout.as_slice(), c.stderr.as_slice()), (&b"hi\n"[..], &b"err\n"[..]));
        assert!(t.elapsed() < PIPE_GRACE + Duration::from_millis(1500), "{:?}", t.elapsed());
    }

    /// 后台进程**一直在写**：通道永远不空、等不到超时。只在没数据时才问「退了没有」的话，永远发现不了子进程早退了（写这版时自己差点漏的）
    #[test]
    fn 后台进程一直在写_也在宽限期后回来() {
        let b = bg("yes");
        let t = Instant::now();
        let c = within(10, {
            let m = b.0.clone();
            move || run_capped(&mut sh(&format!("yes {m} & echo hi")), 1024, ERR_CAP, Overflow::Drain).unwrap()
        });
        assert!(c.truncated && c.status.success(), "子进程自己是正常退出的");
        assert!(t.elapsed() < PIPE_GRACE + Duration::from_millis(1500), "{:?}", t.elapsed());
    }

}
