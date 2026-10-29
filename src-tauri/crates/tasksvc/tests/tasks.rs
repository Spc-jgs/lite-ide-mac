//! tasksvc 的集成测试：真起 zsh、真进程组、真端口。
//!
//! shell 用 `/bin/zsh -ilc`（和产品同一条路），但 `ZDOTDIR` 指到空的临时目录 —— 不把跑测试的人的 `.zshrc` 拉进判据
//! （issue #30：rc 里的 pyenv 一类东西让 pty 测试每隔一轮红一次）。

use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tasksvc::{live, rotated, Spec, State, Task};

struct Dir(PathBuf);
impl Drop for Dir {
    fn drop(&mut self) {
        // 测试失败（断言 panic）时它起的进程还活着 —— `spawn_detached` 那几个不在任何 Task 里，没人收：按命令行里的这个目录找出来结束掉。
        // 目录名带 pid + 纳秒时间戳，碰不到别的进程（2026-10-10：验红时两个监听者各多活了 300 秒）
        let _ = std::process::Command::new("pkill").args(["-f", &self.0.to_string_lossy()]).status();
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn dir(tag: &str) -> Dir {
    let d = std::env::temp_dir().join(format!(
        "tasksvc-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    ));
    std::fs::create_dir_all(d.join("zdot")).unwrap();
    Dir(d.canonicalize().unwrap())
}

fn spec(d: &Path, command: &str) -> Spec {
    Spec {
        shell: "/bin/zsh".into(),
        command: command.into(),
        cwd: d.to_path_buf(),
        env: vec![("ZDOTDIR".into(), d.join("zdot").to_string_lossy().into_owned())],
        log: d.join("runs").join("t.log"),
        cap: tasksvc::LOG_CAP,
    }
}

/// 等到条件成立，最多 `secs` 秒
fn until(secs: u64, mut f: impl FnMut() -> bool) -> bool {
    let t = Instant::now();
    while t.elapsed() < Duration::from_secs(secs) {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(25));
    }
    f()
}

fn read(p: &Path) -> String {
    std::fs::read_to_string(p).unwrap_or_default()
}

/// 起一个监听的命令：**它自己挑端口**（绑 0），把号写进 `file`。
///
/// 原来是测试先绑 0 拿个号、关掉、再让 `nc` 去绑那个号 —— 中间有个空档，而 macOS 的 `nc` 开 `SO_REUSEPORT`：
/// 两条并行的测试拿到同一个号时，第二个 `nc` 照样绑上、不报错，`lsof` 查到的是另一条测试的进程。
/// 「结束占端口的_组长整组停」在 workspace 全量跑时间歇红过一次（组号对不上），就是这个形状（2026-10-10 梳理时实测坐实）。
/// Python 默认不开 `SO_REUSEPORT`，绑着 0 的时候内核给的号是唯一的；`reuse` 打开它，给「一个端口两个监听者」那条用
fn listener(file: &Path, reuse: Option<u16>) -> String {
    let (opt, port) = match reuse {
        Some(p) => ("s.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEPORT, 1); ", p),
        None => ("", 0),
    };
    format!(
        "/usr/bin/python3 -c 'import socket,sys,time; s=socket.socket(); {opt}s.bind((\"127.0.0.1\",{port})); s.listen(); \
         open(sys.argv[1],\"w\").write(str(s.getsockname()[1])); time.sleep(300)' {}",
        file.display()
    )
}

/// 等 [`listener`] 把端口号写出来
fn port_in(file: &Path) -> u16 {
    assert!(until(10, || read(file).parse::<u16>().is_ok()), "10 秒没等到监听的端口号");
    read(file).parse().unwrap()
}

/// 在后台起、**不当我们的子进程**（`sh -c '… &'`，sh 一退它就归 launchd 收尸）：留在测试进程的组里、不是组长。
/// 当子进程的话，它被结束后是僵尸、要等我们 wait，`kill(pid, 0)` 一直说它还在
fn spawn_detached(cmd: &str) {
    assert!(std::process::Command::new("/bin/sh").args(["-c", &format!("{cmd} &")]).status().unwrap().success());
}

fn listening(port: u16) -> bool {
    TcpStream::connect(("127.0.0.1", port)).is_ok()
}

#[test]
fn 两路输出进同一个文件_按写的先后_带上了不缓冲的环境变量() {
    let d = dir("out");
    let s = spec(&d.0, "echo one; echo two >&2; echo three; echo \"unbuf=$PYTHONUNBUFFERED\"");
    let t = Task::start(&s).unwrap();
    t.wait_exit(Duration::from_secs(10)).expect("10 秒没退");
    assert!(until(5, || read(&s.log).contains("unbuf=1")), "日志：{:?}", read(&s.log));
    assert_eq!(read(&s.log), "one\ntwo\nthree\nunbuf=1\n");
}

#[test]
fn 颜色码在写进日志之前剥掉() {
    let d = dir("ansi");
    let s = spec(&d.0, r"printf '\033[31mred\033[0m and \033[1mbold\033[22m\r\n'");
    let t = Task::start(&s).unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || read(&s.log).contains("bold")));
    assert_eq!(read(&s.log), "red and bold\n");
}

#[test]
fn 自己非零退出是失败_我们停的不是() {
    let d = dir("exit");
    let t = Task::start(&spec(&d.0, "exit 3")).unwrap();
    let st = t.wait_exit(Duration::from_secs(10)).unwrap();
    assert_eq!(st, State::Exited { code: Some(3), signal: None, stopped: false });
    assert_eq!(t.wait(), st, "不限时的 wait 等到的是同一个结果（退出事件的 watcher 用它）");
    assert!(st.failed());

    let t = Task::start(&spec(&d.0, "sleep 300")).unwrap();
    t.stop(Duration::from_secs(5));
    let st = t.wait_exit(Duration::from_secs(10)).expect("SIGINT 之后没退");
    assert!(matches!(st, State::Exited { stopped: true, .. }), "{st:?}");
    assert!(!st.failed(), "我们停的不算失败（pnpm 被停后退出码非零，第 0 步实测）：{st:?}");
}

/// issue #48 验收 2 的形状：组长底下还有孙子占着端口，停完**端口要空**。
/// 只停组长（VS Code 被骂的那条）的话，孙子还活着、端口还占着。
///
/// 这两个孙子是 `&` 起的，**它们忽略 SIGINT**：没有作业控制的 shell 里，后台命令按 POSIX 把 SIGINT / SIGQUIT 设成忽略
/// （你在终端按 ⌃C 不该打断后台任务）。所以软停对它们无效，要等宽限期满的 SIGKILL。第一版这里宽限期 5 秒、等 5 秒，
/// 两边赛跑，头一次碰巧过了、之后连红三次。宽限期给 1 秒，验的是「整组最后都没了、端口空了」，不是「SIGINT 就够」
#[test]
fn 停的时候孙进程一起停_端口空了() {
    let d = dir("tree");
    let pf = d.0.join("port");
    // 后台起一个监听的，再后台一个 sleep，前台 wait：组长是 zsh，底下两个孙子
    let t = Task::start(&spec(&d.0, &format!("{} & sleep 300 & wait", listener(&pf, None)))).unwrap();
    let port = port_in(&pf);
    assert!(until(10, || listening(port)), "10 秒内没监听上 {port}，日志：{:?}", read(&d.0.join("runs/t.log")));
    t.stop(Duration::from_secs(1));
    assert!(until(5, || !t.alive()), "宽限期过了 4 秒，组里还有活的");
    assert!(until(2, || !listening(port)), "组没了，{port} 还有人在听 —— 孙子逃出了进程组？");
    assert!(TcpListener::bind(("127.0.0.1", port)).is_ok(), "{port} 绑不上：没释放");
}

/// 忽略 SIGINT 的任务。**等它打出 `armed` 再发信号**：zsh 起来要 0.3 秒，信号落在 `trap` 之前的话 zsh 照样被打死 ——
/// 「强杀」那条第一版就这样，把 `kill()` 改成发 SIGINT 都没红（验红时发现的）
const ARMED: &str = "trap '' INT; echo armed; sleep 300";

fn wait_armed(d: &Path) {
    assert!(until(10, || read(&d.join("runs/t.log")).contains("armed")), "10 秒没等到 armed");
}

/// 软停不理（`trap '' INT`，子进程继承「忽略」）：宽限期过了要强杀。宽限期里还活着 —— 证明不是一上来就 SIGKILL
#[test]
fn 不理软停的_宽限期过了被强杀() {
    let d = dir("stubborn");
    let t = Task::start(&spec(&d.0, ARMED)).unwrap();
    wait_armed(&d.0);
    let t0 = Instant::now();
    t.stop(Duration::from_millis(800));
    std::thread::sleep(Duration::from_millis(400));
    assert!(t.alive(), "宽限期还没过就没了：软停不该是 SIGKILL");
    assert!(until(5, || !t.alive()), "宽限期过了还活着：没有强杀");
    assert!(t0.elapsed() >= Duration::from_millis(800), "比宽限期还早就死了：{:?}", t0.elapsed());
}

#[test]
fn 强杀不等宽限期() {
    let d = dir("kill");
    let t = Task::start(&spec(&d.0, ARMED)).unwrap();
    wait_armed(&d.0);
    t.kill();
    assert!(until(2, || !t.alive()), "SIGKILL 之后 2 秒还活着");
}

/// `java -jar a.jar & sleep 1` 这种：组长先退了，`&` 出去的还在组里 —— 任务没完，状态要还是「在跑」、停得掉（#56 第 1 条）。
/// 原来组长一退就记成已结束，界面上停止按钮灰掉，后台那个占着端口没人停得了
#[test]
fn 组长先退_后台的还在_任务还算在跑_停得掉() {
    let d = dir("bg");
    let t = Task::start(&spec(&d.0, "sleep 300 & echo leader-done")).unwrap();
    // 组长（zsh）回收之后 `kill(pid, 0)` 就是 ESRCH
    assert!(until(10, || unsafe { libc::kill(t.pgid(), 0) } != 0), "10 秒了组长还没退");
    std::thread::sleep(Duration::from_millis(500));
    assert!(t.alive(), "后台的 sleep 该还在组里");
    assert_eq!(t.state(), State::Running, "组里还有活的，不能记成已结束");
    t.stop(Duration::from_millis(500));
    let st = t.wait_exit(Duration::from_secs(5)).expect("停了 5 秒还没完");
    assert!(matches!(st, State::Exited { code: Some(0), stopped: true, .. }) && !st.failed(), "{st:?}");
    assert!(!t.alive());
}

/// 「结束它们」：几组一起停，宽限期只等一次。挨个停的话三个不理软停的要等三个宽限期（#56 第 5 条）
#[test]
fn 几组一起停_宽限期只等一次() {
    let ds: Vec<Dir> = (0..3).map(|i| dir(&format!("many{i}"))).collect();
    let ts: Vec<Task> = ds.iter().map(|d| Task::start(&spec(&d.0, ARMED)).unwrap()).collect();
    ds.iter().for_each(|d| wait_armed(&d.0));
    let t0 = Instant::now();
    tasksvc::stop_groups(&ts.iter().map(Task::pgid).collect::<Vec<_>>(), Duration::from_millis(600));
    let took = t0.elapsed();
    assert!(ts.iter().all(|t| until(2, || !t.alive())), "宽限期过了还有活的");
    assert!(took >= Duration::from_millis(600), "没等宽限期就强杀了：{took:?}");
    assert!(took < Duration::from_millis(1200), "等了不止一个宽限期（挨个停是 1800ms）：{took:?}");
}

/// 应用退出时 `Task` 被丢掉：组里不能留下孤儿（UNINSTALL.md 的承诺）
#[test]
fn 丢掉_task_不留孤儿() {
    let d = dir("drop");
    let t = Task::start(&spec(&d.0, "sleep 300 & wait")).unwrap();
    let pgid = t.pgid();
    assert!(until(5, || tasksvc::group_alive(pgid)));
    std::thread::sleep(Duration::from_millis(300));
    drop(t);
    assert!(until(3, || !tasksvc::group_alive(pgid)), "Task 丢掉了，进程组 {pgid} 还活着");
}

#[test]
fn 写到硬上限优先整行轮转_上一次的挪成点一() {
    let d = dir("rotate");
    let mut s = spec(&d.0, "");
    std::fs::create_dir_all(s.log.parent().unwrap()).unwrap();
    std::fs::write(&s.log, "上一次的输出\n").unwrap();

    // 一次读 64KB 也必须在块内分段；上限不能依赖读线程碰巧每次拿多大。
    s.command = "awk 'BEGIN { for (i = 0; i < 12000; i++) print \"line \" i \" ................\" }'".into();
    s.cap = 1000;
    let t = Task::start(&s).unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || read(&s.log).contains("line 11999")), "最后一行没写进当前那份");
    let (cur, old) = (read(&s.log), read(&rotated(&s.log)));
    assert!(!old.is_empty(), "轮转过，.1 不该是空的");
    assert!(!old.contains("上一次的输出"), "12000 行早就轮转过好几次了，上一次的那份应该已经被顶掉");
    for f in [&cur, &old] {
        if !f.contains("[lite-ide]") {
            for l in f.lines() {
                assert!(l.starts_with("line ") && l.ends_with('.'), "没有标记就把一行劈开了：{l:?}");
            }
        }
    }
    assert!(cur.len() <= 1000 && old.len() <= 1000, "两份都必须在硬上限以内");
}

