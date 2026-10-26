//! agent 子进程：起、看着、停。
//!
//! 不走 `procutil::spawn`（它把 stdin 接空）—— 和语言服务器一样，stdin 是说话的管道、一直开着（rust.md「起子进程时的三条硬纪律」里列的那几种例外，
//! 这是第四种）。纪律照守、答案不同：stdout 由 [`crate::conn`] 的读线程读、一行有上限；stderr 另一条线程一直排空、只留最后一截（诊断用）；
//! 一条等待线程攥着 `Child` 收尸、退了当场报；**放进自己的进程组**，停的时候整组先软后硬 —— `npx` 起的是 node，node 再起 Claude Code，
//! 只杀直接子进程的话孙子留成孤儿（2026-10-26 第 0 步实测：适配器读到 stdin 结束 0.04 秒就退，但我们自己崩了、被 kill -9 那条路要靠整组杀兜着）

use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// stderr 留最后多少字节（适配器的日志很多，出问题时看的是最后那几行）
pub const STDERR_TAIL: usize = 32 << 10;
/// 软停（SIGTERM）之后等多久再硬杀
const GRACE: Duration = Duration::from_millis(1500);

pub struct Agent {
    pgid: i32,
    child: Arc<Mutex<Option<Child>>>,
    tail: Arc<Mutex<Vec<u8>>>,
}

pub struct Pipes {
    pub stdin: ChildStdin,
    pub stdout: ChildStdout,
}

