//! 日志引擎：打开、统计、按行取、过滤、按时间跳、刷新、关闭。

use super::*;

/// 打开日志文件。mmap 是 O(1) 的，此调用不读盘，立即返回。
#[tauri::command]
pub fn open_log(path: String, window: tauri::Window, state: State<'_, AppState>) -> Result<OpenResult, String> {
    crate::diag!("open_log path={path}");
    let file = LogFile::open(&path).map_err(|e| format!("打不开 {path}：{e}"))?;
    let name = std::path::Path::new(&path)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.clone());
    let size = file.size();
    Ok(OpenResult {
        handle: state.insert(window.label(), file),
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
///
/// `pattern` 是过滤框里那一串原文，语法在 `logengine::query`（空格 AND、`-` 排除、`/re/`
/// 正则，和前端 `logview/query.ts` 同一套）。这里切开之后：
///
/// - 字面量按 `label`（文件编码）**编成文件那套字节**再下去搜 —— 一份 GBK 日志里搜
///   「订单」，拿 UTF-8 的「订单」去比对是永远搜不到的。编码只有这一层知道。
/// - 正则用 `regex::bytes` 编译，大小写跟过滤条那个开关。**正则不做编码转换**：非 ASCII
///   的正则在 GBK 文件里对不上（多字节序列里可能夹着 ASCII 的元字符），编译时按文件是不是
///   UTF-8 决定要不要 Unicode 语义 —— 非 UTF-8 文件里 `.` 只吃一个字节。写坏的正则报错
///   回前端，整条过滤不跑：比静默当没这条要诚实。
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn log_filter(
    handle: u32,
    level_bits: u8,
    pattern: String,
    case_sensitive: bool,
    collapse_stacks: bool,
    label: Option<String>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    let label = label.unwrap_or_else(|| "UTF-8".into());
    let q = logengine::query::parse(&pattern);
    let utf8 = label.eq_ignore_ascii_case("utf-8") || label.eq_ignore_ascii_case("utf8");
    let mut text = logengine::TextFilter::default();
    for (term, neg) in q.include.iter().map(|t| (t, false)).chain(q.exclude.iter().map(|t| (t, true))) {
        match term {
            logengine::query::Term::Lit(s) => {
                let bytes = fsservice::encoding::encode(s, &label, false);
                if neg { text.exclude.push(bytes) } else { text.include.push(bytes) }
            }
            logengine::query::Term::Re(src) => {
                let re = logengine::TextFilter::compile_re(src, !case_sensitive, utf8)?;
                if neg { text.exclude_re.push(re) } else { text.include_re.push(re) }
            }
        }
    }
    let spec = FilterSpec {
        levels: LevelMask::from_bits(level_bits),
        text,
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

/// 跳到时间（`logengine::seek`）：第一条时间 ≥ `query` 的行号；文件里认不出时间戳就 None。
/// `near` 是视口顶上那一行，输入不带日期时拿它附近的日期补。二分只有几十次探测，但每次
/// 都可能在 mmap 上缺页 —— 碰盘的一律走阻塞池（rust.md）。
#[tauri::command]
pub async fn log_seek_time(handle: u32, query: String, near: u64, state: State<'_, AppState>) -> Result<Option<u64>, String> {
    let file = state.get(handle).ok_or("句柄已失效")?;
    blocking(move || Ok(file.seek_time(&query, near))).await
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
        Refreshed::Rotated => {
            // 按名重开、句柄不变（`AppState::reopen`）。改名和新建之间那个空档
            // 会 NotFound：报错回去，前端的 tail 轮询把它当「临时不可读」下一轮再试，
            // 表里的旧文件原样留着 —— 正是 `tail -F` 等新文件出现的那段
            state.reopen(handle).map_err(|e| format!("轮转后重开失败：{e}"))?;
            ("rotated", 0)
        }
    };
    // 轮转过就问新那份的行数，旧 `file` 那个 Arc 还指着换掉的文件
    let file = state.get(handle).ok_or("句柄已失效")?;
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
