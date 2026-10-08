//! 窗口登记表：开着哪些窗口、谁在前台、各自开着哪个项目、谁的前端起来了
//! （多窗口第 2 步，docs/MULTIWINDOW.md 3.3–3.5）。
//!
//! # 为什么要有它
//!
//! 原来 Rust 发给前端的三种事件（菜单、文件变动、打开文件）全是 `app.emit` 广播 ——
//! 只有一个窗口时这没问题，两个窗口就是「在 A 里按 ⌘S，B 也保存」。要定向发，
//! 就得有一处知道「现在该发给谁」，这一处只能在 Rust：它是唯一同时看得见所有窗口的地方。
//!
//! # 只做决定，不碰 Tauri
//!
//! 这里的方法全部返回「该发给谁」，真正 `emit_to` 的是 `lib.rs` / `open.rs` 那一薄层。
//! 理由同服务 crate 不依赖 Tauri：路由规则（第 2 节那几条）是这一步最容易错的东西，
//! 它们要能在裸单测里一条条测到，不能只靠起两个窗口去点。
//!
//! # 前台是谁
//!
//! `mru` 是最近获得焦点的顺序，`mru[0]` 就是前台窗口。菜单事件发给它、菜单的可用
//! 状态按它来、「不知道该给谁」的文件也给它。
//!
//! # 每个窗口一个收件箱（替掉原来全局那一个 `open::Inbox`）
//!
//! 窗口建好到它的前端挂上监听之间有几百毫秒，这期间送来的路径先攒在**那个窗口**的
//! 收件箱里，它的前端调 `initial_paths` 时一并取走，从此直接发事件。「攒还是发」
//! 和「取走并标记就绪」在同一把锁下判，中间没有缝 —— 和原来那个全局的一样。
//! 一个窗口都还没登记时送来的（冷启动时系统事件可能比 `setup` 早），先放进
//! `orphans`，第一个来取的窗口一起拿走。
//!
//! # 第 3 步加的：位置、关掉的窗口、存盘（docs/MULTIWINDOW.md 3.5–3.8）
//!
//! 每个窗口记着位置和大小；用户关掉的窗口挪进 `closed`（点 Dock 图标时开回最近关的那个）。
//! [`Windows::snapshot`] 给出要落盘的 [`Saved`]，`winctl.rs` 写进 `windows.json`。
//! **随改随存，不等退出**：⌘Q 不经过窗口的生命周期（第 0 步实测），退出那一刻
//! 没有任何钩子保证跑得完。

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Mutex;

/// 窗口的位置和大小，逻辑像素（换显示器缩放比也对得上）
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Frame {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// 落盘的一个窗口：开着哪个项目、在哪
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct SavedWin {
    pub root: Option<String>,
    pub frame: Option<Frame>,
}

/// `windows.json` 的全部内容
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Saved {
    pub v: u32,
    /// 退出时开着的窗口，**最久没碰的在前、前台的在最后** —— 按这个顺序建，最后建的就在最前面
    pub windows: Vec<SavedWin>,
    /// 用户关掉的窗口，最近关的在前
    pub closed: Vec<SavedWin>,
    /// 最近打开的项目，最新的在前（多窗口第 4 步从前端的 localStorage 搬过来）。
    /// `serde(default)`：第 3 步写的文件没有这个字段，照样读得进来，不用升版本
    #[serde(default)]
    pub recent: Vec<String>,
}

/// 格式版本。字段含义变了就 +1，读到不认识的版本整份当没有（同 session.ts 的 VERSION）
pub const SAVED_VERSION: u32 = 1;
/// 最多恢复几个窗口。防的是坏文件或循环里开窗口 —— 和 MAX_PTYS 一样是防跑飞的闸
pub const MAX_RESTORE: usize = 12;
/// 「最近关掉的」记几个
const MAX_CLOSED: usize = 8;
/// 「最近打开」记几个。和 `session.ts` 的 RECENT_MAX、`menu.rs` 的 RECENT_MAX 是同一个数
/// （照 macOS 自己的「最近使用的项目」）
pub const RECENT_MAX: usize = 8;

/// 读 `windows.json`。**任何坏数据都只当作没有，绝不 panic** —— 这在启动路径上，
/// 抛一次应用就打不开，而用户没办法清掉那份坏文件。
pub fn parse_saved(text: &str) -> Saved {
    match serde_json::from_str::<Saved>(text) {
        Ok(s) if s.v == SAVED_VERSION => s,
        _ => Saved::default(),
    }
}

/// 启动时开哪几个窗口（按建的顺序）。
///
/// - 上次退出时开着几个，就开回几个（封顶 [`MAX_RESTORE`]）；
/// - 上次退出时一个都没开（全关掉之后才 ⌘Q），开回**最近关掉的那一个** ——
///   和点 Dock 图标是同一个意思：「回到我上次在干的」；
/// - 都没有：空，照今天的老路（第一个窗口自己恢复会话）。
///
/// **项目目录已经不在的跳过**（`exists` 由调用方判，要碰盘）：项目删了、挪了、
/// 外接盘没插，开一个指着空目录的窗口只会让人去关它。没有项目的空窗口照常恢复。
pub fn restore_plan(saved: &Saved, exists: impl Fn(&str) -> bool) -> Vec<SavedWin> {
    let alive = |w: &&SavedWin| w.root.as_deref().is_none_or(&exists);
    let windows: Vec<SavedWin> = saved.windows.iter().filter(alive).cloned().collect();
    if !windows.is_empty() {
        let n = windows.len();
        return windows[n.saturating_sub(MAX_RESTORE)..].to_vec();
    }
    saved.closed.iter().find(alive).cloned().into_iter().collect()
}

/// `ExitRequested` 要不要拦（#41）。
///
/// 第 0 步实测：`code == None` **只**出现在「最后一个窗口被销毁」时 —— ⌘Q 根本不走
/// `ExitRequested`，我们自己的「退出」调的是 `app.exit(0)`，带着 `Some(0)`。
/// 所以 None 一律拦（应用留在 Dock 上），Some 一律放。
pub fn should_prevent_exit(code: Option<i32>) -> bool {
    code.is_none()
}

/// 菜单的可用状态（`sync_menu_state` 推过来的那六个开关）。
/// 每个窗口各存一份，哪个窗口到前台就把它那份套到原生菜单上。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MenuState {
    pub has_tab: bool,
    pub has_repo: bool,
    pub has_term: bool,
    pub has_root: bool,
    pub can_move: bool,
    pub split: bool,
}

