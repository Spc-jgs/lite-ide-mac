//! 通用任务运行（#48，设计见 docs/TASKS.md）：起一条命令、输出写进日志文件、一个调用停掉整组。零 Tauri 依赖。
//!
//! 三条是第 0 步实测定下来的（TASKS.md 第 10 节），改之前先看那一节：
//!
//! - **经 `$SHELL -ilc` 起**：从 Finder 启动时没有 shell 的 PATH，而用户的 PATH 在 `.zshrc` 里，zsh 只在交互式（`-i`）时读它。
//!   `-lc` 实测找不到 mvn / pnpm，`-ilc` 找得到，无警告、0.33 秒。
//! - **管道，不用 pty**：管道下 Spring Boot / vite 的颜色码是 0（pty 292 / 42），日志视图不认 ANSI。代价是块缓冲 ——
//!   Python 的 4 行在结束时一起到 —— 给 `PYTHONUNBUFFERED=1` 就和 pty 一样实时。stdout、stderr 接**同一根管道**：
//!   两根管道就得两个线程并发读（rust.md：顺序读会死锁），还排不出先后；一根管道由内核按写入顺序排好。
//! - **自己的进程组，停的时候整组**：`npm run` → node → esbuild 三层，只停最上面那个（VS Code 被骂的那条）端口就还占着。
//!   先 SIGINT（终端里 ⌃C 的效果：Spring 的关闭钩子要跑，实测 0.25 秒全退），宽限期过了还有活的就 SIGKILL。

mod clean;
pub mod live;
pub mod port;
pub mod prune;
mod sink;

pub use clean::Cleaner;
pub use sink::rotated;

use std::io::{self, Read};
use std::os::unix::process::{CommandExt, ExitStatusExt};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

/// 单个日志文件写到多大轮转（TASKS.md Q7）
pub const LOG_CAP: u64 = 1 << 30;
/// 软停之后等多久再强杀（TASKS.md Q5）
pub const GRACE: Duration = Duration::from_secs(5);

