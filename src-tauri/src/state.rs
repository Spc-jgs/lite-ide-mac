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
    ptys: Mutex<HashMap<u32, Pty>>,
    /// 正在跑的远程操作（fetch / push）的取消令牌。
    ///
    /// 存的是令牌不是子进程句柄：kill 由 `gitsvc::remote` 里的看门线程做，
    /// 这儿只负责「让谁看得见这个开关」。**这样这张表上永远不会发生
    /// 「持着锁去 kill 一个子进程」**——那正是 `kill_pty` 踩过的坑。
    remotes: Mutex<HashMap<u32, gitsvc::remote::Cancel>>,
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

/// 一个活着的终端：会话本身，加上它的背压闸。
///
/// **两样东西必须绑在一起放**，不能开两张表。开两张的话「摘了会话忘了关闸」
/// 就成了一个随时可能漏的手工约定，而漏掉的表现是读线程永远卡在水位上 ——
/// 那条路的终点是 issue #2 那次界面永久卡死。
struct Pty {
    sess: Arc<Mutex<Session>>,
    flow: Arc<ptysvc::Flow>,
}

/// 同时能开几个终端（issue #18 第三条）。
///
/// 每个终端 = 一个 zsh + 一条读线程 + 一个**永不卸载**的 xterm 实例
/// （不卸载是有意的：组件一销毁 Session 就 drop，正在跑的命令全没了）。
/// 在这之前没有任何上限。
///
/// **16 不是「够用」的数，是「不正常」的界。** 人手动开到第 16 个终端
/// 基本不会发生；真到了这个数，多半是某处在循环调 `pty_spawn`，
/// 而那种情况下没有上限就是几十个 zsh 加几十条线程。
/// 所以它是一道防跑飞的闸，不是一条产品限制 —— 报错文案也这么写。
pub const MAX_PTYS: usize = 16;

impl AppState {
    /// 登记一个终端，返回它的 id 和背压闸。
    ///
    /// 满了就**拒绝**（调用方负责把 Session drop 掉，也就是 kill）。
    /// 拒绝而不是挤掉最老的那个：挤掉等于在用户不知情的时候
    /// 杀掉一个可能正在跑 gradle 的 shell。
    pub fn insert_pty(&self, sess: Arc<Mutex<Session>>) -> Result<(u32, Arc<ptysvc::Flow>), String> {
        let mut tab = self.ptys.lock().expect("pty 表锁被毒化");
        if tab.len() >= MAX_PTYS {
            return Err(format!(
                "已经开着 {MAX_PTYS} 个终端了，先关掉一个再开。\
                 （这不是产品限制，是一道防跑飞的闸 —— 每个终端都带着一个 zsh 和一条读线程）"
            ));
        }
        let id = self.next_pty.fetch_add(1, Ordering::Relaxed);
        let flow = Arc::new(ptysvc::Flow::new());
        tab.insert(id, Pty { sess, flow: Arc::clone(&flow) });
        Ok((id, flow))
    }

    pub fn pty(&self, id: u32) -> Option<Arc<Mutex<Session>>> {
        self.ptys.lock().expect("pty 表锁被毒化").get(&id).map(|p| Arc::clone(&p.sess))
    }

    /// 这个终端的背压闸。`pty_ack` 拿它把已消费的字节数减掉。
    pub fn pty_flow(&self, id: u32) -> Option<Arc<ptysvc::Flow>> {
        self.ptys.lock().expect("pty 表锁被毒化").get(&id).map(|p| Arc::clone(&p.flow))
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
        let got = self.ptys.lock().expect("pty 表锁被毒化").remove(&id);
        // 先关闸再让 Session 析构：读线程可能正卡在水位上等 ack，
        // 而它卡着就没人排空 pty master —— 退出中的 shell 会写满缓冲区
        // 卡在写上，`child.wait()` 永远等不到（issue #2 那条链路）
        if let Some(p) = &got {
            p.flow.close();
        }
        got.is_some()
    }

    /// 登记一个正在跑的远程操作，返回它的取消令牌。
    ///
    /// **id 由前端给，不是这里生成的。**
    ///
    /// 反过来（这里生成、跟着返回值给出去）写过一版，而那个取消按钮
    /// **永远点不动**：返回值要等操作跑完才到前端，那时已经没什么可取消的了。
    /// 这个 bug 在浏览器里点了一次取消、发现 `git_cancel` 压根没被调到才发现 ——
    /// 类型是对的、编译是过的、界面看着也对。
    /// **撞到同一个 id 就拒绝，不覆盖**（issue #18 第二条）。
    ///
    /// 原来是直接 `insert` 覆盖。两次操作用同一个 id 的话，先跑的那次的
    /// 取消令牌就从表里没了 —— 它还在跑，而**再也取消不掉**，
    /// 界面上那个取消按钮从此是个装饰。
    ///
    /// 今天打不到（前端用 `if (!repo || syncing) return` 挡着并发），
    /// 所以这是在把「防线只有一层」补成两层：前端那层是为了体验，
    /// 这层是为了「就算前端哪天改坏了，也不会丢掉杀死一个网络操作的能力」。
    pub fn begin_remote(&self, id: u32) -> Option<gitsvc::remote::Cancel> {
        let mut tab = self.remotes.lock().expect("远程操作表锁被毒化");
        if tab.contains_key(&id) {
            return None;
        }
        let flag: gitsvc::remote::Cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tab.insert(id, flag.clone());
        Some(flag)
    }

