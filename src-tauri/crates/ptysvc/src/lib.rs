//! 真 zsh 终端。
//!
//! 用 `portable-pty` 而不是模拟 shell —— 模拟的那条路走不通：
//! 交互式程序（vim、less、gradle 的进度条）全靠 pty 的行为，缺一个就不对。
//!
//! 生命周期纪律（UNINSTALL.md 的承诺）：**主窗口退出时必须 kill 所有子进程**，
//! 否则会留下孤儿 zsh 常驻。`Session::drop` 负责这件事。

mod flow;
pub use flow::{Flow, GIVE_UP, HIGH_WATER};

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// spawn 的返回：会话本身（可共享）与它的输出流。
/// 输出流刻意不放进 Session —— 读是独占的长循环，跟写/resize 不该抢同一把锁。
pub type Spawned = (Arc<Mutex<Session>>, Box<dyn Read + Send>);

pub struct Session {
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Box<dyn Write + Send>,
}

impl Session {
    /// 在 `cwd` 起一个登录 shell。`shell` 为空时用 $SHELL，再兜底 /bin/zsh。
    pub fn spawn(cwd: &str, cols: u16, rows: u16) -> std::io::Result<Spawned> {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io)?;

        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
        // cwd 不存在就回落到 $HOME —— 总比把用户扔到 / 强
        let cwd = if std::path::Path::new(cwd).is_dir() {
            cwd.to_string()
        } else {
            std::env::var("HOME").unwrap_or_else(|_| "/".into())
        };
        let mut cmd = CommandBuilder::new(shell);
        // 登录 shell：读用户的 .zshrc，PATH 与别名才跟平时一致
        cmd.arg("-l");
        cmd.cwd(cwd);
        // 让 ncurses 类程序知道终端能力；xterm.js 对得上 xterm-256color
        cmd.env("TERM", "xterm-256color");

        let child = pair.slave.spawn_command(cmd).map_err(to_io)?;
        // slave 端必须尽早丢掉，否则 shell 退出后 reader 不会 EOF，读线程永远挂着
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().map_err(to_io)?;
        let writer = pair.master.take_writer().map_err(to_io)?;

        Ok((
            Arc::new(Mutex::new(Session {
                master: pair.master,
                child,
                writer,
            })),
            reader,
        ))
    }

    pub fn write_input(&mut self, data: &[u8]) -> std::io::Result<()> {
        self.writer.write_all(data)?;
        self.writer.flush()
    }

    pub fn resize(&self, cols: u16, rows: u16) -> std::io::Result<()> {
        self.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(to_io)
    }

    /// 杀掉 shell 并收尸。**保证有界返回** —— 它跑在 `Drop` 里，
    /// 而 `Drop` 又跑在持着 pty 表锁的地方，挂住就是整个终端功能全死。
    ///
    /// 两道措施，都是 issue #2 实测出来的：
    ///
    /// # 一、杀之前先接上排空线程
    ///
    /// 退出中的 shell 还会往 tty 写，而这时候读的那一方通常已经收摊了
    /// （前端关标签 → `on_data.send` 失败 → 读线程 break；测试里是
    /// `read_until` 返回后 rx 被丢）。master 缓冲区一满，shell 就卡在写上
    /// 收不了尾 —— `ps` 看是 `?Es`，而 `wait()` 永远等不到。
    ///
    /// A/B 实测：不排空 15 次尝试挂 6 次，排空 10 次尝试挂 0 次。
    ///
    /// 不 `join` 这个线程：万一有孙子进程攥着 slave 不放，EOF 就不会来，
    /// join 本身就成了第二个挂点。它会在 master EOF 或进程退出时自己走。
    ///
    /// # 二、收尸有界
    ///
    /// 即使排空线程因为什么原因没起来（`try_clone_reader` 失败），
    /// 也不能退回阻塞的 `wait()`。宁可留一个僵尸到进程退出，
    /// 也不能让界面永久卡死 —— 前者用户察觉不到，后者要重启应用。
    pub fn kill(&mut self) {
        let _drain = self.master.try_clone_reader().ok().map(|mut r| {
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while matches!(r.read(&mut buf), Ok(n) if n > 0) {}
            })
        });

        let _ = self.child.kill();

        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match self.child.try_wait() {
                // 收到了，或者已经被别处收过 —— 两种都不用再等
                Ok(Some(_)) | Err(_) => break,
                Ok(None) if Instant::now() >= deadline => {
                    /*
                     * 到点还没收到尸：留一个僵尸到进程退出，界面继续走。
                     * 这个取舍是有意的（界面永久卡死比一个僵尸糟得多），
                     * 但**不能是静默的** —— 兜底一旦真的被用到，说明排空线程
                     * 那条路又漏了，而那正是 issue #2 的形状。
                     *
                     * 无条件打印，不挂在 diag 开关后面：它每次运行最多出现
                     * 几次，而漏掉它的代价是下次又要从零查一遍。
                     */
                    eprintln!(
                        "[ptysvc] pid {:?} 在 5s 内没收到尸，留一个僵尸到进程退出（issue #2）",
                        self.child.process_id()
                    );
                    break;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            }
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // 窗口关了、标签关了，shell 都必须跟着走 —— 不留孤儿进程
        self.kill();
    }
}

