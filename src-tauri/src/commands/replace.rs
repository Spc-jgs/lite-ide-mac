//! 跨文件替换（#42）的命令。业务全在 `replacesvc`；这里只解包参数、取扫描结果、转 DTO 和错误。
//! 扫描结果按窗口登记在 AppState（`set_scan`），窗口关了就收。替换日志放在应用数据目录的 `replace-journal/`。

use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::Manager;

/// 开着的标签（前端传）：路径、编辑器里的文本、有没有未保存改动
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct OpenDocArg {
    pub path: String,
    pub text: String,
    #[serde(default)]
    pub dirty: bool,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PickArg {
    pub rel: String,
    pub hits: Vec<usize>,
}

fn journal_dir(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    Ok(app.path().app_data_dir().map_err(|e| e.to_string())?.join("replace-journal"))
}

/// 有未保存改动的那几个：路径 → 编辑器里的文本
fn dirty_map(docs: Vec<OpenDocArg>) -> HashMap<PathBuf, String> {
    docs.into_iter().filter(|d| d.dirty).map(|d| (PathBuf::from(d.path), d.text)).collect()
}

fn skip_dto(s: replacesvc::Skipped) -> ReplaceSkipDto {
    ReplaceSkipDto { rel: s.rel, why: s.why.code().into(), text: s.why.text() }
}

fn outcome_dto(o: replacesvc::Outcome) -> ReplaceOutcomeDto {
    ReplaceOutcomeDto {
        changed: o
            .changed
            .into_iter()
            .map(|c| ReplaceChangedDto {
                rel: c.rel,
                path: c.abs.to_string_lossy().into_owned(),
                count: c.count,
                edits: c.edits.into_iter().map(|e| ReplaceEditDto { from: e.from, to: e.to, insert: e.insert }).collect(),
                on_disk: c.on_disk,
            })
            .collect(),
        skipped: o.skipped.into_iter().map(skip_dto).collect(),
        undo: o.undo,
    }
}

/// 找出全部命中，记在这个窗口名下。`open`：开着的标签（有未保存改动的用编辑器里那份）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn replace_scan(
    window: tauri::Window,
    state: State<'_, AppState>,
    root: String,
    pattern: String,
    case: bool,
    word: bool,
    regex: bool,
    open: Vec<OpenDocArg>,
) -> Result<ReplaceScanDto, String> {
    let r = root.clone();
    let scan = blocking(move || {
        let (files, index_truncated) =
            searchsvc::list_files_capped(&r, &super::search::skip_for(&r)).map_err(|e| format!("索引项目失败：{e}"))?;
        let q = searchsvc::Query { pattern, case, word, regex };
        replacesvc::scan(std::path::Path::new(&r), &files, index_truncated, &q, &dirty_map(open))
    })
    .await?;
    let dto = ReplaceScanDto {
        files: scan
            .files
            .iter()
            .map(|f| ReplaceFileDto {
                rel: f.rel.clone(),
                path: f.abs.to_string_lossy().into_owned(),
                editor: matches!(f.source, replacesvc::Source::Editor),
                hits: f.hits.iter().map(|h| ReplaceHitDto { line: h.line, col: h.col, text: h.text.clone(), spans: h.spans.clone() }).collect(),
            })
            .collect(),
        skipped: scan.skipped.iter().cloned().map(skip_dto).collect(),
        binary: scan.binary,
        total: scan.total,
        truncated: scan.truncated,
        index_truncated: scan.index_truncated,
    };
    state.set_scan(window.label(), Some(scan));
    Ok(dto)
}

/// 替换串变了：只重算「改后」那一行，不重新扫盘。顺序和扫描结果的 `files[i].hits[j]` 一一对应
#[tauri::command]
pub async fn replace_preview(
    window: tauri::Window,
    state: State<'_, AppState>,
    replacement: String,
) -> Result<Vec<Vec<ReplaceAfterDto>>, String> {
    let scan = state.scan(window.label()).ok_or("没有扫描结果，先搜一次")?;
    blocking(move || {
        Ok(scan
            .after(&replacement)
            .into_iter()
            .map(|f| f.into_iter().map(|a| ReplaceAfterDto { text: a.text, spans: a.spans }).collect())
            .collect())
    })
    .await
}

