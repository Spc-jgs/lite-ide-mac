//! 自动更新（docs/TAKEOVER.md MVP-8）：状态、手动查、重启更新。什么时候查、怎么换在 `crate::update`，读清单 / 验签 / 换 .app 在 `updsvc`。

use super::*;

pub fn update_dto(app: &tauri::AppHandle) -> UpdateDto {
    let s = crate::update::status();
    let phase = match s.phase {
        crate::update::Phase::Idle => "idle",
        crate::update::Phase::Checking => "checking",
        crate::update::Phase::Downloading => "downloading",
        crate::update::Phase::Ready => "ready",
        crate::update::Phase::Latest => "latest",
        crate::update::Phase::Failed => "failed",
        crate::update::Phase::Unsupported => "unsupported",
    };
    UpdateDto { phase: phase.into(), version: s.version, current: app.package_info().version.to_string(), notes: s.notes, error: s.error }
}

/// 现在的状态（前端起来时问一次，之后听 `update-status`）
#[tauri::command]
pub fn update_status(app: tauri::AppHandle) -> UpdateDto {
    update_dto(&app)
}

/// 菜单「检查更新…」：查一次，有新的就下好、验签。走网络，放阻塞池
#[tauri::command]
pub async fn update_check(app: tauri::AppHandle) -> Result<UpdateDto, String> {
    blocking(move || {
        crate::update::check(&app);
        Ok(update_dto(&app))
    })
    .await
}

/// 「重启更新」：换 .app（碰盘，放阻塞池），成了就照 ⌘Q 的路退出、退出后起新的
#[tauri::command]
pub async fn update_apply(app: tauri::AppHandle) -> Result<(), String> {
    blocking(move || crate::update::apply(&app)).await
}
