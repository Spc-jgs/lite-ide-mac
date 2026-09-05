//! 原生菜单栏。
//!
//! # 为什么要有它
//!
//! 不设菜单时 Tauri 会挂一套默认的，实测（AppleScript 从跑着的 .app 里挖的）：
//!
//! ```text
//! File  →  Close Window、Close All        ← 就这两条
//! View  →  Toggle Full Screen             ← 就这一条
//! Help  →  （空的）
//! ```
//!
//! 全英文，而界面全中文；更要紧的是 **⌘S / ⌘P / ⇧⌘F / ⇧⌘O / ⇧⌘G / ⌘J /
//! ⌘1 / ⌃⇧` / 连按两下 ⇧ 一条都不在里面** —— 它们只活在 `App.svelte` 的
//! keydown 监听里。快捷键表只在空态那张卡片上出现过一次，开了文件就再也找不到。
//!
//! 这不是审美问题：**这个应用的功能只有作者知道。**
//!
//! # 这里的文案和键位不是随手写的
//!
//! 唯一出处是前端的 `src/lib/state/keymap.ts`。**这份是它的拷贝**，
//! 因为菜单要在窗口出现之前就建好，那时前端还没起来。
//!
//! 两份拷贝靠 `tests/menu_sync.rs` 卡住 —— 判据和 `dto_sync.rs` 一样：
//! **靠自觉迟早会漂移，而漂移的表现是「菜单上写着 ⌘S，按下去没反应」，不报错。**
//!
//! # 键位归谁管
//!
//! 菜单项一旦带上 accelerator，AppKit 会先把键吃掉，webview 收不到 ——
//! 所以同一个键位**不能两边都接**：两边都接是双触发（⌘W 关掉两个标签），
//! 两边都不接是按了没反应，两种都不报错。
//!
//! 具体谁归谁看 `keymap.ts` 的 `owner` 列。这里只重复三条最容易错的：
//!
//! - **⌘F 一个字都不许出现在这个文件里** —— 那是 CM6 查找面板的键位，
//!   进了菜单就等于把编辑器的查找抢没了（项目内搜索是 ⇧⌘F，不冲突）。
//! - **⌘P 只放标签、不挂 accelerator** —— 挂了的话，焦点在终端里按 ⌘P
//!   会被菜单抢走，而那时更可能是想给 shell 的。
//! - **连按两下 ⇧ 表达不成 accelerator**，写进标签文字里 ——
//!   菜单是这个手势唯一的说明书。

use tauri::{
    menu::{Menu, MenuBuilder, MenuItem, MenuItemBuilder, PredefinedMenuItem, Submenu, SubmenuBuilder},
    AppHandle, Manager, Wry,
};

/// 「最近打开」子菜单里一个项目项的 id 前缀，后面接完整路径
pub const RECENT_PREFIX: &str = "recent:";
/// 「清除最近记录」
pub const RECENT_CLEAR: &str = "recent-clear";
/// 列表为空时的禁用占位。
///
/// 空的时候放一条禁用项而不是让子菜单消失：**菜单项时有时无比灰着更让人困惑**。
const RECENT_EMPTY: &str = "recent-empty";

/// 最近打开的上限。
///
/// 8 是照 macOS 自己的「最近使用的项目」来的。再多子菜单会长过一屏，
/// 而「最近」的价值恰恰在于不用找。
pub const RECENT_MAX: usize = 8;

/// 运行期需要改的那几块菜单，存进 state 供命令使用。
pub struct MenuHandles {
    /// 「最近打开」子菜单 —— 内容由前端灌进来
    pub recent: Submenu<Wry>,
    /// 要按「有没有活动标签 / 是不是 Git 仓库」变灰的那些项。
    ///
    /// 这是白捡的：今天所有键位都是 window 级监听，**不管当下有没有意义
    /// 都会触发** —— 没有标签时按 ⌘S、不是 Git 仓库时按 ⇧⌘G，都是走一遍
    /// 然后什么也没发生。灰掉的菜单项本身就是一句解释：不是坏了，是现在用不上。
    pub needs_tab: Vec<MenuItem<Wry>>,
    pub needs_repo: Vec<MenuItem<Wry>>,
    pub needs_term: Vec<MenuItem<Wry>>,
}

/// 一个普通菜单项。`accel` 为 `None` 就只有标签 —— 那正是 ⌘P 那一类的处理。
fn item(app: &AppHandle<Wry>, id: &str, label: &str, accel: Option<&str>) -> tauri::Result<MenuItem<Wry>> {
    let mut b = MenuItemBuilder::with_id(id, label);
    if let Some(a) = accel {
        b = b.accelerator(a);
    }
    b.build(app)
}

