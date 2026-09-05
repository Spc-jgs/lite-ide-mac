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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TextDto {
    pub content: String,
    /// WHATWG 编码标签，如 `UTF-8` / `GBK`
    pub encoding: String,
    pub bom: bool,
    /// 有解不出的字节 —— 界面必须把这件事说出来，
    /// 带着它保存等于把那些字节永久换成 U+FFFD
    pub lossy: bool,
}

/// 编辑模式读取全文，自动探测编码。
///
/// `label` 非空时按指定编码读（用户点了「以其他编码重新打开」）。
#[tauri::command]
pub fn read_text(path: String, label: Option<String>) -> Result<TextDto, String> {
    let d = fsservice::read_text_detect(&path, label.as_deref().unwrap_or(""))
        .map_err(|e| format!("{e}"))?;
    Ok(TextDto {
        content: d.content,
        encoding: d.encoding.to_string(),
        bom: d.bom,
        lossy: d.lossy,
    })
}

/// 探测一个文件的编码，只读头部采样。日志模式用它决定 TextDecoder 的标签。
///
/// 采样 256KB 而不是读全文：日志可能有 1GB，而编码特征在头部就足够明显。
#[tauri::command]
pub fn detect_encoding(path: String) -> Result<String, String> {
    use std::io::Read;
    const SAMPLE: usize = 256 << 10;
    let mut f = std::fs::File::open(&path).map_err(|e| format!("读不到 {path}：{e}"))?;
    let mut buf = vec![0u8; SAMPLE];
    let n = f.read(&mut buf).map_err(|e| format!("{e}"))?;
    buf.truncate(n);
    Ok(fsservice::encoding::decode(&buf).encoding.to_string())
}

