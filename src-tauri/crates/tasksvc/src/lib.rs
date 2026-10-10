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
    Running,
    /// 发过 SIGINT，在等它收尾（宽限期内再叫一次 `kill` 就立刻强杀）
    Stopping,
    /// 组长退了。`stopped` = 是我们停的：pnpm 被 SIGINT 后会报 `Command failed`、退出码非零，
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
    /// 叫过 stop / kill。组长退出时据此把状态记成「已停止」而不是「失败」
    stopped: AtomicBool,
}

pub struct Task {
    pgid: i32,
    started_us: Option<u64>,
    shared: Arc<Shared>,
}

/// 组里还有没有活着的进程。`kill(-pgid, 0)` 只问不发：0 = 有，ESRCH = 一个都没了。
///
/// 第 0 步撞见过：组里只剩**没被回收的僵尸**时 macOS 回 EPERM —— 这里不会出现，组长由 [`Task::start`] 起的线程立刻回收，
/// 孙进程变孤儿后由 launchd 回收。所以 EPERM 也当「没有活的」。
pub fn group_alive(pgid: i32) -> bool {
    // SAFETY: 信号 0 不发任何东西，只做存在性检查
    unsafe { libc::killpg(pgid, 0) == 0 }
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
    signal_group(pgid, libc::SIGINT);
    let t = Instant::now();
    while t.elapsed() < grace {
        if !group_alive(pgid) {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    signal_group(pgid, libc::SIGKILL);
}

/// 任务的输出放哪：`<base>/<项目根的指纹>/<任务名>.log`（TASKS.md Q7：不往项目里写）。
/// 指纹用 FNV-1a：要的是「同一个项目每次都落在同一个目录」，`DefaultHasher` 不保证跨版本稳定。
/// 名字里的 `/`（`web/dev`）和别的怪字符换成 `_`：它是文件名的一段，不能变成子目录
pub fn log_file(base: &Path, root: &str, name: &str) -> PathBuf {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in root.as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    let safe: String = name.chars().map(|c| if c.is_alphanumeric() || "-_.".contains(c) { c } else { '_' }).collect();
    base.join(format!("{h:016x}")).join(format!("{safe}.log"))
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

    /// 组里还有活的（组长退了、后台的孙子还在，也算）
    pub fn alive(&self) -> bool {
        group_alive(self.pgid)
    }

    /// 记下「是我们停的」：组长退出时据此记成「已停止」而不是「失败」。**停一个有 `Task` 的任务，每条路都要先过这里** ——
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
        self.mark_stopping();
        let pgid = self.pgid;
        let _ = std::thread::Builder::new().name(format!("task-stop-{pgid}")).spawn(move || stop_group(pgid, grace));
    }

    /// 同 [`Task::stop`]，但在当前线程里等到停完（最多 `grace` 多一点）。**会阻塞**，调用方自己在后台线程里
    pub fn stop_wait(&self, grace: Duration) {
        self.mark_stopping();
        stop_group(self.pgid, grace);
    }

    /// 强杀：SIGKILL 整组，不等（软停宽限期里再按一次「停止」走这里，同 IDEA 按钮变成「强制结束」）
    pub fn kill(&self) {
        self.shared.stopped.store(true, Ordering::SeqCst);
        signal_group(self.pgid, libc::SIGKILL);
    }

    /// 等组长退出，不限时。返回最终状态（一定是 `Exited`）
    pub fn wait(&self) -> State {
        let g = self.shared.state.lock().unwrap_or_else(|e| e.into_inner());
        let g = self.shared.changed.wait_while(g, |s| !matches!(s, State::Exited { .. })).unwrap_or_else(|e| e.into_inner());
        g.clone()
    }

    /// 等组长退出，最多 `timeout`。退了返回最终状态
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
        if group_alive(self.pgid) {
            signal_group(self.pgid, libc::SIGKILL);
        }
    }
}