pub fn build(app: &AppHandle<Wry>) -> tauri::Result<(Menu<Wry>, MenuHandles)> {
    // ── 运行期要变灰的，先建出来留住句柄 ──
    let save = item(app, "save", "保存", Some("CmdOrCtrl+S"))?;
    let close_tab = item(app, "close-tab", "关闭标签", Some("CmdOrCtrl+W"))?;
    let close_all = item(app, "close-all-tabs", "关闭所有标签", None)?;
    let encoding = item(app, "encoding", "文件编码…", None)?;
    let toggle_mode = item(app, "toggle-mode", "切换编辑 / 日志模式", None)?;
    let outline = item(app, "outline", "文件结构…", Some("Shift+CmdOrCtrl+O"))?;

    let git_changes = item(app, "git-changes", "改动列表", Some("Shift+CmdOrCtrl+G"))?;
    let git_file_diff = item(app, "git-file-diff", "查看当前文件的改动", None)?;
    let git_log = item(app, "git-log", "提交历史", None)?;
    let git_branches = item(app, "git-branches", "分支与工作树…", None)?;
    let git_refresh = item(app, "git-refresh", "刷新状态", None)?;

    let close_terminal = item(app, "close-terminal", "关闭当前终端", None)?;

    // ── 最近打开：内容由前端启动后灌进来，先摆禁用占位 ──
    let recent = SubmenuBuilder::with_id(app, "recent", "最近打开")
        .item(&MenuItemBuilder::with_id(RECENT_EMPTY, "（还没有）").enabled(false).build(app)?)
        .build()?;

    let file = SubmenuBuilder::new(app, "文件")
        .item(&item(app, "open-folder", "打开文件夹…", Some("CmdOrCtrl+O"))?)
        .item(&recent)
        .separator()
        .item(&save)
        .item(&encoding)
        .separator()
        .item(&close_tab)
        .item(&close_all)
        .separator()
        // predefined 自带 ⇧⌘W 和正确的禁用逻辑
        .close_window_with_text("关闭窗口")
        .build()?;

    /*
     * 编辑菜单全部走 predefined，**不能自己撸**。
     *
     * 剪切/复制/粘贴/撤销在 WKWebView 里是靠 NSResponder 链走的
     * （`undo:` `copy:` 这些选择器）；自己发事件到前端再去操作 DOM，
     * 在 CM6 和 xterm 里都会走样。predefined 项挂的正是那几个选择器。
     */
    let edit = SubmenuBuilder::new(app, "编辑")
        .undo_with_text("撤销")
        .redo_with_text("重做")
        .separator()
        .cut_with_text("剪切")
        .copy_with_text("复制")
        .paste_with_text("粘贴")
        .select_all_with_text("全选")
        .build()?;

    let view = SubmenuBuilder::new(app, "视图")
        .item(&item(app, "toggle-sidebar", "侧边栏", Some("CmdOrCtrl+1"))?)
        .item(&item(app, "toggle-panel", "终端面板", Some("CmdOrCtrl+J"))?)
        .separator()
        .item(&item(app, "toggle-minimap", "代码缩略图", None)?)
        .item(&toggle_mode)
        .separator()
        .fullscreen_with_text("进入全屏")
        .build()?;

    let goto = SubmenuBuilder::new(app, "转到")
        // 手势写进标签 —— 没有别的地方会告诉你它存在
        .item(&item(app, "quick-all", "随处搜索（连按两下 ⇧）", None)?)
        // ⌘P 只放标签：挂了 accelerator，终端里的 ⌘P 就被菜单抢走了
        .item(&item(app, "quick-file", "找文件…", None)?)
        .item(&item(app, "quick-content", "在项目中搜索…", Some("Shift+CmdOrCtrl+F"))?)
        .item(&outline)
        .build()?;

    let git = SubmenuBuilder::new(app, "Git")
        .item(&git_changes)
        .item(&git_file_diff)
        .item(&git_log)
        .separator()
        .item(&git_branches)
        .item(&git_refresh)
        .build()?;

    let term = SubmenuBuilder::new(app, "终端")
        .item(&item(app, "new-terminal", "新建终端", Some("Ctrl+Shift+Backquote"))?)
        .item(&close_terminal)
        .build()?;

    let window = SubmenuBuilder::new(app, "窗口")
        .minimize_with_text("最小化")
        .maximize_with_text("缩放")
        .separator()
        .bring_all_to_front_with_text("前置全部窗口")
        .build()?;

    let help = SubmenuBuilder::new(app, "帮助")
        .item(&item(app, "help-keys", "快捷键速查", Some("CmdOrCtrl+/"))?)
        .separator()
        .item(&item(app, "help-repo", "项目主页", None)?)
        .build()?;

    /*
     * 应用菜单（最左边）走 predefined：`about` / `services` / `quit`
     * 各自挂着系统行为，自己拼一个的话「服务」「隐藏其他」会全没。
     */
    let app_menu = SubmenuBuilder::new(app, "lite-ide")
        .about_with_text("关于 lite-ide", None)
        .separator()
        .services_with_text("服务")
        .separator()
        .hide_with_text("隐藏 lite-ide")
        .hide_others_with_text("隐藏其他")
        .separator()
        .quit_with_text("退出 lite-ide")
        .build()?;

    let menu = MenuBuilder::new(app)
        .items(&[&app_menu, &file, &edit, &view, &goto, &git, &term, &window, &help])
        .build()?;

    // 认了这两个，系统才会自动往「窗口」里塞窗口列表、往「帮助」里塞搜索框
    #[cfg(target_os = "macos")]
    {
        let _ = window.set_as_windows_menu_for_nsapp();
        let _ = help.set_as_help_menu_for_nsapp();
    }

    let handles = MenuHandles {
        recent,
        needs_tab: vec![save, close_tab, close_all, encoding, toggle_mode, outline, git_file_diff],
        needs_repo: vec![git_changes, git_log, git_branches, git_refresh],
        needs_term: vec![close_terminal],
    };
    Ok((menu, handles))
}

