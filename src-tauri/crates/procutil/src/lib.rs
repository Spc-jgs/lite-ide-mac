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
use std::os::unix::process::ExitStatusExt;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
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
/// 读线程撒手不管（后台进程退了它自己会结束）。**不主动杀那个后台进程** —— 钩子留下的后台活可能是用户想要的。
/// 但撒手之后两路都**不再替它读**：读线程拿到下一块就退出、关掉读端，后台进程之后再往这根管道写会收到 SIGPIPE（默认就退了）。
/// 原来只有 stdout 是这样（通道没人收、send 失败就退），stderr 一直读到它退出 —— 2026-10-12 实测撒手后各写两次，写 stdout 的死了、写 stderr 的活着。
/// 统一成「关掉」：一个不读输出、却一直往里写的后台进程，不该由我们的线程和 fd 养着；一直不出声的那种不受影响。
/// 本机管道上，退出时已经写进去的东西几毫秒就读完了，2 秒是给「退出前最后一刻还在写」留的
pub const PIPE_GRACE: Duration = Duration::from_secs(2);

/// 子进程还没退时，多久问一次它退了没有（`try_wait`，一个系统调用）。只在管道上没东西来的时候才问
const POLL: Duration = Duration::from_millis(50);

/// 杀掉之后最多当场等多久。SIGKILL 平常几毫秒就生效；等不到的是**卡在不可中断的系统调用里**的进程
/// （读一个断了线的网络卷）—— SIGKILL 要等那个调用返回才生效，可能是几分钟。到点就交给 [`reap_later`]，自己不等（#66）
const KILL_WAIT: Duration = Duration::from_millis(200);

/// 收尸线程多久问一遍手上的那些
const REAP_POLL: Duration = Duration::from_millis(200);

/// 杀了、当场没等到它退的子进程，交给一条全进程共用的收尸线程：它隔 [`REAP_POLL`] 对手上每一个 `try_wait` 一次，退了就收。
/// 原来是丢掉的那一下「kill 之后阻塞 wait」—— 卡住的进程把调用它的那条线程（阻塞池里的一条）一起卡住，
/// 卡几条池子就少几条。照 Codex `utils/pty/src/child_reaper.rs`（docs/research/codex.md 主题 1 第 5 条）
fn reap_later(child: Child) {
    if let Err((child, e)) = hand_off(&REAPER, child, spawn_reaper) {
        // 没人收：它退了之后留成僵尸，直到本进程退出。不当场 wait —— 那正是交出去要避开的（卡住的进程把这条线程一起卡住）
        applog::write(
            applog::Level::Warn,
            "procutil",
            &format!("收尸线程起不来（{e}）：pid {} 退出后会留成僵尸，直到应用退出；下一个要收尸的会再试着起一次", child.id()),
        );
    }
}

/// 收尸线程的信箱。`None` = 还没起，或者上次没起来 —— **下一次再试**：原来是 `OnceLock`，线程起不来那一次被记一辈子，
/// 之后每个交过来的孩子都 `send` 失败、被悄悄 drop（drop 一个 `Child` 不收尸），app.log 里一个字都没有（code review 2026-10-13）
static REAPER: Mutex<Option<Sender<Child>>> = Mutex::new(None);

/// 把 `child` 交给 `slot` 里那条收尸线程；没有、或者它已经没了（send 失败 = 那头的接收端掉了），用 `spawn` 起一条新的再交。
/// 交不出去就**把孩子还回来**，连同原因 —— 由调用方说出来，不在这里悄悄扔掉。`spawn` 是参数：「线程起不来」造不出来，测试传一个会失败的
fn hand_off(
    slot: &Mutex<Option<Sender<Child>>>,
    child: Child,
    spawn: impl FnOnce() -> io::Result<Sender<Child>>,
) -> Result<(), (Child, io::Error)> {
    let mut g = slot.lock().unwrap_or_else(|e| e.into_inner());
    let child = match g.as_ref() {
        Some(tx) => match tx.send(child) {
            Ok(()) => return Ok(()),
            Err(mpsc::SendError(c)) => c,
        },
        None => child,
    };
    *g = None;
    let tx = match spawn() {
        Ok(tx) => tx,
        Err(e) => return Err((child, e)),
    };
    match tx.send(child) {
        Ok(()) => {
            *g = Some(tx);
            Ok(())
        }
        Err(mpsc::SendError(c)) => Err((c, io::Error::other("收尸线程刚起来就没了"))),
    }
}

