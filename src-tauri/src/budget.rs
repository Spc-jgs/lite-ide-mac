//! 启动时把预算量一遍，写进 `app.log` —— 可观测性的第三层（issue #28）。
//!
//! # 为什么要有
//!
//! issue #10（内存预算破了，实测 155–238MB vs 写死的 <200MB）是**手工
//! `vmmap` 才发现的**。也就是说这类问题今天只有在有人特意去量的时候才会暴露，
//! 而「慢慢变胖」恰恰是最容易在没人注意时发生的一类 ——
//! 入口包从 126 KB 涨到 157 KB 那次是同一个形状，靠 CI 卡住才没继续。
//!
//! **这里要的不是告警，是一串能回看的数。** 单次的数说明不了什么
//! （开几个标签、跑没跑终端差很多），但「同样开 3 个标签，上个月 155MB、
//! 这个月 210MB」是一条明确的线索。`scripts/budget.sh` 负责把它们拉出来对比。
//!
//! # 这一行里的数分别有多可信
//!
//! | 字段 | 从哪来 | 信不信得过 |
//! |---|---|---|
//! | `boot` | 进程真正的起点（`ri_proc_start_abstime`）到前端报「我好了」 | 准。**含 dyld 和框架加载**，见下 |
//! | `self` | `proc_pid_rusage` 的 `ri_phys_footprint` | 准，但**只是主进程** |
//! | `tabs` `terms` `editors` `nodes` | 前端数出来的 | 精确整数，没有采样噪声 |
//!
//! **`boot` 从进程起点算，不是从 `run()` 算。** 拿 `Instant::now()` 在 `run()`
//! 开头打点是更省事的写法，但那样会把 dyld 加载和框架初始化整段漏掉 ——
//! 而那恰恰是「加了一个依赖之后启动变慢」最会体现的地方。一个系统性漏掉
//! 最大那块的耗时数字，比没有数字更害人。
//!
//! **`self` 只是主进程，WebKit 那三个不在里面。** #10 已经量过：主进程
//! 只占 32MB，涨的 106–173MB 全在 `WebKit.WebContent` 上。它们的 ppid 是 1
//! （launchd 起的 XPC 服务），从进程内部认不出来 —— 那件事归
//! `scripts/mem.sh`（它从外面用差分认领）。这里报 `self` 是因为它**免费、
//! 精确、每次启动都有**，而 `editors` / `nodes` 正好从另一头盯着前端那边的增长。
//!
//! **没有 `langs`（语言包缓存里装了几个）。** 一眼看上去它该在这儿 ——
//! 那个 Map 只 set 不 evict，正是「慢慢变胖」的形状。但数它要先
//! `await import("langs-load")`，而那个模块本身就是按需加载的一大块：
//! **量它就等于把它加载起来**，被量的东西会因为量而变大，`nodes` 和 `self`
//! 跟着一起失真。它留在 `diag` 那条通道里（`LITE_IDE_DEBUG=1`，模块那时
//! 早就加载过了），不进这一行。
//!
//! 用 `ri_phys_footprint` 不用 `ri_resident_size`：RSS 会把共享的系统框架
//! 在每个进程里各算一遍，加起来虚高（#10 里量过）。
//!
//! # 为什么不引 libc
//!
//! 用到的三个符号（`proc_pid_rusage` / `mach_absolute_time` /
//! `mach_timebase_info`）都在 `libSystem` 里，而 `libSystem` 是**每个 macOS
//! 二进制都已经链上的**。为三个函数拽进一个 crate，换来的只是别人替我们
//! 抄了同一份声明。

use std::sync::OnceLock;

