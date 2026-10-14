//! 几份日志按时间合成一条时间线（2026-10-14，设计稿 docs/MERGE.md）。
//!
//! 场景：网关、订单服务、库存服务各一份日志，从服务器拉下来，想看「这一次请求在三个服务里各发生了什么」。
//! 一份一份地开、对着时间戳来回切，是人在脑子里做归并；这里让机器做。
//!
//! # 归并
//!
//! 每份日志各自已经大体按时间排好（写日志的人就是按时间写的），要的是 **k 路归并**：每份一个游标，
//! 每次取「当前行时间最早」的那一份吐一行。k 很小（几份），用一个小顶堆，每行 O(log k)。
//! 再加一个省力的地方：取出最早那份之后，**只要它的下一行还不晚于第二早的那份，就接着吐它**，不回堆 ——
//! 日志是一段一段的（一条带堆栈的报错几十行、一次请求的几行），连着吐一段只做一次堆操作。
//!
//! **没时间戳的行跟着上一行走**（堆栈帧、多行 SQL、横幅）：它的时间键沿用这份文件里上一条有时间戳的行。
//! 于是一条报错和它的四十行堆栈在合并后仍然连在一起 —— 键相同、来自同一份，同键时按「第几份」定先后，
//! 别的文件插不进来。文件开头没时间戳的那几行（启动横幅）用这份文件**第一条**时间戳，排在它前面。
//!
//! **从不改一份文件内部的先后**：归并只决定「下一行从哪份取」，取的永远是那份的下一行。多线程写乱了几毫秒的日志，
//! 合并后照样是原文件的顺序 —— 不然一个人对着原文件找同一行会对不上。
//!
//! # 存什么：一行一个字节
//!
//! 合并的结果完全由「第 i 行来自第几份」这一串决定 —— 每份内部的顺序不变，知道来源就知道是那份的第几行。
//! 所以只存这一串（`sel`，每行 1 字节），外加每 [`STRIDE`] 行一个**检查点**：那一刻每份文件的游标（行号 + 字节偏移）。
//! 定位第 r 行 = 找到它前面最近的检查点，把这段 `sel` 走一遍（最多 1023 步，每步一次 memchr）。
//! 和 `index.rs` 的稀疏行索引是同一个思路：用一点扫描换掉大部分内存。
//!
//! 内存账：三份各 1GB（约 2400 万行）→ `sel` 24MB + 检查点 2.3 万个 × 3 份 × 16 字节 = 1.1MB。
//! 原文件一个字节都不复制（还是那几份 mmap）。要是每行存「(文件, 行号)」两个数，是 8 倍。
//!
//! # 时间怎么比
//!
//! `seek::ts_key` 认出来的键是「年月日时分秒毫秒」拼成的整数，不是真正的时刻：`23:59:59` 加 2 秒不会进位到第二天。
//! 这里换成**毫秒数**（`ms`，按公历算到 1970 年以来的天数，不管时区），才能做加减（时钟偏移、跨午夜）。
//! 三种时间戳精度要对齐：
//!
//! - 带年月日的（ISO、nginx）：直接换算
//! - 没有年的（syslog 的 `Aug 24 14:03:21`）：年份借第一份带年的文件
//! - 只有时分秒的（行首裸的 `14:03:21`）：日期借第一份带日期的文件；**往回跳超过 12 小时算过了午夜**，往后加一天
//!
//! 借来的东西要说出来（[`Clock`] 交给界面），不然人会以为两份日志本来就对得上。
//!
//! **时区和时钟偏差靠手动偏移**（`offsets`，毫秒）：一份 UTC、一份北京时间，给 UTC 那份 +8h。
//! 不猜时区 —— `Z`、`+0800` 认得出来，但「没写时区的那份是哪个时区」认不出来，猜错比不对齐更糟（看着对齐了，其实差 8 小时）。

use crate::filter::{self, FilterSpec, FilterTask};
use crate::level::LevelStats;
use crate::{block, seek, LogFile, LogStat};
use memmap2::Mmap;
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 最多合几份。来源记在一个字节里能到 255，但界面上一份一个颜色，超过十几份就分不清了；
/// 过滤时「只看哪几份」是一个 u32 的位掩码 —— 32 是这两头里较紧的那个
pub const MAX_SOURCES: usize = 32;

/// 每多少行落一个检查点。同 `index::DEFAULT_STRIDE` 的取舍：定位一行最多走 1023 步
const STRIDE: usize = 1024;

/// 每攒多少行发布一次快照。首屏只要等第一块（6.5 万行，几毫秒），不用等整个合并跑完；
/// 再小，快照（克隆块列表）的次数就上去了
const CHUNK_ROWS: usize = 64 * STRIDE;

/// 找每份文件的第一条时间戳最多看多少行。启动横幅、Spring 的 ASCII 图都在这个范围里
const LOOKAHEAD_LINES: usize = 2000;

/// 定位时最多往后找多少行去找一条带时间戳的（同 `seek.rs`）
const SEEK_SCAN: u64 = 512;

const DAY: i64 = 24 * 60 * 60 * 1000;

/// 这份文件的时间戳带不带日期 —— 决定要不要借、借了什么（界面要说出来）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Clock {
    /// 年月日都有
    Full,
    /// 有月日、没有年（syslog）
    NoYear,
    /// 只有时分秒
    TimeOnly,
    /// 前 [`LOOKAHEAD_LINES`] 行没认出一条时间戳：整份排在最前（它的行的键是负无穷）
    None,
}

impl Clock {
    pub fn as_str(self) -> &'static str {
        match self {
            Clock::Full => "full",
            Clock::NoYear => "noYear",
            Clock::TimeOnly => "timeOnly",
            Clock::None => "none",
        }
    }
}

/// `seek::pack` 的逆：(年, 月, 日, 一天里的毫秒)
fn unpack(key: u64) -> (u32, u32, u32, i64) {
    let tod = (key % DAY as u64) as i64;
    let day = key / DAY as u64;
    ((day / (13 * 32)) as u32, ((day / 32) % 13) as u32, (day % 32) as u32, tod)
}