/// 界面上给用户挑的编码清单
#[tauri::command]
pub fn list_encodings() -> Vec<(String, String)> {
    fsservice::encoding::COMMON
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
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

/// 在 Finder 里显示。业务在 fsservice —— 这里只转错误。
#[tauri::command]
pub fn reveal_in_finder(path: String) -> Result<(), String> {
    fsservice::reveal_in_finder(&path).map_err(|e| format!("{e}"))
}

/// 新建文件或目录，返回新路径。业务在 fsservice —— 这里只转错误。
///
/// 参数是「哪个目录、叫什么」而不是一条拼好的路径：**join 和名字校验都在
/// Rust 侧**，前端少一个把文件写到别处去的机会。
#[tauri::command]
pub fn create_entry(dir: String, name: String, is_dir: bool) -> Result<String, String> {
    let p = fsservice::create_entry(&dir, &name, is_dir).map_err(|e| format!("{e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 原地改名，返回新路径。
#[tauri::command]
pub fn rename_entry(path: String, name: String) -> Result<String, String> {
    let p = fsservice::rename_entry(&path, &name).map_err(|e| format!("{e}"))?;
    Ok(p.to_string_lossy().into_owned())
}

/// 移到废纸篓。**整个应用里没有第二条删除路径** —— 没有 remove_file。
#[tauri::command]
pub fn trash_entry(path: String) -> Result<(), String> {
    fsservice::move_to_trash(&path).map_err(|e| format!("{e}"))
}

/// 保存。先写临时文件再原子替换，中途崩溃不会留下半个文件。
///
/// 按 `label` 指定的编码写回 —— 用什么编码读进来的就用什么存回去，
/// 不做「顺手转成 UTF-8」这种擅自决定。
///
/// 返回写入后的指纹 —— 前端必须拿它更新记录，否则自己的保存
/// 会在下一次检查时被当成"外部修改"。
#[tauri::command]
pub fn write_text(
    path: String,
    content: String,
    label: Option<String>,
    bom: Option<bool>,
) -> Result<StampDto, String> {
    let label = label.unwrap_or_else(|| "UTF-8".into());
    fsservice::write_text_as(&path, &content, &label, bom.unwrap_or(false))
        .map_err(|e| format!("保存失败：{e}"))?;
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
///
/// `label` 是这个文件的编码。关键字要**先编成文件那套字节**再下去搜 ——
/// 一份 GBK 日志里搜「订单」，拿 UTF-8 的「订单」去比对是永远搜不到的。
/// 编码这件事只有这一层知道（前端探测出来传上来），所以在这里做。
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
    let bytes = if pattern.is_empty() {
        Vec::new()
    } else {
        fsservice::encoding::encode(&pattern, &label, false)
    };
    let spec = FilterSpec {
        levels: LevelMask::from_bits(level_bits),
        pattern: bytes,
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
                        /*
                         * 前端已经关掉这个终端时 send 会失败，正常收摊。
                         *
                         * **但这一 break 是有代价的**：这里退出之后就再没人排空
                         * pty master，而紧接着到来的正是 pty_kill。退出中的 shell
                         * 写满缓冲区就卡在写上收不了尾，`child.wait()` 于是永远
                         * 等不到 —— M23 那次界面永久卡死就是这么来的（issue #2）。
                         *
                         * 现在不挂住，靠的是 `ptysvc::Session::kill()` 在杀之前
                         * 自己接了一条临时排空线程。**改那边之前先看这里** ——
                         * 回归测试在 ptysvc 里，动这个 break 的人不一定会跑到。
                         */
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

// ─────────────────────────── Git ───────────────────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitEntryDto {
    pub path: String,
    /// 暂存区状态字符
    pub index: String,
    /// 工作区状态字符
    pub work: String,
    pub untracked: bool,
    /// 是折叠的未跟踪目录，文件树要按前缀匹配
    pub is_dir: bool,
    pub conflicted: bool,
    pub staged: bool,
    pub unstaged: bool,
    pub orig: Option<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GitStatusDto {
    /// 仓库根的绝对路径。前端拿它把相对路径拼成绝对路径去对文件树
    pub root: String,
    pub branch: String,
    pub upstream: String,
    pub ahead: u32,
    pub behind: u32,
    pub detached: bool,
    pub unborn: bool,
    pub entries: Vec<GitEntryDto>,
    pub truncated: bool,
}

/// 找 `path` 所属的仓库根。不是仓库返回 null —— 这是正常情况，
/// 界面据此让整块 Git 功能隐身，而不是弹错误。
#[tauri::command]
pub fn git_root(path: String) -> Option<String> {
    gitsvc::discover(&path).map(|p| p.to_string_lossy().into_owned())
}

/// 读一次仓库状态。分支、领先落后、变更文件一次拿全。
#[tauri::command]
pub fn git_status(root: String) -> Result<GitStatusDto, String> {
    let st = gitsvc::status_full(&root).map_err(|e| format!("{e}"))?;
    Ok(GitStatusDto {
        root,
        branch: st.branch,
        upstream: st.upstream,
        ahead: st.ahead,
        behind: st.behind,
        detached: st.detached,
        unborn: st.unborn,
        truncated: st.truncated,
        entries: st
            .entries
            .into_iter()
            .map(|e| GitEntryDto {
                staged: e.staged(),
                unstaged: e.unstaged(),
                index: e.index.to_string(),
                work: e.work.to_string(),
                path: e.path,
                untracked: e.untracked,
                is_dir: e.is_dir,
                conflicted: e.conflicted,
                orig: e.orig,
            })
            .collect(),
    })
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DiffDto {
    pub text: String,
    /// 超过 `gitsvc::MAX_DIFF_BYTES` 被掐断了。界面必须把这件事说出来 ——
    /// 一份看着完整、其实少了后半截的差异，比一句「显示不下」危险得多
    pub truncated: bool,
}

impl From<gitsvc::Diff> for DiffDto {
    fn from(d: gitsvc::Diff) -> Self {
        Self {
            text: d.text,
            truncated: d.truncated,
        }
    }
}

#[tauri::command]
pub fn git_diff(
    root: String,
    path: String,
    staged: bool,
    untracked: bool,
) -> Result<DiffDto, String> {
    gitsvc::diff(&root, &path, staged, untracked)
        .map(DiffDto::from)
        .map_err(|e| format!("{e}"))
}

#[tauri::command]
pub fn git_stage(root: String, paths: Vec<String>) -> Result<(), String> {
    gitsvc::stage(&root, &paths).map_err(|e| format!("暂存失败：{e}"))
}

#[tauri::command]
pub fn git_unstage(root: String, paths: Vec<String>) -> Result<(), String> {
    gitsvc::unstage(&root, &paths).map_err(|e| format!("取消暂存失败：{e}"))
}

/// 丢弃工作区改动。**不可撤销** —— 前端必须先让用户确认过才准调。
#[tauri::command]
pub fn git_discard(
    root: String,
    paths: Vec<String>,
    untracked: Vec<String>,
) -> Result<(), String> {
    crate::diag!("git_discard {} 个跟踪 + {} 个未跟踪", paths.len(), untracked.len());
    gitsvc::discard(&root, &paths, &untracked).map_err(|e| format!("丢弃失败：{e}"))
}

#[tauri::command]
pub fn git_commit(root: String, message: String, amend: bool) -> Result<String, String> {
    gitsvc::commit(&root, &message, amend).map_err(|e| format!("{e}"))
}

// ─────────────── Git：历史 · 分支 · 工作树 ───────────────

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntryDto {
    pub sha: String,
    pub short: String,
    pub author: String,
    pub email: String,
    pub when: String,
    pub date: String,
    pub subject: String,
    /// 父提交完整 sha；合并提交有多个，泳道图靠它连线
    pub parents: Vec<String>,
    pub refs: Vec<String>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BranchDto {
    pub name: String,
    pub sha: String,
    pub upstream: String,
    pub is_head: bool,
    pub is_remote: bool,
    pub when: String,
    pub subject: String,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorktreeDto {
    pub path: String,
    pub sha: String,
    pub branch: String,
    pub detached: bool,
    pub bare: bool,
    pub locked: bool,
    pub current: bool,
}

#[tauri::command]
pub fn git_log_entries(
    root: String,
    limit: usize,
    all: bool,
    path: String,
) -> Result<Vec<LogEntryDto>, String> {
    let es = gitsvc::log_entries(&root, limit, all, &path).map_err(|e| format!("读历史失败：{e}"))?;
    Ok(es
        .into_iter()
        .map(|c| LogEntryDto {
            sha: c.sha,
            short: c.short,
            author: c.author,
            email: c.email,
            when: c.when,
            date: c.date,
            subject: c.subject,
            parents: c.parents,
            refs: c.refs,
        })
        .collect())
}

#[tauri::command]
pub fn git_commit_files(root: String, sha: String) -> Result<Vec<GitEntryDto>, String> {
    let es = gitsvc::commit_files(&root, &sha).map_err(|e| format!("读提交内容失败：{e}"))?;
    Ok(es
        .into_iter()
        .map(|e| GitEntryDto {
            staged: true,
            unstaged: false,
            index: e.index.to_string(),
            work: e.work.to_string(),
            path: e.path,
            untracked: false,
            is_dir: false,
            conflicted: false,
            orig: e.orig,
        })
        .collect())
}

#[tauri::command]
pub fn git_commit_diff(root: String, sha: String, path: String) -> Result<DiffDto, String> {
    gitsvc::commit_diff(&root, &sha, &path)
        .map(DiffDto::from)
        .map_err(|e| format!("{e}"))
}

#[tauri::command]
pub fn git_branches(root: String) -> Result<Vec<BranchDto>, String> {
    let bs = gitsvc::branches(&root).map_err(|e| format!("读分支失败：{e}"))?;
    Ok(bs
        .into_iter()
        .map(|b| BranchDto {
            name: b.name,
            sha: b.sha,
            upstream: b.upstream,
            is_head: b.is_head,
            is_remote: b.is_remote,
            when: b.when,
            subject: b.subject,
        })
        .collect())
}

/// 切分支。工作区脏时 git 会自己拒绝，错误原样上抛 —— 它的措辞比我们准。
#[tauri::command]
pub fn git_switch(root: String, name: String, create: bool) -> Result<String, String> {
    crate::diag!("git_switch {name} create={create}");
    gitsvc::switch_branch(&root, &name, create).map_err(|e| format!("{e}"))
}

#[tauri::command]
pub fn git_worktrees(root: String) -> Result<Vec<WorktreeDto>, String> {
    let ws = gitsvc::worktrees(&root).map_err(|e| format!("读工作树失败：{e}"))?;
    Ok(ws
        .into_iter()
        .map(|w| WorktreeDto {
            path: w.path,
            sha: w.sha,
            branch: w.branch,
            detached: w.detached,
            bare: w.bare,
            locked: w.locked,
            current: w.current,
        })
        .collect())
}

/// 新建工作树，返回新目录的绝对路径 —— 前端可以直接把它当项目根打开。
#[tauri::command]
pub fn git_worktree_add(root: String, path: String, branch: String) -> Result<String, String> {
    crate::diag!("git_worktree_add path={path} branch={branch}");
    gitsvc::worktree_add(&root, &path, &branch).map_err(|e| format!("{e}"))
}

/// 移除工作树。**会删掉那个目录**，前端必须先确认。
#[tauri::command]
pub fn git_worktree_remove(root: String, path: String, force: bool) -> Result<(), String> {
    crate::diag!("git_worktree_remove path={path} force={force}");
    gitsvc::worktree_remove(&root, &path, force).map_err(|e| format!("{e}"))
}

// ── 菜单栏 ───────────────────────────────────────────────────────────

/// 开原生的「选择文件夹」面板，返回选中的路径；取消返回 `None`。
///
/// **必须在 Rust 侧开。** `NSOpenPanel` 是主线程亲和的原生控件，
/// webview 里没有等价物（HTML 的 `<input webkitdirectory>` 给的是
/// 一堆文件条目，不是目录路径，而且拿不到绝对路径）。
///
/// 用 `blocking_pick_folder` 而不是回调版：这个命令跑在 Tauri 的命令线程上，
/// 不是主线程 —— 插件内部会把面板调度到主线程，这里阻塞等它就行。
/// 反过来（在主线程上 blocking）才会死锁。
#[tauri::command]
pub async fn pick_folder(app: tauri::AppHandle) -> Option<String> {
    use tauri_plugin_dialog::DialogExt;
    crate::diag!("pick_folder 打开面板");
    let picked = app
        .dialog()
        .file()
        .set_title("打开文件夹")
        .blocking_pick_folder();
    picked.map(|p| p.to_string())
}

/// 刷新「最近打开」子菜单。
///
/// 列表存在前端的会话快照里（那本来就是「上次开的是哪个项目」的归属地），
/// 变了就把整张表推过来重建 —— 最多 8 项，不值得算增量。
#[tauri::command]
pub fn set_recent(app: tauri::AppHandle, paths: Vec<String>) -> Result<(), String> {
    crate::menu::refresh_recent(&app, &paths).map_err(|e| format!("刷新最近打开失败：{e}"))
}

/// 按当下的上下文让菜单项变灰。
///
/// 今天所有键位都是 window 级监听，**不管当下有没有意义都会触发** ——
/// 没有标签时按 ⌘S、不是 Git 仓库时按 ⇧⌘G，都是走一遍然后什么也没发生。
/// 灰掉的菜单项本身就是一句解释：不是坏了，是现在用不上。
#[tauri::command]
pub fn sync_menu_state(app: tauri::AppHandle, has_tab: bool, has_repo: bool, has_term: bool) {
    crate::menu::sync_enabled(&app, has_tab, has_repo, has_term);
}

/// 交给系统默认浏览器打开一个网址。目前只服务「帮助 › 项目主页」。
///
/// **只认 `https://`。** 这是个能让应用启动任意外部程序的口子 ——
/// `open` 会按 scheme 派发，`file://` 能拉起任意程序、自定义 scheme
/// 更是。前端目前只用常量调它，但把判据写在 Rust 侧才算数：
/// 命令一旦存在，它的约束就不能靠调用方自觉。
///
/// 绝对路径而不是靠 PATH，理由同 `fsservice::reveal_in_finder`：
/// 从终端启动时 PATH 是用户的，不该让它决定我们调到哪个 `open`。
#[tauri::command]
pub fn open_external(url: String) -> Result<(), String> {
    if !url.starts_with("https://") {
        return Err(format!("只允许 https 链接，实得：{url}"));
    }
    crate::diag!("open_external {url}");
    let st = std::process::Command::new("/usr/bin/open")
        .arg("--")
        .arg(&url)
        .status()
        .map_err(|e| format!("起不来 open：{e}"))?;
    if st.success() {
        Ok(())
    } else {
        Err(format!("打不开 {url}"))
    }
}

// ── 拉取与推送 ───────────────────────────────────────────────────────

/// 一条进度，推给前端的形状。
///
/// `percent` / `done` 可能是 None —— git 的进度文案不是稳定接口，
/// 认不出来时 `phase` 里是整段原文，界面显示成一行状态而不是进度条。
/// **绝不能因为解析不出来就把一次成功的操作报成失败。**
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProgressDto {
    pub phase: String,
    pub percent: Option<u8>,
    pub done: Option<u64>,
    pub total: Option<u64>,
    pub finished: bool,
}

/// 远程操作失败时给前端的东西。
///
/// `kind` 决定界面显示什么（认证提示 / 先拉一下 / 去解冲突），
/// `raw` 是 git 的原话 —— **必须留着能展开看**：转译错了的时候，
/// 人得有办法绕过我们。和差异视图的 `truncated` 是同一条判据。
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RemoteErrDto {
    /// `auth-https` / `auth-ssh` / `cancelled` / `rejected` / `conflict` / `other`
    pub kind: String,
    pub message: String,
    pub raw: String,
}

fn to_err_dto(e: gitsvc::remote::RemoteError) -> RemoteErrDto {
    use gitsvc::remote::RemoteError as E;
    let kind = match &e {
        E::Auth { https: true, .. } => "auth-https",
        E::Auth { https: false, .. } => "auth-ssh",
        E::Cancelled => "cancelled",
        E::Rejected { .. } => "rejected",
        E::Conflict { .. } => "conflict",
        E::Other { .. } | E::NoGit(_) => "other",
    };
    RemoteErrDto { kind: kind.to_string(), message: e.to_string(), raw: e.raw().to_string() }
}

/// 把 gitsvc 的进度回调接到 Tauri 的 Channel 上。
///
/// 节流已经在 gitsvc 里做过了（阶段变了或百分比变了且距上次 >100ms），
/// 这里只负责转形状 —— **业务不写在命令层**，那是这个文件的规矩。
fn pump(ch: &tauri::ipc::Channel<ProgressDto>) -> impl FnMut(gitsvc::progress::Progress) + '_ {
    move |p| {
        let _ = ch.send(ProgressDto {
            phase: p.phase,
            percent: p.percent,
            done: p.done.map(|(a, _)| a),
            total: p.done.map(|(_, b)| b),
            finished: p.finished,
        });
    }
}

/// 抓远程。**只读，不动工作区。**
#[tauri::command]
pub async fn git_fetch(
    root: String,
    remote: String,
    op_id: u32,
    on_progress: tauri::ipc::Channel<ProgressDto>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), RemoteErrDto> {
    let id = op_id;
    let cancel = state.begin_remote(id);
    crate::diag!("git_fetch id={id} remote={remote}");
    let r = gitsvc::remote::fetch(&root, &remote, &cancel, &mut pump(&on_progress));
    state.end_remote(id);
    r.map_err(to_err_dto)
}

/// 推送当前分支。**这是第一个会改到别人东西的操作。**
///
/// `set_upstream` 只在「这个分支还没有上游」时由前端传真，
/// 而且界面上要把「它要建立什么」写出来，不做成沉默的开关。
#[tauri::command]
pub async fn git_push(
    root: String,
    remote: String,
    branch: String,
    set_upstream: bool,
    op_id: u32,
    on_progress: tauri::ipc::Channel<ProgressDto>,
    state: tauri::State<'_, crate::state::AppState>,
) -> Result<(), RemoteErrDto> {
    let id = op_id;
    let cancel = state.begin_remote(id);
    crate::diag!("git_push id={id} remote={remote} branch={branch} set_upstream={set_upstream}");
    let opts = gitsvc::remote::PushOpts { set_upstream };
    let r = gitsvc::remote::push(&root, &remote, &branch, opts, &cancel, &mut pump(&on_progress));
    state.end_remote(id);
    r.map_err(to_err_dto)
}

/// 把已经抓下来的上游合进当前分支。**不走网络，瞬间完成。**
///
/// 拉取 = `git_fetch` + 这个，不是 `git pull`：复合命令失败时分不清
/// 是网络断了还是合并冲突了（退出码都非零）。
#[tauri::command]
pub fn git_merge_upstream(root: String, upstream: String, mode: String) -> Result<(), RemoteErrDto> {
    use gitsvc::remote::MergeMode;
    let mode = match mode.as_str() {
        "merge" => MergeMode::Merge,
        "rebase" => MergeMode::Rebase,
        // 默认只允许快进 —— 永远不会「拉一下，凭空多出一个合并提交」
        _ => MergeMode::FfOnly,
    };
    crate::diag!("git_merge_upstream {upstream} mode={mode:?}");
    gitsvc::remote::merge_upstream(&root, &upstream, mode).map_err(to_err_dto)
}

/// 取消一个正在跑的远程操作。
///
/// **只对 fetch 开放。** push 进行中不给取消：kill 的是本地这一端，
/// 而远程可能已经收完了 —— 一个点了之后状态不确定的取消按钮，
/// 比没有按钮更糟。前端负责不显示那个按钮，这里不拦（拦了也只是重复一遍）。
#[tauri::command]
pub fn git_cancel(id: u32, state: tauri::State<'_, crate::state::AppState>) -> bool {
    crate::diag!("git_cancel id={id}");
    state.cancel_remote(id)
}


/// 推上去会送出哪些提交。照 IDEA 的推送对话框：**列出提交，不是只给计数**。
#[tauri::command]
pub fn git_outgoing(
    root: String,
    upstream: String,
    branch: String,
) -> Result<Vec<String>, String> {
    // 20 条是对话框的显示上限 —— 再多也没人读，而且要走一趟 IPC
    gitsvc::remote::outgoing(&root, &upstream, &branch, 20).map_err(|e| format!("{e}"))
}
