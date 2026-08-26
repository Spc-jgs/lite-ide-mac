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
pub mod index;

use memmap2::{Advice, Mmap, MmapOptions};
use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub use block::MAX_LINE_BYTES;
pub use index::{LineIndex, DEFAULT_STRIDE};

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
    size: u64,
    map: Arc<Mmap>,
    index: Arc<Mutex<Arc<LineIndex>>>,
    /// 后台索引是否已跑完（供前端停止轮询）
    complete: Arc<AtomicBool>,
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
}

impl LogFile {
    /// 打开文件并启动索引。此调用本身不读盘，立即返回。
    pub fn open(path: impl AsRef<Path>) -> io::Result<Self> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path)?;
        let size = file.metadata()?.len();

        // 空文件也要能开，mmap 空文件在部分平台会报错，单独兜住
        let map = if size == 0 {
            MmapOptions::new().len(0).map_anon()?.make_read_only()?
        } else {
            // SAFETY: 只读映射。外部进程截断文件可能导致 SIGBUS，
            // 由 M1 的 logrotate 检测（inode / size 变化）负责规避。
            let m = unsafe { MmapOptions::new().map(&file)? };
            // 索引是一次从头扫到尾的顺序访问。不给提示的话，1GB 要触发约 26 万次
            // minor page fault，实测吞吐被压到 0.77GB/s —— 远低于 memchr 本身的能力。
            let _ = m.advise(Advice::Sequential);
            m
        };
        let map = Arc::new(map);
        let complete = Arc::new(AtomicBool::new(false));

        // 首屏：同步扫前 1MB，让窗口立刻有内容
        let mut head = LineIndex::new(DEFAULT_STRIDE);
        let head_end = PRIME_BYTES.min(map.len());
        head.extend(&map[..head_end]);
        let fully_scanned = head_end == map.len();
        if fully_scanned {
            head.seal(&map);
        }
        let index = Arc::new(Mutex::new(Arc::new(head.clone())));

        if !fully_scanned {
            let (m, slot, done) = (map.clone(), index.clone(), complete.clone());
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
            size,
            map,
            index,
            complete,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn size(&self) -> u64 {
        self.size
    }

    /// 取一份索引快照。锁只在克隆 Arc 期间持有，与索引进度无关。
    #[inline]
    fn snapshot(&self) -> Arc<LineIndex> {
        Arc::clone(&self.index.lock().expect("index 锁被毒化"))
    }

    pub fn stat(&self) -> LogStat {
        let ix = self.snapshot();
        LogStat {
            line_count: ix.line_count(),
            indexed_bytes: ix.indexed_upto(),
            total_bytes: self.size,
            complete: self.complete.load(Ordering::Acquire),
            index_bytes: ix.memory_footprint() as u64,
        }
    }

    /// 阻塞直到后台索引跑完。仅供测试与 bench 使用。
    pub fn wait_indexed(&self) {
        while !self.complete.load(Ordering::Acquire) {
            std::thread::yield_now();
        }
    }

    /// 读取 `[start, start+count)` 行，返回线格式二进制块（见 `block` 模块）。
    ///
    /// 只在起点定位一次索引，之后顺序扫 —— 连续读 N 行的代价是 O(N) 而非 O(N·stride)。
    pub fn read_block(&self, start: u64, count: u32) -> Vec<u8> {
        let ix = self.snapshot();
        let data: &[u8] = &self.map;

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
