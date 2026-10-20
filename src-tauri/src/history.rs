//! 本地历史接到保存上（docs/IDEA.md 第 21 节）。存、列、读、修剪在 `crates/histsvc`；这里只管「保存前读一眼盘上原来那份、保存后读回来记一笔」。
//! **记不成不影响保存**：本地历史是兜底的东西，它出错（盘满了、目录没权限）只记一句 WARN，保存照常算成功 —— 反过来就是为了一个附属功能把主路堵死。

use std::io::Read;
use std::path::{Path, PathBuf};
use tauri::{AppHandle, Manager};

pub fn store(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("history"))
}

/// 盘上的字节，最多读 `FILE_MAX + 1`（超了 histsvc 自己不存）。不存在 / 读不了给 None
pub fn read_disk(path: &Path) -> Option<Vec<u8>> {
    let f = std::fs::File::open(path).ok()?;
    let mut buf = Vec::new();
    f.take(histsvc::FILE_MAX + 1).read_to_end(&mut buf).ok()?;
    Some(buf)
}

/// 保存成功之后：读回盘上那份，和保存前那份（`old`）一起交给 histsvc
pub fn after_save(app: &AppHandle, path: &str, old: Option<Vec<u8>>) {
    let (Some(store), Some(new)) = (store(app), read_disk(Path::new(path))) else { return };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64);
    if let Err(e) = histsvc::record(&store, path, old.as_deref(), &new, now) {
        applog::write(applog::Level::Warn, "history", &format!("本地历史没记上（{path}）：{e}"));
    }
}
