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
//! # 生成物目录里的事件不算数
//!
//! `cargo build` 往 `target/` 里写几分钟，`pnpm install` 往 `node_modules/`
//! 里写几万个文件。它们每一条都会被上面那套合成「每 300ms 一条 `Files`」——
//! 而前端一条 `Files` 扇出去的是三件事：重读打开的文件 + 重列目录、一次
//! `git status`、一次 `ignored_dirs`（又一个 git 子进程）。上面那段「顶得住」
//! 算的是一条事件的代价，没算一次 build 会连着触发几百条。文件树和搜索本来
//! 就不进这些目录（`excludes`），它们里面变了，界面上没有任何东西需要跟着变。
//!
//! 只跳 [`excludes::CERTAIN_GENERATED_DIRS`]，不跳 `dist` / `build` / `vendor`
//! 那几个有争议的：这里没法问 git（每条事件问一次就是又一个子进程风暴），
//! 而按名字跳掉一个真叫 `build/` 的源码目录，后果是**外部改了它、树不刷、
//! 没有任何提示** —— 正是 issue #13 那种最难查的静默错。多刷几次是可见的
//! 代价，少刷是不可见的，两害取可见的那个。
//!
//! FSEvents 只能整棵递归监听，没有「除了这几个子目录」——所以是在回调里
//! 按路径过滤，不是少监听几个目录。
//!
//! # 事件太多时
//!
//! notify 内部的通道是无界的，事件来得比处理得快就在那儿排队。这里的回调只做
//! 两件事 —— 看一眼路径里有没有 `.git`、置两个标志位 —— 微秒级，排不起来。
//! 真正慢的（刷新）在前端，而前端收到的已经是防抖之后每 300ms 最多一条。

use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher as _};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
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

/// 路径穿过了一个**肯定是生成物**的目录（`target/`、`node_modules/`…）。
/// 只看名字，不看位置：`crates/x/target/` 和根上的 `target/` 一样都是 cargo 写的
fn touches_generated(p: &Path) -> bool {
    p.components()
        .any(|c| c.as_os_str().to_str().is_some_and(excludes::is_certain_generated_dir))
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
            } else if !touches_generated(p) {
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
            debounce(&w3, &s3, |p| !p.git && !p.files, |taken| on_change(if taken.files { Change::Files } else { Change::Git }))
        })
        .map_err(|e| format!("起不了防抖线程：{e}"))?;

    Ok(Watch { _watcher: watcher, stop, wake })
}

/// 一个活着的单文件监听（[`watch_file`]）。**drop 即停**，同 [`Watch`]
pub struct FileWatch {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<bool>, Condvar)>,
}

impl Drop for FileWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.1.notify_all();
    }
}

/// 监听**一个文件**：`settings.json` 那种（issue #44，docs/SETTINGS.md 7.2）。它变了（防抖之后）调一次 `on_change`。
///
/// 三条都是第 0 步实测出来的，改之前先看那一节：
///
/// - **监听它所在的目录（不递归），按文件名过滤，不直接盯这个文件。** 文件可能还不存在（设置文件只在点「设置…」时才建），
///   而 notify 监听一个不存在的路径**直接报错**，之后建出来也收不到。父目录不存在由调用方先建。
///   （原来以为的理由「保存会换 inode」在 macOS 上不成立：FSEvents 按路径监听。）
/// - **不看事件类型，也不说变了什么。** FSEvents 会把这个路径过去的标记一起带上：原地覆写报「改名」，
///   lite-ide 第二次 `write_text` 时只有两条「改名」、没有「内容变了」。所以只报「碰过」，调用方重读、和上次比内容。
/// - **按文件名精确比**：vim 的 `settings.json~`、我们自己的 `.settings.json.lite-ide-tmp` 都带着这个名字；
///   同目录的 `windows.json`、`ui-state.json` 一直在被应用自己写。
///
/// 防抖同 [`watch`]：第一条到了等 [`DEBOUNCE`]，期间的合成一条（vim 存一次就是 11 条）。
pub fn watch_file(path: impl AsRef<Path>, on_change: impl Fn() + Send + 'static) -> Result<FileWatch, String> {
    let path = path.as_ref();
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(format!("{} 不是一个文件路径", path.display()));
    };
    let name = name.to_os_string();
    let wake: Arc<(Mutex<bool>, Condvar)> = Arc::new((Mutex::new(false), Condvar::new()));
    let stop = Arc::new(AtomicBool::new(false));

    let w2 = Arc::clone(&wake);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        if !ev.paths.iter().any(|p| p.file_name() == Some(name.as_os_str())) {
            return;
        }
        let (lock, cv) = &*w2;
        *lock.lock().unwrap_or_else(|e| e.into_inner()) = true;
        cv.notify_one();
    })
    .map_err(|e| format!("起不了文件监听：{e}"))?;
    watcher
        .watch(dir, RecursiveMode::NonRecursive)
        .map_err(|e| format!("监听不了 {}：{e}", dir.display()))?;

    let w3 = Arc::clone(&wake);
    let s3 = Arc::clone(&stop);
    thread::Builder::new()
        .name("file-watch-debounce".into())
        .spawn(move || debounce(&w3, &s3, |hit| !*hit, |_| on_change()))
        .map_err(|e| format!("起不了防抖线程：{e}"))?;

    Ok(FileWatch { _watcher: watcher, stop, wake })
}

