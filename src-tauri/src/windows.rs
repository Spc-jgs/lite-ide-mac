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

use std::collections::HashMap;
use std::sync::Mutex;

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
    /// 前端取过一次收件箱了没有：取过之后直接发事件
    ready: bool,
    inbox: Vec<String>,
    menu: Option<MenuState>,
}

#[derive(Default)]
struct Inner {
    wins: HashMap<String, Win>,
    mru: Vec<String>,
    orphans: Vec<String>,
}

#[derive(Default)]
pub struct Windows {
    inner: Mutex<Inner>,
}

/// 一条路径该去哪。
#[derive(Debug, PartialEq, Eq)]
pub enum Route {
    To(String),
    /// 该开一个新窗口。第 3 步之前还建不了窗口，调用方退回前台窗口（= 今天的行为）
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

    /// 窗口销毁。它收件箱里没来得及送出的路径一起丢掉（窗口都没了，没人看了）
    pub fn remove(&self, label: &str) {
        let mut g = self.lock();
        g.wins.remove(label);
        g.mru.retain(|l| l != label);
    }

    /// 前台窗口
    pub fn front(&self) -> Option<String> {
        self.lock().mru.first().cloned()
    }

    pub fn set_root(&self, label: &str, root: Option<String>) {
        if let Some(w) = self.lock().wins.get_mut(label) {
            w.root = root.filter(|r| !r.is_empty());
        }
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

    /// 送一批路径：每条按 [`route`] 找到窗口，那个窗口的前端起来了就交给调用方去发，
    /// 没起来就攒进它的收件箱。返回「现在就该发」的 (label, 路径)。
    ///
    /// `items` 是 (路径, 是不是目录) —— 判目录要碰盘，由调用方做，这里保持纯。
    pub fn deliver(&self, items: Vec<(String, bool)>) -> Vec<(String, Vec<String>)> {
        let mut g = self.lock();
        let mut now: Vec<(String, Vec<String>)> = Vec::new();
        for (path, is_dir) in items {
            let target = {
                let wins: Vec<(&str, Option<&str>)> =
                    g.wins.iter().map(|(l, w)| (l.as_str(), w.root.as_deref())).collect();
                let mru: Vec<&str> = g.mru.iter().map(String::as_str).collect();
                match route(&path, is_dir, &wins, &mru) {
                    Route::To(l) => Some(l),
                    // 第 3 步之前建不了窗口：退回前台，和今天「在当前窗口里换项目」一样
                    Route::New => mru.first().map(|s| s.to_string()),
                }
            };
            let Some(label) = target else {
                g.orphans.push(path);
                continue;
            };
            let Some(w) = g.wins.get_mut(&label) else {
                g.orphans.push(path);
                continue;
            };
            if w.ready {
                match now.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, v)) => v.push(path),
                    None => now.push((label, vec![path])),
                }
            } else {
                w.inbox.push(path);
            }
        }
        now
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
        assert!(w.deliver(vec![("/a.log".into(), false)]).is_empty(), "前端没起来：不该发");
        assert_eq!(w.take_inbox("main"), ["/a.log"], "取的时候一并拿走");
        assert!(w.take_inbox("main").is_empty(), "取过就空了");
        assert_eq!(
            w.deliver(vec![("/b.log".into(), false)]),
            [("main".to_string(), vec!["/b.log".to_string()])],
            "取过一次之后直接发"
        );
    }

    #[test]
    fn 一个窗口都没登记时送来的_第一个来取的窗口拿走() {
        let w = Windows::default();
        assert!(w.deliver(vec![("/a.log".into(), false)]).is_empty());
        w.register("main");
        assert_eq!(w.take_inbox("main"), ["/a.log"], "冷启动时系统事件比 setup 早，不能丢");
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
        let mut got = w.deliver(vec![
            ("/p/a/1".into(), false),
            ("/p/b/2".into(), false),
            ("/p/a/3".into(), false),
        ]);
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
}
