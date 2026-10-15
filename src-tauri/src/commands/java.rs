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
