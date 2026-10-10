//! 会话表：handle ↔ 已打开的日志文件，以及每个会话上的过滤任务。
//!
//! 前端只拿到 u32 句柄，路径与 mmap 全留在 Rust 侧。
//!
//! # 每样资源都记着它属于哪个窗口（多窗口第 1 步，docs/MULTIWINDOW.md 3.2）
//!
//! 日志句柄、终端、文件监听、远程操作都登记了 `owner`（窗口的 label）。
//! 窗口销毁时 [`AppState::release_window`] 只清它自己的那一份。原来是
//! 「任何一个窗口 `Destroyed` 就 `kill_all_ptys`」—— 单窗口时没区别，
//! 开两个窗口就是「关掉 A，B 里正在跑的 gradle 一起没了」。
//!
//! owner 由命令层从 Tauri 注入的 `Window` 里取，**前端不传** ——
//! 前端传的可能错，Rust 注入的一定对。
//!
//! 句柄 / 终端 id 仍然是**全进程唯一**的（原子计数），所以 `log_*`、`pty_write`
//! 这些按 id 找的路径不用认 owner。远程操作的 id 例外：它是前端发的
//! （发号必须早于用号，见 rust.md），每个窗口都从 1 数起，所以那张表按
//! `(owner, id)` 记。

use logengine::{FilterTask, LogFile};
use ptysvc::Session;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub struct AppState {
    /// 句柄 → (owner, 日志文件)
    files: Mutex<HashMap<u32, (String, Arc<LogFile>)>>,
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
    ///
    /// 键是 `(owner, op_id)`：op_id 由前端发、每个窗口各数各的，两个窗口同时
    /// fetch 都会用 1 号 —— 只按 id 记的话，后来的那个会被当成撞号拒绝。
    remotes: Mutex<HashMap<(String, u32), gitsvc::remote::Cancel>>,
    /// 项目根上的文件系统监听（issue #33 ⑳），**每个窗口最多一个**：换项目根就换掉，
    /// 旧的 drop 即停。原来整个进程只有一个槽位，第二个窗口打开项目会把第一个的顶掉。
    /// **先摘出来再在锁外 drop**，同 pty 那条 —— drop 要等防抖线程退出，
    /// 持着锁等就是在锁里做慢事。
    watch: Mutex<HashMap<String, fsservice::watch::Watch>>,
    /// 开着哪些窗口、谁在前台、各自的项目根和收件箱（多窗口第 2 步，见 `windows.rs`）。
    /// 原来这里是一个全局的 `open::Inbox`，现在每个窗口一个，收在登记表里
    pub windows: crate::windows::Windows,
    /// 每个窗口最近一次的替换扫描（#42）。替换串变了只重算「改后」不重新扫盘（一次完整扫描约 0.45s），
    /// 执行时也用它核对 —— 所以要留着。按窗口记 owner，窗口关了就收（rust.md「活的资源表」）
    scans: Mutex<HashMap<String, Arc<replacesvc::Scan>>>,
    /// 设置（issue #44）：`settings.json` + `ui-state.json`，整个进程一份，变了广播给每个窗口（`settingsctl.rs`）
    pub settings: crate::settings::Store,
    /// 跑着（或跑完了、标签还开着）的任务（#48）。同终端：记 owner，窗口关了只收它自己的
    runs: Mutex<HashMap<u32, Run>>,
    /// 「哪些任务还在跑」的账本（#48 第 4 步），落在 `<应用数据>/runs/live.json`：应用崩了之后收尸用（`tasksvc::live`）
    live: Mutex<LiveBook>,
    /// 摘出了运行表、正在软停收尾的任务（关掉一格、关窗口、重跑时的旧那次）。退出时它们也要等、也要强杀 ——
    /// 原来只活在收尾线程里：关掉一格 5 秒内 ⌘Q，线程跟着进程没了，SIGKILL 发不出去，不理 SIGINT 的就留成孤儿（code review 2026-10-10）。
    /// `Arc` 包一层：收尾线程停完要把自己从这儿划掉
    retiring: Arc<Mutex<Vec<Arc<tasksvc::Task>>>>,
    /// 正在起的（窗口, 项目, 任务名）。连按两次 ⌃R：两条 `task_run` 都找到同一个旧的、各起一份，一份从界面上消失还占着端口（同上）
    starting: Mutex<HashSet<(String, String, String)>>,
    next_handle: AtomicU32,
    next_pty: AtomicU32,
    next_run: AtomicU32,
}

/// `live.json` 在内存里的那份。`stale` 是启动时发现的「上次没停干净」的那几个 —— 人点了「结束」或「留着」才从账上划掉，
/// 没处理之前它们也留在文件里：处理之前应用又崩一次，下次还得找得到它们
#[derive(Default)]
struct LiveBook {
    path: Option<std::path::PathBuf>,
    list: Vec<tasksvc::live::Live>,
    stale: Vec<tasksvc::live::Live>,
}