/// 起一条收尸线程：手上没有就一直睡到来活；有就隔 [`REAP_POLL`] 醒来对每一个 `try_wait` 一遍。
/// 一个卡住的不耽误别的：每一个都只 `try_wait`，从不阻塞在某一个上
fn spawn_reaper() -> io::Result<Sender<Child>> {
    let (tx, rx) = mpsc::channel::<Child>();
    std::thread::Builder::new().name("procutil-reaper".into()).spawn(move || {
        let mut held: Vec<Child> = Vec::new();
        loop {
            let got = if held.is_empty() { rx.recv().ok() } else { rx.recv_timeout(REAP_POLL).ok() };
            held.extend(got);
            // 收完了（`Some`）或者已经不是我们的孩子了（`Err`）都撒手；被信号打断的下一轮再问（同 Codex）
            held.retain_mut(|c| match c.try_wait() {
                Ok(None) => true,
                Ok(Some(_)) => false,
                Err(e) => e.kind() == io::ErrorKind::Interrupted,
            });
        }
    })?;
    Ok(tx)
}

/// 被我们杀掉的那个退出状态（按 SIGKILL 记）。当场没等到真的状态时用 —— 它本来也没有意义（rust.md「被掐掉的子进程退出码没有意义」）
fn killed_status() -> ExitStatus {
    ExitStatus::from_raw(9)
}

/// 杀掉，最多当场等 [`KILL_WAIT`]：等到了返回真的退出状态，等不到就交给收尸线程、返回 [`killed_status`]。**不会无限期阻塞**。
/// 怎么杀由参数给：产品里是 SIGKILL（`Child::kill`），测试里换成一个它能忽略的信号，造出「杀了也不退」—— 卡在网络卷上的进程造不出来
fn abandon(mut child: Child, kill: impl FnOnce(&mut Child)) -> ExitStatus {
    kill(&mut child);
    let t = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(st)) => return st,
            Err(_) => return killed_status(),
            Ok(None) if t.elapsed() >= KILL_WAIT => {
                reap_later(child);
                return killed_status();
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
        }
    }
}

/// 一条读线程的去向。撒手和读完可能同时发生，所以用一个原子量、两边各 `swap` 一次：谁后到谁知道对方做了什么
#[derive(Clone)]
struct Reader(Arc<AtomicU8>);

const READING: u8 = 0;
const DONE: u8 = 1;
const STRANDED: u8 = 2;

/// 撒手之后还卡在 `read()` 上的读线程有几条（全进程）。
///
/// 宽限期到点之后我们不再等管道，但读线程自己还阻塞在 `read()` 上，攥着一个线程、一个 fd，直到后台进程退出或者再写一次
/// （[`PIPE_GRACE`] 末尾那段）。一个每次都留下**不出声**的常驻进程的钩子，提交几百次就是几百条（code review 2026-10-12）。
/// 要它们一到点就退，得把读改成带超时的 `poll`（要 libc），没做；**记下来**：每撒手一条往 app.log 写一句，带当前总数；读线程退出时再减回去
static STRANDED_NOW: AtomicUsize = AtomicUsize::new(0);

pub fn stranded() -> usize {
    STRANDED_NOW.load(Ordering::SeqCst)
}

impl Reader {
    fn new() -> Reader {
        Reader(Arc::new(AtomicU8::new(READING)))
    }

    /// 已经被撒手了：读线程拿到下一块时据此退出、关掉读端
    fn stranded(&self) -> bool {
        self.0.load(Ordering::SeqCst) == STRANDED
    }

    /// 读线程结束时调
    fn done(&self) {
        if self.0.swap(DONE, Ordering::SeqCst) == STRANDED {
            STRANDED_NOW.fetch_sub(1, Ordering::SeqCst);
        }
    }

    /// 我们不等了。线程还在读就记一笔
    fn strand(&self, what: &str) {
        if self.0.compare_exchange(READING, STRANDED, Ordering::SeqCst, Ordering::SeqCst).is_ok() {
            let n = STRANDED_NOW.fetch_add(1, Ordering::SeqCst) + 1;
            applog::write(
                applog::Level::Warn,
                "procutil",
                &format!("{what} 退出 {} 秒后管道还被它留下的后台进程攥着，不等了；现在共有 {n} 条读线程等着那些后台进程退出（各占一个线程、一个 fd）", PIPE_GRACE.as_secs()),
            );
        }
    }
}

