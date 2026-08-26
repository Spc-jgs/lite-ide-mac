//! 过滤：按级别 + 文本筛出命中行。
//!
//! 关键设计（ARCHITECTURE.md §3.6）：**只返回行号，不返回内容**。
//! 几百万条命中如果连内容一起返回，内存直接爆掉；行号列表则是
//! 每条 8 字节，前端虚拟滚动再按需回表取内容。
//!
//! 与架构原文的一处偏离：原计划起 `rg --json` 子进程，这里改为进程内实现。
//! 理由是文件已经 mmap 在内存里，rg 会重新 IO 一遍 1GB；而且单文件搜索用不上
//! rg 的看家本领（多文件遍历、gitignore 处理）。rg 留给 M4 的全局搜索。

use crate::index::LineIndex;
use crate::level::{LevelMap, LevelMask};
use aho_corasick::AhoCorasick;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 过滤条件。
#[derive(Debug, Clone)]
pub struct FilterSpec {
    /// 允许显示的级别
    pub levels: LevelMask,
    /// 文本关键字，空表示不按文本筛
    pub pattern: String,
    /// 是否区分大小写
    pub case_sensitive: bool,
    /// 折叠异常堆栈：连续的 `at ...` 帧只保留第一帧
    pub collapse_stacks: bool,
}

impl FilterSpec {
    /// 什么都不筛 —— 前端可据此直接走未过滤的快路径
    pub fn is_noop(&self) -> bool {
        self.levels.is_all() && self.pattern.is_empty() && !self.collapse_stacks
    }
}

/// 这一行是不是异常堆栈的**栈帧**。
///
/// 只认栈帧，不认异常首行与 `Caused by:` —— 那两类有信息量（异常类型和原因），
/// 折叠掉等于把最该看的东西藏了。被折叠的是几十行 `at com.foo.Bar(...)` 的噪声。
///
/// 覆盖 Java / Python / Go 三种常见形态。
fn is_stack_frame(line: &[u8]) -> bool {
    // Java: "\tat com.foo.Bar(Bar.java:42)" / "    at ..." / "\t... 12 more"
    let trimmed = {
        let mut i = 0;
        while i < line.len() && (line[i] == b'\t' || line[i] == b' ') {
            i += 1;
        }
        // 必须有缩进，否则是普通行
        if i == 0 || i >= line.len() {
            return false;
        }
        &line[i..]
    };
    if trimmed.starts_with(b"at ") || trimmed.starts_with(b"... ") {
        return true;
    }
    // Python: '  File "/path/x.py", line 42, in fn'
    if trimmed.starts_with(b"File \"") {
        return true;
    }
    // Go: "\t/path/file.go:123 +0x1a"
    if trimmed.first() == Some(&b'/') && memchr::memmem::find(trimmed, b".go:").is_some() {
        return true;
    }
    false
}

/// 一次过滤任务的句柄。结果只在跑完时发布一次 ——
/// 命中列表可达数十 MB，中途反复克隆的拷贝量比扫描本身还贵。
pub struct FilterTask {
    hits: Arc<Mutex<Arc<Vec<u64>>>>,
    complete: Arc<AtomicBool>,
    cancelled: Arc<AtomicBool>,
    /// 已扫描行数，用于进度显示
    scanned: Arc<AtomicU64>,
    /// 已命中条数，扫描中也能实时显示
    hit_count: Arc<AtomicU64>,
}

impl FilterTask {
    pub fn hits(&self) -> Arc<Vec<u64>> {
        Arc::clone(&self.hits.lock().expect("hits 锁被毒化"))
    }

    pub fn is_complete(&self) -> bool {
        self.complete.load(Ordering::Acquire)
    }

    pub fn scanned_lines(&self) -> u64 {
        self.scanned.load(Ordering::Relaxed)
    }

    pub fn hit_count(&self) -> u64 {
        self.hit_count.load(Ordering::Relaxed)
    }

    /// 取消：换关键字时旧任务要立刻停，否则大文件上会堆积一堆无用扫描
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Release);
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::Acquire)
    }
}

