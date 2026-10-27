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
        let packs = packs
            .into_iter()
            .map(|p| {
                let install_size = p.npm.as_deref().and_then(crate::langs::npm_set).map(|x| x.2.to_string());
                LspPackDto { id: p.id, name: p.name, files: p.files, install_size }
            })
            .collect();
        Ok(LspPacksDto { packs, problems })
    })
    .await
}

/// 光标底下能不能改名（⇧F6 先问这个：不能改的地方不弹输入框）。null = 这里不能
#[tauri::command]
pub async fn lsp_prepare_rename(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String, line: u32, character: u32) -> Result<Option<JavaRenameTargetDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        Ok(app.state::<AppState>().langs.prepare_rename(&app, &owner, &root, &path, &text, line, character)?.map(|t| JavaRenameTargetDto {
            line: t.range.line,
            character: t.range.character,
            end_line: t.range.end_line,
            end_character: t.range.end_character,
            placeholder: t.placeholder,
        }))
    })
    .await
}

/// 改名：问语言服务器要改哪些，照跨文件替换那套落盘（`java::apply_rename`：撤销日志、开着的标签跟上）。结果的形状同替换
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn lsp_rename(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String, line: u32, character: u32, new_name: String, open: Vec<super::replace::OpenDocArg>) -> Result<ReplaceOutcomeDto, String> {
    let owner = window.label().to_string();
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = app.state::<AppState>().langs.rename(&app, &owner, &root, &path, &text, line, character, &new_name)?;
        let out = crate::java::apply_rename(&plan, std::path::Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(super::replace::outcome_dto(out))
    })
    .await
}

/// 格式化整份：回改动，前端一笔做（⌘Z 能撤，不写盘）
#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn lsp_format(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String, tab_size: u32, insert_spaces: bool) -> Result<Vec<JavaEditDto>, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(app.state::<AppState>().langs.format(&app, &owner, &root, &path, &text, tab_size, insert_spaces)?.into_iter().map(Into::into).collect())).await
}

/// 光标那儿（或选中的那几行）能做的修复（⌥↵）
#[tauri::command]
pub async fn lsp_code_actions(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, text: String, range: JavaRangeArg) -> Result<Vec<JavaActionDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let r = crate::java::Edit { line: range.line, character: range.character, end_line: range.end_line, end_character: range.end_character, text: String::new() };
        Ok(app.state::<AppState>().langs.code_actions(&app, &owner, &root, &path, &text, r)?.into_iter().map(|a| JavaActionDto { title: a.title, kind: a.kind, raw: a.raw }).collect())
    })
    .await
}

/// 做选中的那条修复：只改这一个文件的交回编辑器（不写盘，⌘Z 能撤）；改到别的文件的照改名那套落盘
#[tauri::command]
pub async fn lsp_apply_action(app: tauri::AppHandle, window: tauri::Window, root: String, path: String, raw: String, open: Vec<super::replace::OpenDocArg>) -> Result<JavaActionResultDto, String> {
    let owner = window.label().to_string();
    let journal = super::replace::journal_dir(&app)?;
    blocking(move || {
        let plan = app.state::<AppState>().langs.resolve_action(&app, &owner, &root, &path, &raw)?;
        if plan.renames.is_empty() && plan.files.iter().all(|(p, _)| *p == path) {
            let edits = plan.files.into_iter().flat_map(|(_, e)| e).map(Into::into).collect();
            return Ok(JavaActionResultDto { edits, outcome: None });
        }
        let out = crate::java::apply_rename(&plan, std::path::Path::new(&root), &journal, &super::replace::dirty_map(open))?;
        Ok(JavaActionResultDto { edits: Vec::new(), outcome: Some(super::replace::outcome_dto(out)) })
    })
    .await
}

/// 一键装语言服务器（npm 照编进应用的锁文件装进应用数据目录，核每个包的 sha512、不跑安装脚本）。要上网、几秒到几十秒，放阻塞池
#[tauri::command]
pub async fn lsp_install(app: tauri::AppHandle, id: String) -> Result<LspInstallDto, String> {
    blocking(move || {
        let (path, bytes) = app.state::<AppState>().langs.install(&app, &id)?;
        Ok(LspInstallDto { path: path.to_string_lossy().into_owned(), bytes })
    })
    .await
}
