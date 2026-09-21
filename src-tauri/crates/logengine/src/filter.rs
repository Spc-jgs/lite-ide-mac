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

/// 文本条件（2026-09-21 从单个关键字扩成多条，语法见 `query.rs`）。
///
/// 字面量是要搜的**字节**，不是 String：文件可能是 GBK / Big5，而前端敲进来的关键字
/// 是 UTF-8 —— 直接拿 UTF-8 字节去搜 GBK 文件，中文永远搜不到。所以由命令层先按
/// 文件的编码把每个关键字编成对应字节再传下来。这一层只管字节比对，不掺和编码。
/// 正则同理用 `regex::bytes`，编译（含大小写开关）也在命令层做。
///
/// 语义：`include` 全部命中、`exclude` 一个都不命中，这一行才算中。
#[derive(Debug, Clone, Default)]
pub struct TextFilter {
    pub include: Vec<Vec<u8>>,
    pub exclude: Vec<Vec<u8>>,
    pub include_re: Vec<regex::bytes::Regex>,
    pub exclude_re: Vec<regex::bytes::Regex>,
}

impl TextFilter {
    /// 只有一个关键字（老接口的形状，测试和简单调用方用）
    pub fn single(pattern: Vec<u8>) -> Self {
        let mut t = Self::default();
        if !pattern.is_empty() {
            t.include.push(pattern);
        }
        t
    }

    pub fn is_empty(&self) -> bool {
        self.include.is_empty() && self.exclude.is_empty() && self.include_re.is_empty() && self.exclude_re.is_empty()
    }

    /// 编一条正则。`unicode` 按文件是不是 UTF-8 给：非 UTF-8 文件里 `.` 只吃一个字节，
    /// `\w` 之类只认 ASCII —— 否则 GBK 的多字节序列会被当成坏 UTF-8 而永远不匹配。
    /// regex 这个 crate 只在这个 crate 里引，命令层不直接碰它。
    pub fn compile_re(src: &str, case_insensitive: bool, unicode: bool) -> Result<regex::bytes::Regex, String> {
        regex::bytes::RegexBuilder::new(src)
            .case_insensitive(case_insensitive)
            .unicode(unicode)
            .build()
            .map_err(|e| format!("正则 /{src}/ 写错了：{e}"))
    }
}

/// 过滤条件。
#[derive(Debug, Clone)]
pub struct FilterSpec {
    /// 允许显示的级别
    pub levels: LevelMask,
    /// 文本条件，空表示不按文本筛
    pub text: TextFilter,
    /// 是否区分大小写（只管字面量；正则的大小写在编译时就定了）
    pub case_sensitive: bool,
    /// 折叠异常堆栈：连续的 `at ...` 帧只保留第一帧
    pub collapse_stacks: bool,
}

impl FilterSpec {
    /// 什么都不筛 —— 前端可据此直接走未过滤的快路径
    pub fn is_noop(&self) -> bool {
        self.levels.is_all() && self.text.is_empty() && !self.collapse_stacks
    }
}

/// 一行的文本判定。字面量走一个 aho-corasick（include 和 exclude 合成一张表，按 pattern id
/// 分辨），一趟扫完；正则逐条跑。先字面量后正则：正则贵，能被字面量否掉的行不进正则。
struct LineMatcher {
    ac: Option<AhoCorasick>,
    /// 前 `n_include` 个 pattern 是 include，后面的是 exclude
    n_include: usize,
    n_total: usize,
    include_re: Vec<regex::bytes::Regex>,
    exclude_re: Vec<regex::bytes::Regex>,
    /// 复用的「这一行见过哪些 pattern」，每行清一次
    seen: Vec<bool>,
}

impl LineMatcher {
    fn new(spec: &FilterSpec) -> Self {
        let t = &spec.text;
        let n_include = t.include.len();
        let n_total = n_include + t.exclude.len();
        let ac = if n_total == 0 {
            None
        } else {
            AhoCorasick::builder()
                .ascii_case_insensitive(!spec.case_sensitive)
                .build(t.include.iter().chain(t.exclude.iter()))
                .ok()
        };
        Self {
            ac,
            n_include,
            n_total,
            include_re: t.include_re.clone(),
            exclude_re: t.exclude_re.clone(),
            seen: vec![false; n_total],
        }
    }

    fn is_match(&mut self, line: &[u8]) -> bool {
        if let Some(ac) = &self.ac {
            self.seen.iter_mut().for_each(|s| *s = false);
            let mut hit_include = 0usize;
            for m in ac.find_overlapping_iter(line) {
                let id = m.pattern().as_usize();
                if id >= self.n_include {
                    return false; // 排除词出现了，后面不用看
                }
                if !self.seen[id] {
                    self.seen[id] = true;
                    hit_include += 1;
                }
            }
            if hit_include < self.n_include {
                return false;
            }
        }
        let _ = self.n_total;
        for re in &self.include_re {
            if !re.is_match(line) {
                return false;
            }
        }
        for re in &self.exclude_re {
            if re.is_match(line) {
                return false;
            }
        }
        true
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
    let mut matcher = if spec.text.is_empty() { None } else { Some(LineMatcher::new(spec)) };

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
            let ok = match matcher.as_mut() {
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
            text: TextFilter::single(pat.as_bytes().to_vec()),
            case_sensitive: cs,
            collapse_stacks: false,
        }
    }

