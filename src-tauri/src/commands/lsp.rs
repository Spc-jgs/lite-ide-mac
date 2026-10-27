//! Java 以外的语言服务器（docs/LANGPACK.md）：文档同步、跳定义、查用法、悬停、补全。规则在 `crate::langs`，DTO 和 Java 那边是同一套（都是 LSP 的形状），前端画法共用

use super::*;
use tauri::Manager;

/// 打开 / 更新一个文档（全文）。回这个文件归哪个语言包（`null` = 不是任何包认的文件）；语言服务器没装、起不来是错误（一句人话，带怎么装）
#[tauri::command]
pub async fn lsp_open(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String) -> Result<Option<String>, String> {
    let owner = window.label().to_string();
    blocking(move || app.state::<AppState>().langs.open(&app, &owner, &root, &path, &text)).await
}

#[tauri::command]
pub async fn lsp_close(app: tauri::AppHandle, window: tauri::Window, root: String, path: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || {
        app.state::<AppState>().langs.close(&app, &owner, &root, &path);
        Ok(())
    })
    .await
}

/// 跳到定义。行、列从 0 数，列按 UTF-16
#[tauri::command]
pub async fn lsp_definition(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, line: u32, character: u32) -> Result<Vec<JavaLocationDto>, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(app.state::<AppState>().langs.definition(&app, &owner, &root, &path, line, character)?.into_iter().map(Into::into).collect())).await
}

/// 查用法（不含声明本身），带那一行的文字。超过上限截断，`total` 是真条数
#[tauri::command]
pub async fn lsp_references(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, line: u32, character: u32) -> Result<JavaRefsDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let (locs, total) = app.state::<AppState>().langs.references(&app, &owner, &root, &path, line, character)?;
        let texts = crate::java::line_texts(&locs);
        let items = locs.into_iter().zip(texts).map(|(l, text)| JavaLocationDto { text, ..JavaLocationDto::from(l) }).collect();
        Ok(JavaRefsDto { items, total: total as u32 })
    })
    .await
}

/// 悬停文档：一段 Markdown，没有是空串
#[tauri::command]
pub async fn lsp_hover(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, line: u32, character: u32) -> Result<String, String> {
    let owner = window.label().to_string();
    blocking(move || app.state::<AppState>().langs.hover(&app, &owner, &root, &path, line, character)).await
}

/// 补全。带着编辑器此刻的全文（先同步再问）
#[tauri::command]
pub async fn lsp_complete(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String, line: u32, character: u32) -> Result<JavaCompletionDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let (items, incomplete, total) = app.state::<AppState>().langs.complete(&app, &owner, &root, &path, &text, line, character)?;
        let items = items
            .into_iter()
            .map(|i| JavaCompletionItemDto { label: i.label, kind: i.kind, detail: i.detail, description: i.description, edit: i.edit.map(Into::into), insert: i.insert, snippet: i.snippet, sort: i.sort, raw: i.raw })
            .collect();
        Ok(JavaCompletionDto { items, incomplete, total: total as u32 })
    })
    .await
}

/// 认得的语言包（内置的 + 你自己的）：前端按文件名判断要不要接通用的语言服务器客户端。认不得的包文件在 `problems` 里，一条一句
#[tauri::command]
pub async fn lsp_packs(app: tauri::AppHandle) -> Result<LspPacksDto, String> {
    blocking(move || {
        let (packs, problems) = crate::langs::packs(&app);
        Ok(LspPacksDto { packs: packs.into_iter().map(|p| LspPackDto { id: p.id, name: p.name, files: p.files }).collect(), problems })
    })
    .await
}
