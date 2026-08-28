//! GB 级日志引擎：mmap + 稀疏行索引 + 二进制块读取。
//!
//! 刻意不依赖 Tauri —— 这样能 `cargo test -p logengine` 直接压 1GB 文件，
//! 不必启动整个应用（docs/ARCHITECTURE.md §2）。
//!
//! 打开流程：
//! 1. `mmap` 是 O(1) 的，不读盘 —— 窗口立刻能出来
//! 2. 同步扫前 1MB，首屏马上可渲染
//! 3. 余下部分后台线程分 16MB 块扫，每块结束释放锁，前端全程不卡

pub mod block;
pub mod filter;
pub mod index;
pub mod level;
pub mod probe;

use memmap2::{Advice, Mmap, MmapOptions};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

pub use block::MAX_LINE_BYTES;
pub use filter::{FilterSpec, FilterTask};
pub use index::{LineIndex, DEFAULT_STRIDE};
pub use level::{Level, LevelMap, LevelMask, LevelStats};
pub use probe::{probe, Mode, Probe};

/// 同步预扫的字节数：够填满首屏，又不会让打开变慢。
const PRIME_BYTES: usize = 1 << 20; // 1MB

/// 后台索引每次推进的块大小。锁只在单块期间持有，约 3ms。
const CHUNK_BYTES: usize = 16 << 20; // 16MB

/// 一次打开的日志文件。
///
/// 索引用「后台无锁构建 + 快照发布」而非共享可变结构：后台线程在自己的
/// `LineIndex` 上扫描，每块结束才把一份快照塞进 `Mutex<Arc<..>>`。
/// 读者只需克隆 Arc（O(1)）就能脱离锁去查 —— 这是首屏不被索引线程饿死的关键。
///
/// 早先用 `RwLock<LineIndex>` 让后台线程直接持写锁分块推进，实测首屏读取被拖到
/// 1112ms（约等于全量索引耗时）：写者放锁后立刻重新申请，读者根本插不进去。
pub struct LogFile {
    path: PathBuf,
    /// 打开时的 inode，用来识别 logrotate（文件被换掉而非追加）
    inode: u64,
    /// 可替换 —— tail 追加后文件变长，需要重新映射（mmap 长度创建时即固定）。
    /// 旧映射由 Arc 持有到无人使用为止，绝不能直接 drop：
    /// 正在渲染的行还指着那段内存，拔掉就是段错误。
    map: Arc<Mutex<Arc<Mmap>>>,
    size: Arc<AtomicU64>,
    index: Arc<Mutex<Arc<LineIndex>>>,
    /// 后台索引是否已跑完（供前端停止轮询）
    complete: Arc<AtomicBool>,
    /// 每行级别 + 统计，由第二个后台任务填充
    levels: Arc<Mutex<Arc<LevelMap>>>,
    levels_done: Arc<AtomicBool>,
    /// 级别扫描进度（已扫描字节），只用于进度显示，故用原子量而非快照
    levels_progress: Arc<AtomicU64>,
}

/// `refresh` 的结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refreshed {
    /// 文件没变
    NoChange,
    /// 文件变长，新增了若干行
    Grew { new_lines: u64 },
    /// 文件被截断或轮转（logrotate）——调用方应重新打开
    Rotated,
}

/// 索引进度快照，喂给前端更新滚动条与行号。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LogStat {
    /// 当前已确认的行数（索引未跑完时会持续增长）
    pub line_count: u64,
    /// 已索引的字节数
    pub indexed_bytes: u64,
    /// 文件总字节数
    pub total_bytes: u64,
    /// 索引是否已完成
    pub complete: bool,
    /// 索引结构自身占多少字节 —— 用来验证内存承诺
    pub index_bytes: u64,
    /// 各级别行数，供过滤 chips 显示
    pub levels: LevelStats,
    /// 级别扫描是否已完成（未完成时 levels 全为 0）
    pub levels_complete: bool,
    /// 级别扫描已处理的字节数
    pub levels_scanned: u64,
}

impl LogFile {
    /// 打开文件并启动索引。此调用本身不读盘，立即返回。
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let meta = file.metadata()?;
        let size = meta.len();
        let inode = {
            use std::os::unix::fs::MetadataExt;
            meta.ino()
        };

