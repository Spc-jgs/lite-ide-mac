//! 走网络的那几条：fetch / 合并 / push。
//!
//! # 为什么不用 libgit2
//!
//! 量过三条路（2026-09-05）：
//!
//! - **gix**：没有 push（还在 1.0 路线图上），直接出局。
//! - **git2 / libgit2**：在最难的那件事上只给接口不给方案 —— 凭据要自己写
//!   「ssh-agent → helper → 用户名密码」的优先级，还要自己防 libgit2 无限重试
//!   （cargo 为此专门写了 `with_authentication`）。代价是 **+0.85MB**
//!   （实测 release+LTO+strip）和一个要用 cc 现编的 C 库。
//! - **子进程**：凭据白得 —— git 自己跑 credential helper 和 ssh-agent，
//!   **和用户终端里的行为一模一样**。
//!
//! VS Code 和 IntelliJ IDEA 都是子进程调 `git` 可执行文件，
//! 而 IDEA 的 `git4idea` 还是**从 JGit 撤回来的**，撤的原因正是认证和代理问题。
//!
//! # 凭据：三个开关是有次序的，不是互斥的
//!
//! ```text
//! ① credential.helper（钥匙串）→ ② GIT_ASKPASS → ③ 回退到终端提问
//! ```
//!
//! `GIT_TERMINAL_PROMPT=0` 关掉的是 **③**，而 ② 在它前面。所以
//! 「关掉提问」和「拿得到凭据」根本不冲突 —— 实测：精简环境
//! （模拟从访达启动）+ `GIT_TERMINAL_PROMPT=0` 的 fetch **退出码 0**。
//!
//! **② 还没做**，它服务的是「第一次连一个新远程」——
//! VS Code 和 IDEA 都是生成一个临时脚本、让它通过 IPC 把提示串送回应用弹框。
//! 口子留在这儿：加的时候只要给 `git_cmd` 多设一个 `GIT_ASKPASS` 环境变量，
//! 这个文件里别的地方一行都不用动。
//!
//! 在那之前拿不到凭据就走 [`RemoteError::Auth`]，**清清楚楚地失败**
//! —— 而不是挂在那儿等一个永远不会来的输入。

use crate::progress::{Progress, Splitter, Throttle};
use std::io::Read;
use std::path::Path;
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// 取消令牌。置位之后，正在跑的那条 git 会被 kill。
pub type Cancel = Arc<AtomicBool>;

/// 走网络的操作可能怎么失败。
///
/// 和 `crate::Error` 分开：那个只有「没有 git」和「git 报错了」两档，
/// 而这里的区分**要一路传到界面**上去决定显示什么。
#[derive(Debug)]
pub enum RemoteError {
    /// 拿不到凭据。`https` 为真表示远程走 HTTPS（提示语不一样）
    Auth { https: bool, raw: String },
    /// 用户取消了
    Cancelled,
    /// 推送被拒（远程比本地多）—— 下一步是先拉
    Rejected { raw: String },
    /// 合并合不上（冲突），有 MergeView 接着
    Conflict { raw: String },
    /// 其它。`raw` 是 git 的原话，**要留着能给用户展开看**
    Other { raw: String },
    /// git 起不来
    NoGit(std::io::Error),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::Auth { https: true, .. } => write!(f, "这个仓库需要先认证一次"),
            RemoteError::Auth { https: false, .. } => write!(f, "ssh-agent 里没有可用的钥匙"),
            RemoteError::Cancelled => write!(f, "已取消"),
            RemoteError::Rejected { .. } => write!(f, "远程比你多了东西，推不上去"),
            RemoteError::Conflict { .. } => write!(f, "有冲突要先解决"),
            RemoteError::Other { raw } => write!(f, "{raw}"),
            RemoteError::NoGit(e) => write!(f, "找不到 git 命令：{e}"),
        }
    }
}

impl std::error::Error for RemoteError {}

