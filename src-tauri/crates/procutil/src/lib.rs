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
use std::process::{Child, ChildStdout, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;

/// stderr 默认留多少：够拼出一句能读的报错就行
pub const ERR_CAP: usize = 8 << 10;

/// 在另一条线程里把一路输出读到 EOF，只留前 `cap` 字节。**超过 `cap` 也要接着读**（只是不再存）——
/// 停下来不读，对方写满管道就卡在写上，正是要防的那个死锁
pub fn drain(mut src: impl Read + Send + 'static, cap: usize) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut kept = Vec::new();
        let mut chunk = [0u8; 8 << 10];
        loop {
            match src.read(&mut chunk) {
                Ok(0) | Err(_) => break,
                Ok(n) if kept.len() < cap => kept.extend_from_slice(&chunk[..n.min(cap - kept.len())]),
                Ok(_) => {}
            }
        }
        kept
    })
}

/// 起来了的子进程：stdin 接空（绝不让子进程卡住等输入）、stdout 是管道、**stderr 已经在另一条线程里排空**。
///
/// **丢掉时还没收尸，就杀掉再 wait**：中途 `?` 提前返回、解析出错 panic，都不会留下一个还在跑、或者成了僵尸的子进程。
/// 杀的是它自己，不是进程组 —— 要整组停的（git fetch 的孙进程）用 `process_group(0)` + `killpg`，那是调用方的事
pub struct Running {
    child: Child,
    err: Option<JoinHandle<Vec<u8>>>,
    reaped: bool,
}

/// 收完尸：退出状态 + stderr 的前 `err_cap` 字节
pub struct Finished {
    pub status: ExitStatus,
    pub stderr: Vec<u8>,
}

pub fn spawn(cmd: &mut Command, err_cap: usize) -> io::Result<Running> {
    let mut child = cmd.stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn()?;
    // **先**把 stderr 挂到自己的线程上，再让调用方去读 stdout —— 次序反了就是死锁
    let err = child.stderr.take().map(|e| drain(e, err_cap));
    Ok(Running { child, err, reaped: false })
}

impl Running {
    pub fn id(&self) -> u32 {
        self.child.id()
    }

    /// stdout。调用方读够了就可以丢掉它（读端一关，子进程下一次写收到 EPIPE / SIGPIPE 自己退），或者 [`Running::kill`]
    pub fn stdout(&mut self) -> &mut ChildStdout {
        self.child.stdout.as_mut().expect("spawn 里设了 piped")
    }

    /// 不要它的输出了：杀掉。**之后它的退出码没有意义**，调用方不能拿来判失败（rust.md「被掐掉的子进程退出码没有意义」）
    pub fn kill(&mut self) {
        let _ = self.child.kill();
    }

    /// 等它退出、收尸，拿回 stderr
    pub fn finish(mut self) -> io::Result<Finished> {
        // 先关掉 stdout 的读端：调用方没读完的话，子进程不会因为没人读卡在写上
        drop(self.child.stdout.take());
        let status = self.child.wait()?;
        self.reaped = true;
        let stderr = self.err.take().map(|h| h.join().unwrap_or_default()).unwrap_or_default();
        Ok(Finished { status, stderr })
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
    use std::time::Duration;

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
}
