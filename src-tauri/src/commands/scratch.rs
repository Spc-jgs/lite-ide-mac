//! 草稿：目录、新建、列表、丢弃空草稿，以及在草稿里搜索。

use super::*;

/// 草稿目录：`~/Library/Application Support/<identifier>/scratches`。
///
/// 为什么藏在这儿而不是 `~/Documents`：草稿是「看日志时顺手记两笔」的临时纸，
/// 不是知识 —— 放进文稿目录会跟 iCloud 的「桌面与文稿」同步撞上，而 iCloud
/// 的「优化储存空间」会把不常打开的文件抽成本地占位符，那时点开一份草稿要等
/// 下载。一个以「秒开」立身的工具不能有这种延迟。
///
/// 目录在这里**不建**，留给 `create_scratch` —— 「打开草稿目录」只是想看看，
/// 不该因为看一眼就在盘上留下一个空目录。
fn scratch_root(app: &tauri::AppHandle) -> Result<std::path::PathBuf, String> {
    use tauri::Manager;
    app.path()
        .app_data_dir()
        .map(|d| d.join("scratches"))
        .map_err(|e| format!("取不到应用数据目录：{e}"))
}

/// 草稿目录的绝对路径。给「打开草稿目录」用，不保证它已经在盘上。
#[tauri::command]
pub fn scratch_dir(app: tauri::AppHandle) -> Result<String, String> {
    Ok(scratch_root(&app)?.to_string_lossy().into_owned())
}

/// 新建一份草稿，返回新路径。业务在 fsservice —— 这里只拼目录、转错误。
///
/// `stem` 由前端按本地时间生成（`2026-09-09 1030`）：std 里没有本地时区，
/// 为一个文件名拽一个日期库进来不值，而前端 `new Date()` 天然就是本地的。
#[tauri::command]
pub fn create_scratch(app: tauri::AppHandle, stem: String, anchor: Option<AnchorDto>) -> Result<String, String> {
    let dir = scratch_root(&app)?;
    let a = anchor.map(|a| fsservice::Anchor { project: a.project, branch: a.branch, head: a.head, at: a.at });
    let p = fsservice::create_scratch_with(&dir, &stem, a.as_ref()).map_err(|e| format!("新建草稿失败：{e}"))?;
    crate::diag!("create_scratch -> {}", p.display());
    Ok(p.to_string_lossy().into_owned())
}

/// 草稿目录里有什么，最近的在前。目录还不存在就是空列表。
///
/// 走阻塞池：它要打开每一份草稿读头 4KB 拿摘要，草稿几百份时不该卡主线程。
#[tauri::command]
pub async fn list_scratches(app: tauri::AppHandle) -> Result<Vec<ScratchDto>, String> {
    let dir = scratch_root(&app)?;
    blocking(move || {
        fsservice::list_scratches(&dir)
            .map_err(|e| format!("列草稿目录失败：{e}"))
            .map(|v| {
                v.into_iter()
                    .map(|s| ScratchDto {
                        name: s.name,
                        path: s.path.to_string_lossy().into_owned(),
                        mtime_ms: s.mtime_ms,
                        first_line: s.first_line,
                        anchor: s.anchor.map(|a| AnchorDto { project: a.project, branch: a.branch, head: a.head, at: a.at }),
                    })
                    .collect()
            })
    })
    .await
}

/// 丢掉一份一个字都没写过的草稿。判据全在 fsservice 里，前端说了不算。
///
/// 目录由这边算，**不接受前端传目录** —— 前端能指定草稿目录的话，
/// 「只删草稿目录里的东西」这条判据就等于没有。
#[tauri::command]
pub fn discard_empty_scratch(app: tauri::AppHandle, path: String) -> Result<(), String> {
    let dir = scratch_root(&app)?;
    fsservice::discard_empty_scratch(&dir, &path).map_err(|e| format!("{e}"))
}

/// 搜草稿目录的内容（M10 ③）：和 `grep_project` 同一个 searchsvc，只是根换成草稿目录。
/// 返回的路径是**绝对**的 —— 草稿不在项目里，前端拿相对路径没法拼。
/// 文件头（锚点 frontmatter）里的命中滤掉：搜 `main` 不该把所有 main 分支上写的草稿都翻出来。
#[tauri::command]
pub async fn grep_scratches(
    app: tauri::AppHandle,
    pattern: String,
    case: bool,
    word: bool,
    regex: bool,
    limit: usize,
) -> Result<Vec<HitDto>, String> {
    let dir = scratch_root(&app)?;
    blocking(move || {
        if !dir.is_dir() {
            return Ok(Vec::new());
        }
        let q = searchsvc::Query { pattern, case, word, regex };
        let hits = searchsvc::grep(&dir, &q, limit, &searchsvc::Skip::by_name())
            .map_err(|e| super::search::grep_error(&e))?;
        let mut heads: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        Ok(hits
            .into_iter()
            .filter_map(|h| {
                let abs = dir.join(&h.path);
                let head = *heads
                    .entry(h.path.clone())
                    .or_insert_with(|| fsservice::frontmatter_lines(&abs));
                if h.line as usize <= head {
                    return None;
                }
                Some(HitDto { path: abs.to_string_lossy().into_owned(), ..super::search::hit_dto(h) })
            })
            .collect())
    })
    .await
}
