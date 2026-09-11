//! pty → 前端的背压闸（issue #18 第一条）。
//!
//! # 为什么终端要的是背压，不是截断
//!
//! AGENTS.md 有一条硬纪律：「起子进程前先问它的输出有上限吗，没有就设闸」。
//! 全应用只有 pty 这一处没设 —— 而它有点特殊：**终端本来就该能吐很多**，
//! `cat` 一个 10MB 的文件、`yes` 跑一会儿，都是正常用法。给它设一个
//! 「超过 N 字节就截断」的闸，等于把终端变成一个会骗人的终端。
//!
//! 真终端里 `cat` 大文件不会撑爆内存，靠的不是截断，是**背压**：
//! 读得慢 → pty master 的缓冲区满 → `write()` 阻塞 → shell 自己被顶住。
//! 这个闸要做的就是把这条链路接回来 —— 在这之前它是断的：
//! 读线程 8KB 一片全速往 Channel 里灌，前端收到就 `term.write()`，
//! **中间没有任何流控**，队列在 webview 里无界地涨（xterm 的 `scrollback`
//! 只限它自己保留多少行，限不住还没被消费的写队列）。
//!
//! # 怎么知道前端消费到哪儿了
//!
//! `term.write(bytes, cb)` 的回调在 xterm **真的解析完**这批字节之后才响。
//! 前端在那个回调里把字节数报回来（`pty_ack`），这里减掉。
//! 未确认的量超过 [`HIGH_WATER`] 时读线程就先不读。
//!
//! # 前端不回 ack 怎么办：退回老样子，不能挂住
//!
//! 这是整个设计里最要紧的一条。ack 可能永远不来 —— 组件被拆了、
//! 窗口最小化把定时器掐了、或者哪天有人改了前端忘了这条通道。
//! 那时候读线程**不能永远等下去**：读线程一停，pty master 就没人排空，
//! 退出中的 shell 写满缓冲区卡在写上，`child.wait()` 永远等不到，
//! 界面永久卡死 —— 那正是 issue #2 那次的形状。
//!
//! 所以等超过 [`GIVE_UP`] 还没人 ack，就**把这个 pty 的背压永久关掉**
//! （`trusted = false`），退回到没有背压的老行为，并在日志里说一声。
//! 代价是一次 2 秒的停顿，而且只有一次。
//!
//! **不选「超时之后把 pending 清零继续」**：那样每次涨到水位都停 2 秒，
//! 吞吐掉到 4KB/s，表现是「终端卡死了」—— 比没有背压糟得多。

use std::sync::{Condvar, Mutex};
use std::time::Duration;

/// 未确认字节数的上限。超过就先不读。
///
/// 256KB ≈ 32 片（读线程一片 8KB）。取这个量级的理由：
/// 小到把队列牢牢按住（无界时 `yes` 几秒就能灌进几十 MB），
/// 大到一次往返 IPC 的延迟不会让管道空掉 —— 水位太低的话，
/// 读线程会在每一片之后都等一次 ack，`cat` 大文件会肉眼可见地变慢。
pub const HIGH_WATER: u64 = 256 << 10;

/// 等 ack 等这么久还没来，就认定这条通道不回话，把背压关掉。
pub const GIVE_UP: Duration = Duration::from_secs(2);

/// 一个 pty 的流控状态。读线程和 `pty_ack` 命令共享一份。
pub struct Flow {
    inner: Mutex<State>,
    room: Condvar,
    give_up: Duration,
}

struct State {
    /// 已经发出去、还没被确认的字节数
    pending: u64,
    /// 还信不信这条 ack 通道。一旦不信就再也不等了（见文件头）
    trusted: bool,
    /// 这个 pty 关了，读线程该收摊
    closed: bool,
}

impl Default for Flow {
    fn default() -> Self {
        Self::new()
    }
}

impl Flow {
    pub fn new() -> Self {
        Self::with_give_up(GIVE_UP)
    }