impl RemoteError {
    /// git 的原话。转译错了的时候，人得有办法绕过我们 ——
    /// 和差异视图的 `truncated` 是同一条判据：
    /// **一个会说谎的界面比一个说「我不确定」的界面糟得多。**
    pub fn raw(&self) -> &str {
        match self {
            RemoteError::Auth { raw, .. }
            | RemoteError::Rejected { raw }
            | RemoteError::Conflict { raw }
            | RemoteError::Other { raw } => raw,
            RemoteError::Cancelled => "",
            RemoteError::NoGit(_) => "",
        }
    }
}

type R<T> = Result<T, RemoteError>;

/// 认错误。
///
/// 两个凭据签名是**实测抓的**（2026-09-05），不是猜的：
///
/// ```text
/// fatal: could not read Username for 'https://github.com': terminal prompts disabled
/// git@github.com: Permission denied (publickey).
/// ```
///
/// 加了 askpass 之后第一条基本只在「用户点了取消」时出现，
/// 但签名不变，所以这段不用改。
fn classify(raw: &str, https: bool) -> RemoteError {
    let low = raw.to_ascii_lowercase();
    if low.contains("could not read username")
        || low.contains("could not read password")
        || low.contains("authentication failed")
        || low.contains("permission denied (publickey)")
        || low.contains("terminal prompts disabled")
    {
        return RemoteError::Auth { https, raw: raw.to_string() };
    }
    if low.contains("non-fast-forward") || low.contains("failed to push some refs") {
        return RemoteError::Rejected { raw: raw.to_string() };
    }
    if low.contains("conflict") || low.contains("not possible to fast-forward") {
        return RemoteError::Conflict { raw: raw.to_string() };
    }
    RemoteError::Other { raw: raw.to_string() }
}

/// 出错时给用户看的原话最多留这么多。
///
/// git 失败时的 stderr 可能很长（一堆 hint:），而界面上那块是可展开的小框。
/// 判据同 `MAX_DIFF_BYTES`：**新加任何「读子进程输出」的地方，
/// 先问一句这东西的输出有上限吗。**
const MAX_RAW_BYTES: usize = 8 * 1024;

/// 收尸的上限。
///
/// 抄 `ptysvc` 那次的教训：`wait()` 可能永远不返回（子进程卡在写管道上），
/// 而那会让界面永久卡死。**宁可留一个僵尸到进程退出**——
/// 前者用户察觉不到，后者要重启应用。
const REAP_TIMEOUT: Duration = Duration::from_secs(5);