/// 在另一条线程里把一路输出读到 EOF，只留前 `cap` 字节。**超过 `cap` 也要接着读**（只是不再存）——
/// 停下来不读，对方写满管道就卡在写上，正是要防的那个死锁。
///
/// 读到的放在共享的缓冲里，不是线程结束时才交出来：等不到 EOF（后台进程攥着管道）的时候，[`Drained::take`] 照样拿得到已经读到的
pub fn drain(mut src: impl Read + Send + 'static, cap: usize) -> Drained {
    let buf = Arc::new(Mutex::new(Vec::new()));
    let (done_tx, done) = mpsc::channel::<()>();
    let kept = Arc::clone(&buf);
    let reader = Reader::new();
    let r = reader.clone();
    std::thread::spawn(move || {
        let _done = done_tx; // 线程结束时跟着丢掉：等的一方收到「断开」就知道读完了
        let mut chunk = [0u8; 8 << 10];
        loop {
            match src.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                // 我们已经不等了（[`Drained::take`] 到点撒手）：不再替它读，关掉读端 —— 和 stdout 那一路一样（[`PIPE_GRACE`] 末尾那段）
                Ok(_) if r.stranded() => break,
                Ok(n) => {
                    let mut k = kept.lock().unwrap_or_else(|e| e.into_inner());
                    if k.len() < cap {
                        let room = cap - k.len();
                        k.extend_from_slice(&chunk[..n.min(room)]);
                    }
                }
            }
        }
        r.done();
    });
    Drained { buf, done, reader }
}

/// [`drain`] 的那一路：读到 EOF 了，或者到点了，都能取走已经读到的
pub struct Drained {
    buf: Arc<Mutex<Vec<u8>>>,
    done: Receiver<()>,
    reader: Reader,
}

impl Drained {
    /// 等它读到 EOF，最多等到 `by`。到点还没完（后台进程攥着管道）就拿已经读到的。`what` 是哪条命令（撒手时记进 app.log）
    pub fn take(self, by: Instant, what: &str) -> Vec<u8> {
        if let Err(RecvTimeoutError::Timeout) = self.done.recv_timeout(by.saturating_duration_since(Instant::now())) {
            self.reader.strand(what);
        }
        std::mem::take(&mut *self.buf.lock().unwrap_or_else(|e| e.into_inner()))
    }
}

/// 起来了的子进程：stdin 接空（绝不让子进程卡住等输入）、stdout 由一条线程读进通道、**stderr 已经在另一条线程里排空**。
///
/// **丢掉时还没收尸，就杀掉、收尸**：中途 `?` 提前返回、解析出错 panic，都不会留下一个还在跑、或者成了僵尸的子进程。
/// 收尸不当场死等（[`abandon`]）。杀的是它自己，不是进程组 —— 要整组停的（git fetch 的孙进程）用 `process_group(0)` + `killpg`，那是调用方的事
pub struct Running {
    /// 只在交给收尸线程之后（[`Running::finish`] / `Drop`）才是 `None`
    child: Option<Child>,
    /// stdout 的读线程送过来的块。有界：调用方读得慢，读线程就停在 send 上，对方停在写上 —— 和直接读管道一样的背压。
    /// 丢掉它 = 不要了：读线程下一次 send 失败就退出、关掉读端，子进程再写收到 SIGPIPE 自己退
    out: Option<Receiver<Vec<u8>>>,
    out_reader: Reader,
    /// 程序名（`git`、`rg`）：撒手时记进 app.log，不带参数
    what: String,
    pending: Vec<u8>,
    pos: usize,
    err: Option<Drained>,
    /// 读 stdout 时顺手发现它退了（`try_wait` 收了尸）：退出状态、什么时候退的（[`PIPE_GRACE`] 从这一刻算）
    status: Option<ExitStatus>,
    exited_at: Option<Instant>,
    /// 上次 `try_wait` 是什么时候：有数据一直来的时候也要隔一阵问一次（见 [`Stdout`] 的 `read`）
    polled: Instant,
    reaped: bool,
    /// 叫过 [`Running::kill`]：之后 [`Running::finish`] 不能死等它退出
    killed: bool,
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
    let out_reader = Reader::new();
    let r = out_reader.clone();
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
            r.done();
        });
        rx
    });
    let what = std::path::Path::new(cmd.get_program()).file_name().unwrap_or_default().to_string_lossy().into_owned();
    Ok(Running {
        child: Some(child),
        out,
        out_reader,
        what,
        pending: Vec::new(),
        pos: 0,
        err,
        status: None,
        exited_at: None,
        polled: Instant::now(),
        reaped: false,
        killed: false,
    })
}

