//! 端口被占（docs/TASKS.md Q6）：从任务的输出里认出「端口被占」、抠出端口号，查出现在是谁在听，帮人把它结束掉。
//!
//! IDEA 论坛里被骂最多的那条：`Port 8080 was already in use`，上一次没停干净 / IDE 崩了 JVM 还活着，人只能自己 `lsof -i :8080`
//! 再 `kill`。这里把这两步做了，但**结束谁要人点一下**：可能是别的程序，结束了撤不回来（ui.md 第十三条第三档）。

use crate::Task;
use std::path::Path;
use std::process::Command;
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
    let mut cmd = Command::new("lsof");
    cmd.args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fpgc"]);
    match procutil::run_capped(&mut cmd, 64 * 1024, procutil::ERR_CAP, procutil::Overflow::Kill) {
        Ok(c) => parse_lsof(&String::from_utf8_lossy(&c.stdout)),
        Err(_) => Vec::new(),
    }
}

/// 这个进程组在听哪些 TCP 端口（Services 工具窗里服务名后面那个 `:8080`）。`-a` 把「进程组是它」和「在听 TCP」**取交集**
/// （lsof 默认是并集）；`-Fn` 只要名字那一列：`n*:8080`、`n127.0.0.1:53195`、`n[::1]:8080`。
/// 输出有上限吗：一个服务听几个端口，几十字节；照样只读前 64KB
pub fn listening(pgid: i32) -> Vec<u16> {
    let mut cmd = Command::new("lsof");
    cmd.args(["-nP", "-a", "-g", &pgid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fn"]);
    match procutil::run_capped(&mut cmd, 64 * 1024, procutil::ERR_CAP, procutil::Overflow::Kill) {
        Ok(c) => parse_listen(&String::from_utf8_lossy(&c.stdout)),
        Err(_) => Vec::new(),
    }
}

/// 同 [`listening`]，按进程分开：调试单测时要从组里认出「带调试代理的那个 JVM」听的端口（surefire fork 的 JVM 和 mvn 在同一个组里）
pub fn listening_by_pid(pgid: i32) -> Vec<(i32, Vec<u16>)> {
    let mut cmd = Command::new("lsof");
    cmd.args(["-nP", "-a", "-g", &pgid.to_string(), "-iTCP", "-sTCP:LISTEN", "-Fpn"]);
    match procutil::run_capped(&mut cmd, 64 * 1024, procutil::ERR_CAP, procutil::Overflow::Kill) {
        Ok(c) => parse_listen_by_pid(&String::from_utf8_lossy(&c.stdout)),
        Err(_) => Vec::new(),
    }
}

/// `lsof -Fpn`：`p<pid>` 起一段，后面的 `n` 行是它听的（`f` 行夹在中间，不要）
fn parse_listen_by_pid(out: &str) -> Vec<(i32, Vec<u16>)> {
    let mut all: Vec<(i32, String)> = Vec::new();
    for l in out.lines() {
        if let Some(p) = l.strip_prefix('p').and_then(|p| p.parse().ok()) {
            all.push((p, String::new()));
        } else if let (Some(n), Some(last)) = (l.strip_prefix('n'), all.last_mut()) {
            last.1.push_str(&format!("n{n}\n"));
        }
    }
    all.into_iter().map(|(p, ns)| (p, parse_listen(&ns))).filter(|(_, ps)| !ps.is_empty()).collect()
}

/// 一个进程的命令行（`ps -o command=`，参数用空格连着）。查不到（已经退了）给 None。输出有上限吗：一行命令行，classpath 长的几十 KB，读 256KB
pub fn command_of(pid: i32) -> Option<String> {
    let mut cmd = Command::new("ps");
    cmd.args(["-o", "command=", "-p", &pid.to_string()]);
    let c = procutil::run_capped(&mut cmd, 256 * 1024, procutil::ERR_CAP, procutil::Overflow::Kill).ok()?;
    let s = String::from_utf8_lossy(&c.stdout).trim().to_string();
    (!s.is_empty()).then_some(s)
}

/// `lsof -Fn` 的输出 → 端口，去重、排好（IPv4 和 IPv6 各听一遍同一个端口是常态）
fn parse_listen(out: &str) -> Vec<u16> {
    let mut ports: Vec<u16> = out
        .lines()
        .filter_map(|l| l.strip_prefix('n'))
        .filter_map(|n| n.rsplit(':').next()?.parse::<u16>().ok())
        .filter(|&p| p > 0)
        .collect();
    ports.sort_unstable();
    ports.dedup();
    ports
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

/// 服务器在输出里**宣告自己在听**的端口（#59），按出现次序、去重。只认宣告的说法，不认输出里随便一个 `localhost:端口`：
/// Spring 应用的日志里满是它要去**连**的别的本地服务（`http://localhost:8761/eureka`、`jdbc:postgresql://localhost:5432`），
/// 那些端口停完以后当然还被占着，报出来就是一张带「结束进程」的假卡片。
///
/// 认的写法（测试里一条一条对着）：
/// - Spring Boot：`Tomcat started on port 8080 (http)`、`Tomcat started on port(s): 8080 (http)`（Netty / Undertow / Jetty 同）
/// - vite / Next：`➜  Local:   http://localhost:5173/`、`- Local: http://localhost:3000`
/// - uvicorn / Flask / http-server：`Uvicorn running on http://127.0.0.1:8000`、`Running on http://127.0.0.1:5000`、`Server running at http://localhost:8080/`
/// - Node 常见的：`Listening on port 3000`、`listening on http://0.0.0.0:3000`
pub fn announced(text: &str) -> Vec<u16> {
    const LOCAL: [&str; 5] = ["localhost", "127.0.0.1", "0.0.0.0", "[::1]", "[::]"];
    const SAYS: [&str; 7] = ["local:", "running on", "running at", "listening on", "listening at", "serving on", "available on"];
    let mut out = Vec::new();
    for line in text.lines() {
        let l = line.to_ascii_lowercase();
        let mut found = None;
        if let Some(i) = l.find("started on port") {
            let rest = l[i + "started on port".len()..].trim_start_matches("(s)").trim_start_matches(':').trim_start();
            found = leading_port(rest);
        } else if SAYS.iter().any(|s| l.contains(s)) {
            if let Some(i) = l.find("listening on port ") {
                found = leading_port(&l[i + "listening on port ".len()..]);
            }
            for (i, _) in l.match_indices("://") {
                if found.is_some() || !(l[..i].ends_with("http") || l[..i].ends_with("https")) {
                    continue;
                }
                let rest = &l[i + 3..];
                if let Some(h) = LOCAL.iter().find(|h| rest.starts_with(**h)) {
                    found = rest[h.len()..].strip_prefix(':').and_then(leading_port);
                }
            }
        }
        if let Some(p) = found.filter(|p| !out.contains(p)) {
            out.push(p);
        }
    }
    out
}

/// 停完以后（#59）：任务宣告过的端口还被占着，**而且占着的进程是任务启动之后才起来的** —— 多半是它派生出来、逃出了进程组的
/// （`./gradlew bootRun` 的应用 JVM 是 Gradle 守护进程起的、`setsid` 起的另开一组），整组 SIGINT 够不着。
/// 早就在的进程（你另起的服务、Docker 的端口代理）不算：不是这次任务的，报出来就是在劝人结束别人的进程。
/// 读输出的开头 1 MB（启动时宣告）和结尾 256 KB（devtools 重启后再宣告一次）：日志可能有 1GB
pub fn lingering(log: &Path, since_us: u64) -> Option<(u16, Holder)> {
    use std::io::{Read, Seek, SeekFrom};
    let mut f = std::fs::File::open(log).ok()?;
    let len = f.metadata().ok()?.len();
    let mut head = Vec::new();
    (&mut f).take(1 << 20).read_to_end(&mut head).ok()?;
    let mut tail = Vec::new();
    if len > 1 << 20 {
        f.seek(SeekFrom::Start(len.saturating_sub(256 << 10).max(1 << 20))).ok()?;
        f.take(256 << 10).read_to_end(&mut tail).ok()?;
    }
    let text = format!("{}\n{}", String::from_utf8_lossy(&head), String::from_utf8_lossy(&tail));
    announced(&text).into_iter().find_map(|port| {
        holders(port).into_iter().find(|h| crate::live::start_time_us(h.pid).is_some_and(|t| t >= since_us)).map(|h| (port, h))
    })
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
    #[test]
    fn 宣告的端口_只认服务器说自己在听的_不认它要去连的() {
        let log = "\
2026-10-28 10:00:01.002  INFO 1 --- [main] o.s.b.w.e.tomcat.TomcatWebServer : Tomcat started on port 8080 (http) with context path '/'
2026-10-28 10:00:01.003  INFO 1 --- [main] o.s.b.w.e.tomcat.TomcatWebServer : Tomcat started on port(s): 8081 (http)
  ➜  Local:   http://localhost:5173/
  - Local:        http://localhost:3000
INFO:     Uvicorn running on http://127.0.0.1:8000 (Press CTRL+C to quit)
 * Running on http://127.0.0.1:5000
Listening on port 4000
server listening on http://[::1]:4001
2026-10-28 INFO DiscoveryClient : Registering application with eureka at http://localhost:8761/eureka
2026-10-28 INFO HikariPool : Added connection jdbc:postgresql://localhost:5432/shop
Fetching config from server at : http://localhost:8888
  ➜  Network: http://192.168.1.5:5173/
Tomcat started on port 8080 (http) with context path '/'
";
        assert_eq!(announced(log), [8080, 8081, 5173, 3000, 8000, 5000, 4000, 4001], "去重、按出现次序；Eureka / JDBC / 配置中心那几个要去连的不算，局域网地址不算");
        assert!(announced("").is_empty());
    }

    /// 逃出进程组的那种（`setsid` / Gradle 守护进程起的 JVM）：两次 fork + setsid 的监听进程，宣告了端口。
    /// 任务启动时刻在它之前 → 认出来；在它之后（早就在的进程）→ 不认
    #[test]
    fn 停完还占着端口的_只认任务启动之后才起来的进程() {
        let dir = std::env::temp_dir().join(format!("tasksvc-linger-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let log = dir.join("out.log");
        let pidf = dir.join("pid");
        let before = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_micros() as u64;
        let py = r#"
import os, socket, sys, time
if os.fork(): sys.exit(0)
os.setsid()
if os.fork(): sys.exit(0)
s = socket.socket(); s.bind(("127.0.0.1", 0)); s.listen()
open(sys.argv[2], "w").write(str(os.getpid()))
open(sys.argv[1], "w").write("Listening on http://127.0.0.1:%d/\n" % s.getsockname()[1])
time.sleep(30)
"#;
        let st = std::process::Command::new("python3").args(["-I", "-c", py]).arg(&log).arg(&pidf).status().unwrap();
        assert!(st.success());
        let t = std::time::Instant::now();
        while !(pidf.exists() && log.exists() && std::fs::metadata(&log).map(|m| m.len() > 0).unwrap_or(false)) && t.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        let pid: i32 = std::fs::read_to_string(&pidf).unwrap().trim().parse().unwrap();
        let got = lingering(&log, before);
        let later = lingering(&log, u64::MAX);
        // SAFETY: 测试自己起的那个监听进程
        unsafe {
            libc::kill(pid, libc::SIGKILL);
        }
        let _ = std::fs::remove_dir_all(&dir);
        let (port, h) = got.expect("任务启动之后起来、还占着宣告的端口：认出来");
        assert_eq!(h.pid, pid, "是那个逃出去的进程（端口 {port}）");
        assert_ne!(h.pgid, std::process::id() as i32, "确实不在我们的进程组里");
        assert!(later.is_none(), "比任务还早就在的：不认");
    }

    #[test]
    fn 听的端口_ipv4_ipv6_同一个只算一次_排好() {
        // 9090 只在 IPv6 上听：地址里自己带冒号，端口要从最后一个冒号后面取（只有 8080 那种 v4 / v6 都有的，切错了也看不出来）
        let out = "p4242\nf12\nn*:8080\nf13\nn[::1]:8080\nf14\nn127.0.0.1:53195\nf15\nn*:*\nf16\nn[::1]:9090\n";
        assert_eq!(parse_listen(out), vec![8080, 9090, 53195]);
        assert!(parse_listen("").is_empty());
    }

    #[test]
    fn 按进程分开_每个进程只算自己的端口_不听的不列() {
        // mvn（4242）不听；fork 的 JVM（4300）听调试端口；另一个（4400）v4 v6 各听一遍同一个
        let out = "p4242\np4300\nf9\nn127.0.0.1:52450\np4400\nf3\nn*:8080\nf4\nn[::1]:8080\n";
        assert_eq!(parse_listen_by_pid(out), vec![(4300, vec![52450]), (4400, vec![8080])]);
        assert!(parse_listen_by_pid("").is_empty());
    }

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