impl LiveBook {
    fn save(&self) {
        if let Some(p) = &self.path {
            if let Err(e) = tasksvc::live::save(p, &self.list) {
                applog::write(applog::Level::Warn, "task", &format!("live.json 写不进去：{e}（崩溃后就找不到这次的任务了）"));
            }
        }
    }
}

/// 一次任务运行。`task` 是 `Arc`：停的时候要有一份活过宽限期（`Task` 被 drop 就是立刻 SIGKILL）
pub struct Run {
    pub owner: String,
    pub root: String,
    pub name: String,
    pub task: Arc<tasksvc::Task>,
}

/// 收尾的后半段：软停、等它停完（最多一个宽限期），从收尾表里划掉。**登记**由调用方在这之前同步做 ——
/// 放进后台线程再登记的话，线程还没跑起来时 ⌘Q，退出流程看不见它
fn finish_retire(list: &Mutex<Vec<Arc<tasksvc::Task>>>, task: &Arc<tasksvc::Task>) {
    task.stop_wait(tasksvc::GRACE);
    list.lock().expect("收尾表锁被毒化").retain(|t| !Arc::ptr_eq(t, task));
}

/// [`AppState::begin_run`] 的认领：丢掉时释放（起完、登记进运行表之后再丢）
pub struct RunClaim<'a> {
    st: &'a AppState,
    key: (String, String, String),
}

impl Drop for RunClaim<'_> {
    fn drop(&mut self) {
        self.st.starting.lock().expect("起任务的认领表锁被毒化").remove(&self.key);
    }
}

impl AppState {
    /// 记下这个窗口最近一次的替换扫描（顶掉上一次）
    pub fn set_scan(&self, owner: &str, scan: Option<replacesvc::Scan>) {
        let old = {
            let mut t = self.scans.lock().expect("替换扫描表锁被毒化");
            match scan {
                Some(s) => t.insert(owner.to_string(), Arc::new(s)),
                None => t.remove(owner),
            }
        };
        drop(old);
    }

    pub fn scan(&self, owner: &str) -> Option<Arc<replacesvc::Scan>> {
        self.scans.lock().expect("替换扫描表锁被毒化").get(owner).cloned()
    }

    /// 登记一个打开的日志，记在 `owner` 窗口名下
    pub fn insert(&self, owner: &str, file: LogFile) -> u32 {
        let handle = self.next_handle.fetch_add(1, Ordering::Relaxed);
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .insert(handle, (owner.to_string(), Arc::new(file)));
        handle
    }

    pub fn get(&self, handle: u32) -> Option<Arc<LogFile>> {
        self.files
            .lock()
            .expect("会话表锁被毒化")
            .get(&handle)
            .map(|(_, f)| Arc::clone(f))
    }

