//! 真 zsh 终端。
//!
//! 用 `portable-pty` 而不是模拟 shell —— 模拟的那条路走不通：
//! 交互式程序（vim、less、gradle 的进度条）全靠 pty 的行为，缺一个就不对。
//!
//! 生命周期纪律（UNINSTALL.md 的承诺）：**主窗口退出时必须 kill 所有子进程**，
//! 否则会留下孤儿 zsh 常驻。`Session::drop` 负责这件事。

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

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

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
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
    use std::time::{Duration, Instant};

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
    /// 还活着的两条（都复量过）：登录 shell 不慢
    /// （`zsh -l -c pwd` 0.084s、`-i` 0.183s）；挂住不留孤儿
    /// （11 次挂住的尝试新增孤儿 0 个 —— 进程退出时 master 关闭，shell 才走得掉）。
    ///
    /// # 生产侧是同一个形状
    ///
    /// `commands.rs` 的 pty 读线程也是 `on_data.send(..).is_err() → break`，
    /// 而 `send` 失败正是前端关掉终端标签的时候，紧接着就调 `pty_kill`。
    /// 更糟的是 `state.rs` 的 `kill_pty` **持着整张 pty 表的锁**就地析构 ——
    /// `wait()` 一挂，那把锁再也不放，之后所有终端操作全部堵死。
    ///
    /// 修复是独立的一轮，要配一个在旧代码上会红的回归测试。详见 issue #2。
    ///
    /// # 这个重试机制还留着
    ///
    /// 在修复落地并验证之前，它仍然是唯一挡住 CI 被拖死的东西。
    /// 上一次的教训还有效：不提交猜出来的修复 —— 试过一版（`wait()` 改非阻塞
    /// + 后台收尸），改完看着好了，但把旧写法放回去也一样不挂，
    /// 配套的回归测试在新旧两版下都通过，等于没测。
    fn with_deadline<T: Send + 'static>(
        secs: u64,
        body: impl Fn() -> T + Send + Sync + Clone + 'static,
    ) -> T {
        use std::sync::mpsc;
        const TRIES: u32 = 3;
        for attempt in 1..=TRIES {
            let (tx, rx) = mpsc::channel();
            let b = body.clone();
            probe(&format!("── 第 {attempt}/{TRIES} 次尝试开始"));
            // 故意不 join：卡住的线程 join 不回来，join 本身就成了第二个挂点。
            // 它会随进程退出被回收。
            std::thread::spawn(move || {
                let _ = tx.send(b());
            });
            match rx.recv_timeout(Duration::from_secs(secs)) {
                Ok(v) => {
                    probe(&format!("── 第 {attempt}/{TRIES} 次尝试成功"));
                    return v;
                }
                Err(_) => {
                    // 这一行是主线程写的，所以一定落得下来。上面那个工作线程
                    // 此刻还卡在某处，它最后写下的那个阶段就是挂点
                    probe(&format!("── 第 {attempt}/{TRIES} 次尝试超时 {secs}s，弃掉线程重试"));
                    eprintln!("  pty 第 {attempt}/{TRIES} 次尝试超过 {secs}s 没返回，重试");
                }
            }
        }
        panic!(
            "pty 连续 {TRIES} 次都超过 {secs}s 没返回。\n\
             这是已知的间歇性挂起（见 with_deadline 的注释），但连挂三次说明\n\
             多半真出了问题 —— 先看 ps 里有没有堆积的 `zsh -l` 孤儿进程。"
        )
    }

    /// 读到包含 needle 为止，或超时。
    ///
    /// 必须把 read 放进独立线程 + channel 超时：`Read::read` 是阻塞的，
    /// 在主线程里循环判 deadline 根本判不到 —— 没数据时它就卡死在那儿了。
    fn read_until(mut reader: Box<dyn Read + Send>, needle: &str, secs: u64) -> String {
        use std::sync::mpsc::{self, RecvTimeoutError};
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

        let mut acc = String::new();
        let deadline = Instant::now() + Duration::from_secs(secs);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(chunk) => {
                    acc.push_str(&chunk);
                    if acc.contains(needle) {
                        break;
                    }
                }
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => break,
            }
        }
        acc
    }

    #[test]
    fn 能起_shell_并执行命令() {
        with_deadline(25, || {
            let (sess, reader) = Session::spawn("/tmp", 80, 24).expect("起不来");
            sess.lock()
                .unwrap()
                .write_input(b"echo LITE_IDE_PTY_OK\n")
                .unwrap();
            let out = read_until(reader, "LITE_IDE_PTY_OK", 10);
            assert!(
                out.contains("LITE_IDE_PTY_OK"),
                "没读到回显，实际输出：{out:?}"
            );
        });
    }

    #[test]
    fn 工作目录生效() {
        with_deadline(25, || {
            probe("  spawn 前");
            let (sess, reader) = Session::spawn("/usr", 80, 24).expect("起不来");
            probe("  spawn 回来了");
            sess.lock().unwrap().write_input(b"pwd\n").unwrap();
            probe("  write_input 回来了");
            let out = read_until(reader, "/usr", 10);
            probe("  read_until 回来了");
            assert!(out.contains("/usr"), "cwd 没生效，实际输出：{out:?}");
            probe("  断言过了，开始 drop");
            // drop 写成显式的，否则它发生在闭包末尾，量不到边界。
            // 时机和原来的隐式 drop 完全一样 —— 后面没人再用 sess
            drop(sess);
            probe("  drop 回来了 ✓ 整条链走完");
        });
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