fn classify(key: u64) -> Clock {
    let (y, mo, _, _) = unpack(key);
    if y > 0 {
        Clock::Full
    } else if mo > 0 {
        Clock::NoYear
    } else {
        Clock::TimeOnly
    }
}

/// 公历日期 → 1970-01-01 以来的天数（Howard Hinnant 的 `days_from_civil`）。
/// 用它而不是「365 × 年 + …」：闰年规则（4 / 100 / 400）一条都不能少，这个算法把它们折成了几次整除
pub fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400; // [0, 399]
    let m = m as i64;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d as i64 - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    era * 146_097 + doe - 719_468
}

/// `days_from_civil` 的逆，界面上说「按哪天对齐」、测试里用
pub fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// 缺的那部分日期借谁的（所有文件共用一份）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Fill {
    year: i64,
    /// 1970 以来的天数
    day: i64,
}

impl Fill {
    /// 键 → 毫秒（还没加偏移、没加跨午夜）
    fn ms(&self, key: u64) -> i64 {
        let (y, mo, d, tod) = unpack(key);
        match classify(key) {
            Clock::Full => days_from_civil(y as i64, mo, d) * DAY + tod,
            Clock::NoYear => days_from_civil(self.year, mo, d) * DAY + tod,
            _ => self.day * DAY + tod,
        }
    }
}

/// 从各份的第一条时间戳定出借什么：带年的里最早那天；一份带年的都没有，就借最早那份 syslog 的月日、年份记 1970
fn decide_fill(firsts: &[Option<u64>]) -> (Fill, bool) {
    let full = firsts.iter().flatten().filter(|k| classify(**k) == Clock::Full).min();
    if let Some(&k) = full {
        let (y, mo, d, _) = unpack(k);
        return (Fill { year: y as i64, day: days_from_civil(y as i64, mo, d) }, true);
    }
    let noyear = firsts.iter().flatten().filter(|k| classify(**k) == Clock::NoYear).min();
    if let Some(&k) = noyear {
        let (_, mo, d, _) = unpack(k);
        return (Fill { year: 1970, day: days_from_civil(1970, mo, d) }, false);
    }
    (Fill { year: 1970, day: 0 }, false)
}

/// 一份文件的第一条时间戳（前 [`LOOKAHEAD_LINES`] 行里）
fn first_key(data: &[u8]) -> Option<u64> {
    let mut pos = 0usize;
    for _ in 0..LOOKAHEAD_LINES {
        if pos >= data.len() {
            break;
        }
        let end = memchr::memchr(b'\n', &data[pos..]).map_or(data.len(), |n| pos + n);
        if let Some(k) = seek::ts_key(&data[pos..end]) {
            return Some(k);
        }
        pos = end + 1;
    }
    None
}

/// 每份文件的游标（检查点里存的就是这个）
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Cur {
    /// 下一行是这份文件的第几行（0-based，和 `LineIndex` 同一种数法）
    line: u64,
    /// 下一行从哪个字节开始
    pos: u64,
}

/// 一块合并结果：[`CHUNK_ROWS`] 行的来源，加上块里每 [`STRIDE`] 行一个检查点（每个检查点 k 个游标）。
/// 块一旦发布就不再变 —— 读者拿着 `Arc` 用，不用锁
struct Chunk {
    sel: Box<[u8]>,
    cps: Box<[Cur]>,
}

/// 读者看到的合并结果：已经合好的前 `rows` 行
#[derive(Default)]
struct Snapshot {
    chunks: Vec<Arc<Chunk>>,
    rows: u64,
    /// 每份文件的跨午夜记录：(从第几行起, 累计加了多少毫秒)，按行号递增。只在「只有时分秒」/「没有年」的文件里出现
    bumps: Vec<Vec<(u64, i64)>>,
}

impl Snapshot {
    #[inline]
    fn src_of(&self, row: u64) -> u8 {
        let c = &self.chunks[(row / CHUNK_ROWS as u64) as usize];
        c.sel[(row % CHUNK_ROWS as u64) as usize]
    }

    fn memory_footprint(&self, k: usize) -> usize {
        self.chunks.iter().map(|c| c.sel.len() + c.cps.len() * std::mem::size_of::<Cur>()).sum::<usize>()
            + self.chunks.capacity() * std::mem::size_of::<usize>()
            + k * std::mem::size_of::<Vec<(u64, i64)>>()
    }

    /// 第 `line` 行（这份文件里的）累计跨了几次午夜
    fn bump(&self, src: usize, line: u64) -> i64 {
        let Some(b) = self.bumps.get(src) else { return 0 };
        let i = b.partition_point(|(l, _)| *l <= line);
        if i == 0 {
            0
        } else {
            b[i - 1].1
        }
    }
}

/// 一次合并（换了偏移就重来一次，旧的叫停）
struct Build {
    snap: Mutex<Arc<Snapshot>>,
    done: AtomicBool,
    /// 已经归并掉的字节数（所有份加起来），进度用
    progress: AtomicU64,
    cancel: AtomicBool,
}

impl Build {
    fn snapshot(&self) -> Arc<Snapshot> {
        Arc::clone(&self.snap.lock().expect("合并快照锁被毒化"))
    }
}

/// 一份正在归并的文件
struct Lane<'a> {
    data: &'a [u8],
    cur: Cur,
    /// 当前行在哪结束（不含 `\n`）
    end: usize,
    /// 当前行的合并键（毫秒，已加偏移和跨午夜）
    key: i64,
    /// 上一条有时间戳的行的键（未加偏移）—— 没时间戳的行沿用它
    carry: i64,
    /// 跨午夜判断用：上一条有时间戳的行（未加偏移）
    prev: Option<i64>,
    bump: i64,
    bumps: Vec<(u64, i64)>,
    offset: i64,
}