impl Agent {
    /// 起 `program args…`（工作目录 `cwd`、PATH 用调用方给的 —— 从访达启动的 .app 没有 shell 的 PATH，见 [`find_npx`]）。
    /// `on_exit` 在进程退出时调一次（退出码；被信号杀的是 None）
    pub fn spawn(program: &Path, args: &[String], cwd: &Path, path_env: &str, on_exit: Box<dyn FnOnce(Option<i32>) + Send>) -> io::Result<(Agent, Pipes)> {
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(cwd).env("PATH", path_env).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        #[allow(clippy::disallowed_methods)] // agent 是对话进程：stdin 要开着、stdout 由 conn 的读线程逐行读（同 lspsvc，见本文件头上）
        let mut child = cmd.spawn()?;
        let pgid = child.id() as i32;
        let stdin = child.stdin.take().ok_or_else(|| io::Error::other("没拿到 stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| io::Error::other("没拿到 stdout"))?;
        let stderr = child.stderr.take().ok_or_else(|| io::Error::other("没拿到 stderr"))?;
        let tail = Arc::new(Mutex::new(Vec::new()));
        {
            // 一直读到 EOF，只留最后 STDERR_TAIL 字节：停下来不读的话，适配器写满 stderr 缓冲就卡住（rust.md 两个管道要并发读）
            let tail = Arc::clone(&tail);
            std::thread::Builder::new().name("acp-stderr".into()).spawn(move || {
                let mut r = stderr;
                let mut buf = [0u8; 8192];
                while let Ok(n) = r.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    let mut t = tail.lock().unwrap_or_else(|e| e.into_inner());
                    t.extend_from_slice(&buf[..n]);
                    if t.len() > STDERR_TAIL {
                        let cut = t.len() - STDERR_TAIL;
                        t.drain(..cut);
                    }
                }
            })?;
        }
        let child = Arc::new(Mutex::new(Some(child)));
        {
            let child = Arc::clone(&child);
            std::thread::Builder::new().name("acp-wait".into()).spawn(move || {
                // 轮询 try_wait 而不是攥着锁 wait()：kill() 要能在任何时候拿到这把锁
                loop {
                    let done = {
                        let mut g = child.lock().unwrap_or_else(|e| e.into_inner());
                        match g.as_mut().map(|c| c.try_wait()) {
                            Some(Ok(Some(st))) => {
                                *g = None;
                                Some(st.code())
                            }
                            Some(Ok(None)) => None,
                            Some(Err(_)) | None => Some(None),
                        }
                    };
                    if let Some(code) = done {
                        on_exit(code);
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            })?;
        }
        Ok((Agent { pgid, child, tail }, Pipes { stdin, stdout }))
    }

    /// stderr 最后那一截（出错时给人看：认证失败、npm 下不下来都在这里）
    pub fn stderr_tail(&self) -> String {
        String::from_utf8_lossy(&self.tail.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }

    pub fn alive(&self) -> bool {
        self.child.lock().unwrap_or_else(|e| e.into_inner()).is_some()
    }

    /// 停：整组 SIGTERM，等 [`GRACE`]，还在就整组 SIGKILL。不阻塞调用方超过 GRACE
    pub fn kill(&self) {
        if !self.alive() {
            return;
        }
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.pgid, libc::SIGTERM);
        }
        let end = Instant::now() + GRACE;
        while Instant::now() < end {
            if !self.alive() {
                return;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        #[cfg(unix)]
        unsafe {
            libc::killpg(self.pgid, libc::SIGKILL);
        }
    }
}

impl Drop for Agent {
    fn drop(&mut self) {
        self.kill();
    }
}

/// 找 `npx`：从访达启动的 .app 拿不到 shell 里的 PATH（nvm / fnm / Homebrew 装的 node 都靠 rc 文件加进 PATH），
/// 起一次交互式登录 shell 把 `command -v npx` 和 `$PATH` 打出来。**按标记取最后两行**：rc 文件自己也可能往 stdout 打字（欢迎语、版本提示）。
/// 返回（npx 的绝对路径, PATH）
pub fn find_npx(shell: &str) -> Result<(PathBuf, String), String> {
    let mut cmd = Command::new(shell);
    cmd.args(["-ilc", "printf '\\n__LITE_NPX__%s\\n__LITE_PATH__%s\\n' \"$(command -v npx)\" \"$PATH\""]);
    let out = procutil::run_capped(&mut cmd, 64 << 10, 16 << 10, procutil::Overflow::Drain).map_err(|e| format!("起不来 {shell}：{e}"))?;
    parse_npx(&String::from_utf8_lossy(&out.stdout))
}

pub(crate) fn parse_npx(out: &str) -> Result<(PathBuf, String), String> {
    let npx = out.lines().rev().find_map(|l| l.strip_prefix("__LITE_NPX__")).unwrap_or("").trim();
    let path = out.lines().rev().find_map(|l| l.strip_prefix("__LITE_PATH__")).unwrap_or("").trim();
    if npx.is_empty() || !npx.starts_with('/') {
        return Err("找不到 npx：这台机器上没有 Node.js（brew install node，或者用 nvm / fnm 装一个），AI 助手的适配器要它来起".into());
    }
    Ok((PathBuf::from(npx), path.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn 找_npx_按标记取_rc_文件打的字不算() {
        let out = "Welcome to zsh!\n__LITE_NPX__ fake\n\n__LITE_NPX__/Users/me/.fnm/bin/npx\n__LITE_PATH__/Users/me/.fnm/bin:/usr/bin\n";
        let (npx, path) = parse_npx(out).unwrap();
        assert_eq!(npx, PathBuf::from("/Users/me/.fnm/bin/npx"), "取最后那一条");
        assert_eq!(path, "/Users/me/.fnm/bin:/usr/bin");
        assert!(parse_npx("__LITE_NPX__\n__LITE_PATH__/usr/bin\n").unwrap_err().contains("Node.js"));
    }

    /// 起一个会派生孙进程的「agent」，停它：孙子也得没了（整组杀）
    #[test]
    fn 停的时候整组都没了_孙进程不留() {
        let dir = std::env::temp_dir();
        let (tx, rx) = mpsc::channel();
        let (agent, _pipes) = Agent::spawn(
            Path::new("/bin/sh"),
            &["-c".into(), "sleep 30 & echo $! >&2; wait".into()],
            &dir,
            "/usr/bin:/bin",
            Box::new(move |code| tx.send(code).unwrap()),
        )
        .unwrap();
        // 等孙进程的 pid 从 stderr 出来
        let t0 = Instant::now();
        let pid = loop {
            if let Ok(p) = agent.stderr_tail().trim().parse::<i32>() {
                break p;
            }
            assert!(t0.elapsed() < Duration::from_secs(3), "孙进程没起来");
            std::thread::sleep(Duration::from_millis(20));
        };
        agent.kill();
        rx.recv_timeout(Duration::from_secs(3)).expect("直接子进程退了要报");
        std::thread::sleep(Duration::from_millis(100));
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "孙进程 {pid} 还活着：只杀了直接子进程");
    }
}
