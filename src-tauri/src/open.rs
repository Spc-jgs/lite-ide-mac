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
//! 监听都没挂上 —— 直接 `emit` 就丢了。所以有 [`Inbox`]：前端没就绪时先攒着，
//! 前端启动时用 `initial_paths` 一并取走（同时把 `argv` 里的也带上，两条路
//! 汇进同一个口，前端不用知道文件是从哪条路来的）；取过一次之后再来的
//! 直接发事件。「攒还是发」和「取走」在同一把锁下判，中间没有缝。

use std::path::PathBuf;
use std::sync::Mutex;
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

/// 前端就绪之前攒下来的路径。
#[derive(Default)]
pub struct Inbox {
    /// `(前端取过一次了没, 攒着的路径)`。两个字段放在同一把锁下，
    /// 「攒还是发」的判断和「取走并标记就绪」才不会交错。
    inner: Mutex<(bool, Vec<String>)>,
}

impl Inbox {
    /// 送一批路径给前端：就绪了就发事件，没就绪先攒着。
    pub fn deliver(&self, app: &AppHandle, paths: Vec<String>) {
        if paths.is_empty() {
            return;
        }
        let mut g = self.inner.lock().expect("open inbox 锁被毒化");
        if g.0 {
            crate::diag!("open-paths emit {paths:?}");
            let _ = app.emit(EVENT, &paths);
        } else {
            crate::diag!("open-paths 攒着（前端未就绪）{paths:?}");
            g.1.extend(paths);
        }
    }

    /// 前端启动时取走攒着的，并从此改为直接发事件。
    pub fn take(&self) -> Vec<String> {
        let mut g = self.inner.lock().expect("open inbox 锁被毒化");
        g.0 = true;
        std::mem::take(&mut g.1)
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

    #[test]
    fn 攒着的要能整批取走且只取一次() {
        let inbox = Inbox::default();
        {
            let mut g = inbox.inner.lock().unwrap();
            g.1.push("/a".into());
            g.1.push("/b".into());
        }
        assert_eq!(inbox.take(), ["/a", "/b"]);
        assert!(inbox.take().is_empty(), "取过就空了");
        assert!(inbox.inner.lock().unwrap().0, "取过一次之后标记为就绪");
    }
}
