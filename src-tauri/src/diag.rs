//! 开发期诊断通道。
//!
//! release 构建没有 devtools，WebView 里的前端是个黑盒 —— M0 调试时
//! 「前端到底跑没跑」耗掉的时间全在这上面。保留这条通道，但默认闭嘴：
//! 设 `LITE_IDE_DEBUG=1` 才输出到 stderr。

use std::sync::OnceLock;

pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("LITE_IDE_DEBUG").is_some())
}

/// 此刻的 Unix 毫秒：要和别处（验收脚本、前端的时间线）对时刻的诊断行带上它
pub fn now_ms() -> u128 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis())
}

/// 诊断输出。默认静默，`LITE_IDE_DEBUG=1` 打开。
#[macro_export]
macro_rules! diag {
    ($($arg:tt)*) => {
        if $crate::diag::enabled() {
            eprintln!("[diag] {}", format!($($arg)*));
        }
    };
}
