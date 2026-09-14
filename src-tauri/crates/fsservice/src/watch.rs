//! 文件系统监听：项目根底下有东西变了，告诉前端一声（issue #33 ⑳）。
//!
//! # 为什么要有它
//!
//! 之前外部改动靠两条路：窗口获得焦点时刷一次，10 秒轮询已打开文件的指纹。
//! 而终端就在应用里 —— 在终端里 `git checkout` 完，文件树和 git 状态要等人
//! 切出去再切回来才刷。IDEA / VS Code 都是 FSEvents / inotify，改了当场就变。
//!
//! # 只说「变了」，不说「变了什么」
//!
//! 前端拿到事件之后做的是 `worktree.changed()`（重读打开的文件 + 重列目录）和
//! `git.refresh()`（一次 `git status`），两者都是「整体对一遍」，不需要具体路径。
//! 所以事件只分两档：`.git/` 底下变了只刷 git；别的地方变了两个都刷。
//! 具体路径不传 —— 传了前端也用不上，而 FSEvents 一次 `npm install` 能给几万条。
//!
//! # 防抖
//!
//! 一次 `git checkout` 是几百个文件几百条事件，一次保存是 2–3 条（写、改名、chmod）。
//! 第一条到了之后等 [`DEBOUNCE`] 再发，这段时间里的全部合并成一条。
//! 于是最坏也是每 300ms 一次刷新 —— `git status` 十几毫秒、重列几个目录几毫秒，
//! 顶得住。**不等「安静下来」再发**：`npm install` 能持续一分钟，等安静就是一分钟
//! 什么都不刷，比多刷几次糟。
//!
//! # 我们自己写的也会触发
//!
//! 保存一个文件、在文件树里新建 —— 这些也是事件，也会引起一次刷新。不过滤：
//! 分「是不是我们干的」要在两边记账（写之前登记、事件来了核销），而多刷一次的
//! 代价是十几毫秒。`checkExternalChanges` 靠指纹比对，自己刚存的文件指纹已经更新，
//! 不会误报成外部改动。
//!
//! # 事件太多时
//!
//! notify 内部的通道是无界的，事件来得比处理得快就在那儿排队。这里的回调只做
//! 两件事 —— 看一眼路径里有没有 `.git`、置两个标志位 —— 微秒级，排不起来。
//! 真正慢的（刷新）在前端，而前端收到的已经是防抖之后每 300ms 最多一条。

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

/// 第一条事件到了之后再等多久，把这段时间里的合并成一条
pub const DEBOUNCE: Duration = Duration::from_millis(300);

/// 变了什么。前端据此决定刷哪些
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Change {
    /// 只有 `.git/` 底下变了（提交、切分支、暂存…）：刷 git 状态就够
    Git,
    /// 工作区文件变了（可能连 `.git/` 一起）：重读 + 重列 + 刷 git
    Files,
}

#[derive(Default)]
struct Pending {
    git: bool,
    files: bool,
}

/// 一个活着的监听。**drop 即停**：notify 的 watcher 随之释放，防抖线程看到停止位退出。
pub struct Watch {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<Pending>, Condvar)>,
}

impl Drop for Watch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // 叫醒防抖线程，让它看到停止位；它可能正卡在 wait 上
        self.wake.1.notify_all();
    }
}

/// 路径里有没有 `.git` 这一段。`.git/index.lock`、`.git/refs/heads/x` 都算
fn touches_git(p: &Path) -> bool {
    p.components().any(|c| c.as_os_str() == ".git")
}