    /// 文件被轮转 / 截断了，**同一个句柄**换成按名重开的那份（`tail -F` 的语义）。
    ///
    /// 句柄不变是关键：前端标签上只记句柄，换句柄就得重开标签、过滤和 tail 全丢。
    /// logrotate 是「改名 + 新建同名」，人要看的一直是**这个名字**，不是那个 inode。
    /// 过滤任务一并清掉 —— 它扫的是旧文件的行号，对新文件毫无意义；前端收到
    /// `rotated` 会按原条件重跑一遍。
    ///
    /// 改名和新建之间有个空档（几毫秒到几秒，看 logrotate 的配置），那一瞬
    /// `open` 会 `NotFound` —— 原样返回，**不动表里的旧文件**，前端下一轮再试。
    /// 旧的 `LogFile` 在换掉之后由 Arc 自然析构，析构会叫停它的后台扫描。
    pub fn reopen(&self, handle: u32) -> std::io::Result<bool> {
        let Some(old) = self.get(handle) else { return Ok(false) };
        let fresh = LogFile::open(old.path())?;
        self.clear_filter(handle);
        // 原地换掉文件、owner 不动：轮转是同一个窗口里的同一个标签
        let old = self
            .files
            .lock()
            .expect("会话表锁被毒化")
            .get_mut(&handle)
            .map(|(_, f)| std::mem::replace(f, Arc::new(fresh)));
        // 读和换之间这个句柄可能刚被关掉：那就什么都不换。原来是直接 `insert`，
        // 会把一个已经关掉的句柄重新塞回表里，从此没人再关它
        Ok(old.is_some())
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
    /// 哪个窗口开的。窗口销毁时只杀它自己的（`release_window`）
    owner: String,
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
    /// 启动时读 `live.json`：还活着的（组长启动时间对得上）就是上次没停干净的，留着等人处理；已经没了的划掉
    pub fn init_live(&self, path: std::path::PathBuf) {
        let mut b = self.live.lock().expect("live 账本锁被毒化");
        let stale: Vec<_> = tasksvc::live::load(&path).into_iter().filter(tasksvc::live::still_running).collect();
        b.path = Some(path);
        b.list = stale.clone();
        b.stale = stale;
        b.save();
    }

    pub fn live_add(&self, l: tasksvc::live::Live) {
        let mut b = self.live.lock().expect("live 账本锁被毒化");
        b.list.push(l);
        b.save();
    }

    pub fn live_remove(&self, pgid: i32) {
        let mut b = self.live.lock().expect("live 账本锁被毒化");
        b.list.retain(|l| l.pgid != pgid);
        b.save();
    }

    /// 这个项目上次没停干净的（还活着的才算：启动之后人可能已经在终端里自己 kill 掉了）
    pub fn stale_for(&self, root: &str) -> Vec<tasksvc::live::Live> {
        let b = self.live.lock().expect("live 账本锁被毒化");
        b.stale.iter().filter(|l| l.root == root && tasksvc::live::still_running(l)).cloned().collect()
    }

    /// 从账上划掉这个项目上次没停干净的那几个，交给调用方处理（结束或者不管）
    pub fn stale_take(&self, root: &str) -> Vec<tasksvc::live::Live> {
        let mut b = self.live.lock().expect("live 账本锁被毒化");
        let (mine, rest): (Vec<_>, Vec<_>) = std::mem::take(&mut b.stale).into_iter().partition(|l| l.root == root);
        b.stale = rest;
        b.list.retain(|l| !mine.iter().any(|m| m.pgid == l.pgid));
        b.save();
        mine
    }

    /// 这个进程组是不是我们起过的任务（这次跑的，或者上次没停干净的）：端口卡片上说「是你上次跑的『后端』」
    pub fn task_named_by_group(&self, pgid: i32) -> Option<String> {
        self.live.lock().expect("live 账本锁被毒化").list.iter().find(|l| l.pgid == pgid).map(|l| l.name.clone())
    }

    pub fn insert_run(&self, run: Run) -> u32 {
        let id = self.next_run.fetch_add(1, Ordering::Relaxed);
        self.runs.lock().expect("任务表锁被毒化").insert(id, run);
        id
    }

    pub fn run_task(&self, id: u32) -> Option<Arc<tasksvc::Task>> {
        self.runs.lock().expect("任务表锁被毒化").get(&id).map(|r| Arc::clone(&r.task))
    }

    /// 这个窗口里同一个项目、同一个名字的那次（重跑要先停它，不然端口还占着）
    pub fn find_run(&self, owner: &str, root: &str, name: &str) -> Option<(u32, Arc<tasksvc::Task>)> {
        let tab = self.runs.lock().expect("任务表锁被毒化");
        tab.iter().find(|(_, r)| r.owner == owner && r.root == root && r.name == name).map(|(id, r)| (*id, Arc::clone(&r.task)))
    }

    /// 摘掉一条。**在锁外**处理它（同 kill_pty：别在表锁里做发信号、等收尸这类事）
    pub fn remove_run(&self, id: u32) -> Option<Run> {
        self.runs.lock().expect("任务表锁被毒化").remove(&id)
    }

    /// 退出前：软停全部（SIGINT 整组），返回要等的 —— 连同已经在收尾的那些，调用方等它们一会儿
    pub fn stop_all_runs(&self) -> Vec<Arc<tasksvc::Task>> {
        let all: Vec<Arc<tasksvc::Task>> = self.runs.lock().expect("任务表锁被毒化").values().map(|r| Arc::clone(&r.task)).collect();
        for t in &all {
            if t.alive() {
                t.stop(tasksvc::GRACE);
            }
        }
        let retiring = self.retiring.lock().expect("收尾表锁被毒化").clone();
        all.into_iter().chain(retiring).collect()
    }

    /// 进程真要结束了（`RunEvent::Exit`）：还活着的一律 SIGKILL，**包括正在收尾的**。`process::exit` 不跑析构，
    /// 不在这儿杀，`Task::drop` 那道兜底一次都轮不到 —— 任务在自己的进程组里，不会跟着应用死
    pub fn kill_all_runs(&self) {
        let all: Vec<Arc<tasksvc::Task>> = self.runs.lock().expect("任务表锁被毒化").drain().map(|(_, r)| r.task).collect();
        let retiring = self.retiring.lock().expect("收尾表锁被毒化").clone();
        for t in all.iter().chain(&retiring) {
            if t.alive() {
                t.kill();
            }
        }
    }

    /// 软停一个已经摘出运行表的任务，**在后台线程里攥着它**直到停完：`Arc` 在这条线程里还活着，不会因为最后一份引用没了就当场
    /// SIGKILL（那样 Spring 的关闭钩子就没机会跑）。停完之前它在收尾表里，退出流程看得见它
    pub fn retire(&self, task: Arc<tasksvc::Task>) {
        self.retiring.lock().expect("收尾表锁被毒化").push(Arc::clone(&task));
        let list = Arc::clone(&self.retiring);
        let _ = std::thread::Builder::new().name("task-retire".into()).spawn(move || finish_retire(&list, &task));
    }

    /// 要起一个任务之前：认领（窗口, 项目, 名字），同名的上一次还在跑就**先停干净**（端口要先空出来，TASKS.md「重跑」）。
    /// **会阻塞**（最多一个宽限期）。同一个正在起（连按两次 ⌃R）→ Err。认领在返回值丢掉时释放 —— 调用方起完、登记进运行表之后再丢
    pub fn begin_run(&self, owner: &str, root: &str, name: &str) -> Result<RunClaim<'_>, String> {
        let key = (owner.to_string(), root.to_string(), name.to_string());
        if !self.starting.lock().expect("起任务的认领表锁被毒化").insert(key.clone()) {
            return Err(format!("「{name}」正在起，等它起来再按"));
        }
        let claim = RunClaim { st: self, key };
        if let Some((old, task)) = self.find_run(owner, root, name) {
            self.remove_run(old);
            if task.alive() {
                // 同 retire，只是就在这条线程里等：收尾表里登记着再停，这几秒里 ⌘Q，退出流程照样看得见它
                self.retiring.lock().expect("收尾表锁被毒化").push(Arc::clone(&task));
                finish_retire(&self.retiring, &task);
            }
        }
        Ok(claim)
    }