/// 跑一条会走网络的 git，边跑边把进度抛出去。
///
/// `stdout` 直接丢掉（`Stdio::null()`）：fetch/push 有用的信息全在 stderr，
/// 而**留一个没人读的管道就是 `ptysvc` 那个挂点的形状** ——
/// 管道写满之后子进程卡在写上，`wait()` 永远等不到。
fn run_streaming(
    cwd: &Path,
    args: &[&str],
    https: bool,
    cancel: &Cancel,
    on_progress: &mut dyn FnMut(Progress),
) -> R<()> {
    /*
     * 放进**自己的进程组**，取消时才杀得干净。
     *
     * `git fetch` 会派生 `git-remote-https` 当孙进程，而**孙子攥着 stderr
     * 管道**。只 kill 直接子进程的话，读循环等不到 EOF，只能干等 TCP 超时 ——
     * CI 上实测取消花了 **75 秒**（本地碰巧快，没暴露出来）。
     *
     * `process_group(0)` 让子进程成为新组的组长，于是 `killpg` 能一锅端。
     */
    let mut cmd = crate::git_cmd(cwd, args);
    cmd.stdout(Stdio::null()).stderr(Stdio::piped());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn().map_err(RemoteError::NoGit)?;
    let pid = child.id();

    let stderr = child.stderr.take().expect("stderr 已 piped");
    let child = Arc::new(Mutex::new(child));

    /*
     * 取消靠一个看门线程去 kill。
     *
     * 不在读循环里判 cancel：`read()` 是阻塞的，而连接阶段可能几秒钟
     * 一个字节都没有 —— 那段时间里点取消会毫无反应。
     */
    let done = Arc::new(AtomicBool::new(false));
    let watcher = {
        let (cancel, done, child) = (cancel.clone(), done.clone(), child.clone());
        // pid 按值拷进去 —— 杀组只要它，不用碰 Child，也就不用抢锁
        std::thread::Builder::new()
            .name("git-remote-cancel".into())
            .spawn(move || {
                while !done.load(Ordering::Relaxed) {
                    if cancel.load(Ordering::Relaxed) {
                        /*
                         * 先杀整个进程组，再杀直接子进程兜底。
                         *
                         * 杀组是关键那一下：孙进程（`git-remote-https`）
                         * 攥着 stderr 管道，它不死读循环就等不到 EOF。
                         * `killpg` 不需要锁 —— 只要 pid，所以这一下永远是
                         * 瞬间的，不会被表锁拖住。
                         */
                        #[cfg(unix)]
                        unsafe {
                            libc::killpg(pid as i32, libc::SIGKILL);
                        }
                        // 摘不到锁就下一轮再来 —— 绝不在这儿阻塞等锁
                        if let Ok(mut c) = child.try_lock() {
                            let _ = c.kill();
                        }
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
            })
            .ok()
    };

    let mut sp = Splitter::new();
    let mut th = Throttle::default();
    // 出错时要给的原话。**有界**，而且留的是最后那截（错误在结尾）
    let mut tail: Vec<String> = Vec::new();
    let mut tail_bytes = 0usize;

    let mut buf = [0u8; 8192];
    let mut src = stderr;
    loop {
        match src.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => {
                for p in sp.feed(&buf[..n]) {
                    tail_bytes += p.phase.len() + 1;
                    tail.push(p.phase.clone());
                    while tail_bytes > MAX_RAW_BYTES {
                        if let Some(front) = tail.first() {
                            tail_bytes = tail_bytes.saturating_sub(front.len() + 1);
                        }
                        tail.remove(0);
                    }
                    if th.allow(&p) {
                        on_progress(p);
                    }
                }
            }
            Err(_) => break,
        }
    }
    if let Some(p) = sp.finish() {
        tail.push(p.phase.clone());
        on_progress(p);
    }

    done.store(true, Ordering::Relaxed);
    if let Some(w) = watcher {
        let _ = w.join();
    }

    // 收尸有界 —— 见 REAP_TIMEOUT
    let deadline = Instant::now() + REAP_TIMEOUT;
    let status = loop {
        let got = child.lock().ok().and_then(|mut c| c.try_wait().ok().flatten());
        if let Some(s) = got {
            break Some(s);
        }
        if Instant::now() >= deadline {
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };

    if cancel.load(Ordering::Relaxed) {
        return Err(RemoteError::Cancelled);
    }
    match status {
        Some(s) if s.success() => Ok(()),
        // 收不到尸就不能说它成功了 —— 那正是一个会说谎的界面
        _ => Err(classify(tail.join("\n").trim(), https)),
    }
}

/// 远程用的是不是 HTTPS。认不出来当 HTTPS —— 提示语按更常见的那种给。
fn is_https(cwd: &Path, remote: &str) -> bool {
    crate::remote_url(cwd, remote).map(|u| !u.starts_with("git@") && !u.starts_with("ssh://")).unwrap_or(true)
}

/// 抓远程。**只读，不动工作区** —— 失败了没有任何后果。
///
/// `--prune` 让远程删掉的分支在本地也消失；不加的话分支面板里会一直挂着
/// 已经不存在的 `origin/*`。
pub fn fetch(
    cwd: impl AsRef<Path>,
    remote: &str,
    cancel: &Cancel,
    on_progress: &mut dyn FnMut(Progress),
) -> R<()> {
    let cwd = cwd.as_ref();
    let https = is_https(cwd, remote);
    run_streaming(cwd, &["fetch", "--progress", "--prune", remote], https, cancel, on_progress)
}

/// 本地合并策略。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MergeMode {
    /// 只允许快进。**默认** —— 永远不会「拉一下，凭空多出一个合并提交」
    FfOnly,
    Merge,
    Rebase,
}

/// 把已经抓下来的上游合进当前分支。**不走网络。**
///
/// 拉取是 `fetch()` + 这个，不是 `git pull`：复合命令失败时
/// **分不清是网络断了还是合并冲突了**（退出码都非零）。
/// 拆开之后，fetch 失败就是纯网络/凭据，这一步失败就是冲突。
pub fn merge_upstream(cwd: impl AsRef<Path>, upstream: &str, mode: MergeMode) -> R<()> {
    let cwd = cwd.as_ref();
    let args: Vec<&str> = match mode {
        MergeMode::FfOnly => vec!["merge", "--ff-only", upstream],
        MergeMode::Merge => vec!["merge", "--no-edit", upstream],
        MergeMode::Rebase => vec!["rebase", upstream],
    };
    crate::run(cwd, &args).map(|_| ()).map_err(|e| classify(&e.to_string(), true))
}

/// 推送时的额外开关。
///
/// **现在只有 `set_upstream` 一个可用。** 留成结构体而不是一个 bool，
/// 是为了后面加 `--force-with-lease` 时不用改所有调用点的签名 ——
/// 那条按 [issue #8] 的判据暂时不做入口
/// （个人工具，强推错了没有第二道门；而 `--force-with-lease`
/// 只防「别人推了新东西」，不防「你自己搞错了分支」）。
///
/// 真要加的话，它该是**推送被拒之后**才出现的第三个按钮，
/// 不是常驻下拉 —— 那时人已经知道自己在覆盖什么了。
///
/// [issue #8]: https://github.com/Spc-jgs/lite-ide-mac/issues/8
#[derive(Debug, Clone, Copy, Default)]
pub struct PushOpts {
    /// 这个分支还没有上游，推的同时建立跟踪
    pub set_upstream: bool,
}

/// 推送当前分支。**这是第一个会改到别人东西的操作。**
pub fn push(
    cwd: impl AsRef<Path>,
    remote: &str,
    branch: &str,
    opts: PushOpts,
    cancel: &Cancel,
    on_progress: &mut dyn FnMut(Progress),
) -> R<()> {
    let cwd = cwd.as_ref();
    let https = is_https(cwd, remote);
    let mut args = vec!["push", "--progress"];
    if opts.set_upstream {
        args.push("--set-upstream");
    }
    args.push(remote);
    args.push(branch);
    run_streaming(cwd, &args, https, cancel, on_progress)
}

/// 「推上去会送出哪些提交」的标题列表。
///
/// 照 IDEA 的推送对话框：**列出提交，不是只给一个计数** ——
/// 看清到底送什么出去，比看见「↑2」有用得多。
///
/// `upstream` 为空表示这个分支还没有上游，那时列的是这条分支自己的提交
/// （`--not --remotes` 排掉已经在任何远程上的，否则会把整部历史列出来）。
///
/// `limit` 是硬上限：一个几百个提交的分支，对话框列不下也没人会读。
/// 判据同 `MAX_DIFF_BYTES` —— 读子进程输出先问一句有没有上限。
pub fn outgoing(
    cwd: impl AsRef<Path>,
    upstream: &str,
    branch: &str,
    limit: usize,
) -> Result<Vec<String>, crate::Error> {
    let n = format!("-{limit}");
    let range = if upstream.is_empty() {
        branch.to_string()
    } else {
        format!("{upstream}..{branch}")
    };
    let mut args = vec!["log", "--format=%s", &n, &range];
    if upstream.is_empty() {
        args.push("--not");
        args.push("--remotes");
    }
    let out = crate::run(cwd.as_ref(), &args)?;
    Ok(out.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty()).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 认得出两个凭据签名() {
        // 这两条是 2026-09-05 实测抓的原文，不是编的
        let https_fail =
            "fatal: could not read Username for 'https://github.com': terminal prompts disabled";
        assert!(
            matches!(classify(https_fail, true), RemoteError::Auth { https: true, .. }),
            "HTTPS 没凭据要判成 Auth"
        );

        let ssh_fail = "git@github.com: Permission denied (publickey).\n\
                        fatal: Could not read from remote repository.";
        assert!(
            matches!(classify(ssh_fail, false), RemoteError::Auth { https: false, .. }),
            "SSH 没钥匙要判成 Auth"
        );
    }

    #[test]
    fn 认得出推送被拒() {
        let raw = "! [rejected]  dev -> dev (non-fast-forward)\n\
                   error: failed to push some refs to 'https://github.com/x/y'";
        assert!(matches!(classify(raw, true), RemoteError::Rejected { .. }));
    }

    #[test]
    fn 认得出快进不了() {
        let raw = "fatal: Not possible to fast-forward, aborting.";
        assert!(matches!(classify(raw, true), RemoteError::Conflict { .. }));
    }

    #[test]
    fn 认不出来的落到_other_并且原话留着() {
        let raw = "fatal: 某种我们没见过的错误";
        let e = classify(raw, true);
        assert!(matches!(e, RemoteError::Other { .. }));
        assert_eq!(e.raw(), raw, "原话必须一个字不改地留着 —— 转译错了人要能绕过我们");
    }

    #[test]
    fn 每一档都拿得到原话() {
        // raw() 漏掉哪一档，界面上那个「看 git 的原话」就会是空的
        for e in [
            classify("could not read Username", true),
            classify("non-fast-forward", true),
            classify("CONFLICT (content)", true),
            classify("whatever", true),
        ] {
            assert!(!e.raw().is_empty(), "{e:?} 的 raw() 是空的");
        }
    }

    #[test]
    fn 取消令牌能让操作立刻回来() {
        // 用一个必然慢的命令：clone 一个不存在的地址会卡在连接上
        /*
         * 目录名带上进程 id + 时间戳，并且**开跑前先清一次**。
         *
         * 固定名字的那一版有个卫生问题：这条测试失败时会在清理之前 panic，
         * 于是残留目录让下一次 `git clone` 立刻失败（0.06s），
         * 看起来像「取消变快了」——**一个测完全测错了东西还绿着的形状**。
         */
        let dir = std::env::temp_dir();
        let name = format!(
            "_lite_cancel_probe_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0)
        );
        let _ = std::fs::remove_dir_all(dir.join(&name));
        let cancel: Cancel = Arc::new(AtomicBool::new(false));
        {
            let c = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                c.store(true, Ordering::Relaxed);
            });
        }
        let t0 = Instant::now();
        let r = run_streaming(
            &dir,
            &["clone", "--progress", "https://192.0.2.1/nope.git", &name],
            true,
            &cancel,
            &mut |_| {},
        );
        let took = t0.elapsed();
        assert!(matches!(r, Err(RemoteError::Cancelled)), "实得 {r:?}");
        /*
         * 阈值卡 2s，不是 10s。
         *
         * 10s 那一版**在本地拦不住** —— 少了杀组时本地实测 5.15s，照样绿，
         * 而同一份代码在 CI 上是 **75s**（`git` 派生的 `git-remote-https`
         * 攥着 stderr 管道，只 kill 直接子进程的话读循环要等 TCP 超时）。
         * 这个 bug 就是这么漏到 CI 才被抓住的。
         *
         * 修好之后是 0.22s（取消令牌 200ms 置位 + 看门线程 50ms 轮询），
         * 2s 留了近十倍余量，同时把 5s 那档挡在外面。
         */
        assert!(took < Duration::from_secs(2), "取消花了 {took:?} —— 进程组没杀干净？");
        let _ = std::fs::remove_dir_all(dir.join(&name));
    }
}
