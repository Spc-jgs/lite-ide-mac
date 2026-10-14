//! 日志引擎：打开、统计、按行取、过滤、按时间跳、刷新、关闭。

use super::*;
use tauri::Manager;

/// 打开日志文件。mmap 是 O(1) 的、不读内容 —— 但 open / mmap 本身是系统调用，**日志在网络卷、外接盘上时能卡几秒**，
/// 所以走阻塞池，不在主线程上（rust.md「同步命令跑在主线程上」；2026-10-10 整体审核时挪的，原来在主线程上）
#[tauri::command]
pub async fn open_log(path: String, window: tauri::Window, app: tauri::AppHandle) -> Result<OpenResult, String> {
    let owner = window.label().to_string();
    blocking(move || {
        crate::diag!("open_log path={path}");
        let file = LogFile::open(&path).map_err(|e| format!("打不开 {path}：{e}"))?;
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let size = file.size();
        Ok(OpenResult { handle: app.state::<AppState>().insert(&owner, file), name, size })
    })
    .await
}

/// 索引与级别扫描的进度快照。两者是并行的后台任务，各自独立完成。
#[tauri::command]
pub fn log_stat(handle: u32, state: State<'_, AppState>) -> Result<StatDto, String> {
    let s = match state.merged(handle) {
        Some(m) => m.stat(),
        None => state.get(handle).ok_or("句柄已失效")?.stat(),
    };
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
    if let Some(m) = state.merged(handle) {
        return Response::new(m.read_block(start, count));
    }
    match state.get(handle) {
        Some(file) => Response::new(file.read_block(start, count)),
        // 句柄失效时返回空块：滚动路径上不该因为一次竞态就抛异常
        None => Response::new(logengine::block::encode(start, &[])),
    }
}

/// 启动过滤。传空 pattern + 全级别掩码等于清除过滤。
///
/// `sources` 只对合并视图有意义：只看哪几份（第 i 位 = 第 i 份），不传就是全看。
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
    sources: Option<u32>,
    state: State<'_, AppState>,
) -> Result<bool, String> {
    let merged = state.merged(handle);
    let file = if merged.is_some() { None } else { Some(state.get(handle).ok_or("句柄已失效")?) };
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
    let task = match (&merged, &file) {
        (Some(m), _) => {
            let all = (1u32 << m.sources().len()) - 1;
            let mask = sources.unwrap_or(u32::MAX) & all;
            if spec.is_noop() && mask == all {
                state.clear_filter(handle);
                return Ok(false);
            }
            m.start_filter(spec, mask)
        }
        (None, Some(f)) => {
            if spec.is_noop() {
                state.clear_filter(handle);
                return Ok(false);
            }
            f.start_filter(spec)
        }
        (None, None) => unreachable!("上面已经按句柄查过"),
    }
    .map_err(|e| format!("过滤启动失败：{e}"))?;
    state.set_filter(handle, task);
    Ok(true)
}

/// 跳到时间（`logengine::seek`）：第一条时间 ≥ `query` 的行号；文件里认不出时间戳就 None。
/// `near` 是视口顶上那一行，输入不带日期时拿它附近的日期补。二分只有几十次探测，但每次
/// 都可能在 mmap 上缺页 —— 碰盘的一律走阻塞池（rust.md）。
#[tauri::command]
pub async fn log_seek_time(handle: u32, query: String, near: u64, state: State<'_, AppState>) -> Result<Option<u64>, String> {
    if let Some(m) = state.merged(handle) {
        return blocking(move || Ok(m.seek_time(&query, near))).await;
    }
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
    let Some(task) = state.filter(handle) else {
        return empty();
    };
    let hits = task.hits();
    let from = (start as usize).min(hits.len());
    let to = (from + count as usize).min(hits.len());
    if from >= to {
        return empty();
    }
    if let Some(m) = state.merged(handle) {
        return Response::new(m.read_block_at(&hits[from..to]));
    }
    match state.get(handle) {
        Some(file) => Response::new(file.read_block_at(&hits[from..to])),
        None => empty(),
    }
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
///
/// **走阻塞池**：它不是内存操作 —— 每次 stat 一下文件，长了要重开、重映射、增量索引（还复制一份级别表，
/// 1000 万行约 5MB），轮转了要按名重开。日志在网络卷上时 stat 就能卡几秒，而它每 500ms 来一次（2026-10-10 整体审核时挪的）。
/// 多一次线程调度对一个 500ms 的轮询无所谓；前端那边保证同一个句柄一次只有一个在跑（LogPane 的 `refreshing`）
#[tauri::command]
pub async fn log_refresh(handle: u32, app: tauri::AppHandle) -> Result<RefreshDto, String> {
    blocking(move || refresh_now(handle, &app.state::<AppState>())).await
}

fn refresh_now(handle: u32, state: &AppState) -> Result<RefreshDto, String> {
    // 合并视图不跟 tail（v1：从服务器拉下来的日志是静的；跟的话每份长了都要接着归并，见 docs/MERGE.md「没做的」）
    if let Some(m) = state.merged(handle) {
        return Ok(RefreshDto { kind: "none", new_lines: 0, line_count: m.stat().line_count });
    }
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

/// 合并视图的图例：每份的名字、时间戳精度、偏移
fn merge_info(handle: u32, m: &logengine::merge::MergedLog) -> MergeInfoDto {
    let sources: Vec<MergeSourceDto> = m
        .sources()
        .into_iter()
        .map(|s| MergeSourceDto {
            name: s.path.file_name().map_or_else(|| s.path.to_string_lossy().into_owned(), |n| n.to_string_lossy().into_owned()),
            path: s.path.to_string_lossy().into_owned(),
            clock: s.clock.as_str(),
            offset_ms: s.offset_ms,
        })
        .collect();
    MergeInfoDto {
        handle,
        sources,
        fill_date: m.fill_date().map(|(y, mo, d)| format!("{y:04}-{mo:02}-{d:02}")),
        size: m.stat().total_bytes,
    }
}

/// 几份日志按时间合成一条（`logengine::merge`，docs/MERGE.md）。每份都是 open + mmap —— 碰盘，走阻塞池（同 `open_log`）
#[tauri::command]
pub async fn open_merged(paths: Vec<String>, window: tauri::Window, app: tauri::AppHandle) -> Result<MergeInfoDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        crate::diag!("open_merged {} 份", paths.len());
        let m = logengine::merge::MergedLog::open(&paths).map_err(|e| format!("合并不了：{e}"))?;
        let st = app.state::<AppState>();
        let handle = st.insert_merged(&owner, m);
        let m = st.merged(handle).ok_or("句柄已失效")?;
        Ok(merge_info(handle, &m))
    })
    .await
}