#[test]
fn 应用一直开着_任务启动和输出结束都会清理过期日志() {
    let d = dir("live-prune");
    let base = d.0.join("runs");
    let expired = base.join("0000000000000011/old.log");
    let write_old = || {
        std::fs::create_dir_all(expired.parent().unwrap()).unwrap();
        std::fs::write(&expired, "expired\n").unwrap();
        std::fs::File::options().write(true).open(&expired).unwrap()
            .set_modified(std::time::SystemTime::now() - Duration::from_secs(30 * 86400)).unwrap();
    };
    write_old();
    let mut s = spec(&d.0, "echo ready; while [ ! -f go ]; do sleep 0.05; done; echo done");
    s.log = tasksvc::log_file(&base, "fixture", "main");
    let t = Task::start(&s).unwrap();
    assert!(!expired.exists(), "启动前应清掉旧输出，不依赖应用重启");
    assert!(until(5, || read(&s.log).contains("ready")));
    write_old();
    std::fs::write(d.0.join("go"), "").unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || !expired.exists()), "输出 EOF 后应再次清理；运行卡片仍持有 Task 也要生效");
    assert!(read(&s.log).contains("done"));
}

#[test]
fn 没有换行就退出_排队的日志断点也要写到输出() {
    let d = dir("note-eof");
    let s = spec(&d.0, "printf partial; while [ ! -f go ]; do sleep 0.05; done");
    let t = Task::start(&s).unwrap();
    assert!(until(5, || read(&s.log).contains("partial")));
    t.note("queued note at EOF");
    std::fs::write(d.0.join("go"), "").unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || read(&s.log).contains("queued note at EOF")), "不能因为最后没换行就丢插话");
}

