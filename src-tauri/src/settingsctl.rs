//! 配置文件的读盘、监听、存盘、广播（issue #44 第 2 步，docs/SETTINGS.md 第 7 节）。
//!
//! `settings.rs` 做决定（怎么解析、哪种错怎么退、两份怎么合起来），这里动手 —— 和 `windows.rs` / `winctl.rs`
//! 是同一种拆法：决定要能在裸单测里测全，动手的这一层薄到只剩接线。

use crate::settings::{self, UiState};
use crate::state::AppState;
use std::path::PathBuf;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// 设置变了：广播给**所有**窗口（同 `recent-changed`），负载是全量的 `SettingsDto`
pub const EVENT: &str = "settings-changed";

fn dir(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok()
}

/// `settings.json` 在哪（「设置…」要打开它，状态栏的提示点了也要跳过去）
pub fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join("settings.json"))
}

fn ui_path(app: &AppHandle) -> Option<PathBuf> {
    dir(app).map(|d| d.join("ui-state.json"))
}

/// 不存在、读不了、不是 UTF-8 都当「没有」—— 启动路径上不许抛。读不了和不存在对用户是一回事：用默认值
fn read(p: &Option<PathBuf>) -> Option<String> {
    p.as_ref().and_then(|p| std::fs::read_to_string(p).ok())
}

/// `terminal.shell` 写的路径能不能当 shell 起：是个文件、有执行位。目录、断掉的软链、没 x 位的都不行
pub fn shell_ok(p: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
}

/// 新开终端用哪个 shell：设置里写的（`terminal.shell`），空 = `$SHELL`（`ptysvc::Session::spawn_with` 收到空串就用它）。
/// **起的这一刻再判一次**：读设置时还在、后来被删了的（卸载了 fish）也要退回 `$SHELL`，不然终端起不来
pub fn terminal_shell(st: &AppState) -> String {
    let shell = st.settings.view().0.terminal_shell;
    if shell.is_empty() || shell_ok(&shell) {
        return shell;
    }
    applog::write(applog::Level::Warn, "settings", &format!("terminal.shell 指的 {shell} 用不了了，这个终端用的是 $SHELL"));
    String::new()
}

/// 文件监听活多久应用就活多久，放在这儿而不是每个窗口一份
static WATCH: Mutex<Option<fsservice::watch::FileWatch>> = Mutex::new(None);

/// 启动时（`setup` 里，任何窗口的前端开口要设置之前）：读两份文件，起 `settings.json` 的监听。
///
/// **必须在 `setup` 里读完。** 前端挂载之前就 `await settings()`（第 0 步实测一次往返 < 1ms，所以不做首屏缓存），
/// 而同步命令跑在主线程上，`setup` 没跑完前端的请求只能排队 —— 在这儿读好，前端拿到的就一定是读好的值。
pub fn init(app: &AppHandle) {
    let st = app.state::<AppState>();
    let sp = settings_path(app);
    st.settings.load_settings(read(&sp), shell_ok);
    st.settings.load_ui(read(&ui_path(app)).as_deref());
    let (_, problems) = st.settings.view();
    for p in &problems {
        applog::write(applog::Level::Warn, "settings", &p.text());
    }

    let Some(sp) = sp else { return };
    // 监听的是父目录（`watch_file` 的注释）：目录得先在。它就是 windows.json 待的那个目录，多半早就有了
    if let Some(d) = sp.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    let a = app.clone();
    match fsservice::watch::watch_file(&sp, move || reload(&a)) {
        Ok(w) => *WATCH.lock().unwrap_or_else(|e| e.into_inner()) = Some(w),
        // 起不来就没有热加载，重启照样生效 —— 记一笔，不挡启动
        Err(e) => applog::write(applog::Level::Warn, "settings", &format!("监听不了设置文件，改了要重启才生效：{e}")),
    }
}

/// 文件监听报「碰过」：重读，真变了才广播（原文没变 `load_settings` 自己会认出来）
fn reload(app: &AppHandle) {
    let text = read(&settings_path(app));
    if app.state::<AppState>().settings.load_settings(text, shell_ok) {
        crate::diag!("settings-changed（文件）");
        broadcast(app);
    }
}

pub fn broadcast(app: &AppHandle) {
    let _ = app.emit(EVENT, dto(app));
}

pub fn dto(app: &AppHandle) -> crate::commands::SettingsDto {
    let (e, problems) = app.state::<AppState>().settings.view();
    crate::commands::SettingsDto {
        editor_font_family: e.editor_font_family,
        editor_font_size: e.editor_font_size,
        editor_font_base: e.editor_font_base,
        terminal_font_family: e.terminal_font_family,
        terminal_font_size: e.terminal_font_size,
        terminal_shell: e.terminal_shell,
        minimap: e.minimap,
        tree_compact: e.tree_compact,
        tree_follow: e.tree_follow,
        git_grouped: e.git_grouped,
        problems: problems
            .iter()
            .map(|p| crate::commands::SettingProblemDto {
                text: p.text(),
                line: p.pos().map(|x| x.line),
                col: p.pos().map(|x| x.col),
                fatal: p.is_fatal(),
            })
            .collect(),
        path: settings_path(app).map(|p| p.to_string_lossy().into_owned()).unwrap_or_default(),
    }
}

