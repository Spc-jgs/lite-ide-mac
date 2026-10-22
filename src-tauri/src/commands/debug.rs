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

/// 这个文件的断点换成 `lines`（整份，从 1 数）；`conds` 和 `lines` 一一对应（条件断点的条件，空串是普通断点；不给就都是普通的）。
/// `hits` / `logs` 同样一一对应（命中次数、日志断点要打的那句；不给就都没有）。停用的断点前端不交过来。
/// 正在调试的话当场下到 JVM 里，回「对上了」的那些行
#[tauri::command]
pub async fn debug_breakpoints(
    app: tauri::AppHandle,
    root: String,
    path: String,
    lines: Vec<u32>,
    conds: Option<Vec<String>>,
    hits: Option<Vec<String>>,
    logs: Option<Vec<String>>,
) -> Result<Vec<u32>, String> {
    blocking(move || {
        let (conds, hits, logs) = (conds.unwrap_or_default(), hits.unwrap_or_default(), logs.unwrap_or_default());
        let at = |v: &Vec<String>, i: usize| v.get(i).cloned().unwrap_or_default();
        let bps = lines.into_iter().enumerate().map(|(i, line)| crate::debug::Bp { line, condition: at(&conds, i), hit: at(&hits, i), log: at(&logs, i) }).collect();
        Ok(app.state::<AppState>().debug.set_breakpoints(&root, &path, bps))
    })
    .await
}

/// 异常断点（docs/TAKEOVER.md G14）：未捕获的停 / 被捕获的也停 / 只看这几种（类的全名，空 = 全部）。正在调试的会话当场跟着改
#[tauri::command]
pub async fn debug_exceptions(app: tauri::AppHandle, root: String, uncaught: bool, caught: bool, types: Vec<String>) -> Result<(), String> {
    blocking(move || {
        app.state::<AppState>().debug.set_exceptions(&root, crate::debug::Exc { uncaught, caught, types });
        Ok(())
    })
    .await
}

/// 「重新加载改过的类」（docs/IDEA.md 第 26 节）：构建成功后前端自动调，菜单里也能点。只动这个窗口的会话
#[tauri::command]
pub async fn debug_hotswap(window: tauri::Window, app: tauri::AppHandle, root: String) -> Result<DebugHotswapDto, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(DebugHotswapDto::of(app.state::<AppState>().debug.hotswap(&owner, &root)))).await
}

/// continue / next / stepIn / stepOut / pause
#[tauri::command]
pub async fn debug_step(app: tauri::AppHandle, run: u32, action: String) -> Result<(), String> {
    blocking(move || session(&app, run)?.step(&action)).await
}

/// 运行到光标（⌥F9）：停着时跑到这个文件的这一行（从 1 数）再停
#[tauri::command]
pub async fn debug_run_to(app: tauri::AppHandle, run: u32, path: String, line: u32) -> Result<(), String> {
    blocking(move || app.state::<AppState>().debug.run_to(run, &path, line)).await
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