#[test]
fn 起之前上一次的输出挪成点一() {
    let d = dir("prev");
    let s = spec(&d.0, "echo 这一次");
    std::fs::create_dir_all(s.log.parent().unwrap()).unwrap();
    std::fs::write(&s.log, "上一次\n").unwrap();
    let t = Task::start(&s).unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || read(&s.log) == "这一次\n"));
    assert_eq!(read(&rotated(&s.log)), "上一次\n");
}

/// 崩溃后收尸的判据：组号**和组长启动时间**都对上才算。只看组号的话，号被别人复用了也会去杀
#[test]
fn 收尸的记录_组号和启动时间都对上才算还活着() {
    let d = dir("live");
    let t = Task::start(&spec(&d.0, "sleep 300")).unwrap();
    let started = t.started_us().expect("读不到组长的启动时间");
    let rec = live::Live { pgid: t.pgid(), started_us: started, name: "t".into(), command: "sleep 300".into(), root: "/p".into() };
    let path = d.0.join("live.json");
    live::save(&path, std::slice::from_ref(&rec)).unwrap();
    let back = live::load(&path);
    assert_eq!(back, vec![rec.clone()]);
    assert!(live::still_running(&back[0]));

    let reused = live::Live { started_us: started + 1, ..rec.clone() };
    assert!(!live::still_running(&reused), "启动时间对不上（号被复用了）也认成了还活着");

    t.kill();
    assert!(until(3, || !live::still_running(&rec)), "杀掉之后还认成活着");
    assert!(live::load(&d.0.join("没有这个文件.json")).is_empty());
}