impl Lane<'_> {
    fn done(&self) -> bool {
        self.cur.pos as usize >= self.data.len()
    }

    /// 算当前行的结尾和键
    fn load(&mut self, fill: &Fill) {
        let pos = self.cur.pos as usize;
        self.end = memchr::memchr(b'\n', &self.data[pos..]).map_or(self.data.len(), |n| pos + n);
        if let Some(k) = seek::ts_key(&self.data[pos..self.end]) {
            let mut ms = fill.ms(k) + self.bump;
            /*
             * 跨午夜：只有时分秒的文件，23:59:58 之后是 00:00:01 —— 不加一天的话，这一行会被排到当天最前面去。
             * 判据是「比上一条早了 12 小时以上」：多线程写乱的几毫秒、几秒远到不了，真过了午夜一定到。
             * 没有年的同理，12 月之后是 1 月（阈值半年）。带年月日的不用判
             */
            let (thr, step) = match classify(k) {
                Clock::TimeOnly => (DAY / 2, DAY),
                Clock::NoYear => (183 * DAY, 365 * DAY),
                _ => (i64::MAX, 0),
            };
            if let Some(p) = self.prev {
                if step > 0 && ms < p - thr {
                    self.bump += step;
                    ms += step;
                    self.bumps.push((self.cur.line, self.bump));
                }
            }
            self.prev = Some(ms);
            self.carry = ms;
        }
        self.key = self.carry.saturating_add(self.offset);
    }

    /// 吐掉当前行，游标挪到下一行
    fn advance(&mut self) {
        self.cur.pos = self.end as u64 + 1;
        self.cur.line += 1;
    }
}

/// 把 `maps` 归并成一份，边合边发布快照。**被取消时返回 `false`**，不发布最后那块。
///
/// 拎成函数同 `lib.rs` 的 `build_index`：为了能把取消开关预先置位再调，不靠赛跑
fn merge_into(maps: &[Arc<Mmap>], fill: Fill, offsets: &[i64], build: &Build) -> bool {
    let k = maps.len();
    let mut lanes: Vec<Lane> = maps
        .iter()
        .zip(offsets)
        .map(|(m, &offset)| {
            let data: &[u8] = m;
            // 开头没时间戳的那几行（横幅）用第一条时间戳，排在这份文件第一条记录的前面
            let carry = first_key(data).map_or(i64::MIN / 4, |k0| fill.ms(k0));
            Lane { data, cur: Cur::default(), end: 0, key: 0, carry, prev: None, bump: 0, bumps: Vec::new(), offset }
        })
        .collect();

    // 堆里放 (键, 第几份)：同一毫秒按「第几份」定先后，同一份里永远是原顺序
    let mut heap: BinaryHeap<Reverse<(i64, u8)>> = BinaryHeap::with_capacity(k);
    for (i, l) in lanes.iter_mut().enumerate() {
        if !l.done() {
            l.load(&fill);
            heap.push(Reverse((l.key, i as u8)));
        }
    }

    let mut chunks: Vec<Arc<Chunk>> = Vec::new();
    let mut sel: Vec<u8> = Vec::with_capacity(CHUNK_ROWS);
    let mut cps: Vec<Cur> = Vec::with_capacity(CHUNK_ROWS / STRIDE * k);
    let mut rows = 0u64;

    let publish = |chunks: &[Arc<Chunk>], rows: u64, lanes: &[Lane]| {
        let snap = Snapshot { chunks: chunks.to_vec(), rows, bumps: lanes.iter().map(|l| l.bumps.clone()).collect() };
        *build.snap.lock().expect("合并快照锁被毒化") = Arc::new(snap);
        let consumed: u64 = lanes.iter().map(|l| (l.cur.pos as usize).min(l.data.len()) as u64).sum();
        build.progress.store(consumed, Ordering::Relaxed);
    };

    while let Some(Reverse((_, s))) = heap.pop() {
        let si = s as usize;
        // 第二早的那份：当前这份的下一行只要不晚于它，就接着吐，不回堆
        let bound = heap.peek().map(|r| r.0);
        loop {
            if sel.len() % STRIDE == 0 {
                cps.extend(lanes.iter().map(|l| l.cur));
            }
            sel.push(s);
            rows += 1;
            lanes[si].advance();
            if sel.len() == CHUNK_ROWS {
                chunks.push(Arc::new(Chunk {
                    sel: std::mem::replace(&mut sel, Vec::with_capacity(CHUNK_ROWS)).into_boxed_slice(),
                    cps: std::mem::replace(&mut cps, Vec::with_capacity(CHUNK_ROWS / STRIDE * k)).into_boxed_slice(),
                }));
                publish(&chunks, rows, &lanes);
                // 每块问一次（6.5 万行，毫秒级）
                if build.cancel.load(Ordering::Relaxed) {
                    return false;
                }
            }
            if lanes[si].done() {
                break;
            }
            lanes[si].load(&fill);
            if bound.is_none_or(|b| (lanes[si].key, s) < b) {
                continue;
            }
            heap.push(Reverse((lanes[si].key, s)));
            break;
        }
    }
    if !sel.is_empty() {
        chunks.push(Arc::new(Chunk { sel: sel.into_boxed_slice(), cps: cps.into_boxed_slice() }));
    }
    chunks.shrink_to_fit();
    publish(&chunks, rows, &lanes);
    build.done.store(true, Ordering::Release);
    true
}

/// 在合并结果上顺着走：手里拿着每份文件的游标，一行一行往下吐
struct Walk<'a> {
    snap: &'a Snapshot,
    maps: &'a [Arc<Mmap>],
    cur: Vec<Cur>,
    row: u64,
}

impl<'a> Walk<'a> {
    /// 停在第 `row` 行前面（下一次 `next` 吐的就是它）
    fn at(snap: &'a Snapshot, maps: &'a [Arc<Mmap>], row: u64) -> Option<Self> {
        if row >= snap.rows {
            return None;
        }
        let k = maps.len();
        let chunk = &snap.chunks[(row / CHUNK_ROWS as u64) as usize];
        let within = (row % CHUNK_ROWS as u64) as usize;
        let cp = within / STRIDE;
        let mut w = Walk { snap, maps, cur: chunk.cps[cp * k..cp * k + k].to_vec(), row: row - (within - cp * STRIDE) as u64 };
        while w.row < row {
            w.skip();
        }
        Some(w)
    }

    /// 这一行的来源和这一份里的行号，不读内容
    #[inline]
    fn peek(&self) -> Option<(u8, u64)> {
        (self.row < self.snap.rows).then(|| {
            let s = self.snap.src_of(self.row);
            (s, self.cur[s as usize].line)
        })
    }

