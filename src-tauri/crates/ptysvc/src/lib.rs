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
    /// # 排查到哪一步
    ///
    /// **根因没定位到。** 下面这些是排除过的，但注意时间戳 ——
    /// 除最后一条外都是 2026-08-28 之前那轮排查的结论，没有重新验证过：
    ///
    /// - `child.wait()` 阻塞 —— portable-pty 的 `kill()` 发的是 SIGKILL，
    ///   shell 即使 `trap '' HUP TERM INT` 也照样死，`drop` 只要 55ms
    /// - 登录 shell 启动慢 —— 否。2026-08-31 复量仍然是
    ///   `zsh -l -c pwd` 0.084s、`zsh -l -i -c pwd` 0.183s（各 10 次取中位）
    /// - 并发 —— 串行跑（`--test-threads=1`）失败率反而更高（6 次挂 5 次）
    /// - 孤儿进程干扰 —— 2026-08-31 这 12 轮里 **11 次挂住的尝试，
    ///   新增孤儿 `zsh -l` 0 个**。所以 panic 信息里那条「先看 ps 有没有
    ///   堆积的孤儿」的提示，至少今天是条空线索
    ///
    /// ## 一条已经作废的旧结论，别再照着它推
    ///
    /// 这里原先写着「决定性的一条：用纯 Python 的 `pty.fork()` 写同样的复现
    /// 也会挂，所以根因不在本仓库」。**2026-08-28 复测没复现出来**
    /// —— `/usr`、`/tmp`、临时目录、`$HOME` 各 10 次，40/40 全过。
    /// 那条结论连同它推出的「问题在交互式 shell 挂在 pty 上这件事本身」
    /// 一并作废，上面那张排除表也是同一轮排查得出的，同样需要重新验证。
    ///
    /// # 所以怎么办
    ///
    /// 不提交猜出来的修复。试过一版（把 `wait()` 改成非阻塞 + 后台收尸），
    /// 改完看着好了，但把旧写法放回去也一样不挂 —— 那次只是没触发，
    /// 而配套写的回归测试在新旧两版下都通过，等于没测。
    ///
    /// 所以改成：**每次尝试有硬期限，最多试三次**，三次全挂才算失败。
    ///
    /// 下一步是把探针改成**边跑边往文件里追加并 flush**，而不是攒到最后
    /// 一起 `eprintln!` —— 超时之后那个线程被丢掉（见下面「故意不 join」
    /// 那条），它攒的输出跟着一起没了，每次挂住恰恰是唯一有诊断价值的
    /// 那次，什么都留不下。按上面 55% 的尝试挂率，跑一轮 12 次就能抓到
    /// 10 次以上的现场。
    ///
    /// 详见 issue #2。
    fn with_deadline<T: Send + 'static>(
        secs: u64,
        body: impl Fn() -> T + Send + Sync + Clone + 'static,
    ) -> T {
        use std::sync::mpsc;
        const TRIES: u32 = 3;
        for attempt in 1..=TRIES {
            let (tx, rx) = mpsc::channel();
            let b = body.clone();
            // 故意不 join：卡住的线程 join 不回来，join 本身就成了第二个挂点。
            // 它会随进程退出被回收。
            std::thread::spawn(move || {
                let _ = tx.send(b());
            });
            match rx.recv_timeout(Duration::from_secs(secs)) {
                Ok(v) => return v,
                Err(_) => eprintln!("  pty 第 {attempt}/{TRIES} 次尝试超过 {secs}s 没返回，重试"),
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
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
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
            let (sess, reader) = Session::spawn("/usr", 80, 24).expect("起不来");
            sess.lock().unwrap().write_input(b"pwd\n").unwrap();
            let out = read_until(reader, "/usr", 10);
            assert!(out.contains("/usr"), "cwd 没生效，实际输出：{out:?}");
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