/// 重建「最近打开」。
///
/// 整个子菜单清掉重填，不算增量 —— 最多 8 项，增量的复杂度换不来任何东西。
pub fn refresh_recent(app: &AppHandle<Wry>, paths: &[String]) -> tauri::Result<()> {
    let Some(h) = app.try_state::<MenuHandles>() else {
        return Ok(());
    };
    let menu = &h.recent;
    // items() 拿到的是快照，边遍历边删不会打架
    for it in menu.items()? {
        menu.remove(&it)?;
    }
    if paths.is_empty() {
        menu.append(&MenuItemBuilder::with_id(RECENT_EMPTY, "（还没有）").enabled(false).build(app)?)?;
        return Ok(());
    }
    for p in paths.iter().take(RECENT_MAX) {
        menu.append(&MenuItemBuilder::with_id(format!("{RECENT_PREFIX}{p}"), display_label(p)).build(app)?)?;
    }
    menu.append(&PredefinedMenuItem::separator(app)?)?;
    menu.append(&MenuItemBuilder::with_id(RECENT_CLEAR, "清除最近记录").build(app)?)?;
    Ok(())
}

/// 按当下的上下文让菜单项变灰。
pub fn sync_enabled(app: &AppHandle<Wry>, has_tab: bool, has_repo: bool, has_term: bool) {
    let Some(h) = app.try_state::<MenuHandles>() else {
        return;
    };
    for it in &h.needs_tab {
        let _ = it.set_enabled(has_tab);
    }
    for it in &h.needs_repo {
        let _ = it.set_enabled(has_repo);
    }
    for it in &h.needs_term {
        let _ = it.set_enabled(has_term);
    }
}

/// `/a/b/etianqu-admin` → `etianqu-admin —— ~/b`。
///
/// 显示目录名**加上一级**，不是光目录名：多个项目下都有 `admin` 是常态
/// （`~/work/alpha/admin` 和 `~/work/beta/admin`），只显示目录名的话
/// 那两行一模一样，「最近」就成了猜。
fn display_label(path: &str) -> String {
    let p = std::path::Path::new(path);
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let Some(parent) = p.parent().map(|s| s.to_string_lossy().into_owned()) else {
        return name;
    };
    if parent.is_empty() {
        return name;
    }
    let home = std::env::var("HOME").unwrap_or_default();
    let parent = if !home.is_empty() && parent.starts_with(&home) {
        parent.replacen(&home, "~", 1)
    } else {
        parent
    };
    format!("{name} —— {parent}")
}

#[cfg(test)]
mod tests {
    use super::display_label;

    #[test]
    fn 同名目录必须分得开() {
        // 多个项目下都有 admin 是常态 —— 只显示目录名的话这两行一模一样
        let a = display_label("/Users/x/work/alpha/admin");
        let b = display_label("/Users/x/work/beta/admin");
        assert_ne!(a, b, "同名目录分不开，「最近」就是在猜");
        assert!(a.starts_with("admin —— "), "目录名要排在最前面，实得：{a}");
    }

    #[test]
    fn home_缩成波浪号() {
        let Ok(home) = std::env::var("HOME") else {
            return; // 没有 HOME 的环境（某些 CI）跳过，不是被测逻辑的问题
        };
        if home.is_empty() {
            return;
        }
        let got = display_label(&format!("{home}/proj/demo"));
        assert_eq!(got, "demo —— ~/proj", "实得：{got}");
    }

    #[test]
    fn 极端路径不炸() {
        assert_eq!(display_label("/tmp"), "tmp —— /");
        assert_eq!(display_label("/"), "/");
        assert_eq!(display_label(""), "");
    }
}