/// 端口被占卡片上点「结束它」：占着的是我们起的那种任务（进程组组长）→ 整组停、端口空出来
#[test]
fn 结束占端口的_组长整组停() {
    let d = dir("free-leader");
    let pf = d.0.join("port");
    let t = Task::start(&spec(&d.0, &listener(&pf, None))).unwrap();
    let port = port_in(&pf);
    assert!(until(10, || listening(port)));
    let hs = tasksvc::port::holders(port);
    assert!(hs.len() == 1 && hs[0].pgid == t.pgid(), "只有任务的那一个在听、在任务的进程组里：{hs:?}，任务组 {}", t.pgid());
    tasksvc::port::free_port(port, Duration::from_secs(2), |_| None).unwrap();
    assert!(!listening(port) && until(3, || !t.alive()), "端口空了、整组没了");
}

/// 不是组长的（比如你终端里一个作业里的某一个）：**只动它自己**。它的组里可能还有别人 ——
/// 这里它和跑测试的进程同组，整组杀的话测试进程自己就没了，这条测试当场失败
#[test]
fn 结束占端口的_不是组长只动它自己() {
    let d = dir("free-member");
    let pf = d.0.join("port");
    spawn_detached(&listener(&pf, None));
    let port = port_in(&pf);
    assert!(until(10, || listening(port)));
    let hs = tasksvc::port::holders(port);
    assert!(hs.len() == 1 && hs[0].pgid != hs[0].pid, "它该和测试进程同组、不是组长：{hs:?}");
    tasksvc::port::free_port(port, Duration::from_secs(2), |_| None).unwrap();
    assert!(!listening(port));
}

