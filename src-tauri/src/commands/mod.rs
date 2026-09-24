//! Tauri 命令层。
//!
//! 纪律（ARCHITECTURE.md §5）：这里只做参数解包、句柄查找和错误转换，
//! 一行业务逻辑都不写 —— 业务全在 crates/logengine 里，才能脱离 Tauri 单测和 bench。
//!
//! 按领域分文件（2026-09-24 从一个 2,015 行的 commands.rs 拆开，纯搬家）：DTO 全在 `dto.rs`，
//! 命令在 `fs` / `scratch` / `log` / `pty` / `search` / `git` / `remote` / `app`。这里只留公共的
//! `blocking`，并把各文件重新导出 —— `lib.rs` 的注册表照旧写 `commands::xxx`。

use crate::state::AppState;
use logengine::{FilterSpec, Level, LevelMask, LogFile, Refreshed};
use std::path::Path;
use tauri::ipc::Response;
use tauri::State;

mod app;
mod dto;
mod fs;
mod git;
mod log;
mod pty;
mod remote;
mod scratch;
mod search;

pub use app::*;
pub use dto::*;
pub use fs::*;
pub use git::*;
pub use log::*;
pub use pty::*;
pub use remote::*;
pub use scratch::*;
pub use search::*;

/// 把一段**会阻塞**的活挪到 tokio 的阻塞池上。
///
/// # 为什么必须有这个
///
/// Tauri 的同步命令**跑在主线程上**（官方文档原话：「Commands without the
/// async keyword are executed on the main thread」）。主线程就是 NSApplication
/// 的事件循环 —— 它一堵，窗口就不响应了：菜单点不开、拖不动、转菊花。
///
/// 一开始只挪时长不可控的那几条（`commit` 的钩子、`switch` 检出、`rg`、落盘、
/// `fetch` / `push`），理由是 M 系列实测 `git status` 在 1.1GB / 3518 文件的仓库上
/// 只要 40ms —— 两帧半，不值得为它换来「命令之间不再串行」这个新变量。
///
/// 2026-09-17 又量了一次（issue #12），把 **git 的读写全部**挪了过来：
///
/// | 仓库 | `status` | 主线程被堵多久（IPC 心跳 p99） |
/// |---|---|---|
/// | 本仓库，热 | 11ms | 1ms（量不出） |
/// | 合成 60k 文件，热 | 75ms | **81ms** |
///
/// 80ms 是五帧，而且**每次保存都会来一次**（watch → `status`），正打在敲字的路上。
/// 冷缓存、iCloud / 网络卷、真正的大 Java 仓库只会更长 —— 那些这台机器上造不出来，
/// 但方向已经清楚了。「不再串行」那个变量的答案在前端：读操作的结果按序号丢弃过期的
/// （`git.refresh` 的 `seq`），写操作本来就有 issue #23 那道「仓库正在被写」的守卫。
///
/// # 为什么不是 `#[tauri::command(async)]`
///
/// 那个宏对同步函数生成的是 `async_runtime::spawn`，**活还是跑在 worker 上**
/// （`sync_threadpool` 只是个 tracing 标签，不是 spawn_blocking）。
/// 而 worker 只有 2 个（见 `lib.rs::install_runtime`），两条并发的 git
/// 就能把它占满。阻塞池是按需长、空闲自己收的，这才是这类活该待的地方。
async fn blocking<T, F>(f: F) -> Result<T, String>
where
    F: FnOnce() -> Result<T, String> + Send + 'static,
    T: Send + 'static,
{
    tauri::async_runtime::spawn_blocking(f)
        .await
        .map_err(|e| format!("后台任务没跑完：{e}"))?
}

#[cfg(test)]
mod tests {
    /// `blocking` 必须**真的把活挪到别的线程上** —— 这是它存在的全部理由。
    ///
    /// 退化成「原地调用一下」的话：编译照过、类型照对、返回值照样正确，
    /// 而主线程照样被堵住，界面照样转菊花。这种回归没有任何编译期信号，
    /// 只能靠一条断言线程 id 的测试卡着。
    #[test]
    fn blocking_把活挪到别的线程上() {
        let here = std::thread::current().id();
        let there = tauri::async_runtime::block_on(super::blocking(move || {
            Ok::<_, String>(std::thread::current().id())
        }))
        .expect("blocking 自己不该失败");
        assert_ne!(here, there, "blocking 没把活挪走，还在调用者的线程上");
    }
}