    /// 只给测试用：等 2 秒的那条路要能在单测里跑完。
    pub fn with_give_up(give_up: Duration) -> Self {
        Flow {
            inner: Mutex::new(State { pending: 0, trusted: true, closed: false }),
            room: Condvar::new(),
            give_up,
        }
    }

    /// 读线程在每次 `read()` **之前**叫这一下。
    ///
    /// 返回 `false` 表示这个 pty 已经关了，读线程该退出。
    /// 返回 `true` 表示可以读了 —— 有可能是真的有空位，也有可能是
    /// 等不到 ack 干脆放弃了背压（那时会连带把 `trusted` 关掉）。
    pub fn wait_room(&self) -> bool {
        let mut s = self.lock();
        loop {
            if s.closed {
                return false;
            }
            if !s.trusted || s.pending < HIGH_WATER {
                return true;
            }
            let (guard, t) = self
                .room
                .wait_timeout(s, self.give_up)
                .unwrap_or_else(|p| p.into_inner());
            s = guard;
            // 超时**而且水位还满着**才算放弃：正常被唤醒时 timed_out 是 false，
            // 而伪唤醒会带着 timed_out=false 转回去重判，两种都不该误伤
            if t.timed_out() && !s.closed && s.pending >= HIGH_WATER {
                s.trusted = false;
                return true;
            }
        }
    }

    /// 又往前端发了 n 个字节。
    pub fn sent(&self, n: u64) {
        self.lock().pending += n;
    }

    /// 前端确认吃下了 n 个字节。
    ///
    /// `saturating_sub`：ack 比 send 多是可能的（前端把没拿到 id 之前攒下的
    /// 那些一起报上来），而一个会溢出成天文数字的 pending 等于永久堵死。
    pub fn acked(&self, n: u64) {
        let mut s = self.lock();
        s.pending = s.pending.saturating_sub(n);
        drop(s);
        self.room.notify_all();
    }

    /// 这个 pty 没了。**必须叫** —— 不叫的话读线程可能正卡在水位上，
    /// 而它卡着就没人排空 pty master（issue #2 那条链路）。
    pub fn close(&self) {
        self.lock().closed = true;
        self.room.notify_all();
    }

    pub fn pending(&self) -> u64 {
        self.lock().pending
    }

    /// 还在做背压吗。false = 等不到 ack 已经放弃了
    pub fn trusted(&self) -> bool {
        self.lock().trusted
    }

    /// 锁中毒了照常干活：这把锁保护的是三个普通字段，没有「一半改完」的中间态，
    /// 而为它 panic 一次等于把一个终端永久废掉。
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.lock().unwrap_or_else(|p| p.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::time::Instant;

    #[test]
    fn 水位底下不等() {
        let f = Flow::new();
        assert!(f.wait_room());
        f.sent(HIGH_WATER - 1);
        assert!(f.wait_room(), "还没到水位就不该拦");
    }

    /*
     * 到水位就得停下来等，等到 ack 才放行。
     *
     * 这一条是整个模块的全部意义：少了它，读线程会一直读下去，
     * 队列在 webview 里无界地涨。
     */
    #[test]
    fn 到水位要等到_ack_才放行() {
        let f = Arc::new(Flow::with_give_up(Duration::from_secs(30)));
        f.sent(HIGH_WATER);
        let g = Arc::clone(&f);
        let t = std::thread::spawn(move || {
            let start = Instant::now();
            assert!(g.wait_room());
            start.elapsed()
        });
        // 先确认它真的卡住了，否则这条测试可能只是「跑得快」
        std::thread::sleep(Duration::from_millis(120));
        assert!(!t.is_finished(), "到水位了还往下走 —— 闸没关上");
        f.acked(HIGH_WATER);
        let waited = t.join().unwrap();
        assert!(waited >= Duration::from_millis(100), "它压根没等，实际只等了 {waited:?}");
        assert_eq!(f.pending(), 0);
    }

    /*
     * **ack 不来不能挂住，要退回没有背压的老样子。**
     *
     * 读线程一停就没人排空 pty master，退出中的 shell 卡在写上，
     * `child.wait()` 永远等不到 —— issue #2 那次界面永久卡死就是这个形状。
     * 宁可没有背压，也不能有这个。
     */
    #[test]
    fn 没人_ack_就放弃背压而不是挂住() {
        let f = Arc::new(Flow::with_give_up(Duration::from_millis(80)));
        f.sent(HIGH_WATER * 4);

        // **在另一条线程上跑，主线程看着表。**
        // 直接 `f.wait_room()` 也能测出「放行」，但测不出「挂住」——
        // 真挂住的时候这条测试跟着一起挂，而**在 CI 里挂住比红掉糟得多**：
        // 红的有行号，挂的只有一个超时的 job。
        let g = Arc::clone(&f);
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let start = Instant::now();
            let ok = g.wait_room();
            let _ = tx.send((ok, start.elapsed()));
        });
        let (ok, waited) = rx
            .recv_timeout(Duration::from_secs(3))
            .expect("等不到 ack 时挂住了 —— 读线程一停就没人排空 pty master（issue #2 那条链路）");
        assert!(ok, "等不到 ack 也必须放行");
        assert!(waited >= Duration::from_millis(70), "它根本没等就放行了，说明水位判断没生效");
        assert!(!f.trusted(), "放弃之后要留下痕迹，否则每一片都得再等一次 80ms");