    fn line_end(&self, s: usize) -> usize {
        let data: &[u8] = &self.maps[s];
        let pos = self.cur[s].pos as usize;
        memchr::memchr(b'\n', &data[pos.min(data.len())..]).map_or(data.len(), |n| pos + n)
    }

    fn skip(&mut self) {
        let s = self.snap.src_of(self.row) as usize;
        let end = self.line_end(s);
        self.cur[s] = Cur { line: self.cur[s].line + 1, pos: end as u64 + 1 };
        self.row += 1;
    }

    /// 吐一行：(来源, 这一份里的行号, 内容)
    fn next(&mut self) -> Option<(u8, u64, &'a [u8])> {
        if self.row >= self.snap.rows {
            return None;
        }
        let s = self.snap.src_of(self.row);
        let si = s as usize;
        let maps: &'a [Arc<Mmap>] = self.maps;
        let data: &'a [u8] = &maps[si];
        let pos = (self.cur[si].pos as usize).min(data.len());
        let end = self.line_end(si);
        let line = self.cur[si].line;
        self.cur[si] = Cur { line: line + 1, pos: end as u64 + 1 };
        self.row += 1;
        Some((s, line, &data[pos..end]))
    }
}

/// 合并视图里一份文件的信息，给界面画图例
#[derive(Debug, Clone)]
pub struct SourceInfo {
    pub path: PathBuf,
    pub clock: Clock,
    /// 手动偏移（毫秒）
    pub offset_ms: i64,
}

/// 几份日志合成的一条时间线。对外的样子和 [`LogFile`] 一样（取行、统计、过滤、按时间跳），
/// 命令层拿同一套命令服务它，前端的日志视图一行不用改就能显示
pub struct MergedLog {
    files: Vec<Arc<LogFile>>,
    maps: Vec<Arc<Mmap>>,
    fill: Fill,
    /// 借来的日期是不是从一份带年月日的文件来的（否则是 1970，界面上不说「按哪天对齐」）
    fill_real: bool,
    clocks: Vec<Clock>,
    offsets: Mutex<Vec<i64>>,
    build: Mutex<Arc<Build>>,
}

/// 关掉标签就叫停正在跑的那次合并（同 `LogFile` 的 Drop：放在调用点会有人忘）
impl Drop for MergedLog {
    fn drop(&mut self) {
        self.current().cancel.store(true, Ordering::Release);
    }
}