#[derive(Default)]
struct Win {
    root: Option<String>,
    frame: Option<Frame>,
    /// 前端取过一次收件箱了没有：取过之后直接发事件
    ready: bool,
    inbox: Vec<String>,
    menu: Option<MenuState>,
    /// 前端起来、恢复完之后要新建一份草稿（没有窗口时按了「新建草稿」，[`Windows::want_scratch`]）
    start_scratch: bool,
}

#[derive(Default)]
struct Inner {
    wins: HashMap<String, Win>,
    mru: Vec<String>,
    orphans: Vec<String>,
    closed: Vec<SavedWin>,
    /// 下一个新窗口的编号。**只增不复用**：复用 label 的话，还没收拾干净的旧资源
    /// （`release_window` 还在跑）会撞上新窗口
    next: u32,
    /// 正在退出：还没回话的窗口
    quitting: Option<Vec<String>>,
    /// 登记过窗口了没有。没有 = 还在冷启动、`setup` 都没跑到，这时送来的路径只能当孤儿等第一个窗口；
    /// 有 = 窗口都被关掉了，该开新窗口。**不能拿「现在有没有窗口」判** —— 两种情况下都是零个
    started: bool,
    /// 最近打开的项目，最新的在前。
    ///
    /// 原来每个窗口在自己内存里各有一份、各自写进 localStorage：后写的盖掉先写的，
    /// 而且清理快照时按的是**自己那份**，会把别的窗口刚开的项目当成「挤出去的」删掉。
    /// 原生的「最近打开」菜单整个应用只有一份，名单也只能有一个主人。
    recent: Vec<String>,
    /// 「没有项目的那份会话快照」（`lite-ide.session:`）现在归哪个窗口（[`Windows::claim_empty`]）
    empty: Option<String>,
}

#[derive(Default)]
pub struct Windows {
    inner: Mutex<Inner>,
}

/// 一条路径该去哪。
/// [`Windows::deliver`] 的结果
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Delivery {
    /// 现在就发：(窗口, 路径)
    pub now: Vec<(String, Vec<String>)>,
    /// 要开的新窗口，每个带着它该打开的路径
    pub new: Vec<Vec<String>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    To(String),
    /// 该开一个新窗口
    New,
}

fn trim_slash(p: &str) -> &str {
    if p.len() > 1 { p.trim_end_matches('/') } else { p }
}

/// `path` 在 `root` 底下（或者就是它）。按路径段比，不按字符串前缀 ——
/// `/a/bc` 不在 `/a/b` 底下。
fn under(path: &str, root: &str) -> bool {
    let (path, root) = (trim_slash(path), trim_slash(root));
    if root == "/" {
        return path.starts_with('/');
    }
    path == root || (path.starts_with(root) && path.as_bytes().get(root.len()) == Some(&b'/'))
}

/// 路由规则（docs/MULTIWINDOW.md 第 2 节，2026-10-08 拍板）。纯函数。
///
/// - **目录**：已经有窗口开着它 → 那个窗口；前台窗口是空的（没项目）→ 前台；否则 → 新窗口。
/// - **文件**：项目包含它的窗口，有好几个取根最长的（最具体的那个）；都不包含 → 前台；
///   一个窗口都没有 → 新窗口。
///
/// `wins` 是 (label, 项目根)，`mru[0]` 是前台。
pub fn route(path: &str, is_dir: bool, wins: &[(&str, Option<&str>)], mru: &[&str]) -> Route {
    let front = mru.first().copied();
    if is_dir {
        if let Some((l, _)) = wins.iter().find(|(_, r)| r.is_some_and(|r| trim_slash(r) == trim_slash(path))) {
            return Route::To(l.to_string());
        }
        if let Some(f) = front {
            if wins.iter().any(|(l, r)| *l == f && r.is_none()) {
                return Route::To(f.to_string());
            }
        }
        return Route::New;
    }
    let owner = wins
        .iter()
        .filter_map(|(l, r)| r.filter(|r| under(path, r)).map(|r| (*l, trim_slash(r).len())))
        .max_by_key(|(_, n)| *n);
    match (owner, front) {
        (Some((l, _)), _) => Route::To(l.to_string()),
        (None, Some(f)) => Route::To(f.to_string()),
        (None, None) => Route::New,
    }
}

impl Windows {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// 窗口建好了。新窗口排到最前面：它一出来就是前台
    pub fn register(&self, label: &str) {
        let mut g = self.lock();
        g.started = true;
        g.wins.entry(label.to_string()).or_default();
        g.mru.retain(|l| l != label);
        g.mru.insert(0, label.to_string());
    }

    /// 窗口获得焦点。返回它存着的菜单状态 —— 调用方拿去套到原生菜单上，
    /// 不用等前端再推一次（前端也不知道自己什么时候「到前台」了）
    pub fn focus(&self, label: &str) -> Option<MenuState> {
        self.register(label);
        self.lock().wins.get(label).and_then(|w| w.menu)
    }

