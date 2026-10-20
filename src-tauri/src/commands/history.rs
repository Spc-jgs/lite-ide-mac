//! 本地历史（docs/IDEA.md 第 21 节）。业务在 `crates/histsvc`，接到保存上在 `crate::history`。碰盘，走阻塞池。

use super::*;

/// 这个文件存过的版本，最新的在前
#[tauri::command]
pub async fn history_list(app: tauri::AppHandle, path: String) -> Result<Vec<HistoryEntryDto>, String> {
    blocking(move || {
        let Some(store) = crate::history::store(&app) else { return Ok(Vec::new()) };
        Ok(histsvc::list(&store, &path)
            .into_iter()
            .map(|e| HistoryEntryDto { ms: e.ms, hash: format!("{:016x}", e.hash), size: e.size, kind: if e.kind == histsvc::Kind::Saved { "saved" } else { "before" }.into() })
            .collect())
    })
    .await
}

/// 一版的文字：存的是盘上的原始字节，按这个文件现在的编码解码（和打开文件同一条路）
#[tauri::command]
pub async fn history_read(app: tauri::AppHandle, path: String, hash: String, label: Option<String>) -> Result<String, String> {
    blocking(move || {
        let store = crate::history::store(&app).ok_or("找不到应用数据目录")?;
        let h = u64::from_str_radix(&hash, 16).map_err(|_| format!("不认识的版本号 {hash}"))?;
        let bytes = histsvc::read(&store, &path, h).map_err(|e| e.to_string())?;
        Ok(fsservice::encoding::decode_as(&bytes, label.as_deref().unwrap_or("UTF-8")).content)
    })
    .await
}