impl MergedLog {
    /// 打开几份文件并开始归并。打开本身不读盘（每份都是 mmap），归并在后台线程里，
    /// 第一块（6.5 万行）合好就能显示
    pub fn open(paths: &[impl AsRef<Path>]) -> io::Result<Self> {
        if paths.len() < 2 || paths.len() > MAX_SOURCES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("合并要 2 到 {MAX_SOURCES} 份日志，给了 {} 份", paths.len()),
            ));
        }
        let files = paths.iter().map(|p| LogFile::open(p).map(Arc::new)).collect::<io::Result<Vec<_>>>()?;
        let maps: Vec<Arc<Mmap>> = files.iter().map(|f| f.mapped()).collect();
        let firsts: Vec<Option<u64>> = maps.iter().map(|m| first_key(m)).collect();
        let clocks = firsts.iter().map(|k| k.map_or(Clock::None, classify)).collect();
        let (fill, fill_real) = decide_fill(&firsts);
        let me = Self {
            offsets: Mutex::new(vec![0; files.len()]),
            files,
            maps,
            fill,
            fill_real,
            clocks,
            build: Mutex::new(Arc::new(Build {
                snap: Mutex::new(Arc::new(Snapshot::default())),
                done: AtomicBool::new(false),
                progress: AtomicU64::new(0),
                cancel: AtomicBool::new(false),
            })),
        };
        me.start(vec![0; me.files.len()])?;
        Ok(me)
    }

    fn current(&self) -> Arc<Build> {
        Arc::clone(&self.build.lock().expect("合并任务锁被毒化"))
    }

    /// 起一次归并，顶掉（并叫停）上一次
    fn start(&self, offsets: Vec<i64>) -> io::Result<()> {
        let k = self.files.len();
        let b = Arc::new(Build {
            snap: Mutex::new(Arc::new(Snapshot { chunks: Vec::new(), rows: 0, bumps: vec![Vec::new(); k] })),
            done: AtomicBool::new(false),
            progress: AtomicU64::new(0),
            cancel: AtomicBool::new(false),
        });
        let old = std::mem::replace(&mut *self.build.lock().expect("合并任务锁被毒化"), Arc::clone(&b));
        old.cancel.store(true, Ordering::Release);
        *self.offsets.lock().expect("偏移锁被毒化") = offsets.clone();
        let (maps, fill) = (self.maps.clone(), self.fill);
        std::thread::Builder::new().name("logengine-merge".into()).spawn(move || {
            merge_into(&maps, fill, &offsets, &b);
        })?;
        Ok(())
    }

    /// 改时钟偏移（毫秒，每份一个），整份重新归并。行号全变了：调用方要丢掉缓存、清掉过滤
    pub fn set_offsets(&self, offsets: Vec<i64>) -> io::Result<()> {
        if offsets.len() != self.files.len() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "偏移的个数和文件的份数对不上"));
        }
        self.start(offsets)
    }

    pub fn sources(&self) -> Vec<SourceInfo> {
        let offs = self.offsets.lock().expect("偏移锁被毒化").clone();
        self.files
            .iter()
            .zip(&self.clocks)
            .zip(offs)
            .map(|((f, &clock), offset_ms)| SourceInfo { path: f.path().to_path_buf(), clock, offset_ms })
            .collect()
    }

    /// 缺日期的文件借的是哪一天（年, 月, 日）。没有一份带年月日时是 None —— 那时借来的是个凑数的 1970，不值得说
    pub fn fill_date(&self) -> Option<(i64, u32, u32)> {
        self.fill_real.then(|| civil_from_days(self.fill.day))
    }

    pub fn wait_merged(&self) {
        let b = self.current();
        while !b.done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }

    pub fn stat(&self) -> LogStat {
        let b = self.current();
        let snap = b.snapshot();
        let mut levels = LevelStats::default();
        let (mut levels_complete, mut levels_scanned) = (true, 0u64);
        for f in &self.files {
            let s = f.stat();
            levels.absorb(&s.levels);
            levels_complete &= s.levels_complete;
            levels_scanned += s.levels_scanned;
        }
        LogStat {
            line_count: snap.rows,
            indexed_bytes: b.progress.load(Ordering::Relaxed),
            total_bytes: self.maps.iter().map(|m| m.len() as u64).sum(),
            complete: b.done.load(Ordering::Acquire),
            index_bytes: snap.memory_footprint(self.files.len()) as u64,
            levels,
            levels_complete,
            levels_scanned,
        }
    }

    /// 取 `[start, start + count)` 行，线格式同 [`LogFile::read_block`]
    pub fn read_block(&self, start: u64, count: u32) -> Vec<u8> {
        let snap = self.current().snapshot();
        let Some(mut w) = Walk::at(&snap, &self.maps, start) else {
            return block::encode(start, &[]);
        };
        let mut lines: Vec<&[u8]> = Vec::with_capacity(count as usize);
        for _ in 0..count {
            match w.next() {
                Some((_, _, b)) => lines.push(b),
                None => break,
            }
        }
        block::encode(start, &lines)
    }

    /// 按（合并后的）行号列表取，行号须递增 —— 过滤视图的回表路径
    pub fn read_block_at(&self, rows: &[u64]) -> Vec<u8> {
        let snap = self.current().snapshot();
        let mut lines: Vec<&[u8]> = Vec::with_capacity(rows.len());
        self.walk_rows(&snap, rows, |_, _, b| lines.push(b));
        block::encode(rows.first().copied().unwrap_or(0), &lines)
    }

    /// `[start, start + count)` 每行的来源（第几份）和它在那份文件里的行号 —— 界面画来源色条、行号栏显示原文件的行号
    pub fn rows_meta(&self, start: u64, count: u32) -> (Vec<u8>, Vec<u64>) {
        let snap = self.current().snapshot();
        let (mut src, mut line) = (Vec::new(), Vec::new());
        if let Some(mut w) = Walk::at(&snap, &self.maps, start) {
            for _ in 0..count {
                let Some((s, l)) = w.peek() else { break };
                src.push(s);
                line.push(l);
                w.skip();
            }
        }
        (src, line)
    }

    /// 同 [`MergedLog::rows_meta`]，按行号列表（过滤视图）
    pub fn rows_meta_at(&self, rows: &[u64]) -> (Vec<u8>, Vec<u64>) {
        let snap = self.current().snapshot();
        let (mut src, mut line) = (Vec::with_capacity(rows.len()), Vec::with_capacity(rows.len()));
        self.walk_rows(&snap, rows, |s, l, _| {
            src.push(s);
            line.push(l);
        });
        (src, line)
    }

    /// 沿着递增的行号列表走：隔得近就顺着走，远了就从检查点重新定位
    fn walk_rows<'a>(&'a self, snap: &'a Snapshot, rows: &[u64], mut f: impl FnMut(u8, u64, &'a [u8])) {
        let mut w: Option<Walk<'a>> = None;
        for &r in rows {
            let near = w.as_ref().is_some_and(|w| r >= w.row && r - w.row < STRIDE as u64);
            if !near {
                w = Walk::at(snap, &self.maps, r);
            }
            let Some(walk) = w.as_mut() else { break };
            while walk.row < r {
                walk.skip();
            }
            match walk.next() {
                Some((s, l, b)) => f(s, l, b),
                None => break,
            }
        }
    }

    /// 合并后第 `row` 行起往后（最多 [`SEEK_SCAN`] 行）第一条有时间戳的：(行, 合并键)
    fn key_at_or_after(&self, snap: &Snapshot, row: u64) -> Option<(u64, i64)> {
        let offs = self.offsets.lock().expect("偏移锁被毒化").clone();
        let mut w = Walk::at(snap, &self.maps, row)?;
        for _ in 0..SEEK_SCAN {
            let r = w.row;
            let (s, l, b) = w.next()?;
            if let Some(k) = seek::ts_key(b) {
                let si = s as usize;
                return Some((r, self.fill.ms(k) + snap.bump(si, l) + offs[si]));
            }
        }
        None
    }

    /// 跳到时间：合并后第一条时间 ≥ 目标的行。目标不带日期时用 `near` 那一行附近的日期（同 `seek.rs`）。
    /// 时间是**合并后的时间线**上的：给某份加了 +8h 偏移，按的就是加完之后的钟
    pub fn seek_time(&self, q: &str, near: u64) -> Option<u64> {
        let snap = self.current().snapshot();
        if snap.rows == 0 {
            return None;
        }
        let t = seek::parse_target(q)?;
        let tod = ((t.h as i64 * 60 + t.m as i64) * 60 + t.s as i64) * 1000 + t.ms as i64;
        let target = match t.date {
            Some((y, mo, d)) => days_from_civil(y as i64, mo, d) * DAY + tod,
            None => {
                let near = near.min(snap.rows - 1);
                // 先往后找，找不到再从往前 512 行的地方往后找
                let (_, k) = self
                    .key_at_or_after(&snap, near)
                    .or_else(|| self.key_at_or_after(&snap, near.saturating_sub(SEEK_SCAN)))?;
                k.div_euclid(DAY) * DAY + tod
            }
        };
        // lower bound，谓词同 seek.rs：「从这一行往后最近的时间戳 ≥ 目标」关于行号单调
        let (mut lo, mut hi) = (0u64, snap.rows);
        let mut found = false;
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            match self.key_at_or_after(&snap, mid) {
                Some((r, k)) => {
                    found = true;
                    if k >= target {
                        hi = mid;
                    } else {
                        lo = r + 1;
                    }
                }
                None => hi = mid,
            }
        }
        if !found {
            return None;
        }
        Some(self.key_at_or_after(&snap, lo).map_or(snap.rows - 1, |(r, _)| r))
    }

    /// 过滤。`sources` 是「只看哪几份」的位掩码（第 i 位 = 第 i 份）。
    ///
    /// 每份文件用它自己的级别表和单文件那套过滤循环（`filter::run`）各跑一遍，得到各自的命中行号；
    /// 再沿着合并结果走一遍，把「第 s 份的第 l 行」换成合并后的行号 —— 走一遍就是按合并顺序排好的，不用再排序。
    /// 要等合并、每份的索引和级别扫描都跑完才开始（都在后台跑着，几百毫秒的事）
    pub fn start_filter(&self, spec: FilterSpec, sources: u32) -> io::Result<Arc<FilterTask>> {
        let task = FilterTask::fresh();
        let (t, files, build, maps) = (Arc::clone(&task), self.files.clone(), self.current(), self.maps.clone());
        std::thread::Builder::new().name("logengine-merge-filter".into()).spawn(move || {
            let ready = || build.done.load(Ordering::Acquire) && files.iter().all(|f| f.is_indexed() && f.is_leveled());
            while !ready() {
                if t.is_cancelled() || build.cancel.load(Ordering::Acquire) {
                    return;
                }
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            let all_rows = spec.is_noop();
            let mut per: Vec<Hits> = Vec::with_capacity(files.len());
            let mut base = (0u64, 0u64);
            for (i, f) in files.iter().enumerate() {
                let n = f.stat().line_count;
                if sources & (1 << i) == 0 {
                    t.add_progress(n, 0);
                    per.push(Hits::None);
                } else if all_rows {
                    t.add_progress(n, n);
                    per.push(Hits::All);
                } else {
                    let hits = filter::run(&maps[i], &f.index_snapshot(), &f.levels(), &spec, &t, base);
                    if t.is_cancelled() {
                        return;
                    }
                    per.push(Hits::List(hits));
                }
                base = (t.scanned_lines(), t.hit_count());
            }
            let snap = build.snapshot();
            let rows = map_hits(&snap, &per);
            t.publish(rows);
        })?;
        Ok(task)
    }
}