pub struct Spec {
    /// 用哪个 shell；空串 = `$SHELL`，再没有就 `/bin/zsh`。产品里跟设置的 `terminal.shell` 走
    pub shell: String,
    /// 一整行命令，交给 shell 去解释（`&&`、管道、变量都能用）
    pub command: String,
    pub cwd: PathBuf,
    /// 叠在继承来的环境上
    pub env: Vec<(String, String)>,
    pub log: PathBuf,
    /// 轮转上限，产品里是 [`LOG_CAP`]；测试里注入一个小的
    pub cap: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub enum State {
    /// 组里还有活的。组长退了、`&` 出去的后台进程还在，也算在跑（停的是整组，在跑的也是整组）
    Running,
    /// 发过 SIGINT，在等它收尾（宽限期内再叫一次 `kill` 就立刻强杀）
    Stopping,
    /// 整组都没了。`code` / `signal` 是组长的。`stopped` = 是我们停的：pnpm 被 SIGINT 后会报 `Command failed`、退出码非零，
    /// 那不是失败（第 0 步实测）—— 只有没人停它、它自己非零退出才算失败
    Exited { code: Option<i32>, signal: Option<i32>, stopped: bool },
}

impl State {
    pub fn failed(&self) -> bool {
        matches!(self, State::Exited { code, stopped: false, .. } if *code != Some(0))
    }
}

struct Shared {
    state: Mutex<State>,
    changed: Condvar,
    /// 叫过 stop / kill。整组没了时据此把状态记成「已停止」而不是「失败」
    stopped: AtomicBool,
}

pub struct Task {
    pgid: i32,
    started_us: Option<u64>,
    shared: Arc<Shared>,
}

/// 组里还有没有活着的进程。`kill(-pgid, 0)` 只问不发：0 = 有，ESRCH = 一个都没了，**EPERM 两种意思都有**，要列组员再判（[`judge`]）。
///
/// EPERM 的两个来源（第 0 步实测一个、2026-10-12 读 Codex 时补实测另一个，#65）：组里只剩**没被回收的僵尸**；
/// 或者组里活着的都是**真实 uid 不是我们**的进程 —— 免密的 `sudo` 正以 root 跑命令。原来一律当「没有活的」，
/// 后一种就是「界面说已停止、root 起的服务还占着端口」，账本也划掉了。
pub fn group_alive(pgid: i32) -> bool {
    // SAFETY: 信号 0 不发任何东西，只做存在性检查
    let probe = match unsafe { libc::killpg(pgid, 0) } {
        0 => None,
        _ => io::Error::last_os_error().raw_os_error(),
    };
    judge(probe, || group_members(pgid).into_iter().map(member).collect())
}

/// 组里的一个进程，从我们这边看
#[derive(Clone, Copy, Debug, PartialEq)]
enum Member {
    Live,
    /// 僵尸（父进程还没回收）或者已经没了：两种都不算活的
    Gone,
    /// 在，但不归我们管（`proc_pidinfo` 回 EPERM：别的用户的，典型是 root）
    NotOurs,
}

/// `killpg(组, 0)` 的结果（`None` = 成功，否则 errno）加上组员 → 组里还有没有活的。
/// 拆出来是为了不用真造一个 root 进程就能测 EPERM 那条路（Codex `process_group.rs` 也是这么测的：把「发给组」「发给单个进程」当参数传，测试里注入 EPERM）
fn judge(probe: Option<i32>, members: impl FnOnce() -> Vec<Member>) -> bool {
    match probe {
        None => true,
        Some(libc::EPERM) => members().iter().any(|m| *m != Member::Gone),
        Some(_) => false,
    }
}

/// 组里的全部 pid（含僵尸：实测 `proc_listpgrppids` 会列出父进程还没回收的那个）
#[cfg(target_os = "macos")]
fn group_members(pgid: i32) -> Vec<i32> {
    let mut buf = vec![0 as libc::pid_t; 256];
    loop {
        let bytes = (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int;
        // SAFETY: 缓冲区大小如实告诉内核；返回值是**个数**不是字节数（实测）
        let n = unsafe { libc::proc_listpgrppids(pgid, buf.as_mut_ptr() as *mut libc::c_void, bytes) };
        if n <= 0 {
            return Vec::new();
        }
        // 正好填满：可能还有没装下的，放大再要一次
        if (n as usize) < buf.len() {
            buf.truncate(n as usize);
            return buf;
        }
        buf.resize(buf.len() * 4, 0);
    }
}

#[cfg(not(target_os = "macos"))]
fn group_members(_pgid: i32) -> Vec<i32> {
    Vec::new()
}

/// 一个 pid 现在是什么。
///
/// **查不到不等于不在**（2026-10-12 实测）：对 root 的进程（launchd、logd）`proc_pidinfo` 自己也回 EPERM，而对僵尸回的是 ESRCH。
/// 写成「查得到、又不是僵尸才算活着」（#65 原来记的修法就是这么转述的）的话，root 那个查不到 → 当成没了 —— 正好留着 #65 要修的那个误判
#[cfg(target_os = "macos")]
fn member(pid: i32) -> Member {
    let mut info: libc::proc_bsdinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdinfo>() as libc::c_int;
    // SAFETY: 缓冲区是一个完整的 proc_bsdinfo，大小如实告诉内核
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    if n == size {
        return if info.pbi_status == libc::SZOMB { Member::Gone } else { Member::Live };
    }
    match io::Error::last_os_error().raw_os_error() {
        Some(libc::EPERM) => Member::NotOurs,
        // ESRCH：僵尸（实测）或者已经没了
        _ => Member::Gone,
    }
}

#[cfg(not(target_os = "macos"))]
fn member(_pid: i32) -> Member {
    Member::Gone
}

/// 组里停不掉的一个进程（#68）：还活着、但不归我们管 —— 别的用户起的，典型是免密 `sudo` 的 root。
/// `killpg` 和对它单独发信号都是 EPERM，宽限期过了它照样在；界面要说清是谁、给一条能抄的 `sudo kill`
#[derive(Clone, Debug, PartialEq)]
pub struct Foreign {
    pub pid: i32,
    /// 真实用户名（`root`）；查不到名字就是 `uid 501` 这样
    pub user: String,
    /// 可执行文件的路径；拿不到就是 16 个字以内的进程名
    pub command: String,
}

/// 组里停不掉的那些（[`Foreign`]）。只看活着的：僵尸、已经没了的不算
pub fn foreign_members(pgid: i32) -> Vec<Foreign> {
    group_members(pgid).into_iter().filter(|&p| member(p) == Member::NotOurs).filter_map(describe).collect()
}

/// 一个别的用户的进程是谁、谁的。
///
/// `proc_pidinfo(PROC_PIDTBSDINFO)`（[`member`] 用的那个）对别人的进程回 EPERM，`proc_name` 也是；
/// **短的那份（`PROC_PIDT_SHORTBSDINFO`）和 `proc_pidpath` 拿得到**（2026-10-22 对 launchd 实测，`ps` 能列出 root 进程靠的也是不要权限的那几条）
#[cfg(target_os = "macos")]
fn describe(pid: i32) -> Option<Foreign> {
    let mut info: libc::proc_bsdshortinfo = unsafe { std::mem::zeroed() };
    let size = std::mem::size_of::<libc::proc_bsdshortinfo>() as libc::c_int;
    // SAFETY: 缓冲区是一个完整的 proc_bsdshortinfo，大小如实告诉内核
    let n = unsafe { libc::proc_pidinfo(pid, libc::PROC_PIDT_SHORTBSDINFO, 0, &mut info as *mut _ as *mut libc::c_void, size) };
    if n != size {
        return None;
    }
    let mut path = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: 缓冲区大小如实告诉内核；返回的是写进去的字节数
    let len = unsafe { libc::proc_pidpath(pid, path.as_mut_ptr() as *mut libc::c_void, path.len() as u32) };
    let command = if len > 0 {
        String::from_utf8_lossy(&path[..len as usize]).into_owned()
    } else {
        let comm: Vec<u8> = info.pbsi_comm.iter().take_while(|&&c| c != 0).map(|&c| c as u8).collect();
        String::from_utf8_lossy(&comm).into_owned()
    };
    Some(Foreign { pid, user: user_name(info.pbsi_ruid), command })
}

#[cfg(not(target_os = "macos"))]
fn describe(_pid: i32) -> Option<Foreign> {
    None
}

/// uid → 用户名（`getpwuid_r`，线程安全那个）；查不到就是 `uid N`
fn user_name(uid: libc::uid_t) -> String {
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut buf = vec![0 as libc::c_char; 1024];
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: 结构体、缓冲区、输出指针都是自己的，缓冲区大小如实给
    let rc = unsafe { libc::getpwuid_r(uid, &mut pw, buf.as_mut_ptr(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() || pw.pw_name.is_null() {
        return format!("uid {uid}");
    }
    // SAFETY: getpwuid_r 成功时 pw_name 指向 buf 里一个以 NUL 结尾的串
    unsafe { std::ffi::CStr::from_ptr(pw.pw_name) }.to_string_lossy().into_owned()
}

fn signal_group(pgid: i32, sig: libc::c_int) {
    // SAFETY: 只对我们自己建的那个组发；组已经没了就是 ESRCH，无害
    unsafe {
        libc::killpg(pgid, sig);
    }
}

/// 先软后硬地停一整组：SIGINT，`grace` 内全退了就完，否则 SIGKILL。**会阻塞到结束**（最多 `grace` 多一点），
/// 调用方放线程里跑。崩溃后收尸（只有组号、没有 [`Task`]）也走它
pub fn stop_group(pgid: i32, grace: Duration) {
    stop_groups(&[pgid], grace);
}

/// 同时停几组：每组先都发 SIGINT，再**一起**等一个宽限期，到点还有活的统一 SIGKILL。
/// 挨个 [`stop_group`] 的话，N 组都不理软停就要等 N 个宽限期 —— 上次没停干净的卡片上点「结束它们」，三个就是 15 秒（#56 第 5 条）
pub fn stop_groups(pgids: &[i32], grace: Duration) {
    for &g in pgids {
        signal_group(g, libc::SIGINT);
    }
    let t = Instant::now();
    while t.elapsed() < grace {
        if !pgids.iter().any(|&g| group_alive(g)) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    for &g in pgids {
        signal_group(g, libc::SIGKILL);
    }
}

/// FNV-1a 64 位。要的是「同样的输入每次都得到同样的值」，`DefaultHasher` 不保证跨版本稳定
fn fnv(s: &str) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

/// 任务的输出放哪：`<base>/<项目根的指纹>/<任务名>-<名字的指纹>.log`（TASKS.md Q7：不往项目里写）。
///
/// 名字里的 `/`（`web/dev`）和别的怪字符换成 `_`：它是文件名的一段，不能变成子目录。只这么换会撞 ——
/// `web/dev`（package.json 子目录）和 `web_dev`、`a b` 和 `a_b` 落到同一个文件，APFS 默认还不分大小写（`Dev` 和 `dev`）。
/// 两个同时跑时，后起的把先起的正在写的文件挪成 `.1`，先起那格看到的是后起那个的输出（#56 第 2 条）。
/// 所以后面再带一段**原名**的指纹：换掉的字符、大小写都算进去，前半截照旧给人认
pub fn log_file(base: &Path, root: &str, name: &str) -> PathBuf {
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    base.join(format!("{:016x}", fnv(root))).join(format!("{safe}-{:08x}.log", fnv(name) as u32))
}

impl Task {
    /// 起。日志文件先开好再起进程：开不了（盘满、没权限）就直接报错，不留一个输出没处去的进程
    pub fn start(spec: &Spec) -> io::Result<Task> {
        let mut out = sink::Sink::open(&spec.log, spec.cap)?;
        let shell = if spec.shell.is_empty() {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into())
        } else {
            spec.shell.clone()
        };
        let (mut rd, wr) = io::pipe()?;
        let mut cmd = Command::new(&shell);
        cmd.arg("-ilc")
            .arg(&spec.command)
            .current_dir(&spec.cwd)
            // 要交互的命令请在终端里跑（TASKS.md 第 8 节）；接空的，读 stdin 的会立刻读到 EOF 而不是卡住
            .stdin(Stdio::null())
            .stdout(wr.try_clone()?)
            .stderr(wr)
            // 管道下 Python 块缓冲，输出攒到结束才出来（第 0 步实测）。放在用户的 env 前面：用户真想要缓冲可以写 0 盖掉
            .env("PYTHONUNBUFFERED", "1")
            .envs(spec.env.iter().map(|(k, v)| (k.as_str(), v.as_str())))
            .process_group(0);
        #[allow(clippy::disallowed_methods)] // 任务不走 procutil：长期跑、两路合进一根管道流式落盘（rust.md）
        let mut child = cmd.spawn()?;
        // `cmd` 手里攥着管道写端的两份拷贝。读线程要等**所有**写端都关了才读到 EOF，所以子进程起来之后我们这边一份都不该留。
        // 它到函数末尾本来也会被丢掉，这里显式放掉，免得以后有人把 `cmd` 挪进一个活得更久的地方、读线程从此等不到头
        drop(cmd);
        let pgid = child.id() as i32;
        let started_us = live::start_time_us(pgid);
        let shared = Arc::new(Shared { state: Mutex::new(State::Running), changed: Condvar::new(), stopped: AtomicBool::new(false) });

        std::thread::Builder::new().name(format!("task-out-{pgid}")).spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            let mut cleaner = Cleaner::new();
            let mut cleaned = Vec::with_capacity(buf.len());
            loop {
                match rd.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        cleaned.clear();
                        cleaner.feed(&buf[..n], &mut cleaned);
                        // 写不进去（盘满）也要接着读：停下来不读，管道一满任务就卡在写上
                        let _ = out.write(&cleaned);
                    }
                }
            }
            cleaned.clear();
            cleaner.finish(&mut cleaned);
            let _ = out.write(&cleaned);
        })?;

        let sh = shared.clone();
        std::thread::Builder::new().name(format!("task-wait-{pgid}")).spawn(move || {
            // 回收组长：不回收它就是僵尸，组号一直「还有人」
            let st = child.wait();
            // 组长退了、组里还有活的（命令里 `&` 出去的后台进程，`java -jar a.jar & sleep 1` 这种）：任务**还没完**。
            // 原来组长一退就记成「已结束」，界面上停止按钮跟着灰掉，而后台的 java 还占着端口、谁也停不了它；
            // 账本（live.json）也在这时划掉了，这时崩溃下次没有收尸卡片（#56 第 1 条）。
            // 轮询：组号没有「最后一个进程退出」的通知可等。一个系统调用、200ms 一次，只在这种少见的情形下才转起来
            while group_alive(pgid) {
                std::thread::sleep(Duration::from_millis(200));
            }
            let stopped = sh.stopped.load(Ordering::SeqCst);
            let state = match st {
                Ok(s) => State::Exited { code: s.code(), signal: s.signal(), stopped },
                Err(_) => State::Exited { code: None, signal: None, stopped },
            };
            *sh.state.lock().unwrap_or_else(|e| e.into_inner()) = state;
            sh.changed.notify_all();
        })?;

        Ok(Task { pgid, started_us, shared })
    }

    pub fn pgid(&self) -> i32 {
        self.pgid
    }

    /// 组长的启动时间（记进 live.json，见 [`live`]）
    pub fn started_us(&self) -> Option<u64> {
        self.started_us
    }

    pub fn state(&self) -> State {
        self.shared.state.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// 组里还有活的（组长退了、后台的孙子还在，也算）。
    ///
    /// **已经是 `Exited` 就不再问组号**（code review 2026-10-12）：`Exited` 的意思是等待线程亲眼看到整组没了，
    /// 之后这个号就不是我们的了 —— 运行窗的格子会一直留着，macOS 的 pid 到 99999 回绕，几天之后同一个号可能是别人的进程组长。
    /// 原来只看 `group_alive(pgid)`：关掉那一格（`task_close` → `retire`）或者 ⌘Q（`kill_all_runs`、`Drop`）会把 SIGINT / SIGKILL
    /// 发给那个不相干的进程组
    pub fn alive(&self) -> bool {
        !self.exited() && group_alive(self.pgid)
    }

    fn exited(&self) -> bool {
        matches!(*self.shared.state.lock().unwrap_or_else(|e| e.into_inner()), State::Exited { .. })
    }

    /// 记下「是我们停的」：整组没了时据此记成「已停止」而不是「失败」。**停一个有 `Task` 的任务，每条路都要先过这里** ——
    /// 重跑、关格子、端口卡片原来各自直接 `stop_group(pgid)`，绕开了它，于是 `mvn` 被停后那个 130 报成了「失败」、
    /// 还弹一句「退出了」（code review 2026-10-10）。[`stop_group`] 只留给没有 `Task` 的（崩溃后收尸）
    fn mark_stopping(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        let mut st = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        if *st == State::Running {
            *st = State::Stopping;
            self.shared.changed.notify_all();
        }
    }

    /// 软停：SIGINT 整组，`grace` 后还有活的就 SIGKILL。立刻返回，等待在后台线程里
    pub fn stop(&self, grace: Duration) {
        // 已经整组没了：号可能已经是别人的（`alive` 上面那段），一个信号都不发
        if self.exited() {
            return;
        }
        self.mark_stopping();
        let pgid = self.pgid;
        let _ = std::thread::Builder::new().name(format!("task-stop-{pgid}")).spawn(move || stop_group(pgid, grace));
    }

    /// 同 [`Task::stop`]，但在当前线程里等到停完（最多 `grace` 多一点）。**会阻塞**，调用方自己在后台线程里。
    /// 返回时状态已经是 `Exited`：组没了不等于等待线程已经记下了（它回收组长、再看一眼组，有几百毫秒的空档），
    /// 调用方紧接着读状态的话会读到「正在停」
    pub fn stop_wait(&self, grace: Duration) {
        if self.exited() {
            return;
        }
        self.mark_stopping();
        stop_group(self.pgid, grace);
        self.wait_exit(Duration::from_secs(2));
    }

    /// 组里停不掉的那些（#68）：宽限期过了还在「正在停」时界面来问。已经整组没了就是空的（号可能已经是别人的，`alive` 上面那段）
    pub fn foreign(&self) -> Vec<Foreign> {
        if self.exited() {
            return Vec::new();
        }
        foreign_members(self.pgid)
    }

    /// 强杀：SIGKILL 整组，不等（软停宽限期里再按一次「停止」走这里，同 IDEA 按钮变成「强制结束」）
    pub fn kill(&self) {
        if self.exited() {
            return;
        }
        self.shared.stopped.store(true, Ordering::SeqCst);
        signal_group(self.pgid, libc::SIGKILL);
    }

    /// 等整组都没了，不限时。返回最终状态（一定是 `Exited`）
    pub fn wait(&self) -> State {
        let g = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let g = self.shared.changed.wait_while(g, |s| !matches!(s, State::Exited { .. })).unwrap_or_else(|e| e.into_inner());
        g.clone()
    }

    /// 等整组都没了，最多 `timeout`。没了返回最终状态
    pub fn wait_exit(&self, timeout: Duration) -> Option<State> {
        let g = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let (g, _) = self
            .shared
            .changed
            .wait_timeout_while(g, timeout, |s| !matches!(s, State::Exited { .. }))
            .unwrap_or_else(|e| e.into_inner());
        matches!(*g, State::Exited { .. }).then(|| g.clone())
    }
}

/// 最后一道：`Task` 被丢掉时组里还有活的就 SIGKILL，**不等** —— `Drop` 跑在退出路径上，不能阻塞。
/// 正常退出应用时先 [`Task::stop`] 给它收尾的机会（应用层的事）；这里兜的是忘了停的那些，不留孤儿（UNINSTALL.md 的承诺）
impl Drop for Task {
    fn drop(&mut self) {
        // 同 `alive`：整组早就没了的，号可能已经是别人的，不发
        if self.alive() {
            signal_group(self.pgid, libc::SIGKILL);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 组号复用造不出来，就反过来造：一个**活着的、不相干的**进程组，再造一个拿着同一个号、但早已 `Exited` 的任务 ——
    /// 正是「格子留了几天、号被别人用了」的样子。对它 alive / kill / stop_wait / 丢掉，那个进程组都得毫发无损
    #[test]
    fn 已经结束的任务_号被别人复用了_一个信号都不发() {
        #[allow(clippy::disallowed_methods)] // 测试里起一个「别人的」进程组
        let mut other = Command::new("sleep").arg("30").process_group(0).spawn().unwrap();
        let pgid = other.id() as i32;
        let done = State::Exited { code: Some(0), signal: None, stopped: false };
        let t = Task {
            pgid,
            started_us: None,
            shared: Arc::new(Shared { state: Mutex::new(done), changed: Condvar::new(), stopped: AtomicBool::new(false) }),
        };
        assert!(!t.alive(), "整组早就没了的任务，不该按号去问 —— 那个号现在是别人的");
        t.kill();
        t.stop(Duration::from_millis(100));
        t.stop_wait(Duration::from_millis(100));
        drop(t);
        std::thread::sleep(Duration::from_millis(300));
        assert!(matches!(other.try_wait(), Ok(None)), "别人的进程组被我们的信号打死了");
        let _ = other.kill();
        let _ = other.wait();
    }

    /// #65：EPERM 时看组员。root 进程造不出来（免密 sudo 不能假设有），所以判断拆成 [`judge`]、注入组员
    #[test]
    fn 组里只剩别人的进程_还算活着_只剩僵尸才算没了() {
        use Member::*;
        assert!(judge(None, Vec::new), "killpg 成功 = 有活的");
        assert!(!judge(Some(libc::ESRCH), Vec::new), "ESRCH = 一个都没了");
        assert!(!judge(Some(libc::EPERM), || vec![Gone]), "只剩僵尸：没有活的（第 0 步实测的那种）");
        assert!(judge(Some(libc::EPERM), || vec![Gone, NotOurs]), "root 起的还在：不能说停干净了");
    }

    /// 上面那条注入的「NotOurs」得是真的：root 的进程（launchd 永远在、永远是 root），`proc_pidinfo` 回 EPERM 要认成「在，不归我们」，
    /// 不能当成没了；自己的僵尸要认成没了
    #[test]
    fn 组员认得出_自己的活的_自己的僵尸_root的() {
        assert_eq!(member(1), Member::NotOurs, "launchd 查不到是因为不归我们，不是因为它不在");
        assert_eq!(member(std::process::id() as i32), Member::Live);
        // 组长 sh 起一个马上退的 sleep 0，然后 exec 成 sleep 5 —— 新的 sleep 不回收孩子，那个 sleep 0 成僵尸、父进程还活着
        #[allow(clippy::disallowed_methods)] // 测试里造一个带僵尸的组
        let mut g = Command::new("/bin/sh").args(["-c", "sleep 0 & exec sleep 5"]).process_group(0).spawn().unwrap();
        let pgid = g.id() as i32;
        let t = Instant::now();
        let kinds = loop {
            let k: Vec<Member> = group_members(pgid).into_iter().map(member).collect();
            if k.len() == 2 || t.elapsed() > Duration::from_secs(2) {
                break k;
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let _ = g.kill();
        let _ = g.wait();
        assert!(kinds.contains(&Member::Live) && kinds.contains(&Member::Gone), "组长活着、孩子是僵尸：{kinds:?}");
    }

    /// 停不掉的那个要说得出是谁：root 的进程（launchd）`proc_pidinfo` 的长版本回 EPERM，短版本和路径拿得到（#68）
    #[cfg(target_os = "macos")]
    #[test]
    fn 别人的进程_说得出是谁_谁的() {
        assert_eq!(describe(1), Some(Foreign { pid: 1, user: "root".into(), command: "/sbin/launchd".into() }));
        assert_eq!(user_name(0), "root");
        assert_eq!(user_name(4_000_000_000), "uid 4000000000");
    }

    /// 组里只剩一个没被回收的僵尸：killpg 回 EPERM（第 0 步实测的形状），但不能因为 EPERM 就当它还活着 —— 否则等待线程永远转、任务永远停不完
    #[test]
    fn 组里只剩僵尸_走的是eperm那条路_判成没了() {
        #[allow(clippy::disallowed_methods)] // 测试里造一个只剩僵尸的组：起了不回收
        let mut z = Command::new("/usr/bin/true").process_group(0).spawn().unwrap();
        let pgid = z.id() as i32;
        std::thread::sleep(Duration::from_millis(200));
        // SAFETY: 信号 0 只问不发
        let probe = unsafe { libc::killpg(pgid, 0) };
        let errno = io::Error::last_os_error().raw_os_error();
        let alive = group_alive(pgid);
        let _ = z.wait();
        assert_eq!((probe, errno), (-1, Some(libc::EPERM)), "前提：只剩僵尸的组 killpg 回 EPERM");
        assert!(!alive, "只剩僵尸不算活着");
    }
}