        // 放弃之后不能再等了 —— 每片都停一次的话吞吐掉到几 KB/s，
        // 表现是「终端卡死」，比没有背压糟得多
        let start = Instant::now();
        assert!(f.wait_room());
        assert!(start.elapsed() < Duration::from_millis(20), "放弃之后又等了一次");
    }

    /*
     * 关掉时必须把等在水位上的读线程**立刻**叫醒并让它收摊。
     *
     * 「立刻」是这条测试的重点，不是顺带。第一版只断言了返回 false ——
     * 而把 `close()` 里的 `notify_all()` 删掉，它照样绿：等着的那条线程
     * 会在 GIVE_UP 超时之后自己醒过来，看见 closed 再返回 false。
     * 只是那要 2 秒，而 `pty_kill` 就在后面等着，2 秒的表现是「点了关闭没反应」。
     * 所以判据必须带时限（验红时确认过：不 notify 就红在这条断言上）。
     */
    #[test]
    fn 关掉要立刻叫醒等在水位上的读线程() {
        let f = Arc::new(Flow::with_give_up(Duration::from_secs(30)));
        f.sent(HIGH_WATER);
        let g = Arc::clone(&f);
        let t = std::thread::spawn(move || g.wait_room());
        std::thread::sleep(Duration::from_millis(80));
        assert!(!t.is_finished());
        let start = Instant::now();
        f.close();
        assert!(!t.join().unwrap(), "关掉之后 wait_room 必须返回 false，让读线程退出");
        let woke = start.elapsed();
        assert!(woke < Duration::from_millis(300), "叫醒花了 {woke:?} —— close() 没有 notify");
    }

    /*
     * **把闸接到一个真 pty 上，量它到底按住了多少。**
     *
     * 上面那些测的是闸本身的逻辑；这一条测的是「接上去之后有没有用」——
     * 而它同时是这条 issue 唯一的数字（issue #18 说「复现很容易，没量过数字」）。
     *
     * `yes` 是一个没有上限的输出源，正是无界队列最容易撑爆的那种。
     * 同一台机器、同样 600ms，实测（把 `wait_room` 改成永远放行就能复现）：
     *
     * | | 读出来的字节 |
     * |---|---|
     * | 没有闸 | **49,574,204**（47 MB） |
     * | 有闸 | **262,481**（256 KB，正好压在水位上） |
     *
     * 189 倍。而那 47MB 在真实的应用里是**排在 webview 里等着被 xterm
     * 解析的队列** —— 没人限得住它，xterm 的 `scrollback` 只管它自己
     * 保留多少行。
     */
    #[test]
    fn 接到真_pty_上要把读出来的量按在水位附近() {
        use std::sync::atomic::{AtomicU64, Ordering};

        // 同 lib.rs 里那三条：先把登录 shell 的冷启动付掉，再开始量（issue #30）
        let (sess, mut reader) = crate::Session::spawn("/tmp", 80, 24).expect("起不了 shell");
        // **GIVE_UP 放到 30 秒**：默认的 2 秒是给真实前端的（前端不回话就退回
        // 没有背压），而这条测试里「没人 ack」是**有意的**，闸必须全程关着。
        // 用默认值的话，测量窗口一拖到 2 秒外它就自己放行，读出来几十 MB —— 间歇红。
        let flow = Arc::new(Flow::with_give_up(Duration::from_secs(30)));
        let total = Arc::new(AtomicU64::new(0));

        let f = Arc::clone(&flow);
        let t = Arc::clone(&total);
        let reading = std::thread::spawn(move || {
            use std::io::Read;
            let mut buf = [0u8; 8192];
            // 一个**从不 ack 的前端**。GIVE_UP 是 2 秒，下面只跑 600ms，
            // 所以这一轮里闸始终是关着的 —— 测的正是关着时的效果
            while f.wait_room() {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        t.fetch_add(n as u64, Ordering::Relaxed);
                        f.sent(n as u64);
                    }
                }
            }
        });

        sess.lock().unwrap().write_input(b"yes\n").expect("写不进去");

        /*
         * **等它真的开始吐，再开始计时。**
         *
         * 原来是写完就 `sleep(600ms)` 然后断言 —— 间歇红：起的是**登录
         * shell**，要 source 一遍配置，慢的机器（或者刚开机、缓存是冷的）
         * 上 600ms 根本轮不到 `yes` 跑起来，读出来 5 个字节。
         *
         * 而**一个会误报的测试，过一阵就没人当真了**（issue #22 那个毛病
         * 同源）。所以判据改成「先等到它活了，再量 600ms」——
         * 等待有 10 秒的死线，真起不来时红在一句说得清的话上。
         */
        // **死线放到 30 秒。** 这是一条*活性前提*，不是被测的东西 ——
        // 它只是在等「`yes` 开始吐了」，等多久都不影响后面那 600ms 的测量，
        // 而机器冷的时候 `zsh -l` 实测要 6.2 秒（热的时候 0.08 秒，差 77 倍）。
        // 给前提留窄窗口是间歇红的常见来源，而**一个会误报的测试，
        // 过一阵就没人当真了**。快的时候它照样立刻往下走，30 秒一分不花。
        let 死线 = Instant::now() + Duration::from_secs(30);
        while total.load(Ordering::Relaxed) < 8192 && Instant::now() < 死线 {
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(
            total.load(Ordering::Relaxed) >= 8192,
            "30 秒内 `yes` 都没跑起来（只读到 {} 字节）—— 这台机器上的登录 shell 太慢，\
             或者 write_input 没写进去",
            total.load(Ordering::Relaxed)
        );
        std::thread::sleep(Duration::from_millis(600));
        let got = total.load(Ordering::Relaxed);

        flow.close();
        drop(sess); // Session::drop 会 kill 掉那个 yes
        let _ = reading.join();

        // 水位 + 一片（判到有空位之后才读，所以最后一片可以整片越界）+
        // 一点余量给 shell 自己的提示符
        let 上限 = HIGH_WATER + 8192 * 2;
        assert!(
            got <= 上限,
            "600ms 读出来 {got} 字节，上限应该是 {上限} —— 闸没起作用"
        );

    }

    /*
     * ack 比 send 多是正常的：前端在拿到 pty id 之前收到的那几片会先攒着，
     * 拿到 id 之后一次报上来。减出负数（u64 下溢）就是一个天文数字的 pending，
     * 那个 pty 从此永久堵死 —— 而且只在「第一片来得特别快」时才复现。
     */
    #[test]
    fn ack_比_send_多不能把_pending_减成天文数字() {
        let f = Flow::new();
        f.sent(100);
        f.acked(5000);
        assert_eq!(f.pending(), 0);
        assert!(f.wait_room());
    }
}