/// 一份文件的过滤结果
enum Hits {
    /// 整份都不看
    None,
    /// 整份都算（这份没被筛掉、又没有别的条件）—— 不用把它几百万个行号真的列出来
    All,
    /// 命中的行号，递增
    List(Vec<u64>),
}

/// 沿着合并结果走一遍，把各份的命中行号换成合并后的行号（递增）
fn map_hits(snap: &Snapshot, per: &[Hits]) -> Vec<u64> {
    let mut next = vec![0usize; per.len()];
    let mut line = vec![0u64; per.len()];
    let mut out = Vec::new();
    for r in 0..snap.rows {
        let s = snap.src_of(r) as usize;
        let l = line[s];
        line[s] += 1;
        let hit = match &per[s] {
            Hits::None => false,
            Hits::All => true,
            Hits::List(v) => {
                let ok = v.get(next[s]) == Some(&l);
                if ok {
                    next[s] += 1;
                }
                ok
            }
        };
        if hit {
            out.push(r);
        }
    }
    out.shrink_to_fit();
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::level::LevelMask;
    use crate::TextFilter;
    use std::io::Write;

    struct Tmp(Vec<PathBuf>);
    impl Drop for Tmp {
        fn drop(&mut self) {
            for p in &self.0 {
                std::fs::remove_file(p).ok();
            }
        }
    }

    /// 写几份临时日志。文件名带 pid 和测试名：并行跑的测试别互相踩
    fn logs(tag: &str, bodies: &[&str]) -> Tmp {
        Tmp(bodies
            .iter()
            .enumerate()
            .map(|(i, b)| {
                let p = std::env::temp_dir().join(format!("logengine-merge-{}-{tag}-{i}.log", std::process::id()));
                #[allow(clippy::disallowed_methods)] // 测试自己的临时文件
                let mut f = std::fs::File::create(&p).unwrap();
                f.write_all(b.as_bytes()).unwrap();
                p
            })
            .collect())
    }

    fn texts(m: &MergedLog) -> Vec<String> {
        let n = m.stat().line_count;
        block::decode_full(&m.read_block(0, n as u32 + 5)).1.into_iter().map(|(b, _)| String::from_utf8(b).unwrap()).collect()
    }

    fn open(t: &Tmp) -> MergedLog {
        let m = MergedLog::open(&t.0).unwrap();
        m.wait_merged();
        m
    }

    #[test]
    fn 公历换算_来回一致_闰年也对() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(2000, 3, 1) - days_from_civil(2000, 2, 28), 2, "2000 是闰年（能被 400 整除）");
        assert_eq!(days_from_civil(1900, 3, 1) - days_from_civil(1900, 2, 28), 1, "1900 不是（能被 100 整除）");
        for d in [-1000i64, 0, 1, 59, 10_957, 20_000, 20_689] {
            let (y, m, dd) = civil_from_days(d);
            assert_eq!(days_from_civil(y, m, dd), d, "{d} 天");
        }
        assert_eq!(civil_from_days(days_from_civil(2026, 10, 14)), (2026, 10, 14));
    }

    #[test]
    fn 两份按时间交织_每份内部顺序不变() {
        let t = logs(
            "basic",
            &[
                "2026-08-24 10:00:00.000 INFO  gw a\n2026-08-24 10:00:02.000 INFO  gw c\n2026-08-24 10:00:04.000 INFO  gw e\n",
                "2026-08-24 10:00:01.000 INFO  ord b\n2026-08-24 10:00:03.000 INFO  ord d\n",
            ],
        );
        let m = open(&t);
        let got: Vec<String> = texts(&m).iter().map(|s| s.rsplit(' ').next().unwrap().to_string()).collect();
        assert_eq!(got, ["a", "b", "c", "d", "e"]);
        let (src, line) = m.rows_meta(0, 10);
        assert_eq!(src, [0, 1, 0, 1, 0]);
        assert_eq!(line, [0, 0, 1, 1, 2], "行号是在原文件里的");
        assert_eq!(m.stat().line_count, 5);
        assert!(m.stat().complete);
    }

    /// 这条是这个功能的要害：一条报错和它的堆栈在合并后不能被别的文件插进来拆散
    #[test]
    fn 堆栈跟着它那条记录走_不被拆散() {
        let t = logs(
            "stack",
            &[
                "2026-08-24 10:00:00.000 ERROR boom\njava.lang.RuntimeException: x\n\tat a.B(B.java:1)\n\tat a.C(C.java:2)\n2026-08-24 10:00:05.000 INFO after\n",
                "2026-08-24 10:00:00.500 INFO other\n2026-08-24 10:00:01.000 INFO other2\n",
            ],
        );
        let m = open(&t);
        let got = texts(&m);
        assert_eq!(got[0], "2026-08-24 10:00:00.000 ERROR boom");
        assert_eq!(&got[1..4], ["java.lang.RuntimeException: x", "\tat a.B(B.java:1)", "\tat a.C(C.java:2)"], "堆栈紧跟着报错那行");
        assert_eq!(got[4], "2026-08-24 10:00:00.500 INFO other");
        assert_eq!(got[6], "2026-08-24 10:00:05.000 INFO after");
    }

    #[test]
    fn 同一毫秒按第几份定先后_开头的横幅排在这份第一条前面() {
        let t = logs(
            "tie",
            &[
                "  ____ banner\n2026-08-24 10:00:01.000 INFO a1\n",
                "2026-08-24 10:00:00.000 INFO b0\n2026-08-24 10:00:01.000 INFO b1\n",
            ],
        );
        let got = texts(&open(&t));
        assert_eq!(got, ["2026-08-24 10:00:00.000 INFO b0", "  ____ banner", "2026-08-24 10:00:01.000 INFO a1", "2026-08-24 10:00:01.000 INFO b1"]);
    }

    /// 乱了几毫秒的多线程日志：合并后这一份的行还是原文件的顺序
    #[test]
    fn 一份内部乱序时不重排它() {
        let t = logs(
            "unsorted",
            &["2026-08-24 10:00:03.000 INFO x3\n2026-08-24 10:00:01.000 INFO x1\n", "2026-08-24 10:00:02.000 INFO y2\n"],
        );
        let (src, line) = open(&t).rows_meta(0, 10);
        let x: Vec<u64> = src.iter().zip(&line).filter(|(s, _)| **s == 0).map(|(_, l)| *l).collect();
        assert_eq!(x, [0, 1], "第 0 份的两行保持原顺序");
    }

    #[test]
    fn 只有时分秒的_借带日期那份的日期_过午夜加一天() {
        let t = logs(
            "timeonly",
            &[
                "2026-08-24 23:59:59.000 INFO full-a\n2026-08-25 00:00:02.000 INFO full-b\n",
                "23:59:58 bare-a\n00:00:01 bare-b\n",
            ],
        );
        let m = open(&t);
        let tail: Vec<String> = texts(&m).iter().map(|s| s.rsplit(' ').next().unwrap().to_string()).collect();
        assert_eq!(tail, ["bare-a", "full-a", "bare-b", "full-b"], "00:00:01 是第二天的，不能排到最前面");
        assert_eq!(m.fill_date(), Some((2026, 8, 24)));
        let clocks: Vec<Clock> = m.sources().iter().map(|s| s.clock).collect();
        assert_eq!(clocks, [Clock::Full, Clock::TimeOnly]);
    }

    #[test]
    fn 偏移_整份挪动之后重排() {
        let t = logs(
            "offset",
            &["2026-08-24 10:00:00.000 INFO cst\n2026-08-24 10:00:10.000 INFO cst2\n", "2026-08-24 02:00:05.000 INFO utc\n"],
        );
        let m = open(&t);
        assert_eq!(m.rows_meta(0, 10).0, [1, 0, 0], "没偏移时 UTC 那份 02:00 排最前");
        m.set_offsets(vec![0, 8 * 3600 * 1000]).unwrap();
        m.wait_merged();
        assert_eq!(m.rows_meta(0, 10).0, [0, 1, 0], "+8h 之后它落在 10:00:05，夹在中间");
        assert_eq!(m.sources()[1].offset_ms, 8 * 3600 * 1000);
    }

    /// 跨块、跨检查点：随便挑一行，按行号取出来的要和从头顺着走到它的一样
    #[test]
    fn 跨检查点和块随机定位都对() {
        let mut a = String::new();
        let mut b = String::new();
        // 两份交错，再夹一些没时间戳的行，总行数跨过一块（65536）
        for i in 0..40_000u32 {
            let ms = i * 2;
            a.push_str(&format!("2026-08-24 10:{:02}:{:02}.{:03} INFO a{i}\n", ms / 60_000, (ms / 1000) % 60, ms % 1000));
            if i % 7 == 0 {
                a.push_str(&format!("\tat frame{i}\n"));
            }
            let ms = i * 2 + 1;
            b.push_str(&format!("2026-08-24 10:{:02}:{:02}.{:03} INFO b{i}\n", ms / 60_000, (ms / 1000) % 60, ms % 1000));
        }
        let t = logs("big", &[&a, &b]);
        let m = open(&t);
        let n = m.stat().line_count;
        assert_eq!(n, 40_000 + 40_000_u64.div_ceil(7) + 40_000);
        assert!(n > CHUNK_ROWS as u64, "要跨块才测得到块边界");
        let all = texts(&m);
        for r in [0u64, 1, 1023, 1024, 1025, 65_535, 65_536, 65_537, n - 1] {
            let one = block::decode_full(&m.read_block(r, 1)).1;
            assert_eq!(String::from_utf8(one[0].0.clone()).unwrap(), all[r as usize], "第 {r} 行");
        }
        let picks = [3u64, 4, 1030, 70_000, n - 2];
        let at: Vec<String> = block::decode_full(&m.read_block_at(&picks)).1.into_iter().map(|(b, _)| String::from_utf8(b).unwrap()).collect();
        for (i, r) in picks.iter().enumerate() {
            assert_eq!(at[i], all[*r as usize], "按列表取第 {r} 行");
        }
        // 时间戳是严格交替的：a0 b0 a1 b1 …（中间夹着 a 的堆栈帧）
        assert!(all[0].ends_with(" a0") && all[1].starts_with("\tat frame0") && all[2].ends_with(" b0"));
    }

    #[test]
    fn 按时间跳_落到合并后第一条不早于它的行() {
        let t = logs(
            "seek",
            &[
                "2026-08-24 10:00:00.000 INFO a\n2026-08-24 10:00:20.000 INFO c\n",
                "2026-08-24 10:00:10.000 INFO b\n2026-08-24 10:00:30.000 INFO d\n",
            ],
        );
        let m = open(&t);
        assert_eq!(m.seek_time("10:00:05", 0), Some(1));
        assert_eq!(m.seek_time("10:00:10", 0), Some(1), "正好命中");
        assert_eq!(m.seek_time("10:00:25", 0), Some(3));
        assert_eq!(m.seek_time("2026-08-24 09:00", 0), Some(0));
        assert_eq!(m.seek_time("11:00", 0), Some(3), "比全部都晚 → 最后一行");
        // 给第 1 份 −30s：b 变成 09:59:40、d 变成 10:00:00 —— 跳到 10:00 应当落在合并后的第 1 行（d 和 a 同一毫秒，a 是第 0 份排前面）
        m.set_offsets(vec![0, -30_000]).unwrap();
        m.wait_merged();
        assert_eq!(m.rows_meta(0, 4).0, [1, 0, 1, 0]);
        assert_eq!(m.seek_time("10:00", 0), Some(1), "按偏移之后的钟");
    }

    fn spec(pat: &str) -> FilterSpec {
        FilterSpec { levels: LevelMask::ALL, text: TextFilter::single(pat.as_bytes().to_vec()), case_sensitive: false, collapse_stacks: false }
    }

    fn wait(task: &FilterTask) -> Vec<u64> {
        let t0 = std::time::Instant::now();
        while !task.is_complete() {
            assert!(t0.elapsed().as_secs() < 10, "过滤 10 秒没跑完");
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        task.hits().to_vec()
    }

    #[test]
    fn 过滤_命中换成合并后的行号_还能只看某几份() {
        let t = logs(
            "filter",
            &[
                "2026-08-24 10:00:00.000 INFO order-1 in\n2026-08-24 10:00:02.000 INFO other\n2026-08-24 10:00:04.000 ERROR order-1 fail\n",
                "2026-08-24 10:00:01.000 INFO order-1 reserve\n2026-08-24 10:00:03.000 INFO order-2\n",
            ],
        );
        let m = open(&t);
        // 合并后：0 a:in / 1 b:reserve / 2 a:other / 3 b:order-2 / 4 a:fail
        assert_eq!(wait(&m.start_filter(spec("order-1"), u32::MAX).unwrap()), [0, 1, 4]);
        assert_eq!(wait(&m.start_filter(spec("order-1"), 0b10).unwrap()), [1], "只看第 1 份");
        assert_eq!(wait(&m.start_filter(spec(""), 0b01).unwrap()), [0, 2, 4], "没有别的条件、只筛来源：整份都算");
        let errors = FilterSpec { levels: LevelMask::from_bits(1), text: TextFilter::default(), case_sensitive: false, collapse_stacks: false };
        assert_eq!(wait(&m.start_filter(errors, u32::MAX).unwrap()), [4], "级别过滤用的是每份自己的级别表");
        let (src, line) = m.rows_meta_at(&[1, 4]);
        assert_eq!((src, line), (vec![1, 0], vec![0, 2]));
    }

    /// 关掉标签：正在跑的那次合并要停下来（同 LogFile）
    #[test]
    fn 析构就叫停合并() {
        let t = logs("cancel", &["2026-08-24 10:00:00 INFO a\n", "2026-08-24 10:00:01 INFO b\n"]);
        let m = MergedLog::open(&t.0).unwrap();
        let b = m.current();
        drop(m);
        assert!(b.cancel.load(Ordering::Acquire), "MergedLog 析构了，后台合并却没被通知");
    }

    /// 合并被取消就不交最后那块、不置 done（半份合并会让行数少一截却看着像完整的）
    #[test]
    fn 合并被预先取消时不发布完成() {
        let mut a = String::new();
        for i in 0..70_000u32 {
            a.push_str(&format!("2026-08-24 10:00:00.{:03} INFO a{i}\n", i % 1000));
        }
        let t = logs("precancel", &[&a, "2026-08-24 10:00:00 INFO b\n"]);
        let maps: Vec<Arc<Mmap>> = t.0.iter().map(|p| LogFile::open(p).unwrap().mapped()).collect();
        let b = Build { snap: Mutex::new(Arc::new(Snapshot::default())), done: AtomicBool::new(false), progress: AtomicU64::new(0), cancel: AtomicBool::new(true) };
        assert!(!merge_into(&maps, Fill { year: 2026, day: 0 }, &[0, 0], &b), "取消了还合到底");
        assert!(!b.done.load(Ordering::Acquire));
        assert!(b.snapshot().rows < 70_001);
        let go = Build { snap: Mutex::new(Arc::new(Snapshot::default())), done: AtomicBool::new(false), progress: AtomicU64::new(0), cancel: AtomicBool::new(false) };
        assert!(merge_into(&maps, Fill { year: 2026, day: 0 }, &[0, 0], &go));
        assert_eq!(go.snapshot().rows, 70_001);
    }

    #[test]
    fn 份数不对就报错() {
        let t = logs("count", &["a\n"]);
        assert!(MergedLog::open(&t.0).is_err(), "一份不叫合并");
    }

    /// 末行没有换行、空文件：行号要和 `LineIndex` 一样数（不然「原文件第几行」对不上）
    #[test]
    fn 末行没换行和空文件() {
        let t = logs("edge", &["2026-08-24 10:00:00 INFO a\n2026-08-24 10:00:02 INFO tail-no-nl", "", "2026-08-24 10:00:01 INFO b\n"]);
        let m = open(&t);
        assert_eq!(texts(&m), ["2026-08-24 10:00:00 INFO a", "2026-08-24 10:00:01 INFO b", "2026-08-24 10:00:02 INFO tail-no-nl"]);
        let lone = LogFile::open(&t.0[0]).unwrap();
        lone.wait_indexed();
        assert_eq!(lone.stat().line_count, 2, "单文件也数 2 行");
    }
}