        // 空文件也要能开，mmap 空文件在部分平台会报错，单独兜住
        let map = if size == 0 {
            MmapOptions::new().len(0).map_anon()?.make_read_only()?
        } else {
            /*
             * SAFETY: 只读映射。
             *
             * **风险如实说**：外部进程在我们读某一页的**当口**把文件截断，
             * 会触发 SIGBUS —— 整个进程直接死，连崩溃屏都来不及画。
             *
             * `refresh()` 的 inode / size 检测**规避不了**这一条：它是 500ms
             * 一次的轮询，只能在事后发现「文件被换掉了」，拦不住扫描线程正踩着
             * 的那一页。真要堵死得装 SIGBUS handler + siglongjmp，
             * 对一个个人工具不值当。
             *
             * 接受它的理由：less / lnav / glogg 全都是这个模型。日志被
             * logrotate 换掉是常态（那是改名 + 新建，老 inode 还在，读得下去），
             * 而**原地截断**（`> app.log`）才是危险动作，本来就少见。
             */
            let m = unsafe { MmapOptions::new().map(&file)? };
            // 索引是一次从头扫到尾的顺序访问。不给提示的话，1GB 要触发约 26 万次
            // minor page fault，实测吞吐被压到 0.77GB/s —— 远低于 memchr 本身的能力。
            let _ = m.advise(Advice::Sequential);
            m
        };
        let map_arc = Arc::new(map);
        let size = Arc::new(AtomicU64::new(size));
        let complete = Arc::new(AtomicBool::new(false));

        // 首屏：同步扫前 1MB，让窗口立刻有内容
        let mut head = LineIndex::new(DEFAULT_STRIDE);
        let head_end = PRIME_BYTES.min(map_arc.len());
        head.extend(&map_arc[..head_end]);
        let fully_scanned = head_end == map_arc.len();
        if fully_scanned {
            head.seal(&map_arc);
        }
        let index = Arc::new(Mutex::new(Arc::new(head.clone())));
        let levels = Arc::new(Mutex::new(Arc::new(LevelMap::default())));
        let levels_done = Arc::new(AtomicBool::new(false));
        let levels_progress = Arc::new(AtomicU64::new(0));

        // 第二个后台任务：逐行探测级别。
        // 与索引分开跑是因为级别探测要看行首内容，塞进索引会把它拖慢 6 倍。
        {
            let (m, slot, done, prog) = (
                Arc::clone(&map_arc),
                levels.clone(),
                levels_done.clone(),
                levels_progress.clone(),
            );
            std::thread::Builder::new()
                .name("logengine-levels".into())
                .spawn(move || {
                    // 日志行平均百来字节，按 100 估容量，宁可略大也不反复扩容
                    let mut local = LevelMap::with_capacity((m.len() / 100).max(16));
                    let mut pos = 0usize;
                    for nl in memchr::memchr_iter(b'\n', &m) {
                        local.push(level::detect(&m[pos..nl]));
                        pos = nl + 1;
                        // 只报进度，不发布快照 —— LevelMap 有 MB 级，
                        // 中途反复克隆的拷贝量比扫描本身还贵
                        if local.lines().is_multiple_of(1 << 20) {
                            prog.store(pos as u64, Ordering::Relaxed);
                        }
                    }
                    if pos < m.len() {
                        local.push(level::detect(&m[pos..]));
                    }
                    local.shrink();
                    prog.store(m.len() as u64, Ordering::Relaxed);
                    *slot.lock().expect("levels 锁被毒化") = Arc::new(local);
                    done.store(true, Ordering::Release);
                })?;
        }

        if !fully_scanned {
            let (m, slot, done) = (Arc::clone(&map_arc), index.clone(), complete.clone());
            std::thread::Builder::new()
                .name("logengine-index".into())
                .spawn(move || {
                    // 本地推进，全程不持锁
                    let mut local = head;
                    let total = m.len();
                    let mut upto = PRIME_BYTES;
                    while upto < total {
                        let end = (upto + CHUNK_BYTES).min(total);
                        local.extend(&m[..end]);
                        upto = end;
                        // 发布快照：锁只在这一次 memcpy 期间持有
                        *slot.lock().expect("index 锁被毒化") = Arc::new(local.clone());
                    }
                    local.seal(&m);
                    *slot.lock().expect("index 锁被毒化") = Arc::new(local);
                    done.store(true, Ordering::Release);
                })?;
        } else {
            complete.store(true, Ordering::Release);
        }