    /// 多条件：字面量按空格切，`-` 前缀排除；正则单独给
    fn spec_q(q: &str, res: &[(&str, bool)], cs: bool) -> FilterSpec {
        let mut text = TextFilter::default();
        for w in q.split_whitespace() {
            if let Some(neg) = w.strip_prefix('-') {
                text.exclude.push(neg.as_bytes().to_vec());
            } else {
                text.include.push(w.as_bytes().to_vec());
            }
        }
        for (src, neg) in res {
            let re = regex::bytes::RegexBuilder::new(src).case_insensitive(!cs).build().unwrap();
            if *neg { text.exclude_re.push(re) } else { text.include_re.push(re) }
        }
        FilterSpec { levels: LevelMask::ALL, text, case_sensitive: cs, collapse_stacks: false }
    }

    #[test]
    fn 多条件_全部命中才算_排除词一个不能有() {
        // BODY：0 started / 1 boom OrderService / 2 retry / 3 orderservice done / 4 cache
        assert_eq!(filter(BODY, spec_q("orderservice", &[], false)), vec![1, 3], "单词，不分大小写");
        assert_eq!(filter(BODY, spec_q("orderservice done", &[], false)), vec![3], "AND：两个都在的只有第 3 行");
        assert_eq!(filter(BODY, spec_q("orderservice -done", &[], false)), vec![1], "排除：有 done 的那行去掉");
        assert_eq!(filter(BODY, spec_q("-main", &[], false)), vec![2], "只有排除条件：不含 main 的");
        assert_eq!(filter(BODY, spec_q("OrderService", &[], true)), vec![1], "区分大小写只剩一行");
    }

    #[test]
    fn 正则_和字面量一起算() {
        assert_eq!(filter(BODY, spec_q("", &[(r"a\.[BD]", false)], false)), vec![0, 2], "正则 a.B / a.D");
        assert_eq!(filter(BODY, spec_q("main", &[(r"a\.[BD]", false)], false)), vec![0], "正则 AND 字面量");
        assert_eq!(filter(BODY, spec_q("", &[("order", true)], false)), vec![0, 2, 4], "排除正则（不分大小写）");
        assert_eq!(filter(BODY, spec_q("", &[("ORDER", false)], true)), Vec::<u64>::new(), "区分大小写的正则");
    }

    /// 非 UTF-8 日志里搜非 ASCII 关键字。
    ///
    /// 这条守的是「GBK 日志里搜中文搜不到」那个 bug：过滤器只做**字节**比对，
    /// 关键字必须由上层先编成文件那套字节。这里直接用 GBK 字节构造，
    /// 断言「用 GBK 的『订单』能搜到，用 UTF-8 的『订单』搜不到」。
    #[test]
    fn 非utf8日志按字节比对() {
        // "订单" 的 GBK 是 B6 A9 B5 A5；"库存" 是 BF E2 B4 E6
        let gbk_订单: &[u8] = &[0xB6, 0xA9, 0xB5, 0xA5];
        let gbk_库存: &[u8] = &[0xBF, 0xE2, 0xB4, 0xE6];

        let mut body = Vec::new();
        body.extend_from_slice(b"2026-01-01 ERROR [main] a.B - ");
        body.extend_from_slice(gbk_订单);
        body.extend_from_slice(b" 8001\n");
        body.extend_from_slice(b"2026-01-01 INFO  [main] a.C - ");
        body.extend_from_slice(gbk_库存);
        body.extend_from_slice(b" ok\n");

        let hit_gbk = filter(
            &body,
            FilterSpec {
                levels: LevelMask::ALL,
                text: TextFilter::single(gbk_订单.to_vec()),
                case_sensitive: false,
                collapse_stacks: false,
            },
        );
        assert_eq!(hit_gbk, vec![0], "GBK 字节的关键字应该命中第 1 行");

        // 同一个词的 UTF-8 字节（E8 AE A2 E5 8D 95）在 GBK 文件里搜不到 ——
        // 这正是为什么关键字必须在命令层按文件编码编一次
        let hit_utf8 = filter(
            &body,
            FilterSpec {
                levels: LevelMask::ALL,
                text: TextFilter::single("订单".as_bytes().to_vec()),
                case_sensitive: false,
                collapse_stacks: false,
            },
        );
        assert!(hit_utf8.is_empty(), "UTF-8 字节不该在 GBK 文件里命中");
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