    /// 按进程组找我们自己还开着的任务：端口卡片上「结束它」要走那个 `Task` 停（不然它的退出被记成失败）
    pub fn task_by_group(&self, pgid: i32) -> Option<Arc<tasksvc::Task>> {
        self.runs.lock().expect("任务表锁被毒化").values().find(|r| r.task.pgid() == pgid).map(|r| Arc::clone(&r.task))
    }

    /// 登记一个终端，返回它的 id 和背压闸。
    ///
    /// 满了就**拒绝**（调用方负责把 Session drop 掉，也就是 kill）。
    /// 拒绝而不是挤掉最老的那个：挤掉等于在用户不知情的时候
    /// 杀掉一个可能正在跑 gradle 的 shell。
    ///
    /// 上限按**整个进程**算，不按窗口：它防的是跑飞，闸要装在总阀门上。
    pub fn insert_pty(&self, owner: &str, sess: Arc<Mutex<Session>>) -> Result<(u32, Arc<ptysvc::Flow>), String> {
        let mut tab = self.ptys.lock().expect("pty 表锁被毒化");
        if tab.len() >= MAX_PTYS {
            return Err(format!(
                "已经开着 {MAX_PTYS} 个终端了，先关掉一个再开。\
                 （这不是产品限制，是一道防跑飞的闸 —— 每个终端都带着一个 zsh 和一条读线程）"
            ));
        }
        let id = self.next_pty.fetch_add(1, Ordering::Relaxed);
        let flow = Arc::new(ptysvc::Flow::new());
        tab.insert(id, Pty { sess, flow: Arc::clone(&flow), owner: owner.to_string() });
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
    ///
    /// 「撞号」只在**同一个窗口**里算：别的窗口用同一个数是正常的（各数各的）。
    pub fn begin_remote(&self, owner: &str, id: u32) -> Option<gitsvc::remote::Cancel> {
        let mut tab = self.remotes.lock().expect("远程操作表锁被毒化");
        let key = (owner.to_string(), id);
        if tab.contains_key(&key) {
            return None;
        }
        let flag: gitsvc::remote::Cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        tab.insert(key, flag.clone());
        Some(flag)
    }

    /// 操作结束（成功、失败、被取消都算）时把登记划掉。
    pub fn end_remote(&self, owner: &str, id: u32) {
        self.remotes.lock().expect("远程操作表锁被毒化").remove(&(owner.to_string(), id));
    }

    /// 取消一个正在跑的远程操作。找不到就返回 false（多半是已经结束了）。
    ///
    /// **只置位，不 kill。** 置位之后由那条操作自己的看门线程去 kill ——
    /// 这样这个函数永远是瞬间返回的，不会把表锁攥在手里等一个子进程死。
    pub fn cancel_remote(&self, owner: &str, id: u32) -> bool {
        let flag = self.remotes.lock().expect("远程操作表锁被毒化").get(&(owner.to_string(), id)).cloned();
        match flag {
            Some(f) => {
                f.store(true, Ordering::Relaxed);
                true
            }
            None => false,
        }
    }

    /// 换上 `owner` 窗口的新监听（或 None = 停掉）。旧的在锁外析构
    pub fn set_watch(&self, owner: &str, w: Option<fsservice::watch::Watch>) {
        let old = {
            let mut tab = self.watch.lock().unwrap_or_else(|e| e.into_inner());
            match w {
                Some(w) => tab.insert(owner.to_string(), w),
                None => tab.remove(owner),
            }
        };
        drop(old);
    }

    /// 窗口销毁时，把**这个窗口**名下的东西全部收掉：终端、日志句柄、文件监听、远程操作。
    /// 别的窗口的一样都不碰。
    ///
    /// 原来这里是 `kill_all_ptys`：任何一个窗口 `Destroyed` 就杀掉所有终端，日志句柄、监听、
    /// 远程操作则一概不管（单窗口时进程紧跟着就退了，看不出来）。
    ///
    /// **每张表都是先摘出来、放掉锁、再在锁外析构**，同 `kill_pty`：这条在关窗口的路上，
    /// `Session::drop` 要杀进程、`Watch::drop` 要等防抖线程退出 —— 持着锁做这些，
    /// 卡住的表现是「点了关闭，窗口没反应」，还会把别的窗口的终端操作一起堵住。
    ///
    /// ⌘Q **不走这里**：那条路上没有 `Destroyed`（多窗口第 0 步实测，docs/MULTIWINDOW.md 3.6），
    /// 进程直接结束，pty 由内核收掉 master 时给 shell 发 SIGHUP。
    pub fn release_window(&self, owner: &str) {
        let ptys: Vec<Pty> = {
            let mut tab = self.ptys.lock().expect("pty 表锁被毒化");
            let ids: Vec<u32> = tab.iter().filter(|(_, p)| p.owner == owner).map(|(id, _)| *id).collect();
            ids.iter().filter_map(|id| tab.remove(id)).collect()
        };
        // 同 kill_pty：先把闸全关掉，再让它们析构
        for p in &ptys {
            p.flow.close();
        }
        drop(ptys);

        let logs: Vec<u32> = self
            .files
            .lock()
            .expect("会话表锁被毒化")
            .iter()
            .filter(|(_, (o, _))| o == owner)
            .map(|(h, _)| *h)
            .collect();
        for h in logs {
            self.close(h);
        }

        self.set_watch(owner, None);
        self.set_scan(owner, None);

        // 这个窗口的任务：先礼后兵地停（宽限期在后台线程里等，窗口关掉不用等它们）
        let runs: Vec<Run> = {
            let mut tab = self.runs.lock().expect("任务表锁被毒化");
            let ids: Vec<u32> = tab.iter().filter(|(_, r)| r.owner == owner).map(|(id, _)| *id).collect();
            ids.iter().filter_map(|id| tab.remove(id)).collect()
        };
        for r in runs {
            if r.task.alive() {
                self.retire(r.task);
            }
        }

        // 只置位不 kill，理由同 cancel_remote。关掉的窗口没人看进度了，
        // 网络卡住的 fetch 不取消的话就再也没人能取消它
        let flags: Vec<gitsvc::remote::Cancel> = {
            let mut tab = self.remotes.lock().expect("远程操作表锁被毒化");
            let keys: Vec<(String, u32)> = tab.keys().filter(|(o, _)| o == owner).cloned().collect();
            keys.iter().filter_map(|k| tab.remove(k)).collect()
        };
        for f in flags {
            f.store(true, Ordering::Relaxed);
        }
    }
}

#[cfg(test)]
impl AppState {
    fn watched(&self, owner: &str) -> bool {
        self.watch.lock().unwrap().contains_key(owner)
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
        let 先来的 = st.begin_remote("main", 7).expect("第一次登记该成功");
        assert!(st.begin_remote("main", 7).is_none(), "撞号必须拒绝");

        // 关键在这儿：被拒绝之后，**先来的那个还取消得掉**
        assert!(st.cancel_remote("main", 7), "找不到令牌了 —— 说明表里那条被覆盖或删掉了");
        assert!(先来的.load(Ordering::Relaxed), "取消没落到先来的那个令牌上");

        // 划掉之后同一个 id 可以再用：它是「正在跑的操作」的表，不是黑名单
        st.end_remote("main", 7);
        assert!(st.begin_remote("main", 7).is_some(), "结束之后这个 id 该能重新用");
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
        // 用 `/bin/sh` 不用用户的 `$SHELL`（`Session::spawn_with`，issue #30）：
        // 被测的是这张表的长度，跑一遍别人的 `.zshrc` 只会把
        // 「他装了什么版本管理器」变成这条测试的判据之一
        let (sess, _reader) =
            ptysvc::Session::spawn_with("/bin/sh", "/tmp", 80, 24).expect("起不了 shell");

        let mut ids = Vec::new();
        for i in 0..MAX_PTYS {
            let (id, _flow) = st
                .insert_pty("main", Arc::clone(&sess))
                .unwrap_or_else(|e| panic!("第 {} 个就被拒了：{e}", i + 1));
            ids.push(id);
        }
        // `expect_err` 要 Ok 那侧实现 Debug，而 Flow 没有（也不该有：
        // 它里面是锁和条件变量，打印它没有意义）
        let e = match st.insert_pty("main", Arc::clone(&sess)) {
            Err(e) => e,
            Ok(_) => panic!("开到上限了还让开"),
        };
        // 报错要说清怎么办 —— 一句「失败」等于让人自己猜
        assert!(e.contains("先关掉一个"), "报错没告诉人该做什么：{e}");

        // 关掉一个就该能再开：这是一道闸，不是一次性的配额
        assert!(st.kill_pty(ids[0]));
        assert!(st.insert_pty("main", Arc::clone(&sess)).is_ok(), "关掉一个之后还开不了");
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
        let (sess, _reader) = ptysvc::Session::spawn_with("/bin/sh", "/tmp", 80, 24).expect("起不了 shell");
        let (id, flow) = st.insert_pty("main", sess).unwrap();

        flow.sent(ptysvc::HIGH_WATER);
        let f = Arc::clone(&flow);
        let t = std::thread::spawn(move || f.wait_room());
        std::thread::sleep(std::time::Duration::from_millis(80));
        assert!(!t.is_finished(), "水位满着还没卡住 —— 这条测试什么都没测到");

        assert!(st.kill_pty(id));
        assert!(!t.join().unwrap(), "kill_pty 之后读线程还等在闸上");
    }