    /// 用户关掉了一个窗口：从登记表里拿掉，记进「最近关掉的」（点 Dock 图标时开回它）。
    /// 它收件箱里没来得及送出的路径一起丢掉（窗口都没了，没人看了）。
    ///
    /// ⌘Q 不走这里（没有 `Destroyed`），所以退出时开着的窗口不会被当成「关掉的」。
    pub fn remove(&self, label: &str) {
        let mut g = self.lock();
        if let Some(w) = g.wins.remove(label) {
            if w.root.is_some() || w.frame.is_some() {
                let s = SavedWin { root: w.root, frame: w.frame };
                g.closed.retain(|c| c.root != s.root);
                g.closed.insert(0, s);
                g.closed.truncate(MAX_CLOSED);
            }
        }
        g.mru.retain(|l| l != label);
        if let Some(q) = g.quitting.as_mut() {
            q.retain(|l| l != label);
        }
    }

    /// 还开着几个窗口
    pub fn len(&self) -> usize {
        self.lock().wins.len()
    }

    /// 新窗口的 label：`w-1`、`w-2`……（第一个窗口是配置里的 `main`）
    pub fn next_label(&self) -> String {
        let mut g = self.lock();
        g.next += 1;
        format!("w-{}", g.next)
    }

    pub fn set_frame(&self, label: &str, f: Frame) {
        if let Some(w) = self.lock().wins.get_mut(label) {
            w.frame = Some(f);
        }
    }

    pub fn frame(&self, label: &str) -> Option<Frame> {
        self.lock().wins.get(label).and_then(|w| w.frame)
    }

    /// 取出最近关掉的、项目目录还在的那个（点 Dock 图标重开它）。
    ///
    /// 目录已经没了的一路丢掉（`exists` 由调用方判，要碰盘）—— 同 [`restore_plan`]：
    /// 开一个指着空目录的窗口只会让人去关它，还会经 `set_root` 把这个死目录顶回「最近打开」第一位。
    /// 没有项目的空窗口照常算数。
    pub fn take_closed(&self, exists: impl Fn(&str) -> bool) -> Option<SavedWin> {
        let mut g = self.lock();
        while !g.closed.is_empty() {
            let s = g.closed.remove(0);
            if s.root.as_deref().is_none_or(&exists) {
                return Some(s);
            }
        }
        None
    }

    /// 启动时把上次存的「最近关掉的」和「最近打开」接回来；
    /// `taken` 是这次启动已经开回去的，从「最近关掉的」里去掉
    pub fn load(&self, saved: &Saved, taken: &[SavedWin]) {
        let mut g = self.lock();
        g.closed = saved.closed.iter().filter(|c| !taken.iter().any(|t| t.root == c.root)).cloned().collect();
        g.closed.truncate(MAX_CLOSED);
        g.recent = saved.recent.iter().take(RECENT_MAX).cloned().collect();
    }

    /// 要落盘的样子
    pub fn snapshot(&self) -> Saved {
        let g = self.lock();
        let windows = g
            .mru
            .iter()
            .rev()
            .filter_map(|l| g.wins.get(l))
            .map(|w| SavedWin { root: w.root.clone(), frame: w.frame })
            .collect();
        Saved { v: SAVED_VERSION, windows, closed: g.closed.clone(), recent: g.recent.clone() }
    }

    /// 直接往某个窗口送路径，不走路由（启动时恢复、新建窗口时用：目的地已经定了）。
    /// 前端起来了就返回 true，调用方去发；没起来就攒进它的收件箱。
    pub fn assign(&self, label: &str, paths: Vec<String>) -> bool {
        let mut g = self.lock();
        let w = g.wins.entry(label.to_string()).or_default();
        if w.ready {
            return true;
        }
        w.inbox.extend(paths);
        false
    }

    /// 取走冷启动时攒下的孤儿（`setup` 登记完窗口之后重新路由用）
    pub fn take_orphans(&self) -> Vec<String> {
        std::mem::take(&mut self.lock().orphans)
    }

    /// 这个窗口起来之后要新建一份草稿（没有窗口时按了「新建草稿」）。
    ///
    /// 原来是建完窗口、等它的前端就绪、再补发一个 `menu: new-scratch` —— 可「就绪」只是取过了收件箱，
    /// 它的启动流程还在跑，恢复完一看一个标签都没有就自己落进一份草稿（`launchScratch`），
    /// 两件事撞在一起就是两份草稿。改成记一笔，前端在启动流程里该决定「落进哪份草稿」的那一刻取走
    pub fn want_scratch(&self, label: &str) {
        self.lock().wins.entry(label.to_string()).or_default().start_scratch = true;
    }

    /// 取走「起来之后新建草稿」那一笔。只给一次
    pub fn take_start_scratch(&self, label: &str) -> bool {
        self.lock().wins.get_mut(label).is_some_and(|w| std::mem::take(&mut w.start_scratch))
    }

    /// 开始退出。返回要等哪几个窗口回话（前端起来了的那些）；已经在退出了返回 None
    /// —— 连按两下 ⌘Q 不该把流程跑两遍。
    pub fn begin_quit(&self) -> Option<Vec<String>> {
        let mut g = self.lock();
        if g.quitting.is_some() {
            return None;
        }
        let ready: Vec<String> = g.wins.iter().filter(|(_, w)| w.ready).map(|(l, _)| l.clone()).collect();
        g.quitting = Some(ready.clone());
        Some(ready)
    }

    /// 某个窗口说「我存好了」
    pub fn quit_ack(&self, label: &str) {
        if let Some(q) = self.lock().quitting.as_mut() {
            q.retain(|l| l != label);
        }
    }

