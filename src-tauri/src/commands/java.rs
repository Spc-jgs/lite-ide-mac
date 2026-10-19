//! Java 智能（docs/IDEA.md 第 2 步）：文档同步、跳转、查用法、jar 里的类。业务在 `crate::java`，jdtls 的线和进程在 `lspsvc`。
//! 全部 async + 阻塞池：要等 jdtls 回话（第一次查用法实测最慢 6 秒），文档同步要写管道。

use super::*;
use tauri::Manager;

/// 打开了一个 Java 文件（或者重起之后前端重新登记）：这个项目的 jdtls 没有就起（**不等它起好**），登记这份文档。
/// 不是 Maven / Gradle 项目给 null（不起，也不算错）
#[tauri::command]
pub async fn java_open(window: tauri::Window, app: tauri::AppHandle, root: String, path: String, text: String) -> Result<Option<JavaStatusDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let st = app.state::<AppState>();
        if st.java.get(&root).is_none() && !crate::java::is_build_root(Path::new(&root)) {
            return Ok(None);
        }
        let i = st.java.ensure(&app, &owner, &root).map_err(|s| JavaStatusDto::from(&s).message)?;
        i.open(&path, &text);
        Ok(Some(JavaStatusDto::from(&i.status())))
    })
    .await
}

/// 改了（全文，见 java.rs 头上「文档同步用全文」）。没有 jdtls 就什么都不做 —— 不为一次改动起服务器
#[tauri::command]
pub async fn java_change(app: tauri::AppHandle, root: String, path: String, text: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.change(&path, &text);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn java_save(app: tauri::AppHandle, root: String, path: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.save(&path);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn java_close(app: tauri::AppHandle, root: String, path: String) -> Result<(), String> {
    blocking(move || {
        if let Some(i) = app.state::<AppState>().java.get(&root) {
            i.close(&path);
        }
        Ok(())
    })
    .await
}

fn instance(app: &tauri::AppHandle, root: &str) -> Result<std::sync::Arc<crate::java::Instance>, String> {
    app.state::<AppState>().java.get(root).ok_or_else(|| "这个项目的 Java 语言服务器没在跑".into())
}

/// 跳到定义。位置的行、列从 0 数，列按 UTF-16
#[tauri::command]
pub async fn java_definition(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<Vec<JavaLocationDto>, String> {
    blocking(move || Ok(instance(&app, &root)?.definition(&path, line, character)?.into_iter().map(Into::into).collect())).await
}

/// 查用法（不含声明本身）。超过上限截断，`total` 是真条数
#[tauri::command]
pub async fn java_references(app: tauri::AppHandle, root: String, path: String, line: u32, character: u32) -> Result<JavaRefsDto, String> {
    blocking(move || {
        let (locs, total) = instance(&app, &root)?.references(&path, line, character)?;
        let texts = crate::java::line_texts(&locs);
        let items = locs
            .into_iter()
            .zip(texts)
            .map(|(l, text)| JavaLocationDto { text, ..JavaLocationDto::from(l) })
            .collect();
        Ok(JavaRefsDto { items, total: total as u32 })
    })
    .await
}

/// 补全。带着编辑器此刻的全文（先同步再问，见 `java::Instance::complete`）。位置行列从 0 数、列按 UTF-16
#[tauri::command]
pub async fn java_complete(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<JavaCompletionDto, String> {
    blocking(move || {
        let (items, incomplete, total) = instance(&app, &root)?.complete(&path, &text, line, character)?;
        let items = items
            .into_iter()
            .map(|i| JavaCompletionItemDto {
                label: i.label,
                kind: i.kind,
                detail: i.detail,
                description: i.description,
                edit: i.edit.map(Into::into),
                snippet: i.snippet,
                sort: i.sort,
                raw: i.raw,
            })
            .collect();
        Ok(JavaCompletionDto { items, incomplete, total: total as u32 })
    })
    .await
}

/// 选中的那一项要的自动 import 和说明文字
#[tauri::command]
pub async fn java_resolve(app: tauri::AppHandle, root: String, raw: String) -> Result<JavaResolvedDto, String> {
    blocking(move || {
        let r = instance(&app, &root)?.resolve(&raw)?;
        Ok(JavaResolvedDto { edits: r.edits.into_iter().map(Into::into).collect(), doc: r.doc })
    })
    .await
}

/// 格式化（⌥⌘L）：返回要做的改动（行列从 0 数、列按 UTF-16），前端在编辑器里一笔做完。`range` 为空 = 整份
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_format(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    range: Option<JavaRangeArg>,
    tab_size: u32,
    insert_spaces: bool,
) -> Result<Vec<JavaEditDto>, String> {
    blocking(move || {
        let range = range.map(|r| crate::java::Edit { line: r.line, character: r.character, end_line: r.end_line, end_character: r.end_character, text: String::new() });
        Ok(instance(&app, &root)?.format(&path, &text, range, tab_size, insert_spaces)?.into_iter().map(Into::into).collect())
    })
    .await
}

/// 光标那儿（或选中的那几行）能做的修复（⌥↵）
#[tauri::command]
pub async fn java_code_actions(app: tauri::AppHandle, root: String, path: String, text: String, range: JavaRangeArg) -> Result<Vec<JavaActionDto>, String> {
    blocking(move || {
        let r = crate::java::Edit { line: range.line, character: range.character, end_line: range.end_line, end_character: range.end_character, text: String::new() };
        Ok(instance(&app, &root)?.code_actions(&path, &text, r)?.into_iter().map(|a| JavaActionDto { title: a.title, kind: a.kind, raw: a.raw }).collect())
    })
    .await
}

/// 做选中的那条修复。只改 `path` 这一个文件的：把改动交回编辑器（不写盘，⌘Z 能撤）；改到别的文件、或者要改文件名的：照改名那套落盘（`java::apply_rename`）
#[tauri::command]
pub async fn java_apply_action(app: tauri::AppHandle, root: String, path: String, raw: String, open: Vec<super::replace::OpenDocArg>) -> Result<JavaActionResultDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = instance(&app, &root)?.resolve_action(&raw)?;
        if plan.renames.is_empty() && plan.files.iter().all(|(p, _)| *p == path) {
            let edits = plan.files.into_iter().flat_map(|(_, e)| e).map(Into::into).collect();
            return Ok(JavaActionResultDto { edits, outcome: None });
        }
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(JavaActionResultDto { edits: Vec::new(), outcome: Some(super::replace::outcome_dto(out)) })
    })
    .await
}

/// 一段范围（前端传）：行列从 0 数、列按 UTF-16
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JavaRangeArg {
    pub line: u32,
    pub character: u32,
    pub end_line: u32,
    pub end_character: u32,
}

/// 光标底下能不能改名（⇧F6 先问这个：不能改的地方不弹输入框）。null = 这里不能改名
#[tauri::command]
pub async fn java_prepare_rename(app: tauri::AppHandle, root: String, path: String, text: String, line: u32, character: u32) -> Result<Option<JavaRenameTargetDto>, String> {
    blocking(move || {
        Ok(instance(&app, &root)?.prepare_rename(&path, &text, line, character)?.map(|t| JavaRenameTargetDto {
            line: t.range.line,
            character: t.range.character,
            end_line: t.range.end_line,
            end_character: t.range.end_character,
            placeholder: t.placeholder,
        }))
    })
    .await
}

/// 改名：问 jdtls 要改哪些，照跨文件替换那套落盘（`java::apply_rename`）。
/// `open`：开着的标签（同 `replace_apply`）。结果的形状同替换，前端走同一条路让开着的标签跟上
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn java_rename(
    app: tauri::AppHandle,
    root: String,
    path: String,
    text: String,
    line: u32,
    character: u32,
    new_name: String,
    open: Vec<super::replace::OpenDocArg>,
) -> Result<ReplaceOutcomeDto, String> {
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let i = instance(&app, &root)?;
        let plan = i.rename(&path, &text, line, character, &new_name)?;
        // 盘上改了哪些不用在这儿告诉 jdtls：它的文件监听会看到（`java::Instance::on_disk_changes`），撤销时也一样
        let out = crate::java::apply_rename(&plan, Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(super::replace::outcome_dto(out))
    })
    .await
}

/// jar 里一个类的内容（`jdt://…`）：有源码 jar 就是源码，没有就是反编译的
#[tauri::command]
pub async fn java_class_contents(app: tauri::AppHandle, root: String, uri: String) -> Result<String, String> {
    blocking(move || instance(&app, &root)?.class_contents(&uri)).await
}

/// 现在什么状态（前端刚挂上、错过了事件时补一次）。没起过就是 null
#[tauri::command]
pub async fn java_status(app: tauri::AppHandle, root: String) -> Result<Option<JavaStatusDto>, String> {
    blocking(move || Ok(app.state::<AppState>().java.status(&root).as_ref().map(JavaStatusDto::from))).await
}

/// 重起这个项目的 jdtls（状态栏的按钮）。开着的 Java 文档由前端接着重新 `java_open`
#[tauri::command]
pub async fn java_restart(window: tauri::Window, app: tauri::AppHandle, root: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || app.state::<AppState>().java.restart(&app, &owner, &root)).await
}