        Ok(Self {
            path,
            inode,
            size,
            map: Arc::new(Mutex::new(map_arc)),
            index,
            complete,
            levels,
            levels_done,
            levels_progress,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size.load(Ordering::Acquire)
    }

    /// 取一份当前映射。锁只在克隆 Arc 期间持有；tail 换映射时不影响已取走的引用。
    #[inline]
    fn map(&self) -> Arc<Mmap> {
        Arc::clone(&self.map.lock().expect("map 锁被毒化"))
    }

    /// 取一份索引快照。锁只在克隆 Arc 期间持有，与索引进度无关。
    #[inline]
    fn snapshot(&self) -> Arc<LineIndex> {
        Arc::clone(&self.index.lock().expect("index 锁被毒化"))
    }

    /// 取一份级别表快照。与索引快照同理，锁只在克隆 Arc 期间持有。
    #[inline]
    pub fn levels(&self) -> Arc<LevelMap> {
        Arc::clone(&self.levels.lock().expect("levels 锁被毒化"))
    }

    pub fn stat(&self) -> LogStat {
        let ix = self.snapshot();
        let lv = self.levels();
        LogStat {
            line_count: ix.line_count(),
            indexed_bytes: ix.indexed_upto(),
            total_bytes: self.size(),
            complete: self.complete.load(Ordering::Acquire),
            index_bytes: ix.memory_footprint() as u64,
            levels: lv.stats(),
            levels_complete: self.levels_done.load(Ordering::Acquire),
            levels_scanned: self.levels_progress.load(Ordering::Relaxed),
        }
    }

    /// 阻塞直到后台索引跑完。仅供测试与 bench 使用。
    pub fn wait_indexed(&self) {
        while !self.complete.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }

    /// 阻塞直到级别扫描跑完。仅供测试与 bench 使用。
    pub fn wait_levels(&self) {
        while !self.levels_done.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }

    /// 按给定行号列表读取（行号须递增）——过滤视图的回表路径。
    ///
    /// 命中行号通常稀疏散布，逐行走索引定位；相邻行号靠得近时顺序推进，
    /// 省去重复的 checkpoint 查找。
    pub fn read_block_at(&self, line_nos: &[u64]) -> Vec<u8> {
        let ix = self.snapshot();
        let map = self.map();
        let data: &[u8] = &map;
        let first = line_nos.first().copied().unwrap_or(0);

        let mut lines: Vec<&[u8]> = Vec::with_capacity(line_nos.len());
        // 游标：已定位到的行号与其起始偏移
        let mut cur_line = u64::MAX;
        let mut cur_pos = 0usize;

        for &n in line_nos {
            // 目标就在游标前方不远处时顺序推进，比重查索引便宜
            let pos = if cur_line != u64::MAX && n >= cur_line && n - cur_line < 64 {
                let mut p = cur_pos;
                for _ in 0..(n - cur_line) {
                    match memchr::memchr(b'\n', &data[p..]) {
                        Some(nl) => p = p + nl + 1,
                        None => break,
                    }
                }
                p
            } else {
                match ix.offset_of_line(data, n) {
                    Some(o) => o as usize,
                    None => continue,
                }
            };
            let end = match memchr::memchr(b'\n', &data[pos..]) {
                Some(nl) => pos + nl,
                None => data.len(),
            };
            lines.push(&data[pos..end]);
            cur_line = n;
            cur_pos = pos;
        }
        block::encode(first, &lines)
    }

    /// 检查文件是否有追加，有则重新映射并增量索引（tail 模式）。
    ///
    /// mmap 的长度在创建时就固定了，文件追加之后映射不会自动变长 ——
    /// 这是 tail 最容易踩的坑，必须重新映射。旧映射交给 Arc 自然析构，
    /// 正在渲染的行还指着它。
    pub fn refresh(&self) -> io::Result<Refreshed> {
        use std::os::unix::fs::MetadataExt;

        let meta = std::fs::metadata(&self.path)?;
        let new_size = meta.len();
        let old_size = self.size();

        // 变短或 inode 变了 = 被截断或轮转，索引整个失效
        if new_size < old_size || meta.ino() != self.inode {
            return Ok(Refreshed::Rotated);
        }
        if new_size == old_size {
            return Ok(Refreshed::NoChange);
        }

        let file = File::open(&self.path)?;
        // SAFETY: 只读映射；轮转已在上面拦截
        let fresh = unsafe { MmapOptions::new().map(&file)? };
        let _ = fresh.advise(Advice::Sequential);
        let fresh = Arc::new(fresh);

        // 增量索引：先撤销 seal，末尾那半行要重新算
        let mut ix = (*self.snapshot()).clone();
        let before = ix.line_count();
        ix.unseal();
        ix.extend(&fresh);
        ix.seal(&fresh);
        let after = ix.line_count();

        // 增量级别扫描 —— 仅在首轮全量扫描已完成时做，
        // 否则会与后台任务重复推进同一段
        if self.levels_done.load(Ordering::Acquire) {
            let mut lv = (*self.levels()).clone();
            if let Some(off) = ix.offset_of_line(&fresh, lv.lines()) {
                let mut pos = off as usize;
                while pos < fresh.len() {
                    let end = match memchr::memchr(b'\n', &fresh[pos..]) {
                        Some(nl) => pos + nl,
                        None => fresh.len(),
                    };
                    lv.push(level::detect(&fresh[pos..end]));
                    pos = end + 1;
                }
            }
            *self.levels.lock().expect("levels 锁被毒化") = Arc::new(lv);
        }

        *self.map.lock().expect("map 锁被毒化") = fresh;
        *self.index.lock().expect("index 锁被毒化") = Arc::new(ix);
        self.size.store(new_size, Ordering::Release);

        Ok(Refreshed::Grew {
            new_lines: after.saturating_sub(before),
        })
    }

    /// 启动一次后台过滤。调用方负责在换条件时取消旧任务。
    pub fn start_filter(&self, spec: filter::FilterSpec) -> io::Result<Arc<filter::FilterTask>> {
        filter::spawn(self.map(), self.snapshot(), self.levels(), spec)
    }

    /// 读取 `[start, start+count)` 行，返回线格式二进制块（见 `block` 模块）。
    ///
    /// 只在起点定位一次索引，之后顺序扫 —— 连续读 N 行的代价是 O(N) 而非 O(N·stride)。
    pub fn read_block(&self, start: u64, count: u32) -> Vec<u8> {
        let ix = self.snapshot();
        let map = self.map();
        let data: &[u8] = &map;

        let Some(begin) = ix.offset_of_line(data, start) else {
            return block::encode(start, &[]);
        };
        let available = (ix.line_count() - start).min(count as u64) as usize;

        let mut lines: Vec<&[u8]> = Vec::with_capacity(available);
        let mut pos = begin as usize;
        for _ in 0..available {
            match memchr::memchr(b'\n', &data[pos..]) {
                Some(nl) => {
                    lines.push(&data[pos..pos + nl]);
                    pos += nl + 1;
                }
                // 末行没有换行符结尾
                None => {
                    lines.push(&data[pos..]);
                    break;
                }
            }
        }
        block::encode(start, &lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp_log(name: &str, body: &[u8]) -> PathBuf {
        let p = std::env::temp_dir().join(format!("logengine-test-{name}"));
        let mut f = File::create(&p).unwrap();
        f.write_all(body).unwrap();
        f.sync_all().unwrap();
        p
    }

    fn decode(buf: &[u8]) -> Vec<String> {
        let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
        let mut pos = 12 + count * 4;
        (0..count)
            .map(|i| {
                let len =
                    u32::from_le_bytes(buf[12 + i * 4..16 + i * 4].try_into().unwrap()) as usize;
                let s = String::from_utf8(buf[pos..pos + len].to_vec()).unwrap();
                pos += len;
                s
            })
            .collect()
    }

    #[test]
    fn 小文件读取() {
        let p = temp_log("small", b"alpha\nbeta\ngamma\n");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        assert_eq!(f.stat().line_count, 3);
        assert_eq!(decode(&f.read_block(0, 10)), vec!["alpha", "beta", "gamma"]);
        assert_eq!(decode(&f.read_block(1, 1)), vec!["beta"]);
        assert!(decode(&f.read_block(99, 5)).is_empty(), "越界应返回空块");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 空文件不崩() {
        let p = temp_log("empty", b"");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        assert_eq!(f.stat().line_count, 0);
        assert!(decode(&f.read_block(0, 10)).is_empty());
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 按行号列表回表读取() {
        let p = temp_log("at", b"l0\nl1\nl2\nl3\nl4\nl5\n");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        assert_eq!(decode(&f.read_block_at(&[0, 2, 4])), vec!["l0", "l2", "l4"]);
        // 相邻行号走顺序推进这条路径
        assert_eq!(decode(&f.read_block_at(&[1, 2, 3])), vec!["l1", "l2", "l3"]);
        assert_eq!(decode(&f.read_block_at(&[5])), vec!["l5"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn tail_追加后能读到新行() {
        use std::io::Write as _;
        let p = temp_log("tail", b"a\nb\n");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        f.wait_levels();
        assert_eq!(f.stat().line_count, 2);
        assert_eq!(f.refresh().unwrap(), Refreshed::NoChange);

        let mut fh = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        fh.write_all(b"c\nd\n").unwrap();
        fh.sync_all().unwrap();

        assert_eq!(f.refresh().unwrap(), Refreshed::Grew { new_lines: 2 });
        assert_eq!(f.stat().line_count, 4);
        assert_eq!(decode(&f.read_block(0, 10)), vec!["a", "b", "c", "d"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn tail_续写末尾半行不会重复计数() {
        use std::io::Write as _;
        // 末行没有 \n，seal 时已按一行计入
        let p = temp_log("tail-partial", b"a\nbb");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        f.wait_levels();
        assert_eq!(f.stat().line_count, 2);

        // 追加把这半行补完，再加一行
        let mut fh = std::fs::OpenOptions::new().append(true).open(&p).unwrap();
        fh.write_all(b"bb\ncc\n").unwrap();
        fh.sync_all().unwrap();

        f.refresh().unwrap();
        assert_eq!(f.stat().line_count, 3, "半行被续写，不该多算一行");
        assert_eq!(decode(&f.read_block(0, 10)), vec!["a", "bbbb", "cc"]);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 文件被截断时报告轮转() {
        let p = temp_log("rotate", b"a\nb\nc\n");
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        std::fs::write(&p, b"x\n").unwrap();
        assert_eq!(f.refresh().unwrap(), Refreshed::Rotated);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 级别统计跟着文件走() {
        let body = b"2026-01-01 INFO  a\n2026-01-01 ERROR b\n2026-01-01 INFO  c\n";
        let p = temp_log("levels", body);
        let f = LogFile::open(&p).unwrap();
        f.wait_levels();
        let st = f.stat();
        assert!(st.levels_complete);
        assert_eq!(st.levels.get(Level::Info), 2);
        assert_eq!(st.levels.get(Level::Error), 1);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 后台索引跨块推进后行数正确() {
        // 造一个超过 PRIME_BYTES 的文件，强制走后台索引路径
        let mut body = Vec::with_capacity(2 << 20);
        let mut n = 0u32;
        while body.len() < (2 << 20) {
            body.extend_from_slice(format!("2026-08-26 INFO  line {n}\n").as_bytes());
            n += 1;
        }
        let p = temp_log("bg-index", &body);
        let f = LogFile::open(&p).unwrap();
        f.wait_indexed();
        assert_eq!(f.stat().line_count, n as u64);
        // 抽查首行、中间行、末行
        assert_eq!(decode(&f.read_block(0, 1))[0], "2026-08-26 INFO  line 0");
        let mid = n as u64 / 2;
        assert_eq!(
            decode(&f.read_block(mid, 1))[0],
            format!("2026-08-26 INFO  line {mid}")
        );
        assert_eq!(
            decode(&f.read_block(n as u64 - 1, 1))[0],
            format!("2026-08-26 INFO  line {}", n - 1)
        );
        std::fs::remove_file(p).ok();
    }
}
