//! Tauri 命令层。
//!
//! 纪律（ARCHITECTURE.md §5）：这里只做参数解包、句柄查找和错误转换，
//! 一行业务逻辑都不写 —— 业务全在 crates/logengine 里，才能脱离 Tauri 单测和 bench。

use crate::state::AppState;
use logengine::{FilterSpec, Level, LevelMask, LogFile, Refreshed};
use tauri::ipc::Response;
use tauri::State;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenResult {
    pub handle: u32,
    pub name: String,
    pub size: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StatDto {
    pub line_count: u64,
    pub indexed_bytes: u64,
    pub total_bytes: u64,
    pub complete: bool,
    pub index_bytes: u64,
    /// 各级别行数，顺序同 Level：error/warn/info/debug/trace/other
    pub levels: [u64; Level::COUNT],
    pub levels_complete: bool,
    pub levels_scanned: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FilterStatDto {
    /// 命中条数
    pub hits: u64,
    pub complete: bool,
    pub scanned_lines: u64,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshDto {
    /// "none" | "grew" | "rotated"
    pub kind: &'static str,
    pub new_lines: u64,
    pub line_count: u64,
}

/// 打开日志文件。mmap 是 O(1) 的，此调用不读盘，立即返回。
///
/// TODO(M2)：接入编辑/日志双模式分流（判据见 ARCHITECTURE.md §1 修正 01）。
#[tauri::command]
pub fn open_log(path: String, state: State<'_, AppState>) -> Result<OpenResult, String> {
    crate::diag!("open_log path={path}");
    let file = LogFile::open(&path).map_err(|e| format!("打不开 {path}：{e}"))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let size = file.size();
    Ok(OpenResult {
        handle: state.insert(file),
        name,
        size,
    })
}

/// 索引与级别扫描的进度快照。两者是并行的后台任务，各自独立完成。
#[tauri::command]
pub fn log_stat(handle: u32, state: State<'_, AppState>) -> Result<StatDto, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    let s = file.stat();
    Ok(StatDto {
        line_count: s.line_count,
        indexed_bytes: s.indexed_bytes,
        total_bytes: s.total_bytes,
        complete: s.complete,
        index_bytes: s.index_bytes,
        levels: s.levels.as_array(),
        levels_complete: s.levels_complete,
        levels_scanned: s.levels_scanned,
    })
}

/// 读取一段行，返回二进制块而非 JSON —— 这是 60fps 的前提（ARCHITECTURE.md §3.4）。
#[tauri::command]
pub fn log_lines(handle: u32, start: u64, count: u32, state: State<'_, AppState>) -> Response {
    match state.get(handle) {
        Some(file) => Response::new(file.read_block(start, count)),
        // 句柄失效时返回空块：滚动路径上不该因为一次竞态就抛异常
        None => Response::new(logengine::block::encode(start, &[])),
    }
}

/// 启动过滤。传空 pattern + 全级别掩码等于清除过滤。
#[tauri::command]
pub fn log_filter(
    handle: u32,
    level_bits: u8,
    pattern: String,
    case_sensitive: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    let spec = FilterSpec {
        levels: LevelMask::from_bits(level_bits),
        pattern,
        case_sensitive,
    };
    if spec.is_noop() {
        state.clear_filter(handle);
        return Ok(false);
    }
    let task = file
        .start_filter(spec)
        .map_err(|e| format!("过滤启动失败：{e}"))?;
    state.set_filter(handle, task);
    Ok(true)
}

#[tauri::command]
pub fn log_filter_stat(handle: u32, state: State<'_, AppState>) -> Option<FilterStatDto> {
    let task = state.filter(handle)?;
    Some(FilterStatDto {
        hits: task.hit_count(),
        complete: task.is_complete(),
        scanned_lines: task.scanned_lines(),
    })
}

/// 按过滤结果读取 —— 视图行 `[start, start+count)` 映射到物理行再回表。
#[tauri::command]
pub fn log_lines_filtered(
    handle: u32,
    start: u64,
    count: u32,
    state: State<'_, AppState>,
) -> Response {
    let empty = || Response::new(logengine::block::encode(start, &[]));
    let (Some(file), Some(task)) = (state.get(handle), state.filter(handle)) else {
        return empty();
    };
    let hits = task.hits();
    let from = (start as usize).min(hits.len());
    let to = (from + count as usize).min(hits.len());
    if from >= to {
        return empty();
    }
    Response::new(file.read_block_at(&hits[from..to]))
}

/// 视图行号 → 物理行号，供过滤态下显示真实行号。
#[tauri::command]
pub fn log_filter_map(handle: u32, start: u64, count: u32, state: State<'_, AppState>) -> Vec<u64> {
    let Some(task) = state.filter(handle) else {
        return Vec::new();
    };
    let hits = task.hits();
    let from = (start as usize).min(hits.len());
    let to = (from + count as usize).min(hits.len());
    hits[from..to].to_vec()
}

/// tail 轮询：检查文件是否有追加。
///
/// 用轮询而非 notify：macOS 的 FSEvents 对单文件有秒级合并延迟，
/// 500ms 轮询反而更快更可控，也少一个依赖。
#[tauri::command]
pub fn log_refresh(handle: u32, state: State<'_, AppState>) -> Result<RefreshDto, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    let r = file.refresh().map_err(|e| format!("刷新失败：{e}"))?;
    let (kind, new_lines) = match r {
        Refreshed::NoChange => ("none", 0),
        Refreshed::Grew { new_lines } => ("grew", new_lines),
        Refreshed::Rotated => ("rotated", 0),
    };
    Ok(RefreshDto {
        kind,
        new_lines,
        line_count: file.stat().line_count,
    })
}

#[tauri::command]
pub fn close_log(handle: u32, state: State<'_, AppState>) -> bool {
    state.close(handle)
}

/// 启动参数里带的文件路径，供 `lite-ide foo.log` 直接打开。
#[tauri::command]
pub fn initial_file() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let found = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && std::path::Path::new(a).is_file())
        .cloned();
    crate::diag!("initial_file -> {found:?}");
    found
}

/// 前端把执行轨迹与 JS 错误报回来。release 没有 devtools，
/// 这是 WebView 里唯一的可观测通道。默认静默，`LITE_IDE_DEBUG=1` 打开。
#[tauri::command]
pub fn diag(msg: String) {
    if crate::diag::enabled() {
        eprintln!("[diag/web] {msg}");
    }
}