/// `rusage_info_v0`（`<sys/resource.h>`）。
///
/// **只声明 v0，不碰更高版本。** v0 是这个结构体的第一版，字段和顺序此后
/// 再没动过（后面的版本只往屁股上追加），而我们要的两个字段都在里面。
/// 抄一个有 30 多个字段的 v6 进来，等于给自己找一个「某天某个字段的类型
/// 变了而我们不知道」的坑。
#[repr(C)]
#[derive(Default)]
struct RusageV0 {
    uuid: [u8; 16],
    user_time: u64,
    system_time: u64,
    pkg_idle_wkups: u64,
    interrupt_wkups: u64,
    pageins: u64,
    wired_size: u64,
    /// RSS。**别用它**，见文件头
    resident_size: u64,
    /// 这个才是 `vmmap --summary` 里那个 Physical footprint
    phys_footprint: u64,
    /// 进程起点，mach 绝对时间
    proc_start_abstime: u64,
    proc_exit_abstime: u64,
}

#[repr(C)]
#[derive(Default)]
struct Timebase {
    numer: u32,
    denom: u32,
}

const RUSAGE_INFO_V0: i32 = 0;

extern "C" {
    fn proc_pid_rusage(pid: i32, flavor: i32, buffer: *mut RusageV0) -> i32;
    fn mach_absolute_time() -> u64;
    fn mach_timebase_info(info: *mut Timebase) -> i32;
}

/// mach 绝对时间的刻度。查一次就够 —— 它在进程生命周期内不变。
fn timebase() -> (u64, u64) {
    static TB: OnceLock<(u64, u64)> = OnceLock::new();
    *TB.get_or_init(|| {
        let mut tb = Timebase::default();
        // SAFETY: 写进一个我们自己的 8 字节结构体，失败时字段留 0，下面兜住
        let rc = unsafe { mach_timebase_info(&mut tb) };
        if rc != 0 || tb.denom == 0 {
            // 拿不到刻度就当 1:1（Intel Mac 上本来就是），宁可数字偏一点也不要没有
            (1, 1)
        } else {
            (tb.numer as u64, tb.denom as u64)
        }
    })
}

/// 这个进程活了多少毫秒。**从进程真正的起点算**，含 dyld。
///
/// 取不到（`proc_pid_rusage` 失败、或者算出来的数离谱）就是 `None` ——
/// 报一个错的耗时比不报更害人。
pub fn uptime_ms() -> Option<u64> {
    let ru = rusage()?;
    let now = unsafe { mach_absolute_time() };
    let ticks = now.checked_sub(ru.proc_start_abstime)?;
    let (n, d) = timebase();
    let ms = ticks.checked_mul(n)? / d / 1_000_000;
    // 一次启动跑了一天以上，那说明这个数不是「启动耗时」——
    // 多半是这一行被在别的时机写出来了，报出去只会误导
    (ms < 86_400_000).then_some(ms)
}

/// 主进程的 Physical footprint，MB。WebKit 那三个不在里面（见文件头）。
pub fn phys_footprint_mb() -> Option<u64> {
    let b = rusage()?.phys_footprint;
    // 合理区间之外一律当没量到：这套 FFI 一旦哪天对不上，
    // 错的表现就是一个荒唐的数，而荒唐的数会被当成真的记进趋势里
    (b >= 1 << 20 && b < 64u64 << 30).then(|| b >> 20)
}

fn rusage() -> Option<RusageV0> {
    let mut ru = RusageV0::default();
    /*
     * **pid 要给真的，不能给 0。**
     *
     * `getrusage` 那套里 0 是「自己」，`proc_pid_rusage` 不是 ——
     * 0 是内核任务，非 root 问它一律被拒（返回 -1）。第一版就是这么写的，
     * 两条测试直接红在「量不到」上，而那正是它们存在的意义。
     *
     * SAFETY: 缓冲区是我们自己的 v0 结构体，flavor 与它匹配
     */
    let rc = unsafe { proc_pid_rusage(std::process::id() as i32, RUSAGE_INFO_V0, &mut ru) };
    (rc == 0).then_some(ru)
}

