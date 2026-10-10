//! 端口被占（docs/TASKS.md Q6）：从任务的输出里认出「端口被占」、抠出端口号，查出现在是谁在听，帮人把它结束掉。
//!
//! IDEA 论坛里被骂最多的那条：`Port 8080 was already in use`，上一次没停干净 / IDE 崩了 JVM 还活着，人只能自己 `lsof -i :8080`
//! 再 `kill`。这里把这两步做了，但**结束谁要人点一下**：可能是别的程序，结束了撤不回来（ui.md 第十三条第三档）。

use crate::Task;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// 输出里认出「端口被占」，返回端口号。只看带 `in use` 的行（各家的说法都有这两个词）；认不出端口号的（Python 的
/// `[Errno 48] Address already in use` 就不带端口）返回 None —— 不猜，猜错了会去结束一个不相干的进程。
///
/// 认的写法（测试里一条一条对着）：
/// - Spring Boot：`Port 8080 was already in use.`　vite：`Port 5173 is already in use`　Flask：`Port 5000 is in use by another program.`
/// - Node：`listen EADDRINUSE: address already in use :::3000` / `127.0.0.1:3000`
/// - Go：`listen tcp :8080: bind: address already in use`
/// - uvicorn：`error while attempting to bind on address ('127.0.0.1', 8000): address already in use`
/// - Tomcat：`Failed to start connector [Connector[HTTP/1.1-8080]]`（这一行不带 in use，单独认）
pub fn port_in_use(text: &str) -> Option<u16> {
    // 从后往前看：最后一次报的才是让它退出的那次
    for line in text.lines().rev() {
        let l = line.to_ascii_lowercase();
        if let Some(i) = l.find("connector[http/1.1-") {
            if let Some(p) = leading_port(&l[i + "connector[http/1.1-".len()..]) {
                return Some(p);
            }
        }
        if !l.contains("in use") {
            continue;
        }
        // `port 8080`
        if let Some(i) = l.find("port ") {
            if let Some(p) = leading_port(&l[i + 5..]) {
                return Some(p);
            }
        }
        // `('127.0.0.1', 8000)`
        if let Some(i) = l.find("', ") {
            if let Some(p) = leading_port(&l[i + 3..]) {
                return Some(p);
            }
        }
        // `:::3000`、`127.0.0.1:3000`、`tcp :8080:` —— 最后一个跟着数字的冒号
        let b = l.as_bytes();
        for (i, &c) in b.iter().enumerate().rev() {
            if c == b':' && b.get(i + 1).is_some_and(u8::is_ascii_digit) {
                if let Some(p) = leading_port(&l[i + 1..]) {
                    return Some(p);
                }
            }
        }
    }
    None
}

/// 开头那串数字当端口号（1–65535）
fn leading_port(s: &str) -> Option<u16> {
    let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
    digits.parse::<u16>().ok().filter(|&p| p > 0)
}

/// 正在听这个端口的进程
#[derive(Clone, Debug, PartialEq)]
pub struct Holder {
    pub pid: i32,
    /// 进程组号：和我们记过的任务对得上，就知道是「你上次跑的那个」
    pub pgid: i32,
    /// 进程名（lsof 给的，`java` / `node` / `Python`）
    pub command: String,
}

/// 谁在听 `port`（TCP LISTEN），**全部**。`lsof -F` 的机器格式：`p<pid>`、`g<进程组>`、`c<命令>` 各一行（还有 `f<fd>`，不要）。
///
/// 一个端口可以有好几个监听者：开了 `SO_REUSEPORT` 的程序（macOS 自带的 `nc` 就开，实测两个 `nc -lk` 同时听一个端口都不报错）、
/// Node 的 cluster。原来只取第一个，「结束它」只结束了一个、端口还占着，最后报「没结束掉」（2026-10-10 梳理时实测出来的）。
///
/// 进程组也从 lsof 拿，不另调 `getpgid`：少一个「查的那一刻进程状态变了」的口子。
/// 输出有上限吗（rust.md 起子进程第一问）：一个端口的监听者就一两个进程，几十字节；还是只读前 64KB，防一个不认识的 lsof 版本刷屏
pub fn holders(port: u16) -> Vec<Holder> {
    use std::io::Read;
    let Ok(mut child) = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpgc"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return Vec::new();
    };
    let mut out = String::new();
    if let Some(so) = child.stdout.take() {
        let _ = so.take(64 * 1024).read_to_string(&mut out);
    }
    let _ = child.wait();
    parse_lsof(&out)
}

fn parse_lsof(out: &str) -> Vec<Holder> {
    let mut all: Vec<Holder> = Vec::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            if let Ok(pid) = p.parse::<i32>() {
                all.push(Holder { pid, pgid: pid, command: String::new() });
            }
        } else if let Some(h) = all.last_mut() {
            if let Some(g) = line.strip_prefix('g') {
                h.pgid = g.parse().unwrap_or(h.pid);
            } else if let Some(c) = line.strip_prefix('c') {
                h.command = c.to_string();
            }
        }
    }
    all
}

/// 任务失败了：输出结尾认得出「端口被占」，就查出现在是谁在听（卡片上说第一个）。只读最后 64KB —— 报错就在结尾，日志可能有 1GB
pub fn from_log(log: &Path) -> Option<(u16, Holder)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(log).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024))).ok()?;
    let mut buf = Vec::new();
    f.take(64 * 1024).read_to_end(&mut buf).ok()?;
    let port = port_in_use(&String::from_utf8_lossy(&buf))?;
    Some((port, holders(port).into_iter().next()?))
}