/// 一个文件此刻的样子（[`watch_paths`] 报的）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathChange {
    /// 这段时间里出现过「新建 / 改名到这儿」，现在在 —— 新文件，或者被换掉的（保存是写临时文件再改名过来）
    Created,
    /// 一直在，内容变了
    Changed,
    /// 现在不在了
    Removed,
}

/// 防抖之后的一批。`overflow`：这段时间里变的文件超过了上限，`changes` 只有前一截 —— 调用方要当成「收不全」处理（比如整个重来）
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PathBatch {
    pub changes: Vec<(PathBuf, PathChange)>,
    pub overflow: bool,
}

#[derive(Default)]
struct PathPending {
    /// 路径 → 这段时间里有没有「新建 / 改名」过
    seen: BTreeMap<PathBuf, bool>,
    overflow: bool,
}

/// 一个活着的按路径的监听（[`watch_paths`]）。**drop 即停**，同 [`Watch`]
pub struct PathWatch {
    _watcher: RecommendedWatcher,
    stop: Arc<AtomicBool>,
    wake: Arc<(Mutex<PathPending>, Condvar)>,
}

impl Drop for PathWatch {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        self.wake.1.notify_all();
    }
}

/// 攒下的这一批 → 每个文件此刻的样子。**按此刻盘上在不在判，不信事件类型**：FSEvents 会把同一个路径过去的标记一起带上
/// （[`watch_file`] 头上那段：原地覆写报的是「改名」），单看事件说「改名」的文件可能早就在、说「改了」的可能已经删了
fn classify(p: PathPending, exists: impl Fn(&Path) -> bool) -> PathBatch {
    let changes = p
        .seen
        .into_iter()
        .map(|(path, born)| {
            let c = if !exists(&path) {
                PathChange::Removed
            } else if born {
                PathChange::Created
            } else {
                PathChange::Changed
            };
            (path, c)
        })
        .collect();
    PathBatch { changes, overflow: p.overflow }
}

/// 同 [`watch`]，但**说出变了哪些文件**：给要知道具体路径的（语言服务器的索引，docs/IDEA.md 第 14 节）。
/// 只收 `keep` 认的（`.java`、`pom.xml` 这类），`.git/` 和生成物目录里的照旧不算；一批最多 `cap` 个，超了标 `overflow`、后面的不收 ——
/// 一次切分支能动几万个文件，全攒着是无界的内存。防抖同 [`watch`]
pub fn watch_paths(
    root: impl AsRef<Path>,
    keep: impl Fn(&Path) -> bool + Send + 'static,
    cap: usize,
    on_batch: impl Fn(PathBatch) + Send + 'static,
) -> Result<PathWatch, String> {
    let wake: Arc<(Mutex<PathPending>, Condvar)> = Arc::new((Mutex::new(PathPending::default()), Condvar::new()));
    let stop = Arc::new(AtomicBool::new(false));
    let w2 = Arc::clone(&wake);
    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(ev) = res else { return };
        if matches!(ev.kind, notify::EventKind::Access(_)) {
            return;
        }
        let born = matches!(ev.kind, notify::EventKind::Create(_) | notify::EventKind::Modify(notify::event::ModifyKind::Name(_)));
        let mut hit = false;
        let (lock, cv) = &*w2;
        let mut pend = lock.lock().unwrap_or_else(|e| e.into_inner());
        for p in &ev.paths {
            if touches_git(p) || touches_generated(p) || !keep(p) {
                continue;
            }
            hit = true;
            if let Some(b) = pend.seen.get_mut(p) {
                *b |= born;
            } else if pend.seen.len() < cap {
                pend.seen.insert(p.clone(), born);
            } else {
                pend.overflow = true;
            }
        }
        if hit {
            cv.notify_one();
        }
    })
    .map_err(|e| format!("起不了文件监听：{e}"))?;
    watcher
        .watch(root.as_ref(), RecursiveMode::Recursive)
        .map_err(|e| format!("监听不了 {}：{e}", root.as_ref().display()))?;
    let w3 = Arc::clone(&wake);
    let s3 = Arc::clone(&stop);
    thread::Builder::new()
        .name("path-watch-debounce".into())
        .spawn(move || debounce(&w3, &s3, |p: &PathPending| p.seen.is_empty() && !p.overflow, |taken| on_batch(classify(taken, |p| p.symlink_metadata().is_ok()))))
        .map_err(|e| format!("起不了防抖线程：{e}"))?;
    Ok(PathWatch { _watcher: watcher, stop, wake })
}