/// 前端数出来的那几个。零就是零，不是「没量到」—— 所以不用 Option。
#[derive(Debug, Clone, Copy)]
pub struct Counts {
    pub tabs: u32,
    pub terms: u32,
    /// 整棵 DOM 的元素数，跟着标签开关涨落
    pub nodes: u32,
    /// CM6 的根元素个数。**关掉全部标签之后必须是 0** —— 不是 0 就是
    /// EditorView 没销毁干净，那是真泄漏（issue #10 第 3 条的判据）
    pub editors: u32,
}

/// 拼出那一行。
///
/// **拎出来是为了能测。** 这一行的格式就是 `scripts/budget.sh` 的输入协议，
/// 改了它等于改了「过去那些数还读不读得出来」—— 而那正是这整件事的全部价值。
///
/// 量不到的字段写 `?` 而不是 0：`self=0MB` 会被脚本当成一个真实读数
/// 画进趋势里，`self=?` 一眼就知道该跳过。
pub fn line(boot_ms: Option<u64>, self_mb: Option<u64>, c: Counts, ver: &str, devtools: bool) -> String {
    let n = |v: Option<u64>| v.map_or("?".to_string(), |x| x.to_string());
    format!(
        "boot={}ms self={}MB tabs={} terms={} editors={} nodes={} v={} devtools={}",
        n(boot_ms),
        n(self_mb),
        c.tabs,
        c.terms,
        c.editors,
        c.nodes,
        ver,
        if devtools { 1 } else { 0 }
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    const C: Counts = Counts { tabs: 3, terms: 1, editors: 1, nodes: 4210 };

    #[test]
    fn 预算行的格式就是脚本的输入协议() {
        assert_eq!(
            line(Some(412), Some(34), C, "0.9.0", false),
            "boot=412ms self=34MB tabs=3 terms=1 editors=1 nodes=4210 v=0.9.0 devtools=0"
        );
        // 调试版要在这一行上自报家门（issue #20）：
        // 一份带 Web Inspector 的构建，它的内存数和正式版不可比
        assert!(line(Some(1), Some(1), C, "0.9.0", true).ends_with("devtools=1"));
    }

    /*
     * 量不到的字段必须写 `?`，不能写 0。
     *
     * `self=0MB` 会被 budget.sh 当成一个真实读数画进趋势里 —— 一条本来
     * 平稳的曲线会因为几次没量到而出现假的低谷，而那正是这串数唯一的用途。
     */
    #[test]
    fn 量不到的字段写问号不写零() {
        let l = line(None, None, C, "0.9.0", false);
        assert!(l.starts_with("boot=?ms self=?MB"), "实得 {l}");
        // 计数是真零：一个标签都没开就是 0，那不是「没量到」
        let 空 = Counts { tabs: 0, terms: 0, editors: 0, nodes: 0 };
        assert!(line(Some(0), Some(0), 空, "0.9.0", false).contains("tabs=0 terms=0"));
    }

    /*
     * 这一条是整套 FFI 唯一能自动验的地方：**数得在一个合理的量级上**。
     *
     * 结构体的字段偏移一旦对不上，`phys_footprint` 读到的会是隔壁字段
     * （`resident_size` 或 `proc_start_abstime`）—— 后者是个十几位的
     * 时间戳，换算成 MB 是天文数字，这里会兜住变成 None。
     *
     * 和 `vmmap --summary` 的逐字核对不是测试能做的事（要另起进程、
     * 还得等它跑），那次核对记在 JOURNAL.md 里。
     */
    #[test]
    fn 自己的内存数要在合理量级上() {
        let mb = phys_footprint_mb().expect("量不到自己的 Physical footprint —— FFI 对不上了");
        assert!((1..4096).contains(&mb), "一个跑单测的进程占了 {mb}MB，这个数不可信");
    }

    #[test]
    fn 自己活了多久要是个小数() {
        let ms = uptime_ms().expect("量不到进程起点 —— FFI 对不上了");
        // 单测进程刚起来没几秒。取到 proc_start_abstime 以外的字段的话，
        // 这里会是一个荒唐的大数（或者被 uptime_ms 自己兜成 None）
        assert!(ms < 600_000, "单测进程报自己活了 {ms}ms");
    }
}