fn to_io(e: impl std::fmt::Display) -> std::io::Error {
    std::io::Error::other(e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ─────────────────── 挂起现场探针 ───────────────────
    //
    // 这不是修复，是**取证**。issue #2 查到今天卡在同一个地方：
    // 挂住的那次是唯一有诊断价值的一次，而它什么都留不下。
    //
    // 原因在 `with_deadline` 的写法 —— 超时之后那个工作线程被丢掉了
    // （见那边「故意不 join」的注释），它攒着的 `eprintln!` 跟着一起没。
    // 所以探针必须**边跑边往文件里追加并 flush**，而不是攒到最后一起打印：
    // 进程被 kill、线程被弃、测试 panic，已经落盘的行都还在。
    //
    // 默认关闭，用环境变量开：
    //
    //     PTYSVC_PROBE=/tmp/pty.log cargo test -p ptysvc --lib
    //
    // 关着的时候只多一次 OnceLock 读 + 一个 Option 判空，跑不满一微秒。

    static PROBE: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
    static T0: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

    fn probe_path() -> Option<&'static std::path::Path> {
        PROBE
            .get_or_init(|| std::env::var_os("PTYSVC_PROBE").map(Into::into))
            .as_deref()
    }

    /// 记一个阶段。**每行都 flush**，理由见本节开头。
    ///
    /// 带上 ThreadId 是为了把「被弃掉的那个线程」和后续重试区分开 ——
    /// 挂住时同一个进程里会有好几个线程各自走到不同阶段，
    /// 不标线程根本读不出谁卡在哪。
    fn probe(stage: &str) {
        let Some(path) = probe_path() else { return };
        let ms = T0.get_or_init(Instant::now).elapsed().as_secs_f64() * 1000.0;
        let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        else {
            return;
        };
        // O_APPEND 下这一行是原子的，多线程不会互相插字
        let _ = writeln!(f, "{ms:>10.1}ms  {:?}  {stage}", std::thread::current().id());
        let _ = f.flush();
    }

    /// 给测试体加一道硬性期限 + 有限重试。
    ///
    /// # 为什么需要它
    ///
    /// 起 pty、跑**交互式**登录 shell、读它的输出 —— 这条链会间歇性卡住。
    /// 挂住时默认表现是测试永远不返回：本机敲 Ctrl-C 就完事，CI 上则是
    /// 安静地吃光整个 job 的超时额度，日志停在 `test tests::xxx ...` 那一行，
    /// 一点线索都没有。
    ///
    /// # 挂多频繁（2026-08-31 实测 12 轮，macOS 26.6.2 / zsh 5.9）
    ///
    /// 挂的**永远**是 `工作目录生效`，另外两条从没挂过。按耗时能把
    /// 每次尝试拆开数（25s 是一次 deadline，成功的那次几乎不耗时）：
    ///
    /// | 耗时 | 轮数 | 含义 |
    /// |---|---|---|
    /// | 0.5s | 7 | 第一次尝试就成功 |
    /// | 25.4s | 2 | **第一次挂满 25s，第二次成功** |
    /// | 75.2s | 3 | 三次全挂 → FAILED |
    ///
    /// **每轮失败率 3/12 = 25%；每次尝试的挂率 11/20 = 55%。**
    /// 引用这两个数时要说清是哪个口径 —— 它们差一倍多。
    ///
    /// 那两轮被重试救回来的很有诊断价值：同一个测试二进制里，前一次挂满
    /// deadline，紧接着下一次 0.4s 就过 —— 说明这是**竞态**，
    /// 不是「某个前置条件坏了就一直坏」。
    ///
    /// # 挂在哪：`child.wait()`，探针抓到的
    ///
    /// 2026-08-31 用下面的 `probe` 抓到了现场，三次尝试完全一致：
    ///
    /// ```text
    ///     0.4ms  spawn 前
    ///     3.9ms  spawn 回来了            ← spawn 没问题，每次都 4ms
    ///     4.1ms  write_input 回来了
    ///   196.2ms  read_until 回来了       ← 读也没问题
    ///   196.3ms  断言过了，开始 drop
    ///   411.7ms  child.kill() 回来了     ← kill 回来了
    ///            （child.wait() 一次都没回来）
    /// 25001.3ms  第 1/3 次尝试超时
    /// ```
    ///
    /// **挂点是 `Drop` → `kill()` → `child.wait()`。**
    ///
    /// # 为什么 wait 收不到尸
    ///
    /// `kill()` 返回之后 `ps` 显示子进程是 `?Es`、命令名带括号 ——
    /// 信号生效了，进程在退出，但**收不了尾**。因果链：
    ///
    /// 1. `read_until` 返回后，它的读线程在下一次 `read` 醒来时因为
    ///    `tx.send` 失败而退出 —— **没人再排空 pty master**
    /// 2. `child.kill()` 先发 SIGHUP，5×50ms 宽限期轮询 `try_wait()`
    ///    （正好是实测的 215ms），都没死才补 SIGKILL
    /// 3. 退出中的 shell 继续往 tty 写，master 缓冲区满且无人排空，
    ///    它卡在写上收不了尾
    /// 4. `child.wait()` 于是永远等不到
    ///
    /// A/B 对照坐实了第 1 步就是关键：kill 期间另起一个线程持续排空 master，
    /// **15 次尝试挂 6 次 → 10 次尝试挂 0 次**（连一次 25s 的重试都没有）。
    ///
    /// # 两条被推翻的旧结论
    ///
    /// **一、「`kill()` 发的是 SIGKILL，shell 即使 trap 掉 HUP 也照样死」——错。**
    /// portable-pty 0.9.0 `impl ChildKiller for std::process::Child`：
    ///
    /// ```text
    /// // On unix, we send the SIGHUP signal instead of trying to kill
    /// libc::kill(self.id() as i32, libc::SIGHUP)
    /// ```
    ///
    /// 先 SIGHUP，宽限期之后才补 SIGKILL。原先那条「`child.wait()` 阻塞 → 否」
    /// 的排除结论就建立在这个错事实上 —— 而挂点恰恰就是 `child.wait()`。
    ///
    /// **二、「纯 Python `pty.fork()` 复现也会挂 → 根因不在本仓库」——已作废。**
    /// 2026-08-28 复测 40/40 全过。现在也说得通了：那份复现多半一直在读 master。
    ///
    /// 还活着的一条：挂住不留孤儿
    /// （11 次挂住的尝试新增孤儿 0 个 —— 进程退出时 master 关闭，shell 才走得掉）。
    ///
    /// **作废的第三条：「登录 shell 不慢（`zsh -l -c pwd` 0.084s）」。**
    /// 2026-09-11 复量发现那个数只是**快的那一峰**：同一条命令冷的时候
    /// 要 6.2–60.4 秒（CPU 6%，全在等）。这台机器上它是双峰的，
    /// 而当初只量到了一边 —— 见 issue #30。
    /// 教训：**一个只量了一次就写下来的「不慢」，挡不住任何东西。**
    ///
    /// # 已经修了（2026-08-31 同一天）
    ///
    /// `Session::kill()` 里杀之前先接排空线程 + 收尸有界；
    /// `state.rs` 的 `kill_pty` / `kill_all_ptys` 改成先摘出来再在锁外析构
    /// （原来整个 `kill()` 跑在全局 pty 表锁里，挂住就是所有终端一起死）。
    ///
    /// 回归测试是下面的 `关掉不排空的终端不能把kill卡死`：
    /// 原始 `kill()` 上 10/10 红，修复后 0/10 红。
    ///
    /// # 所以这里为什么还留着
    ///
    /// 只留**硬期限**，重试已经去掉（`TRIES` 3 → 1，见下）。期限的作用变了：
    /// 不再是磨平已知的挂起，而是**万一回归，让它在 25s 内失败**，
    /// 而不是安静地吃光 CI 整个 job 的额度、日志停在
    /// `test tests::xxx ...` 那一行什么都没有。
    ///
    /// 老教训仍然有效，而且这轮又验证了一次：不提交猜出来的修复。
    /// 更早试过一版（`wait()` 改非阻塞 + 后台收尸），改完看着好了，
    /// 但把旧写法放回去也一样不挂 —— 配套的回归测试在新旧两版下都通过，
    /// 等于没测。这次是先让测试在旧代码上 10/10 红，才动的手。
    /*
     * **参数是 `FnOnce`，不是 `Fn + Clone`。**
     *
     * 原来要 `Clone` 是为了那个重试循环，而重试早就改成 1 次了
     * （见上面那段：重试只会盖住回归）—— 于是 `Clone` 成了一条纯粹的
     * 限制：调用方没法把 `Session` 和 `Output` **移**进闭包里。
     *
     * 而那正是 issue #30 要的东西：把「等 shell 起来」挪到期限**外面**。
     * 登录 shell 在这台机器上实测是双峰的（0.08s / 60s），把它算进
     * 「有没有挂住」的预算里，结果是全量测试每隔一轮就集体误报一次。
     */
    fn with_deadline<T: Send + 'static>(secs: u64, body: impl FnOnce() -> T + Send + 'static) -> T {
        use std::sync::mpsc::{self, RecvTimeoutError};
        // 曾经有个 TRIES=3 的重试循环。根因（issue #2）修掉之后改回 1 ——
        // **重试只会盖住回归**：它当初把「每次尝试 55% 会挂」磨成了
        // 「每轮 25% 的可见失败率」，两个数还各自被记进了两处文档，
        // 看着像互相矛盾。1 次就没有循环可言了，这里直接跑一遍。
        //
        // 硬期限留着，作用变了：不再是磨平已知的挂起，而是**万一回归，
        // 让它在 25s 内失败**，而不是安静地吃光 CI 整个 job 的额度、
        // 日志停在 `test tests::xxx ...` 那一行什么都没有。
        let (tx, rx) = mpsc::channel();
        probe("── 计时开始");
        // 故意不 join：卡住的线程 join 不回来，join 本身就成了第二个挂点。
        // 它会随进程退出被回收。
        std::thread::spawn(move || {
            let _ = tx.send(body());
        });
        match rx.recv_timeout(Duration::from_secs(secs)) {
            Ok(v) => {
                probe("── 走完了");
                v
            }
            // **断连不是超时。** 断连 = 工作线程自己 panic 了（它的断言信息
            // 已经打出来了），这时候再喊一句「多半是回归」只会盖住真正的原因。
            // 原来两种都走同一条 panic，查错时被误导过
            Err(RecvTimeoutError::Disconnected) => {
                panic!("pty 测试体自己失败了 —— 往上看它的断言信息")
            }
            Err(RecvTimeoutError::Timeout) => {
                // 这一行是主线程写的，所以一定落得下来。上面那个工作线程
                // 此刻还卡在某处，它最后写下的那个阶段就是挂点
                probe(&format!("── 超时 {secs}s，弃掉线程"));
                panic!(
                    "pty 超过 {secs}s 没返回。\n\
                     **等 shell 起来那段不在这个预算里**（issue #30 之后挪出去了），\n\
                     所以这次是真的卡在了命令往返或者收尸上 —— 当回归看\n\
                     （issue #2 那个挂起是 kill 时排空 master 修掉的）。\n\
                     用探针看它卡在哪个阶段 ——\n\
                     \x20  PTYSVC_PROBE=/tmp/pty.log cargo test -p ptysvc --lib\n\
                     文件最后一行就是它走到的最后一步。"
                )
            }
        }
    }

    /*
     * 等 shell 吐出第一个字的预算。
     *
     * **90 秒不是「慢」的余量，是环境双峰的余量。** 这台机器上
     * `zsh -l -c true` 稳态 0.08 秒，而冷的那一下实测到过 **60.4 秒**
     * （CPU 只占 6%，全在等，像个网络超时）。40 次连起都复现不了，
     * 所以它不是「机器忙」，放宽也不是妥协 —— 是把一个**不属于被测代码**
     * 的变量移出判据。
     *
     * 快的时候它一分钟都不花：`wait_prompt` 是轮询，第一个字一到就往下走。
     */
    const PROMPT_BUDGET: u64 = 90;

    /// pty 的输出累加器。**可以反复等**，这是它和原来那个一次性
    /// `read_until` 的唯一区别，而那个区别是必须的（见 [`Output::wait_for`]）。
    ///
    /// 必须把 read 放进独立线程 + channel 超时：`Read::read` 是阻塞的，
    /// 在主线程里循环判 deadline 根本判不到 —— 没数据时它就卡死在那儿了。
    struct Output {
        rx: std::sync::mpsc::Receiver<String>,
        acc: String,
    }

    impl Output {
        fn new(mut reader: Box<dyn Read + Send>) -> Self {
            use std::sync::mpsc;
            let (tx, rx) = mpsc::channel::<String>();
            std::thread::spawn(move || {
                probe("    read 线程起来了");
                let mut buf = [0u8; 4096];
                let mut first = true;
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => {
                            probe("    read 到 EOF/错误，退出");
                            break;
                        }
                        Ok(n) => {
                            if first {
                                probe("    read 拿到第一批数据");
                                first = false;
                            }
                            if tx
                                .send(String::from_utf8_lossy(&buf[..n]).into_owned())
                                .is_err()
                            {
                                break;
                            }
                        }
                    }
                }
            });
            Self { rx, acc: String::new() }
        }

        /// 等到累计输出里出现 `needle`，或者超时。已经收到的内容会留着。
        fn wait_for(&mut self, needle: &str, secs: u64) -> bool {
            use std::sync::mpsc::RecvTimeoutError;
            if self.acc.contains(needle) {
                return true;
            }
            let deadline = Instant::now() + Duration::from_secs(secs);
            while Instant::now() < deadline {
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(chunk) => {
                        self.acc.push_str(&chunk);
                        if self.acc.contains(needle) {
                            return true;
                        }
                    }
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            false
        }

        /// 等到 shell 吐出**任何**东西（也就是提示符）为止。
        ///
        /// 这一步是 [issue #16] 的解药。原来的测试是 spawn 完**立刻**往 pty 里
        /// 写命令，而登录 shell 那时还在 source rc 文件 —— tty 的行规程会把
        /// 敲进去的字回显出来（所以输出里看得到 `pwd\r\n`），但 rc 里只要有
        /// 一处清输入队列的动作（instant prompt、`zle` 复位、`stty`），
        /// **那条命令就永远不会被执行**，而不是「晚一点执行」。
        ///
        /// 表现正是当初抓到的那个形状：只读到回显，10s 内等不到结果。
        /// 机器一忙、rc 一慢，撞上的概率就高 —— 所以它是间歇的。
        ///
        /// 真人不会这么用：人是看见提示符才敲的。测试也照做。
        ///
        /// [issue #16]: https://github.com/Spc-jgs/lite-ide-mac/issues/16
        fn wait_prompt(&mut self, secs: u64) -> bool {
            use std::sync::mpsc::RecvTimeoutError;
            let deadline = Instant::now() + Duration::from_secs(secs);
            while self.acc.trim().is_empty() && Instant::now() < deadline {
                match self.rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(chunk) => self.acc.push_str(&chunk),
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                }
            }
            !self.acc.trim().is_empty()
        }

        fn text(&self) -> &str {
            &self.acc
        }
    }

    /// 写一条命令并等它的输出，**写不中就再写一遍**。
    ///
    /// 等提示符已经把绝大多数情况挡掉了，这一层是兜底：有的 rc 会先画一个
    /// 提示符、然后接着跑（p10k 的 instant prompt 就是这样），那一段里打进去的
    /// 字照样可能被清掉。重写一遍是无害的 —— 多跑一次 `pwd` 而已。
    ///
    /// **它不会把「cwd 错了」磨绿**：断言的是输出里有没有那个路径，
    /// cwd 真错的话写多少遍都等不到，最后照样红（只是慢 20 秒）。
    fn send_until(sess: &Arc<Mutex<Session>>, cmd: &[u8], needle: &str, out: &mut Output) -> bool {
        for _ in 0..20 {
            if sess.lock().unwrap().write_input(cmd).is_err() {
                return false;
            }
            if out.wait_for(needle, 1) {
                return true;
            }
        }
        false
    }

    #[test]
    fn 能起_shell_并执行命令() {
        // 起 shell 和等提示符**在期限外面**（issue #30）：那段耗时属于
        // 用户的 .zshrc，不属于被测代码
        let (sess, reader) = Session::spawn("/tmp", 80, 24).expect("起不来");
        let mut out = Output::new(reader);
        assert!(out.wait_prompt(PROMPT_BUDGET), "{PROMPT_BUDGET}s 内 shell 一个字都没吐出来");
        with_deadline(25, move || {
            assert!(
                send_until(&sess, b"echo LITE_IDE_PTY_OK\n", "LITE_IDE_PTY_OK", &mut out),
                "没读到回显，实际输出：{:?}",
                out.text()
            );
        });
    }

    #[test]
    fn 工作目录生效() {
        probe("  spawn 前");
        let (sess, reader) = Session::spawn("/usr", 80, 24).expect("起不来");
        probe("  spawn 回来了");
        let mut out = Output::new(reader);
        // 先等提示符再敲命令 —— issue #16，理由见 Output::wait_prompt。
        // 这一段在期限外面，理由见 PROMPT_BUDGET（issue #30）
        assert!(out.wait_prompt(PROMPT_BUDGET), "{PROMPT_BUDGET}s 内 shell 一个字都没吐出来");
        probe("  等到提示符了");
        with_deadline(25, move || {
            let ok = send_until(&sess, b"pwd\n", "/usr", &mut out);
            probe("  send_until 回来了");
            assert!(ok, "cwd 没生效，实际输出：{:?}", out.text());
            probe("  断言过了，开始 drop");
            // drop 写成显式的，否则它发生在闭包末尾，量不到边界。
            // 时机和原来的隐式 drop 完全一样 —— 后面没人再用 sess
            drop(sess);
            probe("  drop 回来了 ✓ 整条链走完");
        });
    }

    /// issue #2 的回归测试：**没人排空 master 时，kill 不能被卡死**。
    ///
    /// 复现的是生产里最常见的那条路 —— 前端关掉终端标签 → 读线程的
    /// `on_data.send` 失败 → 读线程退出 → 没人排空 master → 紧接着 `pty_kill`。
    ///
    /// 这里用 `yes` 灌满 master 缓冲区来加压：真实场景下 shell 退出时
    /// 自己吐的那点输出就够触发，但只有 40% 的挂率，做不成可靠的红。
    ///
    /// 注意**必须自己带超时**：挂住时默认表现是永远不返回，
    /// 一条挂住的测试会安静地吃光 CI 整个 job 的额度。这里 8s 到点就判失败。
    #[test]
    fn 关掉不排空的终端不能把kill卡死() {
        let (sess, reader) = Session::spawn("/tmp", 80, 24).expect("起不来");

        // 不靠提示符长什么样 —— 用户的 zsh 主题里 $ / % / ❯ 都可能
        let mut out = Output::new(reader);
        // 期限外（issue #30）—— 这条测试本来就自己带超时，不走 with_deadline
        assert!(out.wait_prompt(PROMPT_BUDGET), "{PROMPT_BUDGET}s 内 shell 一个字都没吐出来");
        assert!(
            send_until(&sess, b"echo READY\n", "READY", &mut out),
            "shell 没起来，实际输出：{:?}",
            out.text()
        );
        // **这一句是这条测试的一半**：丢掉 rx，读线程下一次 read 醒来就 break 退出，
        // 从这一刻起没人再排空 master —— 正是要复现的状态。
        // 原来它是 `read_until` 返回时隐式发生的，看不出来是有意的
        drop(out);

        // 灌满 master 缓冲区。**有界**，不能用 `yes` ——
        // 无限流会让排空线程全速空转烧掉一个核，把并行跑的兄弟测试拖到超时
        // （踩过：`工作目录生效` 因此在 10s 读超时上红了 8/15 次）。
        // pty 缓冲区只有几十 KB，seq 到 10 万行（约 600KB）绰绰有余，而且它会自己结束。
        sess.lock().unwrap().write_input(b"seq 1 100000\n").unwrap();
        std::thread::sleep(Duration::from_millis(400));

        // 卡的阈值是 2s，不是「别永久挂住」那种宽松的上限。理由：
        // `kill()` 里有两道独立的措施（排空线程 + 有界收尸），任一条都能
        // 让这个测试不挂 —— 只断言「最终返回了」的话，把排空线程删掉它照样绿，
        // 而那时 kill 要等满 5s 的兜底 deadline 才返回。
        //
        // 实测：有排空 55ms，去掉排空 5214ms。2s 卡在中间，
        // 离正常值有 36 倍余量，离退化值有 2.6 倍余量。
        let t0 = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            drop(sess); // → Session::drop → kill()
            let _ = tx.send(());
        });
        let done = rx.recv_timeout(Duration::from_secs(8)).is_ok();
        let ms = t0.elapsed().as_millis();
        assert!(
            done,
            "drop 超过 8s 没回来 —— 收尸又被没排空的 pty 卡死了（issue #2）"
        );
        assert!(
            ms < 2000,
            "drop 花了 {ms}ms。没挂死，但也远超正常的 ~55ms —— \
             多半是 kill() 里的排空线程没了，只剩有界收尸在硬等 deadline（issue #2）"
        );
    }

    #[test]
    fn drop_之后子进程必须已退出() {
        with_deadline(25, || {
        let (sess, _reader) = Session::spawn("/tmp", 80, 24).expect("起不来");
        let pid = {
            let s = sess.lock().unwrap();
            s.child.process_id()
        };
        drop(sess);
        std::thread::sleep(Duration::from_millis(300));
        if let Some(pid) = pid {
            // kill -0 探测进程是否还在
            let alive = libc_kill_zero(pid as i32);
            assert!(!alive, "PID {pid} 还活着，Drop 没把 shell 带走");
        }
        });
    }

    /// 用 `kill(pid, 0)` 探活，不引第三方 crate
    fn libc_kill_zero(pid: i32) -> bool {
        unsafe extern "C" {
            fn kill(pid: i32, sig: i32) -> i32;
        }
        unsafe { kill(pid, 0) == 0 }
    }
}