/// 结束占着 `port` 的**所有**进程并等端口空出来（最多 `grace` 多一秒）。**会阻塞**，调用方放线程里。
///
/// 动手前**再查一次**是谁：卡片出来到人点按钮之间可能过了几分钟，原来那个进程早退了、号被别人复用了 —— 拿旧的 pid 去杀就杀错了人。
/// 每个监听者按 [`end`] 的三种情况处理，**并排进行**：好几个都不理软停的话，总共也只等一个宽限期。
pub fn free_port(port: u16, grace: Duration, ours: impl Fn(i32) -> Option<Arc<Task>> + Sync) -> Result<(), String> {
    let mut hs = holders(port);
    if hs.is_empty() {
        return Ok(());
    }
    // 同一组里的几个（一个任务的两个进程都在听）只处理一次：整组停一次就够了
    let mut seen = std::collections::HashSet::new();
    hs.retain(|h| h.pgid != h.pid && ours(h.pgid).is_none() || seen.insert(h.pgid));
    std::thread::scope(|s| {
        for h in &hs {
            let ours = &ours;
            s.spawn(move || end(h, grace, ours));
        }
    });
    // 进程都退了，再确认端口真空了（只在这里调 lsof：原来等的时候每 100ms 起一个，最坏一次「结束它」起 70 个）
    let t = Instant::now();
    loop {
        let left = holders(port);
        let Some(h) = left.first() else { return Ok(()) };
        if t.elapsed() >= Duration::from_secs(1) {
            return Err(format!("{port} 还被 {}（PID {}）占着，没结束掉 —— 可能是别的用户的进程，没有权限", h.command, h.pid));
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// 结束一个监听者：
/// - 是我们自己还开着的任务（`ours` 按进程组认出来）→ 走那个 [`Task`] 停，它才记得是「我们停的」，不报成失败；
/// - 别的进程组的组长（大多数守护进程）→ 整组先软后硬；
/// - 不是组长 → 只动它自己（它的组可能是你终端里的一整个作业，一起停就殃及了别人）：SIGTERM，宽限期后还在就 SIGKILL。
///   等它退用 `kill(pid, 0)`（一个系统调用），不再每 100ms 起一次 lsof
fn end(h: &Holder, grace: Duration, ours: &impl Fn(i32) -> Option<Arc<Task>>) {
    if let Some(t) = ours(h.pgid) {
        t.stop_wait(grace);
    } else if h.pgid == h.pid {
        crate::stop_group(h.pgid, grace);
    } else {
        // SAFETY: 发给 lsof 刚查出来的那个 pid；信号 0 只问不发
        unsafe { libc::kill(h.pid, libc::SIGTERM) };
        let t = Instant::now();
        while t.elapsed() < grace && unsafe { libc::kill(h.pid, 0) } == 0 {
            std::thread::sleep(Duration::from_millis(50));
        }
        if unsafe { libc::kill(h.pid, 0) } == 0 {
            unsafe { libc::kill(h.pid, libc::SIGKILL) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 各家的说法都认得出端口() {
        let cases = [
            ("***************************\nAPPLICATION FAILED TO START\n***************************\nDescription:\nWeb server failed to start. Port 8080 was already in use.\n", 8080),
            ("  error when starting dev server:\nError: Port 5173 is already in use\n", 5173),
            ("Address already in use\nPort 5000 is in use by another program. Either identify and stop that program, or start the server with a different port.\n", 5000),
            ("Error: listen EADDRINUSE: address already in use :::3000\n    at Server.setupListenHandle\n", 3000),
            ("Error: listen EADDRINUSE: address already in use 127.0.0.1:3001\n", 3001),
            ("2026/10/10 09:00:00 listen tcp :8081: bind: address already in use\n", 8081),
            ("ERROR:    [Errno 48] error while attempting to bind on address ('127.0.0.1', 8000): address already in use\n", 8000),
            ("SEVERE [main] org.apache.catalina.core.StandardService.initInternal Failed to start connector [Connector[HTTP/1.1-8082]]\n", 8082),
        ];
        for (text, want) in cases {
            assert_eq!(port_in_use(text), Some(want), "认不出：{text}");
        }
    }

    /// 不带端口号的不猜：猜错了会去结束一个不相干的进程
    #[test]
    fn 认不出端口号就不猜() {
        assert_eq!(port_in_use("OSError: [Errno 48] Address already in use\n"), None);
        assert_eq!(port_in_use("Started App on port 8080 in 1.2 seconds\n"), None, "没有 in use 的行不算");
        assert_eq!(port_in_use(""), None);
    }

    #[test]
    fn 多次报的取最后一次() {
        assert_eq!(port_in_use("Port 1111 is already in use\ntrying 2222\nPort 2222 is already in use\n"), Some(2222));
    }

    #[test]
    fn 查得出谁在听_查得出组号() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hs = holders(port);
        assert_eq!(hs.len(), 1, "只有自己一个在听：{hs:?}");
        assert_eq!(hs[0].pid, std::process::id() as i32);
        assert_eq!(hs[0].pgid, unsafe { libc::getpgid(0) });
        drop(l);
        assert!(holders(port).is_empty(), "关了还查得到");
    }

    /// lsof 的机器格式里一个端口好几个监听者（`f` 行夹在中间）：一个不落、各自带上自己的组和名字
    #[test]
    fn 好几个监听者都认出来() {
        let out = "p100\ng90\ncnc\nf3\np101\ng90\ncnc\nf3\np200\ng200\ncjava\nf12\n";
        let hs = parse_lsof(out);
        assert_eq!(hs.iter().map(|h| (h.pid, h.pgid, h.command.as_str())).collect::<Vec<_>>(), [(100, 90, "nc"), (101, 90, "nc"), (200, 200, "java")]);
    }
}
