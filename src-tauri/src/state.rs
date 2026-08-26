//! 会话表：handle ↔ 已打开的日志文件。
//!
//! 前端只拿到一个 u32 句柄，路径与 mmap 全留在 Rust 侧。

use logengine::LogFile;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AppState {
    files: Mutex<HashMap<u32, Arc<LogFile>>>,
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
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .remove(&handle)
            .is_some()
    }
}
