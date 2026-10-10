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

/// 谁在听 `port`（TCP LISTEN）。`lsof -F` 的机器格式：`p<pid>`、`g<进程组>`、`c<命令>` 各一行。
/// 进程组也从 lsof 拿，不另调 `getpgid`：第一版是调的，workspace 全量跑时间歇地对不上任务的组号（单独跑 15 遍复现不了），
/// 少一次系统调用就少一个「查的那一刻进程状态变了」的口子。
/// 输出有上限吗（rust.md 起子进程第一问）：一个端口的监听者就一两个进程，几十字节；还是只读前 64KB，防一个不认识的 lsof 版本刷屏
pub fn holder(port: u16) -> Option<Holder> {
    use std::io::Read;
    let mut child = Command::new("lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpgc"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut out = String::new();
    let _ = child.stdout.take()?.take(64 * 1024).read_to_string(&mut out);
    let _ = child.wait();
    let mut pid = None;
    let mut pgid = None;
    let mut command = String::new();
    for line in out.lines() {
        if let Some(p) = line.strip_prefix('p') {
            if pid.is_some() {
                break; // 只要第一个进程
            }
            pid = p.parse::<i32>().ok();
        } else if let Some(g) = line.strip_prefix('g') {
            pgid = g.parse::<i32>().ok();
        } else if let Some(c) = line.strip_prefix('c') {
            command = c.to_string();
        }
    }
    let pid = pid?;
    Some(Holder { pid, pgid: pgid.unwrap_or(pid), command })
}

/// 任务失败了：输出结尾认得出「端口被占」，就查出现在是谁在听。只读最后 64KB —— 报错就在结尾，日志可能有 1GB
pub fn from_log(log: &Path) -> Option<(u16, Holder)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(log).ok()?;
    let len = f.metadata().ok()?.len();
    f.seek(SeekFrom::Start(len.saturating_sub(64 * 1024))).ok()?;
    let mut buf = Vec::new();
    f.take(64 * 1024).read_to_end(&mut buf).ok()?;
    let port = port_in_use(&String::from_utf8_lossy(&buf))?;
    Some((port, holder(port)?))
}

/// 结束占着 `port` 的那个进程并等端口空出来（最多 `grace` 多一秒）。**会阻塞**，调用方放线程里。
///
/// 动手前**再查一次**是谁：卡片出来到人点按钮之间可能过了几分钟，原来那个进程早退了、号被别人复用了 —— 拿旧的 pid 去杀就杀错了人。
/// 是我们自己还开着的任务（`ours` 按进程组认出来）→ 走那个 [`Task`] 停，它才记得是「我们停的」，不报成失败；
/// 别的进程组的组长（大多数守护进程）→ 整组先软后硬；不是组长 → 只动它自己（它的组可能是你终端里的一整个作业，
/// 一起停就殃及了别人）：SIGTERM，宽限期后 SIGKILL。
pub fn free_port(port: u16, grace: Duration, ours: impl Fn(i32) -> Option<Arc<Task>>) -> Result<(), String> {
    let Some(h) = holder(port) else { return Ok(()) };
    if let Some(t) = ours(h.pgid) {
        t.stop_wait(grace);
    } else if h.pgid == h.pid {
        crate::stop_group(h.pgid, grace);
    } else {
        // SAFETY: 发给 lsof 刚查出来的那个 pid
        unsafe { libc::kill(h.pid, libc::SIGTERM) };
        let t = Instant::now();
        while t.elapsed() < grace && holder(port).is_some_and(|x| x.pid == h.pid) {
            std::thread::sleep(Duration::from_millis(100));
        }
        unsafe { libc::kill(h.pid, libc::SIGKILL) };
    }
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(1) + grace {
        if holder(port).is_none() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(format!("{port} 还被 {}（PID {}）占着，没结束掉 —— 可能是别的用户的进程，没有权限", h.command, h.pid))
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
        let h = holder(port).expect("lsof 没查到自己开的监听");
        assert_eq!(h.pid, std::process::id() as i32);
        assert_eq!(h.pgid, unsafe { libc::getpgid(0) });
        drop(l);
        assert!(holder(port).is_none(), "关了还查得到");
    }
}