/// 占着端口的是我们自己还开着的任务（卡片说「是你之前跑的」）：要走那个 `Task` 停 —— 它才记得是「我们停的」。
/// 原来直接 `stop_group`，nc 被 SIGINT 带走，组长的退出记成了「失败」，界面上那一格变红、还弹一句「退出了」（code review 2026-10-10）
#[test]
fn 结束占端口的_是自己的任务记成我们停的() {
    let d = dir("free-ours");
    let pf = d.0.join("port");
    let t = std::sync::Arc::new(Task::start(&spec(&d.0, &listener(&pf, None))).unwrap());
    let port = port_in(&pf);
    assert!(until(10, || listening(port)));
    let pgid = t.pgid();
    tasksvc::port::free_port(port, Duration::from_secs(2), |g| (g == pgid).then(|| t.clone())).unwrap();
    let st = t.wait_exit(Duration::from_secs(3)).expect("组长该退了");
    assert!(matches!(st, State::Exited { stopped: true, .. }) && !st.failed(), "是我们停的，不算失败：{st:?}");
}

/// 一个端口两个监听者（`SO_REUSEPORT`：macOS 的 nc、Node 的 cluster）：**都要结束**。原来只结束 lsof 列的第一个，
/// 端口还被第二个占着，等满之后报「没结束掉」
#[test]
fn 一个端口两个监听者_都结束() {
    let d = dir("free-two");
    let (a, b) = (d.0.join("a"), d.0.join("b"));
    spawn_detached(&listener(&a, Some(0)));
    let port = port_in(&a);
    spawn_detached(&listener(&b, Some(port)));
    assert_eq!(port_in(&b), port);
    assert!(until(10, || tasksvc::port::holders(port).len() == 2), "两个都该在听：{:?}", tasksvc::port::holders(port));
    tasksvc::port::free_port(port, Duration::from_secs(2), |_| None).expect("两个都该结束掉");
    assert!(tasksvc::port::holders(port).is_empty());
}

/// 日志文件名：同一个项目落同一个目录、名字里的 `/` 不变成子目录、换掉字符之后不撞（#56 第 2 条）
#[test]
fn 日志放哪() {
    let base = Path::new("/r");
    let a = tasksvc::log_file(base, "/p", "web/dev");
    assert_eq!(a, tasksvc::log_file(base, "/p", "web/dev"), "同一个任务每次落同一个文件");
    assert_eq!(a.parent(), tasksvc::log_file(base, "/p", "x").parent(), "同一个项目同一个目录");
    assert_ne!(a.parent(), tasksvc::log_file(base, "/q", "x").parent(), "不同项目不同目录");
    assert_eq!(a.parent().unwrap().parent(), Some(base), "`/` 没变成子目录");
    let name = a.file_name().unwrap().to_str().unwrap();
    assert!(name.starts_with("web_dev-") && name.ends_with(".log"), "前半截还给人认：{name}");
    for (x, y) in [("web/dev", "web_dev"), ("a b", "a_b"), ("Dev", "dev")] {
        let (fx, fy) = (tasksvc::log_file(base, "/p", x), tasksvc::log_file(base, "/p", y));
        // APFS 默认不分大小写：比小写之后的，才是「会不会落到同一个文件」
        assert_ne!(fx.to_string_lossy().to_lowercase(), fy.to_string_lossy().to_lowercase(), "「{x}」和「{y}」落到了同一个文件");
    }
}

