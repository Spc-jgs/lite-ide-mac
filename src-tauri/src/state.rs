//! 会话表：handle ↔ 已打开的日志文件，以及每个会话上的过滤任务。
//!
//! 前端只拿到 u32 句柄，路径与 mmap 全留在 Rust 侧。

use logengine::{FilterTask, LogFile};
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AppState {
    files: Mutex<HashMap<u32, Arc<LogFile>>>,
    /// 每个文件当前生效的过滤任务。换条件时旧任务会被取消并替换。
    filters: Mutex<HashMap<u32, Arc<FilterTask>>>,
    next_handle: AtomicU32,
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