/// 监听 `root`（递归）。每次变化（防抖之后）调一次 `on_change`。
///
/// `on_change` 在监听自己的线程上跑，**不要在里面做慢事** —— 前端那边发一个事件
/// 就够了。出错（路径不存在、FSEvents 起不来）直接返回 Err，让调用方决定要不要报。
pub fn watch(
    root: impl AsRef<Path>,
    on_change: impl Fn(Change) + Send + 'static,
) -> Result<Watch, String> {
    let wake: Arc<(Mutex<Pending>, Condvar)> = Arc::new((Mutex::new(Pending::default()), Condvar::new()));
    let stop = Arc::new(AtomicBool::new(false));

    // notify 的回调：只置位，不做事
    let w2 = Arc::clone(&wake);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(ev) = res else { return };
        // 只关心「内容 / 结构变了」。Access 那类（读了一下）FSEvents 不给，
        // 但别的后端会给，过滤掉免得读文件也触发刷新
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        let mut git = false;
        let mut files = false;
        for p in &ev.paths {
            if touches_git(p) {
                git = true;
            } else {
                files = true;
            }
        }
        if !git && !files {
            return;
        }
        let (lock, cv) = &*w2;
        let mut pend = lock.lock().unwrap_or_else(|e| e.into_inner());
        pend.git |= git;
        pend.files |= files;
        cv.notify_one();
    })
    .map_err(|e| format!("起不了文件监听：{e}"))?;

    watcher
        .watch(root.as_ref(), RecursiveMode::Recursive)
        .map_err(|e| format!("监听不了 {}：{e}", root.as_ref().display()))?;

    // 防抖线程：等第一条，再等 DEBOUNCE，然后把攒下的合成一条发出去
    let w3 = Arc::clone(&wake);
    let s3 = Arc::clone(&stop);
    thread::Builder::new()
        .name("fs-watch-debounce".into())
        .spawn(move || {
            let (lock, cv) = &*w3;
            loop {
                let mut pend = lock.lock().unwrap_or_else(|e| e.into_inner());
                while !pend.git && !pend.files {
                    if s3.load(Ordering::SeqCst) {
                        return;
                    }
                    pend = cv.wait(pend).unwrap_or_else(|e| e.into_inner());
                }
                drop(pend);
                thread::sleep(DEBOUNCE);
                if s3.load(Ordering::SeqCst) {
                    return;
                }
                let taken = {
                    let mut pend = lock.lock().unwrap_or_else(|e| e.into_inner());
                    std::mem::take(&mut *pend)
                };
                on_change(if taken.files { Change::Files } else { Change::Git });
            }
        })
        .map_err(|e| format!("起不了防抖线程：{e}"))?;

    Ok(Watch { _watcher: watcher, stop, wake })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    /// 每个测试一个独立目录。**用计数器不用时间**：第一版用的
    /// `Instant::now().elapsed()`，那永远是 0 —— 五个测试挤进同一个目录，
    /// 「只动 .git」那条会收到邻居写的普通文件，间歇红。
    fn tmp() -> std::path::PathBuf {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, Ordering::SeqCst);
        let d = std::env::temp_dir().join(format!("fsw-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn 改一个文件要收到一条_files() {
        let d = tmp();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        // FSEvents 起来要一点时间，先等一拍再写
        thread::sleep(Duration::from_millis(200));
        std::fs::write(d.join("a.txt"), "x").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        assert_eq!(got, Change::Files);
    }

    #[test]
    fn 只动_git_底下就是_git() {
        let d = tmp();
        std::fs::create_dir_all(d.join(".git/refs/heads")).unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(200));
        std::fs::write(d.join(".git/refs/heads/main"), "abc").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        assert_eq!(got, Change::Git);
    }

    #[test]
    fn 一阵写只合成一条() {
        let d = tmp();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(200));
        for i in 0..50 {
            std::fs::write(d.join(format!("f{i}.txt")), "x").unwrap();
        }
        let _first = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        // 防抖窗口之内的 50 次写合成一条；再等一个窗口，不该有第二条
        // （FSEvents 自己也会攒一小会儿，所以放宽到两个窗口）
        let extra = rx.recv_timeout(DEBOUNCE * 2);
        assert!(extra.is_err(), "50 次写发出了不止一条：{extra:?}");
    }

    #[test]
    fn drop_之后不再发() {
        let d = tmp();
        let (tx, rx) = mpsc::channel();
        let w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(200));
        drop(w);
        std::fs::write(d.join("a.txt"), "x").unwrap();
        assert!(rx.recv_timeout(Duration::from_secs(1)).is_err(), "停了还在发");
    }

    #[test]
    fn 不存在的目录要报错() {
        let (tx, _rx) = mpsc::channel::<Change>();
        let r = watch("/nonexistent/dir/for/lite-ide", move |c| tx.send(c).unwrap());
        assert!(r.is_err());
    }
}
