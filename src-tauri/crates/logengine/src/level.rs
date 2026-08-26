//! 日志级别探测。
//!
//! 索引阶段对每一行调用，所以必须极快：单次线性扫描 + 首字母筛选，
//! 只有首字母命中才做完整比较。绝大多数行在前 30 字节内就返回。
//!
//! 只扫行首一段：级别标记不会出现在正文深处，扫全行既慢又容易误判
//! （消息里出现 "ERROR" 字样的情况很常见）。
//!
//! 两条路径，**结构化在前**：
//! 1. **结构化日志的 `level=` / `"level":`**（logfmt、JSON）—— 值通常是小写。
//!    必须先走这条：JSON 行的正文里常有大写 ERROR 字样，
//!    先扫大写会把 `msg":"Connection ERROR"` 误判成 ERROR 级别。
//!    入口有一次 SIMD 快速排除（没有 `=` 也没有 `{` 就直接放行），
//!    传统日志几乎不为它买单。
//! 2. **大写关键字**（Java / syslog / 多数传统日志）—— 单次线性扫描，极快。

/// 只在行首这么多字节里找级别标记。
const SCAN_HEAD: usize = 64;

/// 结构化日志的键可能排在稍后一点（JSON 里 time 字段常常在前），但也不会太靠后
const STRUCT_SCAN_HEAD: usize = 96;

/// 快速排除的探测窗口。取 48 是有讲究的：
/// logfmt 的第一个 `=` 必在开头附近（`time=...`），JSON 行首就是 `{`；
/// 而传统日志正文里的 `=`（`orderId=8842011` 之类）都在时间戳、级别、
/// 线程名、logger 之后，48 字节根本够不着。
/// 早先用 200，测试日志里的 `orderId=` 落进了窗口，快速排除全部失效，
/// 级别扫描从 88ms 掉到 517ms。
const STRUCT_PROBE: usize = 48;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Level {
    Error = 0,
    Warn = 1,
    Info = 2,
    Debug = 3,
    Trace = 4,
    /// 没有识别出级别 —— 堆栈行、多行消息的续行、非标准格式都落这里
    None = 5,
}

impl Level {
    pub const COUNT: usize = 6;

    #[inline]
    pub fn index(self) -> usize {
        self as usize
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Level::Error => "ERROR",
            Level::Warn => "WARN",
            Level::Info => "INFO",
            Level::Debug => "DEBUG",
            Level::Trace => "TRACE",
            Level::None => "OTHER",
        }
    }
}

/// 探测一行的级别。
#[inline]
pub fn detect(line: &[u8]) -> Level {
    // 结构化日志（logfmt / JSON）的级别值是小写的，专门认一次。
    // 放在前面是因为这类行往往也含大写词，先扫大写容易误判
    if let Some(l) = detect_structured(line) {
        return l;
    }
    let end = line.len().min(SCAN_HEAD);
    let head = &line[..end];

    let mut i = 0;
    while i < end {
        // 首字母筛选：绝大多数字节在这里就被跳过
        match head[i] {
            b'E' => {
                if head[i..].starts_with(b"ERROR") {
                    return Level::Error;
                }
            }
            b'W' => {
                if head[i..].starts_with(b"WARN") {
                    return Level::Warn;
                }
            }
            b'I' => {
                if head[i..].starts_with(b"INFO") {
                    return Level::Info;
                }
            }
            b'D' => {
                if head[i..].starts_with(b"DEBUG") {
                    return Level::Debug;
                }
            }
            b'T' => {
                if head[i..].starts_with(b"TRACE") {
                    return Level::Trace;
                }
            }
            // FATAL / SEVERE 都当 ERROR 看
            b'F' if head[i..].starts_with(b"FATAL") => return Level::Error,
            b'S' if head[i..].starts_with(b"SEVERE") => return Level::Error,
            _ => {}
        }
        i += 1;
    }
    Level::None
}

/// 各级别的行数统计。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct LevelStats {
    counts: [u64; Level::COUNT],
}

