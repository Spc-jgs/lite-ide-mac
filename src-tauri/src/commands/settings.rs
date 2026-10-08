//! 设置（issue #44）：取、补全用的定义、改界面状态、迁移旧偏好。业务全在 `settings.rs` / `settingsctl.rs`。

use super::*;

/// 当前生效的设置。**只读内存**（启动时 `setup` 里就读好了），所以留在主线程上 ——
/// 前端挂载之前就要 `await` 它（第 0 步实测一次往返 < 1ms），挪去阻塞池只会多一次调度
#[tauri::command]
pub fn settings(app: tauri::AppHandle) -> SettingsDto {
    crate::settingsctl::dto(&app)
}

#[tauri::command]
pub fn settings_schema() -> Vec<SettingDefDto> {
    crate::settingsctl::schema()
}

/// 改一个开关（缩略图、文件树紧凑 / 跟随、Git 分组）。碰盘（存 `ui-state.json`），走阻塞池
#[tauri::command]
pub async fn set_ui_state(app: tauri::AppHandle, key: String, value: bool) -> Result<SettingsDto, String> {
    blocking(move || crate::settingsctl::update_ui(&app, |u, _| u.set(&key, value))).await
}

/// ⌘= / ⌘- / ⌘0：`delta` 是 None 就回到 `settings.json` 里写的字号。
/// 在 Rust 这边按当前值加减，不由前端传绝对值 —— 两个窗口同时按，前端各自的旧值会互相盖掉
#[tauri::command]
pub async fn step_font(app: tauri::AppHandle, delta: Option<i64>) -> Result<SettingsDto, String> {
    blocking(move || crate::settingsctl::update_ui(&app, |u, base| Ok(u.step_font(base, delta)))).await
}

/// 升级后第一次启动：前端把 localStorage 里的 5 个旧偏好交过来（没存过的是 None）。
/// 收不收由 `Store::adopt_legacy` 定：`ui-state.json` 原来就有、或者别的窗口已经交过，就不收
#[tauri::command]
pub async fn adopt_ui_state(
    app: tauri::AppHandle,
    minimap: Option<bool>,
    tree_compact: Option<bool>,
    tree_follow: Option<bool>,
    git_grouped: Option<bool>,
    editor_font: Option<i64>,
) -> Result<SettingsDto, String> {
    let legacy = crate::settings::UiState::from_legacy(minimap, tree_compact, tree_follow, git_grouped, editor_font);
    blocking(move || Ok(crate::settingsctl::adopt_legacy(&app, legacy))).await
}

/// 「设置…」：`settings.json` 不存在就建模板，返回路径（前端开一个标签）。碰盘，走阻塞池
#[tauri::command]
pub async fn open_settings(app: tauri::AppHandle) -> Result<String, String> {
    blocking(move || crate::settingsctl::open_settings(&app)).await
}
