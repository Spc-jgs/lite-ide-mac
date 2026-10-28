//! 文件树与文本读写。
//!
//! 与 logengine 同样的纪律：零 Tauri 依赖，可独立测试。
//!
//! 编码策略见 [`encoding`] 模块：**探测 → 记住 → 原样写回**。
//! 用什么编码读进来的就用什么编码存回去，保存不做「顺手转成 UTF-8」这种擅自决定。

pub mod editorconfig;
pub mod encoding;
pub mod eol;
pub mod watch;

use std::ffi::OsString;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

// 2026-10-12 打磨时按职责拆开（原来 2,394 行一个文件、九件不相干的事）。只搬了位置，没改逻辑；
// 对外的路径不变（`fsservice::xxx`），下面一层 `pub use` 全部导出
mod clipboard;
mod cli;
mod dir;
mod entries;
mod finder;
mod image;
mod scratch;
mod text;
mod trash;

pub use clipboard::*;
pub use cli::*;
pub use dir::*;
pub use entries::*;
pub use finder::*;
pub use image::*;
pub use scratch::*;
pub use text::*;
pub use trash::*;

/// 把用户给的路径统一成**盘上真正的那一条**，给前端当 key 用。
///
/// 目录整条 canonicalize；文件只 canonicalize **父目录**，最后一段照原样留着。
///
/// 为什么要做：git 会解析符号链接（`rev-parse --show-toplevel` 给的是
/// `/private/tmp/x`），而 `stat` / `open` 照单全收（`/tmp/x/a.txt` 就是
/// `/tmp/x/a.txt`）。前端拿标签的 path 和仓库根做前缀匹配 —— 改动标记、
/// 文件树的 git 字母 —— 两边形态不一样就全部静默失效，而且同一个文件从两条
/// 路径打开会开出两个标签。在入口统一一次，比在每个比较点各修一遍可靠。
///
/// 为什么最后一段不解析：`link.txt -> real/config.txt` 这种**文件本身是软链**
/// 的情形，用户点的是 `link.txt`，标签就该叫 `link.txt`、树里高亮的也是它；
/// 保存时 `write_text` 自己会跟到真身（见 `resolve_symlink`）。
/// 整条解析的话，打开的就成了 `config.txt`，而树里根本找不到这一行。
///
/// 解析不了（断掉的软链、没权限）就原样返回 —— 这一步是整理不是校验，
/// 校验由后面的 `metadata` 做。
pub fn canonical(path: impl AsRef<Path>) -> PathBuf {
    let p = path.as_ref();
    if p.is_dir() {
        return fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => fs::canonicalize(parent)
            .map(|d| d.join(name))
            .unwrap_or_else(|_| p.to_path_buf()),
        _ => p.to_path_buf(),
    }
}

#[cfg(test)]
mod tests;