    /// logrotate 是「改名 + 新建同名」：句柄要还是那个句柄，背后换成新文件，
    /// 过滤任务清掉。改名和新建之间那一瞬 `reopen` 要报错而不是把旧文件弄丢
    #[test]
    fn 轮转后按名重开_句柄不变() {
        let d = std::env::temp_dir().join(format!("lite-ide-reopen-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let p = d.join("app.log");
        std::fs::write(&p, "a\nb\nc\n").unwrap();

        let st = AppState::default();
        let h = st.insert("main", LogFile::open(&p).unwrap());
        assert_eq!(st.get(h).unwrap().stat().line_count, 3);
        let spec = logengine::FilterSpec {
            levels: logengine::LevelMask::ALL,
            text: logengine::TextFilter::single(b"a".to_vec()),
            case_sensitive: true,
            collapse_stacks: false,
        };
        let task = st.get(h).unwrap().start_filter(spec).unwrap();
        st.set_filter(h, task);

        // 空档：文件改名走了、新的还没建 —— 报错，旧的原样留着
        std::fs::rename(&p, d.join("app.log.1")).unwrap();
        assert!(st.reopen(h).is_err(), "文件不在时要报错让前端下一轮再试");
        assert_eq!(st.get(h).unwrap().stat().line_count, 3, "报错不能把旧文件弄丢");

        // 新文件出现：同一个句柄换成它，过滤任务清掉
        std::fs::write(&p, "x\n").unwrap();
        assert!(st.reopen(h).unwrap());
        assert_eq!(st.get(h).unwrap().stat().line_count, 1, "句柄背后应该是新文件");
        assert!(st.filter(h).is_none(), "旧过滤扫的是旧文件的行号，必须清掉");
        assert!(!st.reopen(999).unwrap(), "不存在的句柄：不是错误，只是没东西可换");
        let _ = std::fs::remove_dir_all(&d);
    }

    /*
     * 多窗口第 1 步（docs/MULTIWINDOW.md 3.2）：**关掉一个窗口，只收它自己的东西。**
     *
     * 原来任何一个窗口 `Destroyed` 就 `kill_all_ptys` —— 开两个窗口时，关掉 A，
     * B 里正在跑的 gradle 一起没了。四张表各放两个窗口的东西，收掉 a，逐张看 b 还在。
     * 终端仍然是一个真 Session 的 Arc 复制两份（同「终端开到上限」那条的理由）。
     */
    #[test]
    fn 关掉一个窗口只收它自己的东西() {
        let st = AppState::default();
        let (sess, _reader) = ptysvc::Session::spawn_with("/bin/sh", "/tmp", 80, 24).expect("起不了 shell");
        let (pa, fa) = st.insert_pty("a", Arc::clone(&sess)).unwrap();
        let (pb, fb) = st.insert_pty("b", Arc::clone(&sess)).unwrap();

        let d = std::env::temp_dir().join(format!("lite-ide-release-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("wa")).unwrap();
        std::fs::create_dir_all(d.join("wb")).unwrap();
        let log = d.join("x.log");
        std::fs::write(&log, "a\nb\n").unwrap();
        let la = st.insert("a", LogFile::open(&log).unwrap());
        let lb = st.insert("b", LogFile::open(&log).unwrap());

        st.set_watch("a", Some(fsservice::watch::watch(d.join("wa"), |_| {}).unwrap()));
        st.set_watch("b", Some(fsservice::watch::watch(d.join("wb"), |_| {}).unwrap()));

        let ra = st.begin_remote("a", 1).unwrap();
        let rb = st.begin_remote("b", 1).unwrap();

        let q = replacesvc::Query::literal("a");
        st.set_scan("a", Some(replacesvc::scan(&d, &[], false, &q, &Default::default()).unwrap()));
        st.set_scan("b", Some(replacesvc::scan(&d, &[], false, &q, &Default::default()).unwrap()));

        // 任务（#48）：真起两个 sleep，a 的那个要被停掉、b 的不能动。zsh + 空 ZDOTDIR：不读跑测试的人的 .zshrc（issue #30）
        std::fs::create_dir_all(d.join("zdot")).unwrap();
        let start = |owner: &str| {
            let spec = tasksvc::Spec {
                shell: "/bin/zsh".into(),
                command: "sleep 300".into(),
                cwd: d.clone(),
                env: vec![("ZDOTDIR".into(), d.join("zdot").to_string_lossy().into_owned())],
                log: d.join(format!("run-{owner}.log")),
                cap: tasksvc::LOG_CAP,
            };
            let task = Arc::new(tasksvc::Task::start(&spec).unwrap());
            st.insert_run(Run { owner: owner.into(), root: "/p".into(), name: "t".into(), task: Arc::clone(&task) });
            task
        };
        let (ta, tb) = (start("a"), start("b"));

        st.release_window("a");

        assert!(st.pty(pa).is_none(), "a 的终端该收掉");
        assert!(st.pty(pb).is_some(), "关的是 a，b 的终端不能跟着没");
        assert!(!fa.wait_room(), "a 的背压闸要一起关（理由同「关掉终端要把背压闸一起关掉」）");
        assert!(fb.wait_room(), "b 的闸不能被关");

        assert!(st.get(la).is_none(), "a 的日志句柄该关掉");
        assert!(st.get(lb).is_some(), "b 的日志句柄不能跟着关");

        assert!(!st.watched("a"), "a 的监听该停");
        assert!(st.watched("b"), "b 的监听不能被顶掉");

        assert!(ra.load(Ordering::Relaxed), "a 还在跑的远程操作要取消 —— 窗口没了就再没人能取消它");
        assert!(!rb.load(Ordering::Relaxed), "b 的远程操作不能被取消");
        assert!(st.cancel_remote("b", 1), "b 的登记还该在表里");
        assert!(st.scan("a").is_none(), "a 的替换扫描该收掉（里面攥着命中文件的全文）");
        assert!(st.scan("b").is_some(), "b 的不能跟着没");

        assert!(st.find_run("a", "/p", "t").is_none(), "a 的任务该从表里摘掉");
        assert!(st.find_run("b", "/p", "t").is_some(), "b 的任务不能跟着没");
        let t = std::time::Instant::now();
        while ta.alive() && t.elapsed() < std::time::Duration::from_secs(8) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!ta.alive(), "a 的任务进程组该停掉（窗口没了就再没人能停它）");
        assert!(tb.alive(), "关的是 a，b 的任务还在跑");
        st.kill_all_runs();
        let t = std::time::Instant::now();
        while tb.alive() && t.elapsed() < std::time::Duration::from_secs(3) {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        assert!(!tb.alive(), "kill_all_runs 之后 b 也该没了（退出时那一下）");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 崩溃后收尸（#48 第 4 步）：账本里还活着的才算「上次没停干净」，死了的、号被复用的划掉；
    /// 人处理之前它们要一直留在文件里（处理前又崩一次，下次还得找得到）
    #[test]
    fn 上次没停干净的_活着的才算_处理前一直在账上() {
        let d = std::env::temp_dir().join(format!("lite-ide-live-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("zdot")).unwrap();
        let spec = tasksvc::Spec {
            shell: "/bin/zsh".into(),
            command: "sleep 300".into(),
            cwd: d.clone(),
            env: vec![("ZDOTDIR".into(), d.join("zdot").to_string_lossy().into_owned())],
            log: d.join("t.log"),
            cap: tasksvc::LOG_CAP,
        };
        let t = tasksvc::Task::start(&spec).unwrap();
        let alive = tasksvc::live::Live { pgid: t.pgid(), started_us: t.started_us().unwrap(), name: "后端".into(), command: "sleep 300".into(), root: "/p".into() };
        // 组号对、启动时间不对：号被别人复用了，不能认成我们的
        let reused = tasksvc::live::Live { started_us: alive.started_us + 7, name: "别人".into(), ..alive.clone() };
        let path = d.join("live.json");
        tasksvc::live::save(&path, &[alive.clone(), reused]).unwrap();

        let st = AppState::default();
        st.init_live(path.clone());
        assert_eq!(st.stale_for("/p").iter().map(|l| l.name.as_str()).collect::<Vec<_>>(), ["后端"], "只有真活着的那个算");
        assert!(st.stale_for("/别的项目").is_empty(), "按项目分");
        assert_eq!(tasksvc::live::load(&path), vec![alive.clone()], "复用了号的那条要从文件里划掉，活着的那条要留着");
        assert_eq!(st.task_named_by_group(t.pgid()).as_deref(), Some("后端"), "端口卡片要认得出是谁的");

        let took = st.stale_take("/p");
        assert_eq!(took.len(), 1);
        assert!(st.stale_for("/p").is_empty(), "处理过了就不再出卡片");
        assert!(tasksvc::live::load(&path).is_empty(), "处理过了从文件里划掉");
        t.kill();
        let _ = std::fs::remove_dir_all(&d);
    }

    /*
     * op_id 是前端发的，每个窗口都从 1 数起。只按 id 记的话，
     * 两个窗口同时 fetch，后来的那个被当成撞号拒绝；取消 A 还会取消到 B 头上。
     */
    #[test]
    fn 两个窗口用同一个操作编号不算撞号() {
        let st = AppState::default();
        let a = st.begin_remote("a", 1).expect("a 的 1 号");
        let b = st.begin_remote("b", 1).expect("b 也用 1 号，不该被当成撞号");
        assert!(st.begin_remote("a", 1).is_none(), "同一个窗口里撞号仍然要拒绝");

        assert!(st.cancel_remote("a", 1));
        assert!(a.load(Ordering::Relaxed), "取消落到了 a 上");
        assert!(!b.load(Ordering::Relaxed), "取消 a 不能取消到 b");
    }

    /// 换项目根 = 同一个窗口换一个监听；另一个窗口的不受影响
    #[test]
    fn 每个窗口一个监听_互不顶替() {
        let st = AppState::default();
        let d = std::env::temp_dir().join(format!("lite-ide-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        st.set_watch("a", Some(fsservice::watch::watch(&d, |_| {}).unwrap()));
        st.set_watch("b", Some(fsservice::watch::watch(&d, |_| {}).unwrap()));
        assert!(st.watched("a") && st.watched("b"), "b 开项目不能把 a 的监听顶掉");
        st.set_watch("a", None);
        assert!(!st.watched("a") && st.watched("b"), "停 a 不能停到 b");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 起一个真任务（`/bin/zsh -ilc`，空 ZDOTDIR），给下面几条用
    fn real_task(d: &std::path::Path, tag: &str, command: &str) -> Arc<tasksvc::Task> {
        std::fs::create_dir_all(d.join("zdot")).unwrap();
        let spec = tasksvc::Spec {
            shell: "/bin/zsh".into(),
            command: command.into(),
            cwd: d.to_path_buf(),
            env: vec![("ZDOTDIR".into(), d.join("zdot").to_string_lossy().into_owned())],
            log: d.join(format!("{tag}.log")),
            cap: tasksvc::LOG_CAP,
        };
        Arc::new(tasksvc::Task::start(&spec).unwrap())
    }

    fn until(secs: f64, mut f: impl FnMut() -> bool) -> bool {
        let t = std::time::Instant::now();
        while t.elapsed().as_secs_f64() < secs {
            if f() {
                return true;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        f()
    }

    /// 关掉一格（或关窗口、重跑）之后马上 ⌘Q：正在收尾的那个也要被退出流程看见、被强杀。
    /// 原来它只活在收尾线程里，进程一结束线程就没了，不理 SIGINT 的任务留成孤儿（code review 2026-10-10）
    #[test]
    fn 收尾中的任务_退出时也等也强杀() {
        let d = std::env::temp_dir().join(format!("lite-ide-retire-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let st = AppState::default();
        // 等它打出 armed 再停：zsh 起来要 0.3 秒，SIGINT 落在 trap 之前的话它照样被打死，这条就成了软停自己绿的空断言
        // （第一版就是这样，把「退出时强杀收尾中的」删掉都没红 —— 同 tasksvc 测试里 ARMED 那条的教训）
        let t = real_task(&d, "r", "trap '' INT; echo armed; sleep 300");
        let id = st.insert_run(Run { owner: "a".into(), root: "/p".into(), name: "t".into(), task: Arc::clone(&t) });
        assert!(until(10.0, || std::fs::read_to_string(d.join("r.log")).is_ok_and(|x| x.contains("armed"))), "10 秒没等到 armed");
        let r = st.remove_run(id).unwrap();
        st.retire(r.task);
        assert!(st.stop_all_runs().iter().any(|x| Arc::ptr_eq(x, &t)), "退出流程要等的里面得有它");
        st.kill_all_runs();
        // 不理 SIGINT：收尾线程自己要等满 5 秒才强杀；1.5 秒内没了只能是 kill_all_runs 杀的
        assert!(until(1.5, || !t.alive()), "退出时收尾中的也要强杀");
        let _ = std::fs::remove_dir_all(&d);
    }

    /// 重跑：旧的那次要记成「我们停的」（不然 mvn 被停后的 130 报成失败、弹「退出了」）；
    /// 同一个正在起的时候第二次被挡住（连按两次 ⌃R 不起两份）—— code review 2026-10-10
    #[test]
    fn 重跑_旧的记成我们停的_同一个正在起时挡住第二次() {
        let d = std::env::temp_dir().join(format!("lite-ide-rerun-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let st = AppState::default();
        let old = real_task(&d, "o", "sleep 300");
        st.insert_run(Run { owner: "a".into(), root: "/p".into(), name: "t".into(), task: Arc::clone(&old) });
        assert!(until(5.0, || old.alive()));

        let claim = st.begin_run("a", "/p", "t").expect("第一次该认领到");
        let s = old.state();
        assert!(matches!(s, tasksvc::State::Exited { stopped: true, .. }) && !s.failed(), "旧的那次是我们停的，不算失败：{s:?}");
        assert!(st.find_run("a", "/p", "t").is_none(), "旧的从表里摘掉了");
        assert!(st.begin_run("a", "/p", "t").is_err(), "正在起的时候第二次要被挡住");
        assert!(st.begin_run("b", "/p", "t").is_ok(), "别的窗口不受影响");
        drop(claim);
        assert!(st.begin_run("a", "/p", "t").is_ok(), "起完（认领丢掉）之后可以再起");
        let _ = std::fs::remove_dir_all(&d);
    }
}