impl Running {
    pub fn id(&self) -> u32 {
        self.child.as_ref().map_or(0, Child::id)
    }

    /// stdout。读到 EOF，或者子进程退出后 [`PIPE_GRACE`] 到点，都算读完（返回 0）。
    /// 调用方读够了就可以不读了（[`Running::finish`] 丢掉通道，子进程再写收到 SIGPIPE 自己退），或者 [`Running::kill`]
    pub fn stdout(&mut self) -> Stdout<'_> {
        Stdout(self)
    }

    /// 不要它的输出了：杀掉。**之后它的退出码没有意义**，调用方不能拿来判失败（rust.md「被掐掉的子进程退出码没有意义」）
    pub fn kill(&mut self) {
        if let Some(c) = &mut self.child {
            let _ = c.kill();
            self.killed = true;
        }
    }

    /// 等它退出、收尸，拿回 stderr。stderr 被后台进程攥着的话，从退出那一刻起最多再等 [`PIPE_GRACE`]
    pub fn finish(mut self) -> io::Result<Finished> {
        // 不要 stdout 了：调用方没读完的话，读线程下一次 send 失败就关掉读端，子进程不会因为没人读卡在写上
        self.out = None;
        let status = match (self.status.take(), self.child.take()) {
            (Some(s), _) => s,
            (None, Some(c)) if self.killed => {
                // 杀过了：同 Drop，不当场死等（卡在网络卷上的话，wait 能等几分钟）
                let s = abandon(c, |_| {});
                self.exited_at = Some(Instant::now());
                s
            }
            (None, Some(mut c)) => {
                let s = c.wait()?;
                self.exited_at = Some(Instant::now());
                s
            }
            (None, None) => killed_status(),
        };
        self.reaped = true;
        let by = self.exited_at.unwrap_or_else(Instant::now) + PIPE_GRACE;
        let stderr = self.err.take().map(|d| d.take(by, &self.what)).unwrap_or_default();
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
                if let Some(Ok(Some(st))) = r.child.as_mut().map(Child::try_wait) {
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
                        r.out_reader.strand(&r.what);
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
        if let (false, Some(c)) = (self.reaped, self.child.take()) {
            abandon(c, |c| {
                let _ = c.kill();
            });
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

    /// `ps` 眼里这个 pid 现在是什么状态：空串 = 已经被收走了，`Z` 开头 = 僵尸（退了没人收）
    fn ps_stat(pid: u32) -> String {
        #[allow(clippy::disallowed_methods)] // 测试里查进程状态，输出就一个词
        let o = Command::new("ps").args(["-o", "stat=", "-p", &pid.to_string()]).output().unwrap();
        String::from_utf8_lossy(&o.stdout).trim().to_string()
    }

    /// #66：杀了也不退的（卡在网络卷上的不可中断调用里）不能把丢掉它的那条线程卡住；它后来退了，收尸线程要收走，不留僵尸。
    /// 「杀了也不退」造不出来，就让「杀」什么都不做：子进程照跑 1 秒 —— 原来的「kill 之后 wait」在这里要等满 1 秒
    #[test]
    fn 杀了也不退的_不当场死等_后来退了有人收尸() {
        #[allow(clippy::disallowed_methods)] // 测试里直接起一个子进程，交给 abandon
        let child = Command::new("sleep").arg("1").spawn().unwrap();
        let pid = child.id();
        let t = Instant::now();
        let st = abandon(child, |_| {});
        let took = t.elapsed();
        assert!(took < KILL_WAIT + Duration::from_millis(300), "当场等了 {took:?}：卡住的进程会把调用它的线程一起卡住");
        assert_eq!(st.signal(), Some(9), "没等到真的状态，按被杀记");
        assert!(!ps_stat(pid).is_empty(), "前提：交出去的时候它还在跑");
        let t = Instant::now();
        while !ps_stat(pid).is_empty() && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert_eq!(ps_stat(pid), "", "它退了之后没人收尸，留成了僵尸");
    }

    /// 收尸线程起不来时（线程数打满，`spawn` 报 EAGAIN）：孩子要**交回来**让调用方记 WARN，不能悄悄 drop；
    /// 下一次再试着起，不把这一次失败记一辈子（原来是 `OnceLock`，code review 2026-10-13）
    #[test]
    fn 收尸线程起不来_孩子交回来_下次再起() {
        let slot = Mutex::new(None);
        #[allow(clippy::disallowed_methods)] // 测试里直接起一个子进程，交给 hand_off
        let child = Command::new("true").spawn().unwrap();
        let (child, e) = hand_off(&slot, child, || Err(io::Error::other("模拟起不来"))).expect_err("起不来：要把孩子交回来");
        assert!(e.to_string().contains("模拟起不来"), "原因要带回来：{e}");
        assert!(slot.lock().unwrap().is_none(), "失败不能占住信箱");
        hand_off(&slot, child, spawn_reaper).expect("下一次起得来就用它");
        assert!(slot.lock().unwrap().is_some());
    }

    /// 信箱里那条线程没了（接收端掉了，`send` 失败）：重起一条接着交，不是把孩子扔掉
    #[test]
    fn 收尸线程没了_重起一条() {
        let (dead, rx) = mpsc::channel::<Child>();
        drop(rx);
        let slot = Mutex::new(Some(dead));
        #[allow(clippy::disallowed_methods)] // 测试里直接起一个子进程，交给 hand_off
        let child = Command::new("true").spawn().unwrap();
        let mut spawned = 0;
        hand_off(&slot, child, || {
            spawned += 1;
            spawn_reaper()
        })
        .expect("重起一条交给它");
        assert_eq!(spawned, 1, "信箱里那条已经没了，要重起");
    }

    /// 撒手的读线程要有账（code review 2026-10-12）：写端还被攥着时到点撒手 → 记成「撒手」；写端后来关了、线程读到 EOF 退出 → 记成「读完」（总数减回去）。
    /// 断言打在每条线程自己的标记上，不看全进程的 [`stranded`]：别的测试并行撒手，那个数会跟着动
    #[test]
    fn 撒手的读线程有账_后台进程退了销账() {
        let (rd, wr) = io::pipe().unwrap();
        let d = drain(rd, 100);
        let st = d.reader.clone();
        let got = d.take(Instant::now() + Duration::from_millis(50), "测试");
        assert!(got.is_empty());
        assert_eq!(st.0.load(Ordering::SeqCst), STRANDED, "写端还开着、线程还卡在 read 上：该记一笔");
        drop(wr);
        let t = Instant::now();
        while st.0.load(Ordering::SeqCst) != DONE && t.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(st.0.load(Ordering::SeqCst), DONE, "写端关了线程就退出，账要销掉");
    }

    /// 撒手之后不再替后台进程读：它再写一块，读线程就退出、关掉读端；它接着写撞上的是关掉的管道
    /// （真进程是 SIGPIPE；测试进程里 Rust 忽略了 SIGPIPE，表现为 BrokenPipe）。原来 stderr 这一路会一直读下去
    #[test]
    fn 撒手之后_后台进程再写_读端就关了() {
        use std::io::Write;
        let (rd, mut wr) = io::pipe().unwrap();
        let d = drain(rd, 100);
        let st = d.reader.clone();
        let _ = d.take(Instant::now() + Duration::from_millis(50), "测试");
        wr.write_all(b"late\n").unwrap();
        let t = Instant::now();
        while st.0.load(Ordering::SeqCst) != DONE && t.elapsed() < Duration::from_secs(2) {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(st.0.load(Ordering::SeqCst), DONE, "撒手后拿到下一块就该退出");
        let again = wr.write_all(b"again\n");
        assert_eq!(again.map_err(|e| e.kind()), Err(io::ErrorKind::BrokenPipe), "读端该关了");
    }

    #[test]
    fn 正常读完的_不记账() {
        let (rd, wr) = io::pipe().unwrap();
        let d = drain(rd, 100);
        let st = d.reader.clone();
        drop(wr);
        let _ = d.take(Instant::now() + Duration::from_secs(2), "测试");
        assert_eq!(st.0.load(Ordering::SeqCst), DONE);
    }

    /// 平常的路：SIGKILL 当场生效，拿到的是真的状态，不经过收尸线程
    #[test]
    fn 杀得掉的_当场收尸() {
        #[allow(clippy::disallowed_methods)] // 同上
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let st = abandon(child, |c| {
            let _ = c.kill();
        });
        assert_eq!(st.signal(), Some(9));
        assert_eq!(ps_stat(pid), "", "当场就该收走");
    }
}
