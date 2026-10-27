//! `GIT_ASKPASS`（#62）：第一次连一个新远程、钥匙串里还没有它的凭据时，git 要问用户名 / 密码 —— 问到界面上去。
//!
//! git 拿凭据的次序是 credential.helper（钥匙串）→ `GIT_ASKPASS` → 终端提问（`remote.rs` 头上那段）。
//! 终端那步由 `GIT_TERMINAL_PROMPT=0` 关着（从访达启动的应用没有终端，git 会挂在一个永远等不到的输入上）；
//! 原来第二步也没接，于是第一次推一个 HTTPS 的公司 GitLab 只会说「认证失败」，要去终端里推一次。
//!
//! 照 VS Code / IDEA：git 要凭据时执行 `$GIT_ASKPASS "Username for 'https://…': "`、读它的 stdout。
//! - **问的那一头是应用自己的可执行文件**（[`client`]，`main.rs` 一开头就认：带着 [`SOCK_ENV`] 起来的不起界面），不往盘上写脚本
//! - **通道**：每次远程操作一个 Unix socket，放在只有自己能进的目录里（0700，`$TMPDIR` 本来就是按用户分的），
//!   再带一个随机口令（[`KEY_ENV`]）—— 连得上 socket 的只有同一个用户的进程，口令挡的是同一个用户下别的程序乱问
//! - **不设超时**：这是在等人（docs/research/codex.md 主题 2「问人的请求不超时」）。远程操作结束（成功、失败、取消）时 [`Askpass`] 析构，
//!   还在问的那一头读到 EOF、退出码 1，git 当成拿不到凭据
//! - 答案只在内存里过：不落盘、不进日志。git 拿到之后交给 credential.helper 存不存，是你的 git 配置决定的（macOS 默认存钥匙串）

use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// 应用自己的可执行文件带着它起来 = 问的那一头（[`client`]）
pub const SOCK_ENV: &str = "LITE_ASKPASS_SOCK";
pub const KEY_ENV: &str = "LITE_ASKPASS_KEY";

/// 问一句：`(提示串, 结束标志)` → 答案；`None` = 人点了取消（或者操作已经结束、标志置位了）。**会一直阻塞到有答案**
pub type Ask = Box<dyn Fn(&str, &AtomicBool) -> Option<String> + Send + Sync>;

/// 一次远程操作的问答通道。活着的时候 git 问得进来；析构即关
pub struct Askpass {
    dir: PathBuf,
    sock: PathBuf,
    key: String,
    exe: PathBuf,
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Askpass {
    /// `exe`：问的那一头（应用自己的可执行文件）。`ask`：问到界面上去
    pub fn start(exe: PathBuf, ask: Ask) -> std::io::Result<Askpass> {
        let key = random_hex(16)?;
        let dir = std::env::temp_dir().join(format!("lite-askpass-{}", random_hex(8)?));
        {
            use std::os::unix::fs::DirBuilderExt;
            std::fs::DirBuilder::new().mode(0o700).create(&dir)?;
        }
        let sock = dir.join("s");
        let listener = UnixListener::bind(&sock)?;
        // 不阻塞地等连接：要能看见「操作结束了」的标志（阻塞在 accept 上的线程叫不醒）
        listener.set_nonblocking(true)?;
        let stop = Arc::new(AtomicBool::new(false));
        let (k, s) = (key.clone(), stop.clone());
        let thread = std::thread::Builder::new().name("git-askpass".into()).spawn(move || serve(listener, &k, &s, &ask))?;
        Ok(Askpass { dir, sock, key, exe, stop, thread: Some(thread) })
    }

    /// 给那条 git 的环境变量
    pub fn env(&self) -> Vec<(&'static str, String)> {
        vec![
            ("GIT_ASKPASS", self.exe.display().to_string()),
            (SOCK_ENV, self.sock.display().to_string()),
            (KEY_ENV, self.key.clone()),
        ]
    }
}

impl Drop for Askpass {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        #[allow(clippy::disallowed_methods)] // 自己这次建的临时目录（名字带随机串，里面只有一个 socket），不是用户的
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

fn serve(listener: UnixListener, key: &str, stop: &AtomicBool, ask: &Ask) {
    while !stop.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((s, _)) => {
                let _ = s.set_nonblocking(false);
                // 一问一答：git 问用户名、问密码是先后两次执行，各一个连接
                let _ = answer(s, key, stop, ask);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => return,
        }
    }
}

/// 一行 `口令\t提示串\n` 进来；答一行出去。口令不对、取消了：什么都不写就关掉（那一头读到 EOF，退出码 1）
fn answer(s: UnixStream, key: &str, stop: &AtomicBool, ask: &Ask) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(5)))?;
    let mut line = String::new();
    // 最多 4 KB：提示串是 git 给的一句话，不让一个乱来的连接把内存撑大
    BufReader::new(s.try_clone()?).take(4096).read_line(&mut line)?;
    let Some((k, prompt)) = line.trim_end_matches('\n').split_once('\t') else { return Ok(()) };
    if k != key {
        return Ok(());
    }
    if let Some(a) = ask(prompt, stop) {
        // 一行一个答案：带换行的（粘贴进来的）只取第一行，不然 git 会把后半截当成下一次的输入
        let a = a.lines().next().unwrap_or("");
        (&s).write_all(format!("{a}\n").as_bytes())?;
    }
    Ok(())
}