    /// 还有几个窗口没回话
    pub fn quit_pending(&self) -> usize {
        self.lock().quitting.as_ref().map_or(0, Vec::len)
    }

    /// 前台窗口
    pub fn front(&self) -> Option<String> {
        self.lock().mru.first().cloned()
    }

    /// 这个窗口现在开着哪个项目。开了一个项目就记进「最近打开」——
    /// 一个项目会从好几条路被打开（⌘O、拖进来、Finder、工作树、会话恢复），
    /// 记在这一个口上不会漏。返回「最近打开」变了没有（调用方据此刷菜单、通知各窗口）。
    ///
    /// **只有这个窗口的项目根真的换了才算「刚打开」。** 每个窗口的前端起来都会再报一次
    /// 自己的根（启动时 Rust 已经替它设过了）—— 那几次不能动顺序，不然每次启动之后
    /// 「最近打开」的顺序取决于哪个窗口的前端先加载完。
    pub fn set_root(&self, label: &str, root: Option<String>) -> bool {
        let root = root.filter(|r| !r.is_empty()).map(|r| trim_slash(&r).to_string());
        let mut g = self.lock();
        if let Some(w) = g.wins.get_mut(label) {
            if w.root == root {
                return false;
            }
            w.root = root.clone();
        }
        let Some(r) = root else { return false };
        let before = g.recent.clone();
        g.recent.retain(|x| *x != r);
        g.recent.insert(0, r);
        g.recent.truncate(RECENT_MAX);
        g.recent != before
    }

    pub fn recent(&self) -> Vec<String> {
        self.lock().recent.clone()
    }

    /// 清理会话快照时一律留着的项目根：开着的窗口各自的项目，加上「最近关掉的」那几个。
    ///
    /// 关掉的也要留：点 Dock 图标会把它们开回来（[`Windows::take_closed`]），快照被删了的话
    /// 开回来的是一个空窗口。它们不一定还在「最近打开」那 8 个里 —— 关掉之后又开了几个别的项目，
    /// 就被挤出去了，而原来清理只认「最近打开 + 开着的」。
    pub fn keep_roots(&self) -> Vec<String> {
        let g = self.lock();
        let mut out: Vec<String> = g.wins.values().filter_map(|w| w.root.clone()).collect();
        for r in g.closed.iter().filter_map(|c| c.root.as_ref()) {
            if !out.contains(r) {
                out.push(r.clone());
            }
        }
        out
    }

    /// 一个没有项目的窗口要用「没有项目的那份会话快照」：给不给它。
    ///
    /// 那份快照只有一个键（`lite-ide.session:`），两个没有项目的窗口都去恢复它，
    /// 同一批标签、同一份草稿就会同时开在两个窗口里，之后两边轮流写，谁后写谁算数。
    /// 所以同一时刻只归一个窗口：没人占着、或者占着它的窗口已经关了 / 已经有项目了，就给。
    /// 不用专门「释放」—— 判的时候看占着的那个现在还算不算数。有项目的窗口要不到。
    pub fn claim_empty(&self, label: &str) -> bool {
        let mut g = self.lock();
        if !g.wins.get(label).is_some_and(|w| w.root.is_none()) {
            return false;
        }
        if let Some(h) = &g.empty {
            if h != label && g.wins.get(h).is_some_and(|w| w.root.is_none()) {
                return false;
            }
        }
        g.empty = Some(label.to_string());
        true
    }

    /// 从「最近打开」里拿掉一个（点了才发现目录没了）。返回变了没有
    pub fn forget_recent(&self, dir: &str) -> bool {
        let mut g = self.lock();
        let n = g.recent.len();
        g.recent.retain(|x| x != trim_slash(dir));
        g.recent.len() != n
    }

    pub fn clear_recent(&self) -> bool {
        let mut g = self.lock();
        let changed = !g.recent.is_empty();
        g.recent.clear();
        changed
    }

    /// 从老版本接过来的「最近打开」（升级后第一次启动，前端读到旧的全局快照时交过来）。
    /// **只在这边还是空的时候收**：已经有了说明迁移做过，再收一次会把旧名单盖回去
    pub fn adopt_recent(&self, list: Vec<String>) -> bool {
        let mut g = self.lock();
        if !g.recent.is_empty() {
            return false;
        }
        for r in list {
            let r = trim_slash(&r).to_string();
            if !r.is_empty() && !g.recent.contains(&r) {
                g.recent.push(r);
            }
        }
        g.recent.truncate(RECENT_MAX);
        !g.recent.is_empty()
    }

    /// 存下这个窗口的菜单状态。返回 true = 它就是前台，调用方该立刻套到原生菜单上；
    /// false = 它在后台，先存着，等它到前台时由 [`Windows::focus`] 交出来
    pub fn set_menu(&self, label: &str, st: MenuState) -> bool {
        let mut g = self.lock();
        let front = g.mru.first().is_some_and(|f| f == label);
        if let Some(w) = g.wins.get_mut(label) {
            w.menu = Some(st);
        }
        front
    }

    /// 前端起来了，取走攒着的路径（连同还没认领的孤儿），从此直接发事件。
    ///
    /// 前端必须**先挂监听再调它**，反过来中间那一拍到的就丢了（同原来的 Inbox）。
    pub fn take_inbox(&self, label: &str) -> Vec<String> {
        let mut g = self.lock();
        let orphans = std::mem::take(&mut g.orphans);
        let w = g.wins.entry(label.to_string()).or_default();
        w.ready = true;
        let mut out = std::mem::take(&mut w.inbox);
        out.extend(orphans);
        out
    }

