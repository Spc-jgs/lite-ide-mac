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

fn free_port() -> u16 {
    TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port()
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
    let port = free_port();
    // 后台起一个监听的，再后台一个 sleep，前台 wait：组长是 zsh，底下两个孙子
    let t = Task::start(&spec(&d.0, &format!("nc -lk 127.0.0.1 {port} & sleep 300 & wait"))).unwrap();
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
fn 写到上限在换行处轮转_一行不劈开_上一次的挪成点一() {
    let d = dir("rotate");
    let mut s = spec(&d.0, "");
    std::fs::create_dir_all(s.log.parent().unwrap()).unwrap();
    std::fs::write(&s.log, "上一次的输出\n").unwrap();

    s.command = "i=0; while [ $i -lt 400 ]; do echo \"line $i ................\"; i=$((i+1)); done".into();
    s.cap = 1000;
    let t = Task::start(&s).unwrap();
    t.wait_exit(Duration::from_secs(10)).unwrap();
    assert!(until(5, || read(&s.log).contains("line 399")), "最后一行没写进当前那份");
    let (cur, old) = (read(&s.log), read(&rotated(&s.log)));
    assert!(!old.is_empty(), "轮转过，.1 不该是空的");
    assert!(!old.contains("上一次的输出"), "400 行早就轮转过好几次了，上一次的那份应该已经被顶掉");
    for f in [&cur, &old] {
        for l in f.lines() {
            assert!(l.starts_with("line ") && l.ends_with('.'), "有一行被劈开了：{l:?}");
        }
    }
    // 闸的意义：每份都在上限附近，不会无限长（上限 + 一块的余量）
    assert!(cur.len() < 1000 + 64 * 1024 && old.len() < 1000 + 64 * 1024);
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
    let port = free_port();
    let t = Task::start(&spec(&d.0, &format!("nc -lk 127.0.0.1 {port}"))).unwrap();
    assert!(until(10, || listening(port)));
    let h = tasksvc::port::holder(port).expect("查不到谁在听");
    // 间歇红过一次（workspace 全量跑时，单独跑复现不了）：对不上时把现场打出来，下次撞上不用再猜
    let ps = |g: i32| String::from_utf8_lossy(&std::process::Command::new("ps").args(["-o", "pid,pgid,stat,command", "-g", &g.to_string()]).output().unwrap().stdout).into_owned();
    assert_eq!(h.pgid, t.pgid(), "nc 应该在任务的进程组里。查到的：{h:?}\n任务组 {}：\n{}\n查到的组：\n{}", t.pgid(), ps(t.pgid()), ps(h.pgid));
    tasksvc::port::free_port(port, Duration::from_secs(2), |_| None).unwrap();
    assert!(!listening(port) && until(3, || !t.alive()), "端口空了、整组没了");
}

/// 不是组长的（比如你终端里一个作业里的某一个）：**只动它自己**。它的组里可能还有别人 ——
/// 这里它和跑测试的进程同组，整组杀的话测试进程自己就没了，这条测试当场失败
#[test]
fn 结束占端口的_不是组长只动它自己() {
    let port = free_port();
    let mut c = std::process::Command::new("nc").args(["-lk", "127.0.0.1", &port.to_string()]).spawn().unwrap();
    assert!(until(10, || listening(port)));
    let h = tasksvc::port::holder(port).unwrap();
    assert_ne!(h.pgid, h.pid, "它该和测试进程同组、不是组长");
    tasksvc::port::free_port(port, Duration::from_secs(2), |_| None).unwrap();
    assert!(!listening(port));
    let _ = c.wait();
}

/// 占着端口的是我们自己还开着的任务（卡片说「是你之前跑的」）：要走那个 `Task` 停 —— 它才记得是「我们停的」。
/// 原来直接 `stop_group`，nc 被 SIGINT 带走，组长的退出记成了「失败」，界面上那一格变红、还弹一句「退出了」（code review 2026-10-10）
#[test]
fn 结束占端口的_是自己的任务记成我们停的() {
    let d = dir("free-ours");
    let port = free_port();
    let t = std::sync::Arc::new(Task::start(&spec(&d.0, &format!("nc -lk 127.0.0.1 {port}"))).unwrap());
    assert!(until(10, || listening(port)));
    let pgid = t.pgid();
    tasksvc::port::free_port(port, Duration::from_secs(2), |g| (g == pgid).then(|| t.clone())).unwrap();
    let st = t.wait_exit(Duration::from_secs(3)).expect("组长该退了");
    assert!(matches!(st, State::Exited { stopped: true, .. }) && !st.failed(), "是我们停的，不算失败：{st:?}");
}

/// 日志文件名：同一个项目落同一个目录、名字里的 `/` 不变成子目录
#[test]
fn 日志放哪() {
    let base = Path::new("/r");
    let a = tasksvc::log_file(base, "/p", "web/dev");
    assert_eq!(a.parent(), tasksvc::log_file(base, "/p", "x").parent(), "同一个项目同一个目录");
    assert_ne!(a.parent(), tasksvc::log_file(base, "/q", "x").parent(), "不同项目不同目录");
    assert_eq!(a.file_name().unwrap(), "web_dev.log");
    assert_eq!(a.parent().unwrap().parent(), Some(base));
}