    /// 操作结束（成功、失败、被取消都算）时把登记划掉。
    pub fn end_remote(&self, id: u32) {
        self.remotes.lock().expect("远程操作表锁被毒化").remove(&id);
    }

    /// 取消一个正在跑的远程操作。找不到就返回 false（多半是已经结束了）。
    ///
    /// **只置位，不 kill。** 置位之后由那条操作自己的看门线程去 kill ——
    /// 这样这个函数永远是瞬间返回的，不会把表锁攥在手里等一个子进程死。
    pub fn cancel_remote(&self, id: u32) -> bool {
        let flag = self.remotes.lock().expect("远程操作表锁被毒化").get(&id).cloned();
        match flag {
            Some(f) => {
                f.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
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
            .map(|(_, p)| p)
            .collect();
        // 同 kill_pty：先把闸全关掉，再让它们析构
        for p in &all {
            p.flow.close();
        }
        drop(all);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /*
     * **撞号要拒绝，不能覆盖**（issue #18 第二条）。
     *
     * 原来是直接 `insert`：两次操作用同一个 id 的话，先跑的那次的取消令牌
     * 就从表里没了 —— 它还在跑，而再也取消不掉，界面上那个取消按钮
     * 从此是个装饰。今天靠前端的 `if (!repo || syncing) return` 挡着，
     * 这条测试是把那道单层防线补成两层。
     */
    #[test]
    fn 远程操作撞号要拒绝而不是覆盖() {
        let st = AppState::default();
        let 先来的 = st.begin_remote(7).expect("第一次登记该成功");
        assert!(st.begin_remote(7).is_none(), "撞号必须拒绝");

        // 关键在这儿：被拒绝之后，**先来的那个还取消得掉**
        assert!(st.cancel_remote(7), "找不到令牌了 —— 说明表里那条被覆盖或删掉了");
        assert!(先来的.load(Ordering::Relaxed), "取消没落到先来的那个令牌上");

        // 划掉之后同一个 id 可以再用：它是「正在跑的操作」的表，不是黑名单
        st.end_remote(7);
        assert!(st.begin_remote(7).is_some(), "结束之后这个 id 该能重新用");
    }

    /*
     * 终端数量的上限（issue #18 第三条）。
     *
     * **一个真 Session 的 Arc 复制 16 份**，不是起 16 个 zsh ——
     * 这张表存的就是 Arc，数的是表的长度，而起 16 个登录 shell
     * 只是为了测一个 `len() >= N` 去把机器跑满。
     */
    #[test]
    fn 终端开到上限就拒绝再开() {
        let st = AppState::default();
        let (sess, _reader) = ptysvc::Session::spawn("/tmp", 80, 24).expect("起不了 shell");

        let mut ids = Vec::new();
        for i in 0..MAX_PTYS {
            let (id, _flow) = st
                .insert_pty(Arc::clone(&sess))
                .unwrap_or_else(|e| panic!("第 {} 个就被拒了：{e}", i + 1));
            ids.push(id);
        }
        // `expect_err` 要 Ok 那侧实现 Debug，而 Flow 没有（也不该有：
        // 它里面是锁和条件变量，打印它没有意义）
        let e = match st.insert_pty(Arc::clone(&sess)) {
            Err(e) => e,
            Ok(_) => panic!("开到上限了还让开"),
        };
        // 报错要说清怎么办 —— 一句「失败」等于让人自己猜
        assert!(e.contains("先关掉一个"), "报错没告诉人该做什么：{e}");

        // 关掉一个就该能再开：这是一道闸，不是一次性的配额
        assert!(st.kill_pty(ids[0]));
        assert!(st.insert_pty(Arc::clone(&sess)).is_ok(), "关掉一个之后还开不了");
    }

    /*
     * `kill_pty` 必须把背压闸一起关掉。
     *
     * 不关的话，读线程可能正卡在水位上等一个永远不会来的 ack ——
     * 而它卡着就没人排空 pty master，退出中的 shell 写满缓冲区卡在写上，
     * `child.wait()` 永远等不到。那正是 issue #2 那次界面永久卡死的形状。
     */
    #[test]
    fn 关掉终端要把背压闸一起关掉() {
        let st = AppState::default();
        let (sess, _reader) = ptysvc::Session::spawn("/tmp", 80, 24).expect("起不了 shell");
        let (id, flow) = st.insert_pty(sess).unwrap();

        flow.sent(ptysvc::HIGH_WATER);
        let f = Arc::clone(&flow);
        let t = std::thread::spawn(move || f.wait_room());
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(!t.is_finished(), "水位满着还没卡住 —— 这条测试什么都没测到");

        assert!(st.kill_pty(id));
        assert!(!t.join().unwrap(), "kill_pty 之后读线程还等在闸上");
    }
}