/// 在后台跑一次过滤。
///
/// `data` 必须是完整文件内容，`index` 与 `levels` 应已扫描完成
/// （未完成时按当前已知范围过滤，前端会在完成后重跑）。
pub fn spawn(
    data: Arc<memmap2::Mmap>,
    index: Arc<LineIndex>,
    levels: Arc<LevelMap>,
    spec: FilterSpec,
) -> std::io::Result<Arc<FilterTask>> {
    let task = Arc::new(FilterTask {
        hits: Arc::new(Mutex::new(Arc::new(Vec::new()))),
        complete: Arc::new(AtomicBool::new(false)),
        cancelled: Arc::new(AtomicBool::new(false)),
        scanned: Arc::new(AtomicU64::new(0)),
        hit_count: Arc::new(AtomicU64::new(0)),
    });
    let t = Arc::clone(&task);

    std::thread::Builder::new()
        .name("logengine-filter".into())
        .spawn(move || {
            let hits = run(&data, &index, &levels, &spec, &t);
            if !t.is_cancelled() {
                *t.hits.lock().expect("hits 锁被毒化") = Arc::new(hits);
                t.complete.store(true, Ordering::Release);
            }
        })?;
    Ok(task)
}

/// 同步执行过滤，返回命中行号。
fn run(
    data: &[u8],
    index: &LineIndex,
    levels: &LevelMap,
    spec: &FilterSpec,
    task: &FilterTask,
) -> Vec<u64> {
    let total = index.line_count();
    let matcher = if spec.pattern.is_empty() {
        None
    } else {
        AhoCorasick::builder()
            .ascii_case_insensitive(!spec.case_sensitive)
            .build([spec.pattern.as_bytes()])
            .ok()
    };

    // 命中率未知，先按 1/8 预留，避免过滤 INFO 这种大头时反复扩容
    let mut hits: Vec<u64> = Vec::with_capacity((total / 8).min(1 << 20) as usize);

    // 顺序推进行边界，避免每行都去查索引
    let mut pos = 0usize;
    let mut line = 0u64;
    // 上一行是不是栈帧 —— 折叠时用它判断"连续块的第一帧"
    let mut prev_frame = false;

    while line < total && pos <= data.len() {
        let end = match memchr::memchr(b'\n', &data[pos..]) {
            Some(nl) => pos + nl,
            None => data.len(),
        };

        // 折叠堆栈：连续帧只留第一帧，其余跳过
        let frame = spec.collapse_stacks && is_stack_frame(&data[pos..end]);
        let folded = frame && prev_frame;
        prev_frame = frame;
        if folded {
            pos = end + 1;
            line += 1;
            continue;
        }

        // 先按级别筛 —— 纯内存查表，比文本匹配便宜得多
        if spec.levels.allows(levels.get(line)) {
            let ok = match &matcher {
                None => true,
                Some(m) => m.is_match(&data[pos..end]),
            };
            if ok {
                hits.push(line);
                task.hit_count.store(hits.len() as u64, Ordering::Relaxed);
            }
        }

        pos = end + 1;
        line += 1;

        // 每 64K 行看一次取消标志与进度，不必更密
        if line.is_multiple_of(65_536) {
            if task.is_cancelled() {
                return Vec::new();
            }
            task.scanned.store(line, Ordering::Relaxed);
        }
    }
    task.scanned.store(line, Ordering::Relaxed);
    hits.shrink_to_fit();
    hits
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::{self, Level, LevelMap};

    fn build(body: &[u8]) -> (LineIndex, LevelMap) {
        let mut ix = LineIndex::new(4);
        ix.extend(body);
        ix.seal(body);

        let mut lv = LevelMap::with_capacity(16);
        let mut pos = 0usize;
        for nl in memchr::memchr_iter(b'\n', body) {
            lv.push(level::detect(&body[pos..nl]));
            pos = nl + 1;
        }
        if pos < body.len() {
            lv.push(level::detect(&body[pos..]));
        }
        (ix, lv)
    }

    fn filter(body: &[u8], spec: FilterSpec) -> Vec<u64> {
        let (ix, lv) = build(body);
        let task = FilterTask {
            hits: Arc::new(Mutex::new(Arc::new(Vec::new()))),
            complete: Arc::new(AtomicBool::new(false)),
            cancelled: Arc::new(AtomicBool::new(false)),
            scanned: Arc::new(AtomicU64::new(0)),
            hit_count: Arc::new(AtomicU64::new(0)),
        };
        run(body, &ix, &lv, &spec, &task)
    }

    const BODY: &[u8] = b"2026-01-01 INFO  [main] a.B - started\n\
2026-01-01 ERROR [main] a.C - boom OrderService\n\
2026-01-01 WARN  [pool] a.D - retry\n\
2026-01-01 INFO  [main] a.E - orderservice done\n\
2026-01-01 DEBUG [main] a.F - cache\n";

    fn spec(mask: LevelMask, pat: &str, cs: bool) -> FilterSpec {
        FilterSpec {
            levels: mask,
            pattern: pat.into(),
            case_sensitive: cs,
            collapse_stacks: false,
        }
    }

    const STACKY: &[u8] = b"2026-01-01 ERROR [main] a.B - boom\n\
java.lang.IllegalStateException: pool exhausted\n\
\tat com.zaxxer.HikariPool.createTimeout(HikariPool.java:696)\n\
\tat com.zaxxer.HikariPool.getConnection(HikariPool.java:197)\n\
\tat com.liteide.OrderService.persist(OrderService.java:142)\n\
\t... 12 more\n\
Caused by: java.sql.SQLException: timed out\n\
\tat java.base/java.lang.Thread.run(Thread.java:840)\n\
2026-01-01 INFO  [main] a.C - next\n";

    #[test]
    fn 折叠堆栈只留每段第一帧() {
        let mut sp = spec(LevelMask::ALL, "", false);
        sp.collapse_stacks = true;
        // 保留：0 日志行、1 异常首行、2 第一帧、6 Caused by、7 它下面的第一帧、8 下一条日志
        assert_eq!(filter(STACKY, sp), vec![0, 1, 2, 6, 7, 8]);
    }

    #[test]
    fn 不折叠时全部保留() {
        assert_eq!(filter(STACKY, spec(LevelMask::ALL, "", false)).len(), 9);
    }

    #[test]
    fn 异常首行与_caused_by_永不折叠() {
        let mut sp = spec(LevelMask::ALL, "", false);
        sp.collapse_stacks = true;
        let got = filter(STACKY, sp);
        assert!(got.contains(&1), "异常首行被折叠了");
        assert!(got.contains(&6), "Caused by 被折叠了");
    }

    #[test]
    fn 折叠也算一种筛选条件() {
        let mut sp = spec(LevelMask::ALL, "", false);
        sp.collapse_stacks = true;
        assert!(!sp.is_noop(), "只开折叠也该走过滤路径");
    }

    #[test]
    fn 不筛时全部命中() {
        assert_eq!(
            filter(BODY, spec(LevelMask::ALL, "", false)),
            vec![0, 1, 2, 3, 4]
        );
    }

    #[test]
    fn 只筛级别() {
        let only_err = LevelMask::from_bits(1 << Level::Error.index());
        assert_eq!(filter(BODY, spec(only_err, "", false)), vec![1]);

        let err_warn =
            LevelMask::from_bits((1 << Level::Error.index()) | (1 << Level::Warn.index()));
        assert_eq!(filter(BODY, spec(err_warn, "", false)), vec![1, 2]);
    }

    #[test]
    fn 文本大小写不敏感() {
        assert_eq!(
            filter(BODY, spec(LevelMask::ALL, "orderservice", false)),
            vec![1, 3]
        );
    }

    #[test]
    fn 文本大小写敏感() {
        assert_eq!(
            filter(BODY, spec(LevelMask::ALL, "OrderService", true)),
            vec![1]
        );
    }

    #[test]
    fn 级别与文本同时生效() {
        let only_info = LevelMask::from_bits(1 << Level::Info.index());
        assert_eq!(
            filter(BODY, spec(only_info, "orderservice", false)),
            vec![3]
        );
    }

    #[test]
    fn 无命中返回空() {
        assert!(filter(BODY, spec(LevelMask::ALL, "不存在的词", false)).is_empty());
    }

    #[test]
    fn is_noop_判定() {
        assert!(spec(LevelMask::ALL, "", false).is_noop());
        assert!(!spec(LevelMask::ALL, "x", false).is_noop());
        assert!(!spec(LevelMask::from_bits(1), "", false).is_noop());
    }
}