/// 执行。成功后这个窗口的扫描结果作废（盘上已经变了，再拿它执行一次只会全被判成「预览之后被改过」）
#[tauri::command]
pub async fn replace_apply(
    app: tauri::AppHandle,
    window: tauri::Window,
    state: State<'_, AppState>,
    replacement: String,
    picks: Vec<PickArg>,
    open: Vec<OpenDocArg>,
    allow_no_undo: bool,
) -> Result<ReplaceOutcomeDto, ReplaceErrorDto> {
    let err = |code: &str, message: String| ReplaceErrorDto { code: code.into(), message, bytes: 0 };
    let scan = state.scan(window.label()).ok_or_else(|| err("failed", "没有扫描结果，先搜一次".into()))?;
    let dir = journal_dir(&app).map_err(|e| err("failed", e))?;
    let out = blocking(move || {
        let picks: Vec<replacesvc::Pick> = picks.into_iter().map(|p| replacesvc::Pick { rel: p.rel, hits: p.hits }).collect();
        let dirty = dirty_map(open);
        let o = replacesvc::ApplyOpts {
            journal: &dir,
            replacement: &replacement,
            picks: &picks,
            dirty_now: &dirty,
            allow_no_undo,
            crash_after: None,
            journal_cap: None,
        };
        Ok(replacesvc::apply(&scan, &o))
    })
    .await
    .map_err(|e: String| err("failed", e))?;
    state.set_scan(window.label(), None);
    match out {
        Ok(o) => Ok(outcome_dto(o)),
        Err(replacesvc::ApplyError::Truncated) => Err(err("truncated", "命中太多，缩小范围再替换".into())),
        Err(replacesvc::ApplyError::Pending) => Err(err("pending", "上一次替换中断了还没处理".into())),
        Err(replacesvc::ApplyError::TooBigForUndo { bytes }) => {
            Err(ReplaceErrorDto { code: "too-big".into(), message: "这次改动太大，撤销和中断恢复都做不了".into(), bytes })
        }
        Err(replacesvc::ApplyError::Failed { rel, msg }) => {
            Err(err("failed", if rel.is_empty() { msg } else { format!("{rel}：{msg}") }))
        }
    }
}

/// 撤销最近一次替换。`open`：此刻开着的标签
#[tauri::command]
pub async fn replace_undo(app: tauri::AppHandle, open: Vec<OpenDocArg>) -> Result<ReplaceOutcomeDto, String> {
    let dir = journal_dir(&app)?;
    blocking(move || {
        let open: HashMap<PathBuf, replacesvc::OpenNow> = open
            .into_iter()
            .map(|d| (PathBuf::from(d.path), replacesvc::OpenNow { text: d.text, dirty: d.dirty }))
            .collect();
        replacesvc::undo(&dir, &open).map(outcome_dto)
    })
    .await
}

/// 有没有上次留下的替换日志（启动时问一次；撤销卡片也靠它）
#[tauri::command]
pub async fn replace_pending(app: tauri::AppHandle) -> Result<Option<ReplacePendingDto>, String> {
    let dir = journal_dir(&app)?;
    blocking(move || {
        Ok(replacesvc::pending(&dir).map(|p| ReplacePendingDto {
            state: match p.state {
                replacesvc::State::Preparing => "preparing",
                replacesvc::State::Committing => "committing",
                replacesvc::State::Done => "done",
            }
            .into(),
            root: p.root,
            at_ms: p.at_ms,
            files: p.files,
            changed: p.changed,
        }))
    })
    .await
}

/// 收拾上次中断的替换：`rollback` 退回改前，否则保留现状
#[tauri::command]
pub async fn replace_recover(app: tauri::AppHandle, rollback: bool) -> Result<ReplaceOutcomeDto, String> {
    let dir = journal_dir(&app)?;
    blocking(move || replacesvc::recover(&dir, rollback).map(outcome_dto)).await
}
