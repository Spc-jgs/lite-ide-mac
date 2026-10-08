//! 从系统送进来的「打开这个文件」（issue #40 第二层）。
//!
//! # macOS 上「打开文件」不是命令行参数
//!
//! Finder 双击、拖到 Dock 图标、右键「打开方式」、`open -a lite-ide x.log`——
//! 全部经过 Launch Services，它给目标应用发一个 `odoc` Apple Event；
//! **`argv` 里什么都没有**。只有直接 exec 二进制（`…/MacOS/lite-ide x.log`）
//! 才走 `argv`。原来只读 `std::env::args()`，于是上面四条路一条都不通，
//! 而 USAGE 里还教人 `open -a lite-ide /var/log/system.log` —— 实测
//! `app.log` 记的是 `boot=464ms tabs=0`：应用起来了，文件没开。
//!
//! Tauri 把那个 Apple Event 包成 `RunEvent::Opened { urls }`（macOS 独有），
//! 冷启动和已在运行都走它。**单实例是白捡的**：LS 发现应用已经在跑，
//! 就把事件发给它而不是再起一个进程，不需要 single-instance 插件。
//!
//! # 时序：事件可能比前端先到
//!
//! Finder 双击冷启动时，`Opened` 在事件循环一转就来，而前端那时还在加载，
//! 监听都没挂上 —— 直接 `emit` 就丢了。所以有收件箱：前端没就绪时先攒着，
//! 前端启动时用 `initial_paths` 一并取走（`argv` 里的在 `setup` 时也送进同一个收件箱，
//! 两条路汇进同一个口，前端不用知道文件是从哪条路来的）；取过一次之后再来的
//! 直接发事件。「攒还是发」和「取走」在同一把锁下判，中间没有缝。
//!
//! 多窗口之后（第 2 步）收件箱变成每个窗口一个，在 `windows.rs` 的登记表里；
//! 这里只剩「URL → 路径」和「判目录、交给登记表、定向发」。

use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Url};

/// 发给前端的事件名。负载是 `Vec<String>`（绝对路径）。
pub const EVENT: &str = "open-paths";

/// `file://` URL → 本地路径。
///
/// 百分号编码由 `to_file_path` 解（中文、空格都在里面）；它对非 `file` scheme
/// 直接返回 `Err`，所以 `https://…` `mailto:` 之类自然被丢掉 —— **别再加一道
/// `scheme() == "file"` 的过滤**，第一版加了，去掉测试照样绿，那是一行没人验得了的代码。
/// 不检查存在性 —— 那是前端 `openPath` 的事，它报错会说人话。
pub fn paths_from_urls(urls: &[Url]) -> Vec<String> {
    urls.iter()
        .filter_map(|u| u.to_file_path().ok())
        .map(|p: PathBuf| p.to_string_lossy().into_owned())
        .collect()
}

/// 把一批路径送到该去的窗口（issue #40 + 多窗口第 2、3 步）。
///
/// 往哪个窗口送由 `windows::route` 定（目录找开着它的窗口，文件找项目包含它的窗口，
/// 都没有就给前台）；那个窗口的前端还没起来就先攒在它的收件箱里；该开新窗口的
/// （没开着的目录而前台已经有项目、或者一个窗口都没有）就开一个。
/// 判目录要碰盘，所以在这儿做，登记表保持纯。
pub fn deliver(app: &AppHandle, paths: Vec<String>) {
    use tauri::Manager;
    if paths.is_empty() {
        return;
    }
    let items = paths
        .into_iter()
        .map(|p| {
            let dir = std::path::Path::new(&p).is_dir();
            (p, dir)
        })
        .collect();
    let d = app.state::<crate::state::AppState>().windows.deliver(items);
    for (label, paths) in d.now {
        crate::diag!("open-paths → {label} {paths:?}");
        let _ = app.emit_to(label.as_str(), EVENT, &paths);
        // 送到哪个窗口，哪个窗口到前面来：「再开一次 A」的意思就是「回到 A 那个窗口」，
        // Finder 双击一个属于 B 项目的文件，也该是 B 的窗口出来
        if let Some(w) = app.get_webview_window(&label) {
            let _ = w.set_focus();
        }
    }
    for paths in d.new {
        // 一个目录一个窗口，那个目录就是它的项目；散文件合开一个没有项目的窗口
        let root = match paths.as_slice() {
            [p] if std::path::Path::new(p).is_dir() => Some(p.clone()),
            _ => None,
        };
        crate::winctl::create(app, root, None, paths);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn u(s: &str) -> Url {
        Url::parse(s).unwrap()
    }

    #[test]
    fn 百分号编码的中文和空格要解回来() {
        let urls = [
            u("file:///Users/me/%E6%97%A5%E5%BF%97/app%20server.log"),
            u("file:///tmp/plain.txt"),
        ];
        assert_eq!(
            paths_from_urls(&urls),
            ["/Users/me/日志/app server.log", "/tmp/plain.txt"]
        );
    }

    #[test]
    fn 非_file_scheme_一律忽略() {
        let urls = [u("https://example.com/a.log"), u("file:///a.md"), u("mailto:x@y")];
        assert_eq!(paths_from_urls(&urls), ["/a.md"]);
    }
}
