//! 会话表：handle ↔ 已打开的日志文件，以及每个会话上的过滤任务。
//!
//! 前端只拿到 u32 句柄，路径与 mmap 全留在 Rust 侧。

use logengine::{FilterTask, LogFile};
use ptysvc::Session;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AppState {
    files: Mutex<HashMap<u32, Arc<LogFile>>>,
    /// 每个文件当前生效的过滤任务。换条件时旧任务会被取消并替换。
    filters: Mutex<HashMap<u32, Arc<FilterTask>>>,
    /// 活着的终端会话。Session::drop 会 kill 掉 shell，
    /// 所以从这张表里移除 == 终止那个终端（UNINSTALL.md 的「不留孤儿进程」）。
    ptys: Mutex<HashMap<u32, Arc<Mutex<Session>>>>,
    next_handle: AtomicU32,
    next_pty: AtomicU32,
}

impl AppState {
    pub fn insert(&self, file: LogFile) -> u32 {
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .insert(handle, Arc::new(file));
        handle
    }

    pub fn get(&self, handle: u32) -> Option<Arc<LogFile>> {
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .get(&handle)
            .cloned()
    }

    pub fn close(&self, handle: u32) -> bool {
        self.clear_filter(handle);
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .remove(&handle)
            .is_some()
    }

    /// 装上新的过滤任务，并取消上一个 —— 用户改关键字时旧扫描必须立刻停，
    /// 否则大文件上会堆积一串无用的后台扫描。
    pub fn set_filter(&self, handle: u32, task: Arc<FilterTask>) {
        let mut g = self.filters.lock().expect("过滤表锁被毒化");
        if let Some(old) = g.insert(handle, task) {
            old.cancel();
        }
    }

    pub fn filter(&self, handle: u32) -> Option<Arc<FilterTask>> {
        self.filters
            .lock()
            .expect("过滤表锁被毒化")
            .get(&handle)
            .cloned()
    }

    pub fn clear_filter(&self, handle: u32) {
        if let Some(old) = self.filters.lock().expect("过滤表锁被毒化").remove(&handle) {
            old.cancel();
        }
    }
}

impl AppState {
    pub fn insert_pty(&self, sess: Arc<Mutex<Session>>) -> u32 {
        let id = self.next_pty.fetch_add(1, Ordering::Relaxed);
        self.ptys.lock().expect("pty 表锁被毒化").insert(id, sess);
        id
    }

    pub fn pty(&self, id: u32) -> Option<Arc<Mutex<Session>>> {
        self.ptys.lock().expect("pty 表锁被毒化").get(&id).cloned()
    }

    pub fn kill_pty(&self, id: u32) -> bool {
        // 先把它**摘出来**，放掉表锁，再让它在锁外面析构。
        //
        // 原来是 `self.ptys.lock()….remove(&id).is_some()` —— 临时值的析构
        // 发生在语句末尾，那时候 MutexGuard 还活着，于是 Session::drop → kill()
        // 整个跑在锁里面。kill() 一慢，所有终端操作（开、写、改大小、关）
        // 全部堵在这把锁上；kill() 挂住就是永久堵死，只能重启应用。
        //
        // kill() 现在自己保证有界返回了（见 ptysvc），但**没有理由把一个
        // 可能起线程、发信号、等收尸的操作放在全局锁里**。issue #2。
        let sess = self.ptys.lock().expect("pty 表锁被毒化").remove(&id);
        sess.is_some()
    }

    /// 窗口关闭时兜底：把所有终端一并带走。
    ///
    /// 同样先摘出来再在锁外析构 —— 这条还在退出路径上，
    /// 卡住的表现是「点了关闭，窗口没反应」。
    pub fn kill_all_ptys(&self) {
        let all: Vec<_> = self
            .ptys
            .lock()
            .expect("pty 表锁被毒化")
            .drain()
            .map(|(_, s)| s)
            .collect();
        drop(all);
    }
}
