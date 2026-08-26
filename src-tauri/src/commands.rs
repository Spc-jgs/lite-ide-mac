//! Tauri 命令层。
//!
//! 纪律（ARCHITECTURE.md §5）：这里只做参数解包、句柄查找和错误转换，
//! 一行业务逻辑都不写 —— 业务全在 crates/logengine 里，才能脱离 Tauri 单测和 bench。

use crate::state::AppState;
use logengine::{FilterSpec, Level, LevelMask, LogFile, Refreshed};
use std::path::Path;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PathInfo {
    /// "file" | "dir"
    pub kind: &'static str,
    /// "edit" | "log"，kind == "dir" 时无意义
    pub mode: &'static str,
    pub path: String,
    pub name: String,
    pub size: u64,
    /// 判为 log 模式的原因，用于界面上说明「为什么这个文件是只读的」
    pub reason: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DirEntryDto {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub size: u64,
}

/// 探测一个路径：是目录还是文件，文件该用哪种模式打开。
///
/// 判据是复合的（ARCHITECTURE.md §1 修正 01）：大小 / 行数 / 最长行任一超标都走
/// 日志模式。只读文件头部采样，不加载全文。
#[tauri::command]
pub fn probe_path(path: String) -> Result<PathInfo, String> {
    let p = Path::new(&path);
    let name = p
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let meta = std::fs::metadata(p).map_err(|e| format!("读不到 {path}：{e}"))?;

    if meta.is_dir() {
        return Ok(PathInfo {
            kind: "dir",
            mode: "edit",
            path,
            name,
            size: 0,
            reason: String::new(),
        });
    }

    let pr = logengine::probe(p).map_err(|e| format!("探测 {path} 失败：{e}"))?;
    crate::diag!("probe {path} -> {:?} ({})", pr.mode, pr.reason);
    Ok(PathInfo {
        kind: "file",
        mode: pr.mode.as_str(),
        path,
        name,
        size: pr.size,
        reason: pr.reason.to_string(),
    })
}

/// 列一层目录。不递归 —— 文件树按需展开，大仓库才不会卡。
#[tauri::command]
pub fn list_dir(path: String, show_hidden: bool) -> Result<Vec<DirEntryDto>, String> {
    let entries =
        fsservice::list_dir(&path, show_hidden).map_err(|e| format!("列目录失败：{e}"))?;
    Ok(entries
        .into_iter()
        .map(|e| DirEntryDto {
            name: e.name,
            path: e.path.to_string_lossy().into_owned(),
            is_dir: e.is_dir,
            size: e.size,
        })
        .collect())
}

/// 编辑模式读取全文。非 UTF-8 会被明确拒绝（见 fsservice 模块注释）。
#[tauri::command]
pub fn read_text(path: String) -> Result<String, String> {
    fsservice::read_text(&path).map_err(|e| format!("{e}"))
}

#[derive(serde::Serialize, Clone, Copy)]
#[serde(rename_all = "camelCase")]
pub struct StampDto {
    pub mtime_ms: u64,
    pub size: u64,
}

/// 取文件指纹，用于判断是否被外部改动过。
#[tauri::command]
pub fn file_stamp(path: String) -> Result<StampDto, String> {
    let s = fsservice::stamp(&path).map_err(|e| format!("{e}"))?;
    Ok(StampDto {
        mtime_ms: s.mtime_ms,
        size: s.size,
    })
}

/// 保存。先写临时文件再原子替换，中途崩溃不会留下半个文件。
///
/// 返回写入后的指纹 —— 前端必须拿它更新记录，否则自己的保存
/// 会在下一次检查时被当成"外部修改"。
#[tauri::command]
pub fn write_text(path: String, content: String) -> Result<StampDto, String> {
    fsservice::write_text(&path, &content).map_err(|e| format!("保存失败：{e}"))?;
    let s = fsservice::stamp(&path).map_err(|e| format!("{e}"))?;
    Ok(StampDto {
        mtime_ms: s.mtime_ms,
        size: s.size,
    })
}

/// 打开日志文件。mmap 是 O(1) 的，此调用不读盘，立即返回。
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
    collapse_stacks: bool,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    let spec = FilterSpec {
        levels: LevelMask::from_bits(level_bits),
        pattern,
        case_sensitive,
        collapse_stacks,
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

/// 启动参数里带的路径，供 `lite-ide foo.log` 或 `lite-ide ~/proj` 直接打开。
///
/// 文件和目录都接受：目录会成为项目根，文件则打开并把父目录当根。
/// 早先只认 `is_file()`，`lite-ide <目录>` 静默什么都不做。
#[tauri::command]
pub fn initial_path() -> Option<String> {
    let args: Vec<String> = std::env::args().collect();
    let found = args
        .iter()
        .skip(1)
        .find(|a| !a.starts_with('-') && Path::new(a).exists())
        .cloned();
    crate::diag!("initial_path -> {found:?}");
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

// ─────────────────────────── 终端 ───────────────────────────

/// 起一个终端。输出走 Channel 流式回传，不经 JSON 数组。
///
/// 用真 pty 而不是模拟 shell —— vim / less / gradle 进度条全靠 pty 的行为。
#[tauri::command]
pub fn pty_spawn(
    cwd: String,
    cols: u16,
    rows: u16,
    on_data: tauri::ipc::Channel<Vec<u8>>,
    state: State<'_, AppState>,
) -> Result<u32, String> {
    use std::io::Read;

    let (sess, mut reader) =
        ptysvc::Session::spawn(&cwd, cols, rows).map_err(|e| format!("终端起不来：{e}"))?;
    let id = state.insert_pty(sess);
    crate::diag!("pty_spawn id={id} cwd={cwd}");

    std::thread::Builder::new()
        .name(format!("pty-read-{id}"))
        .spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match reader.read(&mut buf) {
                    // EOF：shell 退出了
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // 前端已经关掉这个终端时 send 会失败，正常收摊
                        if on_data.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        })
        .map_err(|e| format!("读线程起不来：{e}"))?;

    Ok(id)
}

#[tauri::command]
pub fn pty_write(id: u32, data: String, state: State<'_, AppState>) -> Result<(), String> {
    let sess = state.pty(id).ok_or("终端已关闭")?;
    // 先落成局部变量，让 MutexGuard 在本语句结束时就释放；
    // 直接把链式表达式当返回值会让 guard 活过 sess，借用检查不过
    let r = sess
        .lock()
        .expect("pty 锁被毒化")
        .write_input(data.as_bytes());
    r.map_err(|e| format!("写入失败：{e}"))
}

#[tauri::command]
pub fn pty_resize(id: u32, cols: u16, rows: u16, state: State<'_, AppState>) -> Result<(), String> {
    let sess = state.pty(id).ok_or("终端已关闭")?;
    let r = sess.lock().expect("pty 锁被毒化").resize(cols, rows);
    r.map_err(|e| format!("调整尺寸失败：{e}"))
}

#[tauri::command]
pub fn pty_kill(id: u32, state: State<'_, AppState>) -> bool {
    crate::diag!("pty_kill id={id}");
    state.kill_pty(id)
}

/// 终端是否已自行退出（用户敲了 exit）
#[tauri::command]
pub fn pty_alive(id: u32, state: State<'_, AppState>) -> bool {
    match state.pty(id) {
        Some(sess) => sess.lock().expect("pty 锁被毒化").try_wait().is_none(),
        None => false,
    }
}

// ─────────────────────────── 搜索 ───────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HitDto {
    pub path: String,
    pub line: u64,
    pub text: String,
}

/// 列出项目里的文件（相对路径），供前端做模糊匹配。
///
/// 匹配放前端做是有意为之：每敲一个字符都往 Rust 跑一趟，IPC 往返会让输入发木。
/// 几万条路径传过去也就几 MB。
#[tauri::command]
pub fn list_project_files(root: String) -> Result<Vec<String>, String> {
    searchsvc::list_files(&root).map_err(|e| format!("索引项目失败：{e}"))
}

/// 全局内容搜索。有 rg 用 rg，没有就用进程内实现，两者结果一致。
#[tauri::command]
pub fn grep_project(root: String, pattern: String, limit: usize) -> Result<Vec<HitDto>, String> {
    let hits = searchsvc::grep(&root, &pattern, limit).map_err(|e| format!("搜索失败：{e}"))?;
    Ok(hits
        .into_iter()
        .map(|h| HitDto {
            path: h.path,
            line: h.line,
            text: h.text,
        })
        .collect())
}

/// 界面上标注当前走的哪条搜索路径
#[tauri::command]
pub fn ripgrep_available() -> bool {
    searchsvc::ripgrep_available()
}
