//! 断点调试（docs/IDEA.md 第 18 节）。业务在 `crate::debug`，DAP 的线在 `dapsvc`。
//! 全部 async + 阻塞池：要等调试插件回话（展开变量要在目标 JVM 里调 `toString()`，慢的几秒）。

use super::*;
use tauri::Manager;

fn session(app: &tauri::AppHandle, run: u32) -> Result<std::sync::Arc<crate::debug::Dbg>, String> {
    app.state::<AppState>().debug.get(run).ok_or_else(|| "这次运行没在调试".into())
}

/// 这次运行的调试会话此刻什么样（运行窗晚挂上的时候问一句）。不在调试给 null
#[tauri::command]
pub async fn debug_state(app: tauri::AppHandle, run: u32) -> Result<Option<DebugStateDto>, String> {
    blocking(move || Ok(app.state::<AppState>().debug.get(run).map(|d| DebugStateDto::of(run, &d.snapshot())))).await
}

/// 这个文件的断点换成 `lines`（整份，从 1 数）。正在调试的话当场下到 JVM 里，回「对上了」的那些行
#[tauri::command]
pub async fn debug_breakpoints(app: tauri::AppHandle, root: String, path: String, lines: Vec<u32>) -> Result<Vec<u32>, String> {
    blocking(move || Ok(app.state::<AppState>().debug.set_breakpoints(&root, &path, lines))).await
}

/// continue / next / stepIn / stepOut / pause
#[tauri::command]
pub async fn debug_step(app: tauri::AppHandle, run: u32, action: String) -> Result<(), String> {
    blocking(move || session(&app, run)?.step(&action)).await
}

#[tauri::command]
pub async fn debug_thread(app: tauri::AppHandle, run: u32, thread: i64) -> Result<DebugStateDto, String> {
    blocking(move || Ok(DebugStateDto::of(run, &session(&app, run)?.select_thread(thread)?))).await
}

/// 一帧的局部变量（给 `frame`），或者一个变量的下一层（给 `reference`，数组 / 集合带上 `indexed`）
#[tauri::command]
pub async fn debug_vars(app: tauri::AppHandle, run: u32, frame: Option<i64>, reference: Option<i64>, indexed: u32) -> Result<DebugVarsDto, String> {
    blocking(move || {
        let (vars, more) = session(&app, run)?.variables(frame, reference, indexed)?;
        Ok(DebugVarsDto { vars: vars.into_iter().map(Into::into).collect(), more: more as u32 })
    })
    .await
}

#[tauri::command]
pub async fn debug_eval(app: tauri::AppHandle, run: u32, frame: i64, expr: String) -> Result<DebugVarDto, String> {
    blocking(move || Ok(session(&app, run)?.evaluate(frame, &expr)?.into())).await
}