/// 一段行各自来自哪份、是那份的第几行。`filtered` 时 `[start, start + count)` 是第几条命中（同 `log_lines_filtered`）
#[tauri::command]
pub fn merge_rows(handle: u32, start: u64, count: u32, filtered: bool, state: State<'_, AppState>) -> MergeRowsDto {
    let empty = || MergeRowsDto { src: Vec::new(), line: Vec::new() };
    let Some(m) = state.merged(handle) else { return empty() };
    let (src, line) = if filtered {
        let Some(task) = state.filter(handle) else { return empty() };
        let hits = task.hits();
        let from = (start as usize).min(hits.len());
        let to = (from + count as usize).min(hits.len());
        m.rows_meta_at(&hits[from..to])
    } else {
        m.rows_meta(start, count)
    };
    MergeRowsDto { src, line }
}

/// 改时钟偏移（毫秒，每份一个），整份重新归并。行号全变了：过滤清掉，前端丢缓存、按原条件重跑
#[tauri::command]
pub async fn merge_set_offsets(handle: u32, offsets: Vec<i64>, app: tauri::AppHandle) -> Result<MergeInfoDto, String> {
    blocking(move || {
        let st = app.state::<AppState>();
        let m = st.merged(handle).ok_or("句柄已失效")?;
        st.clear_filter(handle);
        m.set_offsets(offsets).map_err(|e| format!("改不了偏移：{e}"))?;
        Ok(merge_info(handle, &m))
    })
    .await
}

/// 日志归类（`logengine::pattern`，docs/PATTERNS.md）：这个句柄第一次问时在后台起，之后返回它当前的快照（边归边看）。
/// `restart` 是「重新归类」（日志长了）。只起一个线程、拷一份前 `limit` 类 —— 扫文件在那个线程里，所以可以留在主线程（同 `log_filter`）
#[tauri::command]
pub fn log_patterns(handle: u32, limit: u32, restart: bool, state: State<'_, AppState>) -> Result<PatternsDto, String> {
    let maps = match state.merged(handle) {
        Some(m) => m.maps(),
        None => vec![state.get(handle).ok_or("句柄已失效")?.mapped()],
    };
    let task = state.patterns(handle, restart, || logengine::pattern::spawn(maps)).map_err(|e| format!("归类起不来：{e}"))?;
    Ok(patterns_dto(&task, limit))
}

fn patterns_dto(task: &logengine::pattern::PatternTask, limit: u32) -> PatternsDto {
    let s = task.stat();
    PatternsDto {
        patterns: s
            .top
            .iter()
            .take(limit as usize)
            .map(|p| PatternDto { template: p.template.clone(), count: p.count, base: p.base, levels: p.levels })
            .collect(),
        total: s.patterns,
        lines: s.lines,
        base_lines: s.base_lines,
        overflow: s.overflow,
        scanned_bytes: s.scanned_bytes,
        total_bytes: s.total_bytes,
        done: s.done,
    }
}

/// 日志归类的对比（docs/PATTERNS.md）：这个句柄（今天的）和 `base` 那几份（昨天的、上线前的）放进同一棵树里一起归，
/// 每一类两边各几次，按「新出现 / 没了 / 变化幅度」排。基准换了就重来，同一个基准再问就是边归边看的快照。
/// 要打开基准那几份（open + mmap 碰盘），走阻塞池
#[tauri::command]
pub async fn log_patterns_compare(handle: u32, base: Vec<String>, limit: u32, restart: bool, app: tauri::AppHandle) -> Result<PatternsDto, String> {
    blocking(move || {
        let state = app.state::<AppState>();
        let cur = match state.merged(handle) {
            Some(m) => m.maps(),
            None => vec![state.get(handle).ok_or("句柄已失效")?.mapped()],
        };
        let task = state
            .compare(handle, &base, restart, || {
                // 用 LogFile 打开只为了拿它的 mmap（空文件、只读映射那几条规矩在它里面）；它一析构就叫停自己的索引线程，映射由归类任务攥着
                let maps = base.iter().map(|p| LogFile::open(p).map(|f| f.mapped())).collect::<std::io::Result<Vec<_>>>()?;
                logengine::pattern::spawn_compare(cur, maps)
            })
            .map_err(|e| format!("对比不了：{e}"))?;
        Ok(patterns_dto(&task, limit))
    })
    .await
}