/// 两个监听共用的防抖：等到有事（`idle` 说不空了），再睡 [`DEBOUNCE`]，把这段时间攒下的整个拿走交给 `fire`；`stop` 置位就退出。
///
/// **拎出来是为了不经过 FSEvents 测**（2026-10-12）。原来两份一模一样的循环各写在两个线程闭包里，只能「真写文件、看收到几条」地测 ——
/// 而系统事件什么时候送到是 FSEvents 的事：CI 的慢机器上它会迟到、会把 20 次写拆成隔了 300ms 以上的两批，于是
/// 「单文件_一阵写只合成一条」间歇红（`ac41149` 那次）。那条测试断言的其实是 FSEvents 的时机，不是我们的代码。
/// 现在「一阵只发一条」在这里用人造的信号确定地测；真写文件的那几条只断言「收得到、合并过」
fn debounce<T: Default>(wake: &(Mutex<T>, Condvar), stop: &AtomicBool, idle: impl Fn(&T) -> bool, mut fire: impl FnMut(T)) {
    let (lock, cv) = wake;
    loop {
        let mut pend = lock.lock().unwrap_or_else(|e| e.into_inner());
        while idle(&pend) {
            if stop.load(Ordering::SeqCst) {
                return;
            }
            pend = cv.wait(pend).unwrap_or_else(|e| e.into_inner());
        }
        drop(pend);
        thread::sleep(DEBOUNCE);
        if stop.load(Ordering::SeqCst) {
            return;
        }
        let taken = std::mem::take(&mut *lock.lock().unwrap_or_else(|e| e.into_inner()));
        fire(taken);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn 按路径_此刻在不在决定类型_不信事件说的() {
        let mut p = PathPending::default();
        p.seen.insert("/p/New.java".into(), true);
        p.seen.insert("/p/Old.java".into(), false);
        p.seen.insert("/p/Gone.java".into(), true);
        p.overflow = true;
        let got = classify(p, |q| q != Path::new("/p/Gone.java"));
        assert_eq!(
            got.changes,
            vec![
                ("/p/Gone.java".into(), PathChange::Removed),
                ("/p/New.java".into(), PathChange::Created),
                ("/p/Old.java".into(), PathChange::Changed),
            ],
            "出现过新建、现在不在了的算删了（事件说的是「新建」）"
        );
        assert!(got.overflow, "收不全要传出去");
    }

    #[test]
    fn 按路径_只收认的文件_删了的报删除() {
        let d = tmp();
        std::fs::write(d.join("A.java"), "class A {}").unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch_paths(&d, |p| p.extension().is_some_and(|e| e == "java"), 100, move |b| tx.send(b).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        std::fs::write(d.join("B.java"), "class B {}").unwrap();
        std::fs::write(d.join("notes.txt"), "不认").unwrap();
        std::fs::remove_file(d.join("A.java")).unwrap();
        // FSEvents 可能拆成几批送：收 5 秒，合起来看
        let mut all: BTreeMap<PathBuf, PathChange> = BTreeMap::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while all.len() < 2 && std::time::Instant::now() < deadline {
            if let Ok(b) = rx.recv_timeout(Duration::from_millis(500)) {
                for (p, c) in b.changes {
                    all.insert(p.file_name().map(PathBuf::from).unwrap_or(p), c);
                }
            }
        }
        assert_eq!(all.get(Path::new("B.java")), Some(&PathChange::Created), "{all:?}");
        assert_eq!(all.get(Path::new("A.java")), Some(&PathChange::Removed), "{all:?}");
        assert!(!all.contains_key(Path::new("notes.txt")), "不认的文件不收：{all:?}");
    }

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
        /*
         * **先把陈事件排干净。** `tmp()` 刚建的目录本身、`.git/refs/heads` 那几层，
         * FSEvents 在慢机器上会在 watch 起来之后才吐出来（CI 的 macOS runner 上
         * 2026-09-15 连红两次：收到的第一条是 `Files`，就是根目录的创建事件）。
         * 它落在我们那次写的防抖窗口里就合成一条 `Files`，`Git` 永远等不到。
         * 等两个防抖窗口、把这段时间里到的全丢掉，再写。
         */
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        std::fs::write(d.join(".git/refs/heads/main"), "abc").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        assert_eq!(got, Change::Git);
    }

    /// 不是仓库的目录里 `git init`：从没有 `.git` 到有，也得报成 `Git` —— 前端靠这一下重新找仓库（`git.onGitDirChanged`，2026-10-13）
    #[test]
    fn 原来没有_git_新建出来也是_git() {
        let d = tmp();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        // 同上一条：先把建临时目录的陈事件排干净
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        std::fs::create_dir_all(d.join(".git/refs/heads")).unwrap();
        std::fs::write(d.join(".git/HEAD"), "ref: refs/heads/main\n").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        assert_eq!(got, Change::Git);
    }

    #[test]
    fn 一阵写只合成一条() {
        let d = tmp();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        // 同上一条：先把 `tmp()` 建目录那条陈事件排干净。它在 CI 上迟到，落成第一条，
        // 50 次写就成了「第二条」（v1.2.0 打标签时红的，和 v1.1.0 那次是同一个形状）
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        for i in 0..50 {
            std::fs::write(d.join(format!("f{i}.txt")), "x").unwrap();
        }
        let _first = rx.recv_timeout(Duration::from_secs(5)).expect("5 秒内没收到事件");
        // 同单文件那条：「恰好一条」是 FSEvents 的时机（v1.1.0、v1.2.0 打标签时都为它红过），这里只管收得到、合并过；
        // 「一阵只发一条」由 `防抖_一阵信号只发一次` 确定地测
        let mut more = 0;
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {
            more += 1;
        }
        assert!(more <= 2, "50 次写发出了 {} 条：没合并", more + 1);
    }

    /// 模拟一次 build：往 `target/` 里连写两秒。改前这两秒是 7 条 `Files`
    /// （每条在前端扇出两个 git 子进程 + 一次重列），改后要是 0 条。
    /// 最后往 `src/` 写一个，确认监听本身还活着、不是把所有事件都吞了。
    #[test]
    fn 生成物目录里写不发_源码目录照发() {
        let d = tmp();
        std::fs::create_dir_all(d.join("target/debug/deps")).unwrap();
        std::fs::create_dir_all(d.join("crates/x/node_modules")).unwrap();
        std::fs::create_dir_all(d.join("src")).unwrap();
        let (tx, rx) = mpsc::channel();
        let _w = watch(&d, move |c| tx.send(c).unwrap()).unwrap();
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        let start = std::time::Instant::now();
        let mut n = 0;
        while start.elapsed() < Duration::from_secs(2) {
            std::fs::write(d.join(format!("target/debug/deps/o{n}.rlib")), "x").unwrap();
            std::fs::write(d.join(format!("crates/x/node_modules/m{n}.js")), "x").unwrap();
            n += 1;
            thread::sleep(Duration::from_millis(20));
        }
        let mut got = 0;
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {
            got += 1;
        }
        eprintln!("两秒写了 {n}×2 个生成物文件，收到 {got} 条事件");
        assert_eq!(got, 0, "生成物目录里的写触发了 {got} 次刷新");
        std::fs::write(d.join("src/main.rs"), "x").unwrap();
        let got = rx.recv_timeout(Duration::from_secs(5)).expect("源码目录的写 5 秒内没收到");
        assert_eq!(got, Change::Files);
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

    // ── watch_file（issue #44）──

    /// 起一个单文件监听，先把建目录、建文件留下的陈事件排干净（同上面 `只动_git_底下就是_git` 那段注释）
    fn file_watch(path: &Path) -> (FileWatch, mpsc::Receiver<()>) {
        let (tx, rx) = mpsc::channel();
        let w = watch_file(path, move || tx.send(()).unwrap()).expect("起不来");
        thread::sleep(Duration::from_millis(200));
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        (w, rx)
    }

    #[test]
    fn 单文件_一开始不存在_后来建出来要收得到() {
        // 设置文件只在点「设置…」时才建：启动时它多半不存在，而你可能在 Finder 里或者用别的编辑器第一次把它建出来
        let d = tmp();
        let f = d.join("settings.json");
        let (_w, rx) = file_watch(&f);
        std::fs::write(&f, "{}").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("建出来之后 5 秒内没收到");
    }

    #[test]
    fn 单文件_照编辑器那样存_临时文件加改名_连存两次都收得到() {
        let d = tmp();
        let f = d.join("settings.json");
        std::fs::write(&f, "{}").unwrap();
        let (_w, rx) = file_watch(&f);
        crate::write_text(&f, "{\"a\": 1}").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("第一次存没收到");
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {}
        // 第二次存：实测这次 FSEvents 只给「改名」、没有「内容变了」—— 按类型筛就会漏
        crate::write_text(&f, "{\"a\": 2}").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("第二次存没收到");
    }

    #[test]
    fn 单文件_同目录别的文件被写_不触发() {
        let d = tmp();
        let f = d.join("settings.json");
        std::fs::write(&f, "{}").unwrap();
        let (_w, rx) = file_watch(&f);
        // 照 winctl::save_now 写 windows.json；ui-state.json 原地写；一个只是名字里带着 settings.json 的
        std::fs::write(d.join("windows.json.tmp"), "{}").unwrap();
        std::fs::rename(d.join("windows.json.tmp"), d.join("windows.json")).unwrap();
        std::fs::write(d.join("ui-state.json"), "{}").unwrap();
        std::fs::write(d.join("settings.json~"), "{}").unwrap();
        let got = rx.recv_timeout(Duration::from_millis(1500));
        assert!(got.is_err(), "别的文件被写也触发了");
        // 监听本身还活着
        std::fs::write(&f, "{\"b\": 1}").unwrap();
        rx.recv_timeout(Duration::from_secs(5)).expect("真改了反而没收到");
    }

    #[test]
    fn 单文件_一阵写只合成一条() {
        // vim 存一次是 11 条事件
        let d = tmp();
        let f = d.join("settings.json");
        std::fs::write(&f, "{}").unwrap();
        let (_w, rx) = file_watch(&f);
        for i in 0..20 {
            std::fs::write(&f, format!("{{\"n\": {i}}}")).unwrap();
        }
        rx.recv_timeout(Duration::from_secs(5)).expect("没收到");
        // 「恰好一条」不在这里断言：FSEvents 在慢机器上会把这 20 次拆成两批送，那是它的时机，不是我们的代码（`debounce` 上面那段）。
        // 这里只管「真写文件收得到、而且合并过」—— 「一阵只发一条」由 `防抖_一阵信号只发一次` 确定地测
        let mut more = 0;
        while rx.recv_timeout(DEBOUNCE * 2).is_ok() {
            more += 1;
        }
        assert!(more <= 2, "20 次写发出了 {} 条：没合并", more + 1);
    }

    /// 防抖本身，不经过 FSEvents：防抖窗口里来的五下信号只发一条；之后再来一下，再发一条（没卡死）；`stop` 之后线程退出。
    /// 信号隔 10ms 一下、一共 40ms，离 300ms 的窗口留足余量（CI 慢、测试并行跑）—— 去掉那一下 `sleep(DEBOUNCE)` 就是五条
    #[test]
    fn 防抖_一阵信号只发一次() {
        use std::sync::atomic::AtomicUsize;
        let wake = Arc::new((Mutex::new(false), Condvar::new()));
        let stop = Arc::new(AtomicBool::new(false));
        let n = Arc::new(AtomicUsize::new(0));
        let (w, s2, n2) = (Arc::clone(&wake), Arc::clone(&stop), Arc::clone(&n));
        let t = thread::spawn(move || debounce(&w, &s2, |hit| !*hit, |_| {
            n2.fetch_add(1, Ordering::SeqCst);
        }));
        let poke = || {
            *wake.0.lock().unwrap() = true;
            wake.1.notify_one();
        };
        for _ in 0..5 {
            poke();
            thread::sleep(Duration::from_millis(10));
        }
        thread::sleep(DEBOUNCE * 2);
        assert_eq!(n.load(Ordering::SeqCst), 1, "防抖窗口里的五下该合成一条");
        poke();
        thread::sleep(DEBOUNCE * 2);
        assert_eq!(n.load(Ordering::SeqCst), 2, "之后再来一下要再发一条");
        stop.store(true, Ordering::SeqCst);
        wake.1.notify_all();
        let t0 = std::time::Instant::now();
        while !t.is_finished() && t0.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(20));
        }
        assert!(t.is_finished(), "stop 之后防抖线程没退出");
    }

}