/// 同一时刻只许一个线程写 `ui-state.json`，而且**改和写在同一把锁里**。
///
/// 命令走阻塞池（碰盘），两个窗口同时按 ⌘= 就是两条线程：`fsservice::write_text` 的临时文件名是固定的，
/// 不排队就是 `windows.json` 那次的并发写坏（winctl 的 `SAVE_LOCK`）；改和写分开拿锁的话，
/// 后改的那份可能先落盘、又被先改的那份盖掉。
static UI_SAVE: Mutex<()> = Mutex::new(());

/// 改界面状态 → 存盘 → 广播。返回改完的样子（发起的窗口直接用，不用等广播）
pub fn update_ui(
    app: &AppHandle,
    f: impl FnOnce(&mut UiState, i64) -> Result<bool, String>,
) -> Result<crate::commands::SettingsDto, String> {
    let changed = {
        let _g = UI_SAVE.lock().unwrap_or_else(|e| e.into_inner());
        let st = app.state::<AppState>();
        let changed = st.settings.update_ui(f)?;
        if changed {
            save_ui_locked(app, &st.settings.ui_text());
        }
        changed
    };
    if changed {
        broadcast(app);
    }
    Ok(dto(app))
}

/// 升级后第一次启动：前端交来的旧 localStorage 偏好（`Store::adopt_legacy` 决定收不收）
pub fn adopt_legacy(app: &AppHandle, legacy: UiState) -> crate::commands::SettingsDto {
    let changed = {
        let _g = UI_SAVE.lock().unwrap_or_else(|e| e.into_inner());
        let st = app.state::<AppState>();
        let changed = st.settings.adopt_legacy(legacy);
        if changed {
            save_ui_locked(app, &st.settings.ui_text());
        }
        changed
    };
    if changed {
        broadcast(app);
    }
    dto(app)
}

fn save_ui_locked(app: &AppHandle, text: &str) {
    let Some(p) = ui_path(app) else { return };
    if let Some(d) = p.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Err(e) = fsservice::write_text(&p, text) {
        applog::write(applog::Level::Warn, "settings", &format!("界面状态存不下：{e}"));
    }
}

/// 「设置…」：`settings.json` 不存在就建一份模板（`settings::template`），返回它的路径，前端在当前窗口打开。
pub fn open_settings(app: &AppHandle) -> Result<String, String> {
    let p = settings_path(app).ok_or("找不到应用数据目录")?;
    ensure_template(&p).map_err(|e| format!("建不了设置文件 {}：{e}", p.display()))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 不存在才建，**已经有了一个字节都不碰**（SETTINGS.md 3.2：那是你写的）。
///
/// 用 `create_new` 而不是「先看在不在、不在再写」：两个窗口同时点「设置…」、或者你恰好在 Finder 里刚建了它，
/// 先查后写中间那一下就会把你的内容换成模板（rust.md「改盘的 std API 默认都会吃掉已有文件」）。
fn ensure_template(p: &std::path::Path) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(d) = p.parent() {
        std::fs::create_dir_all(d)?;
    }
    match std::fs::OpenOptions::new().write(true).create_new(true).open(p) {
        Ok(mut f) => f.write_all(settings::template().as_bytes()),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(e) => Err(e),
    }
}

/// 给前端补全用的键定义（`settings::DEFS` 原样转出去）
pub fn schema() -> Vec<crate::commands::SettingDefDto> {
    settings::DEFS
        .iter()
        .map(|d| {
            let (kind, min, max) = match d.kind {
                settings::Kind::Str { .. } => ("string", None, None),
                settings::Kind::Int { min, max } => ("integer", Some(min), Some(max)),
            };
            crate::commands::SettingDefDto {
                key: d.key.into(),
                kind: kind.into(),
                default: d.default.into(),
                doc: d.doc.into(),
                min,
                max,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 设置文件_不存在才建模板_已经有了一个字节都不碰() {
        let d = std::env::temp_dir().join(format!("lite-ide-settings-tpl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let p = d.join("sub/settings.json");
        ensure_template(&p).unwrap();
        assert_eq!(std::fs::read_to_string(&p).unwrap(), settings::template(), "不存在：建出模板（目录也建出来）");
        std::fs::write(&p, "{ \"editor.fontSize\": 15 } // 我写的").unwrap();
        ensure_template(&p).unwrap();
        let after = std::fs::read_to_string(&p).unwrap();
        let _ = std::fs::remove_dir_all(&d);
        assert_eq!(after, "{ \"editor.fontSize\": 15 } // 我写的", "已经有了：不能换成模板");
    }
}