/// 问的那一头：连上 socket、送口令和提示串、把答案打到 stdout。返回进程的退出码（0 = 拿到了答案）
pub fn client(prompt: &str) -> i32 {
    let (Some(sock), Some(key)) = (std::env::var_os(SOCK_ENV), std::env::var(KEY_ENV).ok()) else { return 1 };
    match ask_over(Path::new(&sock), &key, prompt) {
        Some(a) => {
            let mut out = std::io::stdout();
            let ok = out.write_all(a.as_bytes()).and_then(|_| out.write_all(b"\n")).and_then(|_| out.flush());
            if ok.is_ok() { 0 } else { 1 }
        }
        None => 1,
    }
}

/// [`client`] 去掉环境变量的那一半（测试直接调它）
pub fn ask_over(sock: &Path, key: &str, prompt: &str) -> Option<String> {
    let s = UnixStream::connect(sock).ok()?;
    // 提示串里的换行 / Tab 换成空格：协议是一行
    let prompt: String = prompt.chars().map(|c| if c == '\n' || c == '\t' { ' ' } else { c }).collect();
    (&s).write_all(format!("{key}\t{prompt}\n").as_bytes()).ok()?;
    let mut line = String::new();
    BufReader::new(&s).read_line(&mut line).ok()?;
    line.ends_with('\n').then(|| line.trim_end_matches('\n').to_string())
}

fn random_hex(n: usize) -> std::io::Result<String> {
    let mut b = vec![0u8; n];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut b)?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    fn recorder(answers: Vec<Option<&'static str>>) -> (Ask, Arc<Mutex<Vec<String>>>) {
        let seen = Arc::new(Mutex::new(Vec::new()));
        let s2 = seen.clone();
        let q = Mutex::new(answers.into_iter());
        (Box::new(move |p: &str, _: &AtomicBool| {
            s2.lock().unwrap().push(p.to_string());
            q.lock().unwrap().next().flatten().map(str::to_string)
        }), seen)
    }

    #[test]
    fn 问到界面上_答案原样回去_取消就是没有答案() {
        let (ask, seen) = recorder(vec![Some("alice"), Some("p@ss\nsecond line"), None]);
        let a = Askpass::start(PathBuf::from("/x/lite-ide"), ask).unwrap();
        let env: std::collections::HashMap<_, _> = a.env().into_iter().collect();
        let (sock, key) = (PathBuf::from(&env[SOCK_ENV]), env[KEY_ENV].clone());
        assert_eq!(env["GIT_ASKPASS"], "/x/lite-ide");
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(std::fs::metadata(sock.parent().unwrap()).unwrap().permissions().mode() & 0o777, 0o700, "只有自己进得去");
        assert_eq!(ask_over(&sock, &key, "Username for 'https://git.example.com': ").as_deref(), Some("alice"));
        assert_eq!(ask_over(&sock, &key, "Password for 'https://alice@git.example.com': ").as_deref(), Some("p@ss"), "粘进来的多行只取第一行");
        assert_eq!(ask_over(&sock, &key, "Password again").as_deref(), None, "点了取消：没有答案（git 当成拿不到凭据）");
        assert_eq!(ask_over(&sock, "wrong-key", "Username for x").as_deref(), None, "口令不对：不问");
        assert_eq!(seen.lock().unwrap().len(), 3, "口令不对的那次没问到界面上");
        let dir = sock.parent().unwrap().to_path_buf();
        drop(a);
        assert!(!dir.exists(), "操作结束：目录和 socket 都收掉");
        assert_eq!(ask_over(&sock, &key, "Username for x"), None, "结束之后再来问：连不上");
    }

    #[test]
    fn 操作结束时还在问的那一头_读到_eof_不挂着() {
        // 界面那头一直没人答：结束标志置位前不返回
        let ask: Ask = Box::new(|_, stop: &AtomicBool| {
            while !stop.load(Ordering::SeqCst) {
                std::thread::sleep(Duration::from_millis(20));
            }
            None
        });
        let a = Askpass::start(PathBuf::from("/x"), ask).unwrap();
        let env: std::collections::HashMap<_, _> = a.env().into_iter().collect();
        let (sock, key) = (PathBuf::from(&env[SOCK_ENV]), env[KEY_ENV].clone());
        let h = std::thread::spawn(move || ask_over(&sock, &key, "Username for x"));
        std::thread::sleep(Duration::from_millis(200));
        let t = std::time::Instant::now();
        drop(a);
        assert_eq!(h.join().unwrap(), None);
        assert!(t.elapsed() < Duration::from_secs(2), "析构就放手：{:?}", t.elapsed());
    }
}