impl LevelStats {
    #[inline]
    pub fn add(&mut self, lvl: Level) {
        self.counts[lvl.index()] += 1;
    }

    #[inline]
    pub fn get(&self, lvl: Level) -> u64 {
        self.counts[lvl.index()]
    }

    #[inline]
    pub fn total(&self) -> u64 {
        self.counts.iter().sum()
    }

    pub fn as_array(&self) -> [u64; Level::COUNT] {
        self.counts
    }
}

/// 级别过滤掩码：每个 bit 对应一个级别是否显示。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LevelMask(u8);

impl LevelMask {
    pub const ALL: LevelMask = LevelMask(0b0011_1111);

    pub fn from_bits(bits: u8) -> Self {
        LevelMask(bits & 0b0011_1111)
    }

    #[inline]
    pub fn allows(self, lvl: Level) -> bool {
        self.0 & (1 << lvl.index()) != 0
    }

    #[inline]
    pub fn is_all(self) -> bool {
        self.0 == Self::ALL.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 识别标准_java_日志行() {
        let cases: [(&[u8], Level); 5] = [
            (
                b"2026-08-24 14:03:21.442 ERROR [http-nio-1] c.l.Svc - boom",
                Level::Error,
            ),
            (
                b"2026-08-24 14:03:21.442 WARN  [pool-3] c.l.Retry - retry",
                Level::Warn,
            ),
            (
                b"2026-08-24 14:03:21.442 INFO  [main] c.l.App - started",
                Level::Info,
            ),
            (
                b"2026-08-24 14:03:21.442 DEBUG [main] c.l.Cache - evict",
                Level::Debug,
            ),
            (
                b"2026-08-24 14:03:21.442 TRACE [main] c.l.X - enter",
                Level::Trace,
            ),
        ];
        for (line, want) in cases {
            assert_eq!(detect(line), want, "行: {}", String::from_utf8_lossy(line));
        }
    }

    #[test]
    fn fatal_与_severe_归为_error() {
        assert_eq!(detect(b"2026-08-24 FATAL something died"), Level::Error);
        assert_eq!(detect(b"2026-08-24 SEVERE jul style"), Level::Error);
    }

    #[test]
    fn 堆栈行没有级别() {
        assert_eq!(
            detect(b"\tat com.liteide.OrderService.persist(OrderService.java:142)"),
            Level::None
        );
        assert_eq!(
            detect(b"java.lang.IllegalStateException: pool exhausted"),
            Level::None
        );
    }

    #[test]
    fn 正文里的_error_字样不误判为级别() {
        // 超出行首扫描窗口，不该被认成 ERROR 行
        let line =
            b"2026-08-24 14:03:21.442 INFO  [main] c.l.App - handled ERROR response from upstream";
        assert_eq!(
            detect(line),
            Level::Info,
            "应取行首的 INFO 而非正文里的 ERROR"
        );
    }

    #[test]
    fn 空行与短行不崩() {
        assert_eq!(detect(b""), Level::None);
        assert_eq!(detect(b"E"), Level::None);
        assert_eq!(detect(b"ERR"), Level::None);
    }

    #[test]
    fn 识别_logfmt_的小写级别() {
        assert_eq!(
            detect(br#"time="2026-08-24T14:03:21Z" level=error msg="boom""#),
            Level::Error
        );
        assert_eq!(detect(br#"time="..." level=info msg="ok""#), Level::Info);
        assert_eq!(detect(b"ts=1724500000 lvl=warn msg=retry"), Level::Warn);
    }

    #[test]
    fn 识别_json_日志的级别() {
        assert_eq!(
            detect(br#"{"time":"2026-08-24T14:03:21Z","level":"error","msg":"boom"}"#),
            Level::Error
        );
        assert_eq!(
            detect(br#"{"@timestamp":"...","severity":"WARNING","msg":"x"}"#),
            Level::Warn
        );
        assert_eq!(detect(br#"{"levelname":"DEBUG","msg":"x"}"#), Level::Debug);
    }

    #[test]
    fn 不把_loglevel_这种子串当字段名() {
        // 前面紧挨着字母，不该被当成 level 字段
        assert_eq!(
            detect(b"my_loglevel=error is just a message here"),
            Level::None
        );
    }

    #[test]
    fn 结构化路径不影响传统日志() {
        // 传统 Java 行里没有 level= 字段，仍走大写扫描
        assert_eq!(
            detect(b"2026-08-24 14:03:21.442 ERROR [http-nio-1] c.l.Svc - boom"),
            Level::Error
        );
    }

    #[test]
    fn 掩码过滤() {
        assert!(LevelMask::ALL.allows(Level::Error));
        assert!(LevelMask::ALL.is_all());
        let only_err = LevelMask::from_bits(1 << Level::Error.index());
        assert!(only_err.allows(Level::Error));
        assert!(!only_err.allows(Level::Info));
        assert!(!only_err.is_all());
    }

    #[test]
    fn 统计累加() {
        let mut s = LevelStats::default();
        s.add(Level::Error);
        s.add(Level::Info);
        s.add(Level::Info);
        assert_eq!(s.get(Level::Error), 1);
        assert_eq!(s.get(Level::Info), 2);
        assert_eq!(s.total(), 3);
    }
}

/// 每行级别的紧凑记录 + 统计。
///
/// 为什么单独一遍而不是塞进索引：级别探测要逐字节看行首，实测把索引从
/// 145ms 拖到 870ms（6×）。索引是关键路径 —— 它决定「打开后多久能准确滚动」，
/// 必须保持纯粹。级别只影响 chips 数字与过滤，慢一点无感。
///
/// 顺便把每行的级别存下来（4 bit/行），级别过滤因此是纯内存操作：
/// 914 万行遍历约 5ms，点 chips 立即响应，不必重扫文件。
///
/// 内存：1GB 日志约 4.5MB，10GB 约 40MB —— 是本引擎唯一与行数线性相关的结构，
/// 换来的是过滤的即时性。
#[derive(Debug, Clone, Default)]
pub struct LevelMap {
    /// 每行 4 bit，低半字节存偶数行，高半字节存奇数行
    packed: Vec<u8>,
    stats: LevelStats,
    lines: u64,
}

impl LevelMap {
    pub fn with_capacity(lines: usize) -> Self {
        Self {
            packed: Vec::with_capacity(lines.div_ceil(2)),
            stats: LevelStats::default(),
            lines: 0,
        }
    }

    /// 追加一行的级别。必须按行号顺序调用。
    #[inline]
    pub fn push(&mut self, lvl: Level) {
        let nibble = lvl.index() as u8;
        if self.lines.is_multiple_of(2) {
            self.packed.push(nibble);
        } else {
            let last = self.packed.len() - 1;
            self.packed[last] |= nibble << 4;
        }
        self.stats.add(lvl);
        self.lines += 1;
    }

    #[inline]
    pub fn get(&self, line: u64) -> Level {
        if line >= self.lines {
            return Level::None;
        }
        let byte = self.packed[(line / 2) as usize];
        let nibble = if line.is_multiple_of(2) {
            byte & 0x0F
        } else {
            byte >> 4
        };
        match nibble {
            0 => Level::Error,
            1 => Level::Warn,
            2 => Level::Info,
            3 => Level::Debug,
            4 => Level::Trace,
            _ => Level::None,
        }
    }

    #[inline]
    pub fn lines(&self) -> u64 {
        self.lines
    }

    #[inline]
    pub fn stats(&self) -> LevelStats {
        self.stats
    }

    pub fn memory_footprint(&self) -> usize {
        self.packed.capacity()
    }

    pub fn shrink(&mut self) {
        self.packed.shrink_to_fit();
    }
}

#[cfg(test)]
mod map_tests {
    use super::*;

    #[test]
    fn 打包与读取往返一致() {
        let seq = [
            Level::Error,
            Level::Warn,
            Level::Info,
            Level::Debug,
            Level::Trace,
            Level::None,
            Level::Info,
            Level::Error,
            Level::Warn,
        ];
        let mut m = LevelMap::with_capacity(seq.len());
        for l in seq {
            m.push(l);
        }
        assert_eq!(m.lines(), seq.len() as u64);
        for (n, want) in seq.iter().enumerate() {
            assert_eq!(m.get(n as u64), *want, "第 {n} 行");
        }
        assert_eq!(m.get(999), Level::None, "越界返回 None");
    }

    #[test]
    fn 统计与打包同步() {
        let mut m = LevelMap::with_capacity(4);
        m.push(Level::Error);
        m.push(Level::Info);
        m.push(Level::Info);
        assert_eq!(m.stats().get(Level::Info), 2);
        assert_eq!(m.stats().total(), 3);
    }

    #[test]
    fn 每行只占半字节() {
        let mut m = LevelMap::with_capacity(1000);
        for i in 0..1000 {
            m.push(if i % 2 == 0 {
                Level::Info
            } else {
                Level::Debug
            });
        }
        assert_eq!(m.memory_footprint(), 500, "1000 行应只占 500 字节");
    }
}

/// 结构化日志的级别键：`level=warn`、`"level":"error"`、`"severity":"INFO"`。
///
/// 只在行首一段里找 —— 正文里出现 `level=` 的概率远低于它作为字段名。
fn detect_structured(line: &[u8]) -> Option<Level> {
    const KEYS: [&[u8]; 4] = [b"level", b"lvl", b"severity", b"levelname"];
    let end = line.len().min(STRUCT_SCAN_HEAD);
    let head = &line[..end];

    // 快速排除：logfmt 的首个 `=` 或 JSON 的 `{` 必在行首附近。
    // 这一次 SIMD 扫描让绝大多数传统日志跳过下面四轮子串搜索
    if line.first() != Some(&b'{')
        && memchr::memchr(b'=', &line[..line.len().min(STRUCT_PROBE)]).is_none()
    {
        return None;
    }

    for key in KEYS {
        let mut from = 0usize;
        while let Some(rel) = memchr::memmem::find(&head[from..], key) {
            let at = from + rel;
            // 键名前面应该是引号、空格、逗号或 { —— 否则可能是 "loglevel" 这种子串
            let ok_before =
                at == 0 || matches!(head[at - 1], b'"' | b'\'' | b' ' | b',' | b'{' | b'\t');
            if ok_before {
                if let Some(l) = value_after(head, at + key.len()) {
                    return Some(l);
                }
            }
            from = at + 1;
            if from >= head.len() {
                break;
            }
        }
    }
    None
}

/// 跳过 `":` / `=` / 引号 / 空格，读出紧跟着的那个词并解析成级别
fn value_after(buf: &[u8], mut i: usize) -> Option<Level> {
    let mut saw_sep = false;
    while i < buf.len() {
        match buf[i] {
            b'"' | b'\'' | b' ' | b'\t' => i += 1,
            b':' | b'=' => {
                saw_sep = true;
                i += 1;
            }
            _ => break,
        }
    }
    if !saw_sep {
        return None;
    }
    let start = i;
    while i < buf.len() && (buf[i].is_ascii_alphabetic()) {
        i += 1;
    }
    if i == start {
        return None;
    }
    word_to_level(&buf[start..i])
}

/// 大小写不敏感地把一个词转成级别
fn word_to_level(w: &[u8]) -> Option<Level> {
    const MAP: [(&[u8], Level); 11] = [
        (b"error", Level::Error),
        (b"fatal", Level::Error),
        (b"critical", Level::Error),
        (b"severe", Level::Error),
        (b"panic", Level::Error),
        (b"warn", Level::Warn),
        (b"warning", Level::Warn),
        (b"info", Level::Info),
        (b"notice", Level::Info),
        (b"debug", Level::Debug),
        (b"trace", Level::Trace),
    ];
    for (name, lvl) in MAP {
        if w.len() == name.len()
            && w.iter()
                .zip(name)
                .all(|(a, b)| a.to_ascii_lowercase() == *b)
        {
            return Some(lvl);
        }
    }
    None
}