    /// 送一批路径：每条按 [`route`] 找到窗口，那个窗口的前端起来了就放进 `now` 交给调用方去发，
    /// 没起来就攒进它的收件箱；该开新窗口的放进 `new`（每个目录一个窗口，散文件合进一个）。
    ///
    /// `items` 是 (路径, 是不是目录) —— 判目录要碰盘，由调用方做，这里保持纯。
    ///
    /// `from`：是某个窗口自己要打开的（`open_window`：⌘O、最近打开、拖进来一个文件夹），
    /// 路由时就把**它**当前台，不看焦点顺序。焦点顺序回答的是「用户现在在看哪个窗口」，
    /// 而这里问的是「谁要开」—— 往后台窗口里拖一个文件夹时两者不是同一个：前台恰好是个空窗口的话，
    /// 目录会被塞进那个空窗口，而不是给拖的那个窗口开一个新的（代码审查查出来的）。
    /// 系统送来的（Finder、`open -a`、Dock）没有发起的窗口，传 None。
    pub fn deliver(&self, items: Vec<(String, bool)>, from: Option<&str>) -> Delivery {
        let mut g = self.lock();
        let mut out = Delivery::default();
        let mut loose: Vec<String> = Vec::new();
        for (path, is_dir) in items {
            /*
             * 还没登记过窗口 = 冷启动、`setup` 都没跑到（Finder 双击启动时 `Opened` 比 `setup` 早）。
             * 这时路由什么都看不见，判出来一律是「开新窗口」—— 目录照那个判断走的话，
             * 就会在 main 旁边多开一个窗口、main 自己空着（第 3 步真机验收撞见的）。
             * 先全部攒着，`setup` 登记完、恢复完之后由 [`Windows::take_orphans`] 取出来重新路由。
             */
            if !g.started {
                g.orphans.push(path);
                continue;
            }
            let r = {
                let wins: Vec<(&str, Option<&str>)> =
                    g.wins.iter().map(|(l, w)| (l.as_str(), w.root.as_deref())).collect();
                let mut mru: Vec<&str> = g.mru.iter().map(String::as_str).collect();
                if let Some(f) = from.filter(|f| g.wins.contains_key(*f)) {
                    mru.retain(|l| *l != f);
                    mru.insert(0, f);
                }
                route(&path, is_dir, &wins, &mru)
            };
            let label = match r {
                Route::To(l) => l,
                Route::New if is_dir => {
                    out.new.push(vec![path]);
                    continue;
                }
                Route::New => {
                    // 窗口都关了（#41）之后送来的散文件：合进一个新窗口
                    loose.push(path);
                    continue;
                }
            };
            let Some(w) = g.wins.get_mut(&label) else {
                g.orphans.push(path);
                continue;
            };
            if w.ready {
                match out.now.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, v)) => v.push(path),
                    None => out.now.push((label, vec![path])),
                }
            } else {
                w.inbox.push(path);
            }
        }
        if !loose.is_empty() {
            out.new.push(loose);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn to(l: &str) -> Route {
        Route::To(l.into())
    }

    // ── 路由：第 2 节的每一条规则一条断言 ──

    #[test]
    fn 目录_已经开着就去那个窗口() {
        let wins = [("main", Some("/p/a")), ("w-1", Some("/p/b"))];
        assert_eq!(route("/p/b", true, &wins, &["main", "w-1"]), to("w-1"), "不管谁在前台");
        assert_eq!(route("/p/b/", true, &wins, &["main"]), to("w-1"), "末尾的斜杠不算差别");
    }

    #[test]
    fn 目录_没开着_前台是空窗口就用它_否则开新窗口() {
        let wins = [("main", None), ("w-1", Some("/p/b"))];
        assert_eq!(route("/p/c", true, &wins, &["main", "w-1"]), to("main"), "前台是空的：就用它");
        assert_eq!(route("/p/c", true, &wins, &["w-1", "main"]), Route::New, "前台有项目：开新的（空窗口在后台也不抢）");
        assert_eq!(route("/p/c", true, &[], &[]), Route::New, "一个窗口都没有");
    }

    #[test]
    fn 文件_落到项目包含它的窗口_取最具体的那个() {
        // 工作树放在主仓库里面（`.codex/worktrees/x`）是你的真实用法
        let wins = [("main", Some("/p/repo")), ("w-1", Some("/p/repo/.codex/worktrees/x"))];
        let mru = ["main", "w-1"];
        assert_eq!(route("/p/repo/src/A.java", false, &wins, &mru), to("main"));
        assert_eq!(route("/p/repo/.codex/worktrees/x/src/A.java", false, &wins, &mru), to("w-1"), "两个都包含它：取根最长的");
    }

    /*
     * 第一版这条测试放的是 `/p/b` 和 `/p/bc` 两个窗口 —— 验红时把 `under` 改成纯字符串前缀，
     * 它照样绿：错的 `/p/b` 也「包含」了文件，可「取最长的根」又把正确的 `/p/bc` 挑了回来，
     * 一条规则替另一条把错盖住了。所以这里只留一个**只有按字符串才会匹配上**的窗口。
     */
    #[test]
    fn 文件_按路径段比_不按字符串前缀() {
        let wins = [("main", Some("/p/b")), ("w-1", Some("/q"))];
        assert_eq!(route("/p/bc/x.log", false, &wins, &["w-1", "main"]), to("w-1"), "/p/bc 不在 /p/b 底下：谁都不包含它，给前台");
    }

    #[test]
    fn 文件_谁都不包含就落到前台_没有窗口就开新的() {
        let wins = [("main", Some("/p/a")), ("w-1", Some("/p/b"))];
        assert_eq!(route("/var/log/system.log", false, &wins, &["w-1", "main"]), to("w-1"), "日志多半不在任何项目里：给正在用的那个");
        assert_eq!(route("/var/log/system.log", false, &[], &[]), Route::New);
    }

    // ── 登记表 ──

    #[test]
    fn 没起来的窗口先攒_取走之后直接发() {
        let w = Windows::default();
        w.register("main");
        assert!(w.deliver(vec![("/a.log".into(), false)], None).now.is_empty(), "前端没起来：不该发");
        assert_eq!(w.take_inbox("main"), ["/a.log"], "取的时候一并拿走");
        assert!(w.take_inbox("main").is_empty(), "取过就空了");
        assert_eq!(
            w.deliver(vec![("/b.log".into(), false)], None).now,
            [("main".to_string(), vec!["/b.log".to_string()])],
            "取过一次之后直接发"
        );
    }

    #[test]
    fn 一个窗口都没登记时送来的_第一个来取的窗口拿走() {
        let w = Windows::default();
        assert_eq!(w.deliver(vec![("/a.log".into(), false)], None), Delivery::default(), "冷启动早于 setup：既不发也不开新窗口");
        w.register("main");
        assert_eq!(w.take_inbox("main"), ["/a.log"], "冷启动时系统事件比 setup 早，不能丢");
    }

    /*
     * 第 3 步真机验收撞见的：Finder 双击一个目录冷启动，`Opened` 比 `setup` 早，
     * 那时路由看不见任何窗口，判成「开新窗口」—— 结果 main 空着，旁边多开一个。
     * 目录也得先攒着，等 setup 登记完再取出来重新路由。
     */
    #[test]
    fn 冷启动时送来的目录_也先攒着_不开新窗口() {
        let w = Windows::default();
        assert_eq!(w.deliver(vec![("/p/a".into(), true)], None), Delivery::default(), "setup 之前：不能开新窗口");
        w.register("main");
        assert_eq!(w.take_orphans(), ["/p/a"], "setup 之后取出来重新路由");
        assert_eq!(w.deliver(vec![("/p/a".into(), true)], None).new, Vec::<Vec<String>>::new(), "这时 main 是空的前台：进 main，不开新窗口");
        assert_eq!(w.take_inbox("main"), ["/p/a"]);
    }

    #[test]
    fn 一批路径按窗口分组_各发各的() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1");
        w.set_root("main", Some("/p/a".into()));
        w.set_root("w-1", Some("/p/b".into()));
        w.take_inbox("main");
        w.take_inbox("w-1");
        let mut got = w
            .deliver(vec![("/p/a/1".into(), false), ("/p/b/2".into(), false), ("/p/a/3".into(), false)], None)
            .now;
        got.sort();
        assert_eq!(
            got,
            [
                ("main".to_string(), vec!["/p/a/1".to_string(), "/p/a/3".to_string()]),
                ("w-1".to_string(), vec!["/p/b/2".to_string()]),
            ]
        );
    }

    #[test]
    fn 菜单状态_按窗口存_到前台时交出来() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1"); // w-1 后建，在前台
        let a = MenuState { has_tab: true, ..Default::default() };
        let b = MenuState { has_repo: true, ..Default::default() };
        assert!(!w.set_menu("main", a), "main 在后台：先存着，不该改原生菜单");
        assert!(w.set_menu("w-1", b), "w-1 在前台：立刻套上");
        assert_eq!(w.focus("main"), Some(a), "main 到前台时交出它自己那份");
        assert_eq!(w.front().as_deref(), Some("main"));
        assert_eq!(w.focus("w-1"), Some(b), "切回来是 w-1 那份，没被 main 的盖掉");
    }

    #[test]
    fn 窗口没了就不再是前台() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1");
        w.remove("w-1");
        assert_eq!(w.front().as_deref(), Some("main"), "菜单事件要落到还活着的那个");
        w.remove("main");
        assert_eq!(w.front(), None);
    }

    // ── 第 3 步：开新窗口、关窗不退出、存盘、退出 ──

    #[test]
    fn 该开新窗口的_目录各开一个_散文件合开一个() {
        let w = Windows::default();
        w.register("main");
        w.set_root("main", Some("/p/a".into()));
        w.take_inbox("main");
        let d = w.deliver(vec![("/p/b".into(), true), ("/p/c".into(), true)], None);
        assert_eq!(d.new, [vec!["/p/b".to_string()], vec!["/p/c".to_string()]], "前台有项目：每个没开着的目录一个新窗口");
        assert!(d.now.is_empty());

        // 窗口都关了（#41：应用还在 Dock 上）之后送来的散文件：合开一个窗口，不能当孤儿 ——
        // 孤儿要等「下一个来取收件箱的窗口」，而这时候没有窗口会来
        w.remove("main");
        let d = w.deliver(vec![("/x.log".into(), false), ("/y.log".into(), false)], None);
        assert_eq!(d.new, [vec!["/x.log".to_string(), "/y.log".to_string()]]);
    }

    #[test]
    fn 关窗不退出_只拦最后一个窗口没了的那种() {
        assert!(should_prevent_exit(None), "最后一个窗口销毁：拦下，应用留在 Dock 上");
        assert!(!should_prevent_exit(Some(0)), "我们自己的「退出」调的 app.exit(0)：放行");
    }

    #[test]
    fn 关掉的窗口记下来_点dock时开回最近的那个() {
        let w = Windows::default();
        let f = Frame { x: 10.0, y: 20.0, w: 800.0, h: 600.0 };
        for (l, r) in [("main", "/p/a"), ("w-1", "/p/b")] {
            w.register(l);
            w.set_root(l, Some(r.into()));
            w.set_frame(l, f);
        }
        w.remove("main");
        w.remove("w-1");
        assert_eq!(w.len(), 0);
        let all = |_: &str| true;
        assert_eq!(w.take_closed(all).and_then(|s| s.root).as_deref(), Some("/p/b"), "最近关的在前");
        assert_eq!(w.take_closed(all).and_then(|s| s.root).as_deref(), Some("/p/a"));
        assert!(w.take_closed(all).is_none());
    }

    #[test]
    fn 同一个项目关两次只记一条() {
        let w = Windows::default();
        for l in ["main", "w-1"] {
            w.register(l);
            w.set_root(l, Some("/p/a".into()));
            w.remove(l);
        }
        assert_eq!(w.snapshot().closed.len(), 1, "不然 Dock 点两下开回同一个项目两次");
    }

    #[test]
    fn 存盘的顺序_最久没碰的在前_前台的在最后() {
        let w = Windows::default();
        for (l, r) in [("main", "/p/a"), ("w-1", "/p/b"), ("w-2", "/p/c")] {
            w.register(l);
            w.set_root(l, Some(r.into()));
        }
        w.focus("main");
        let roots: Vec<_> = w.snapshot().windows.into_iter().map(|s| s.root.unwrap()).collect();
        assert_eq!(roots, ["/p/b", "/p/c", "/p/a"], "按这个顺序建，最后建的 main 那个项目落在最前面");
    }

    #[test]
    fn 坏的windows_json一律当没有() {
        for bad in ["", "{", "null", "[]", r#"{"v":99,"windows":[],"closed":[]}"#, r#"{"v":1,"windows":"x"}"#] {
            assert_eq!(parse_saved(bad), Saved::default(), "{bad:?}");
        }
        let good = Saved {
            v: SAVED_VERSION,
            windows: vec![SavedWin { root: Some("/p/a".into()), frame: Some(Frame { x: 1.0, y: 2.0, w: 3.0, h: 4.0 }) }],
            closed: vec![],
            recent: vec!["/p/a".into()],
        };
        assert_eq!(parse_saved(&serde_json::to_string(&good).unwrap()), good, "写出去的要读得回来");
    }

    #[test]
    fn 启动时开哪几个() {
        let win = |r: &str| SavedWin { root: Some(r.into()), frame: None };
        let all = |_: &str| true;
        let s = Saved { v: 1, windows: vec![win("/a"), win("/b")], closed: vec![win("/c")], recent: vec![] };
        assert_eq!(restore_plan(&s, all), [win("/a"), win("/b")], "上次开着几个就开几个，顺序不变");

        let s = Saved { v: 1, windows: vec![], closed: vec![win("/c"), win("/d")], recent: vec![] };
        assert_eq!(restore_plan(&s, all), [win("/c")], "全关掉之后才 ⌘Q 的：开回最近关的那一个");

        assert!(restore_plan(&Saved::default(), all).is_empty(), "什么都没存：照老路");

        let many = Saved { v: 1, windows: (0..30).map(|i| win(&format!("/{i}"))).collect(), closed: vec![], recent: vec![] };
        let plan = restore_plan(&many, all);
        assert_eq!(plan.len(), MAX_RESTORE, "坏文件里写了 30 个窗口也只开这么多");
        assert_eq!(plan.last().unwrap().root.as_deref(), Some("/29"), "留的是最近的那几个（前台在最后）");
    }

    #[test]
    fn 项目目录没了的窗口不恢复() {
        let win = |r: Option<&str>| SavedWin { root: r.map(Into::into), frame: None };
        let gone = |r: &str| r != "/deleted";
        let s = Saved { v: 1, windows: vec![win(Some("/deleted")), win(Some("/b")), win(None)], closed: vec![], recent: vec![] };
        assert_eq!(restore_plan(&s, gone), [win(Some("/b")), win(None)], "删掉的项目跳过；没有项目的空窗口照常回来");

        let s = Saved { v: 1, windows: vec![win(Some("/deleted"))], closed: vec![win(Some("/deleted")), win(Some("/c"))], recent: vec![] };
        assert_eq!(restore_plan(&s, gone), [win(Some("/c"))], "开着的全没了：退到最近关的、还在的那个");
    }

    #[test]
    fn 退出_等前端起来了的窗口回话_连按两下不重来() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1");
        w.register("w-2");
        w.take_inbox("main");
        w.take_inbox("w-1"); // w-2 的前端还没起来：等它也等不来回话
        let mut wait = w.begin_quit().unwrap();
        wait.sort();
        assert_eq!(wait, ["main", "w-1"]);
        assert!(w.begin_quit().is_none(), "连按两下 ⌘Q：流程不该跑两遍");
        w.quit_ack("main");
        assert_eq!(w.quit_pending(), 1);
        w.remove("w-1");
        assert_eq!(w.quit_pending(), 0, "等着的窗口自己关掉了：不能一直等它");
    }

    // ── 第 4 步：「最近打开」只有一份，在这儿 ──

    #[test]
    fn 窗口报上项目根就记进最近打开_各窗口共用一份() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1");
        assert!(w.set_root("main", Some("/p/a".into())), "第一次开：变了");
        assert!(w.set_root("w-1", Some("/p/b/".into())), "另一个窗口开的也记进同一份，末尾斜杠去掉");
        assert_eq!(w.recent(), ["/p/b", "/p/a"], "最新的在前");
        assert!(!w.set_root("w-1", Some("/p/b".into())), "同一个窗口报同一个根：不是「刚打开」");
        assert!(!w.set_root("main", Some("/p/a".into())), "前端起来再报一次自己的根（启动时 Rust 已经设过）：不能动顺序");
        assert_eq!(w.recent(), ["/p/b", "/p/a"]);
        assert!(!w.set_root("main", None), "关闭项目不往名单里加东西");
        assert!(w.set_root("main", Some("/p/a".into())), "关掉再开回来：算刚打开，挪到最前面");
        assert_eq!(w.recent(), ["/p/a", "/p/b"]);
        for i in 0..20 {
            w.set_root("main", Some(format!("/p/{i}")));
        }
        assert_eq!(w.recent().len(), RECENT_MAX, "封顶");
    }

    #[test]
    fn 最近打开_遗忘清空_老名单只在空的时候收() {
        let w = Windows::default();
        assert!(w.adopt_recent(vec!["/a".into(), "/b/".into(), "/a".into(), "".into()]), "升级后第一次：收下老名单");
        assert_eq!(w.recent(), ["/a", "/b"], "去重、去斜杠、丢空串");
        assert!(!w.adopt_recent(vec!["/old".into()]), "已经有了：迁移做过，不能把旧名单盖回去");
        assert!(w.forget_recent("/a"));
        assert!(!w.forget_recent("/a"), "没有的不算变");
        assert!(w.clear_recent());
        assert!(w.recent().is_empty());
    }

    #[test]
    fn 最近打开跟着windows_json走_第3步写的旧文件也读得进来() {
        let old = r#"{"v":1,"windows":[],"closed":[]}"#;
        assert_eq!(parse_saved(old).recent, Vec::<String>::new(), "没有 recent 字段：当空的，不是整份作废");
        let w = Windows::default();
        w.register("main");
        w.set_root("main", Some("/p/a".into()));
        let saved = parse_saved(&serde_json::to_string(&w.snapshot()).unwrap());
        let back = Windows::default();
        back.load(&saved, &[]);
        assert_eq!(back.recent(), ["/p/a"], "存了能读回来");
    }

    // ── 审查之后补的 ──

    #[test]
    fn 点dock重开_跳过目录已经没了的() {
        let w = Windows::default();
        for (l, r) in [("main", "/p/a"), ("w-1", "/deleted"), ("w-2", "/p/c")] {
            w.register(l);
            w.set_root(l, Some(r.into()));
        }
        w.remove("main");
        w.remove("w-2");
        w.remove("w-1"); // 最近关的是 /deleted，后来它的目录被删了
        let gone = |r: &str| r != "/deleted";
        assert_eq!(w.take_closed(gone).and_then(|s| s.root).as_deref(), Some("/p/c"), "删掉的那个跳过，开下一个还在的");
        assert!(w.snapshot().closed.iter().all(|c| c.root.as_deref() != Some("/deleted")), "死目录顺手丢掉，下次不用再试");
    }

    #[test]
    fn 清理快照时_关掉的窗口的项目也留着() {
        let w = Windows::default();
        w.register("main");
        w.set_root("main", Some("/p/closed".into()));
        w.remove("main");
        w.register("w-1");
        for i in 0..RECENT_MAX {
            w.set_root("w-1", Some(format!("/p/{i}")));
        }
        assert!(!w.recent().contains(&"/p/closed".to_string()), "前提：它已经被挤出「最近打开」了");
        let keep = w.keep_roots();
        assert!(keep.contains(&"/p/closed".to_string()), "点 Dock 还会把它开回来：快照不能删");
        assert!(keep.contains(&format!("/p/{}", RECENT_MAX - 1)), "开着的照样留");
    }

    #[test]
    fn 没有项目的那份快照_同一时刻只归一个窗口() {
        let w = Windows::default();
        w.register("main");
        w.register("w-1");
        w.register("w-2");
        w.set_root("w-2", Some("/p/a".into()));
        assert!(w.claim_empty("main"), "没人占着：给");
        assert!(w.claim_empty("main"), "再要一次还是它的");
        assert!(!w.claim_empty("w-1"), "main 占着：另一个没项目的窗口要不到");
        assert!(!w.claim_empty("w-2"), "有项目的窗口要不到");
        w.set_root("main", Some("/p/b".into()));
        assert!(w.claim_empty("w-1"), "main 有项目了：它不再算数，给 w-1");
        w.remove("w-1");
        w.set_root("main", None);
        assert!(w.claim_empty("main"), "占着的窗口关了：谁来要给谁");
    }

    #[test]
    fn 窗口自己要开的目录_按发起的窗口路由_不按焦点() {
        let w = Windows::default();
        w.register("w-1");
        w.set_root("w-1", Some("/p/a".into()));
        w.register("main"); // 空窗口，后建，焦点在它这儿
        w.take_inbox("main");
        w.take_inbox("w-1");
        let d = w.deliver(vec![("/p/c".into(), true)], Some("w-1"));
        assert_eq!(d.new, [vec!["/p/c".to_string()]], "w-1 有项目、是它要开：开新窗口，不塞进前台那个空窗口");
        assert!(d.now.is_empty());
        let d = w.deliver(vec![("/p/c".into(), true)], None);
        assert_eq!(d.now, [("main".to_string(), vec!["/p/c".to_string()])], "系统送来的照旧看焦点：前台是空窗口就用它");
        let d = w.deliver(vec![("/p/x".into(), true)], Some("w-9"));
        assert_eq!(d.now.len() + d.new.len(), 1, "发起的窗口已经没了：退回焦点顺序，不丢");
    }

    #[test]
    fn 没有窗口时新建草稿_记一笔_只给一次() {
        let w = Windows::default();
        w.register("w-1");
        assert!(!w.take_start_scratch("w-1"), "没记过：不给");
        w.want_scratch("w-1");
        assert!(w.take_start_scratch("w-1"));
        assert!(!w.take_start_scratch("w-1"), "取过就没了：刷新页面不该再新建一份");
    }
}
