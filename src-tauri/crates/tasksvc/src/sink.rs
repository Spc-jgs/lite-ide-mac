//! 任务输出落盘：一个任务一个文件，**到上限就轮转**。
//!
//! 输出没有上限（一个开着几天的 dev server 能吐几个 GB），rust.md 那条「起子进程先问输出有没有上限，没有就设闸」——
//! 优先在换行处轮转；单行/跨块输出到线时明确标记分段，不能一直等换行而越写越大。
//! 盘上最多 `2 × cap`。日志视图本来就认轮转（`logengine::LogFile::refresh` 看 inode 换没换，smoke ⑱）。
//!
//! 每次跑之前，上一次的输出挪成 `.1` —— 停完想回头看上一次为什么挂，还在。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const SPLIT: &[u8] = "\n[lite-ide] 日志分段\n".as_bytes();
const CONTINUED: &[u8] = "[lite-ide] 上行续接\n".as_bytes();
const PENDING_CAP: usize = 64 * 1024;

pub struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
    cap: u64,
    /// 程序写过的最后一个字节是换行（容量到线时允许带提示地分段）
    at_line_start: bool,
    /// 别人要插进来的整行（[`Sink::note`]），程序正写到一行中间时先攒着，等它这一行写完再插
    pending: Vec<u8>,
    finished: bool,
}

pub fn rotated(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

impl Sink {
    /// 开一份新的。已经有上一次的就挪成 `.1`（再上一次的 `.1` 被覆盖：只留一份）
    #[allow(clippy::disallowed_methods)] // 自己的任务输出：上一次的挪成 .1、新建一份
    pub fn open(path: &Path, cap: u64) -> io::Result<Sink> {
        if cap < (SPLIT.len() + CONTINUED.len() + 5) as u64 {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "日志容量不足以容纳分段提示"));
        }
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if fs::symlink_metadata(path).is_ok() {
            fs::rename(path, rotated(path))?;
        }
        // 上一行刚把同名的挪走了，这里是新文件；`create` 的截断语义在这儿正是要的
        let file = File::create(path)?;
        Ok(Sink { path: path.to_path_buf(), file, written: 0, cap, at_line_start: true, pending: Vec::new(), finished: false })
    }

    /// 写一块（已经洗过的）。**每块写完就落到文件里**（不经缓冲）：看日志的那一边要跟得上。
    ///
    /// 轮转放在下一段写入前；最后一段始终留在当前文件，跟随视图不会停在空文件上。
    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        // 攒着的插话：这一块里程序那一行写完了（有换行），就插在换行后面
        if !self.pending.is_empty() {
            if let Some(i) = data.iter().position(|&b| b == b'\n') {
                self.write_raw(&data[..=i])?;
                let p = std::mem::take(&mut self.pending);
                self.write_raw(&p)?;
                return self.write_raw(&data[i + 1..]);
            }
        }
        self.write_raw(data)
    }

    /// 插一整行进来（调试器的日志断点打的那句，docs/TAKEOVER.md G14）。程序正写到一行中间时不插 ——
    /// 插进去就把程序的一行劈成两半、日志视图按时间戳归类时认错；攒到它这一行写完。`line` 不带换行
    pub fn note(&mut self, line: &str) -> io::Result<()> {
        if self.finished { return Ok(()); }
        let line = line.trim_end_matches(['\r', '\n']);
        let mut end = line.len().min(PENDING_CAP - 128);
        while !line.is_char_boundary(end) { end -= 1; }
        let mut l = line.as_bytes()[..end].to_vec();
        l.push(b'\n');
        if end < line.len() {
            l.extend_from_slice(format!("[lite-ide] 日志断点过长，省略 {} 字节\n", line.len() - end).as_bytes());
        }
        if self.pending.len() + l.len() > PENDING_CAP {
            self.flush_pending()?;
        }
        if self.at_line_start {
            self.write_raw(&l)
        } else {
            self.pending.extend_from_slice(&l);
            Ok(())
        }
    }

    fn write_raw(&mut self, mut data: &[u8]) -> io::Result<()> {
        // 留出提示的空间：原行没有换行，也不能把文件上限变成建议值。
        let payload_cap = self.cap - SPLIT.len() as u64 - 4;
        while !data.is_empty() {
            let room = payload_cap.saturating_sub(self.written) as usize;
            if data.len() <= room {
                return self.piece(data);
            }
            // 一块里有很多行时一起写；不把每行变成一次系统调用。
            if let Some(i) = data[..room.min(data.len())].iter().rposition(|&b| b == b'\n') {
                self.piece(&data[..=i])?;
                data = &data[i + 1..];
            } else if self.at_line_start && self.written > 0 {
                self.rotate()?;
            } else {
                // 给跨块 UTF-8 字符最多补三个续字节；提示不能插到一个字的中间。
                let mut cut = room;
                let extra = self.cap.saturating_sub(self.written + SPLIT.len() as u64 + room as u64).min(3) as usize;
                while cut < data.len() && cut < room + extra && data[cut] & 0xc0 == 0x80 { cut += 1; }
                self.piece(&data[..cut])?;
                // 补完一个字恰好吃完本块，就等下一块再转，不能让当前文件只剩续接提示。
                if cut == data.len() { return Ok(()); }
                self.piece(SPLIT)?;
                self.rotate()?;
                self.piece(CONTINUED)?;
                // 提示结束了，但程序的那一行还没结束；note 继续等程序的换行。
                self.at_line_start = false;
                data = &data[cut..];
            }
        }
        Ok(())
    }

    fn piece(&mut self, data: &[u8]) -> io::Result<()> {
        self.file.write_all(data)?;
        self.written += data.len() as u64;
        if let Some(&last) = data.last() {
            self.at_line_start = last == b'\n';
        }
        Ok(())
    }

    /// 输出到 EOF 时也要把排队的日志断点写出来；不能因为程序没补换行就丢掉。
    pub fn finish(&mut self) -> io::Result<()> {
        self.finished = true;
        self.flush_pending()
    }

    fn flush_pending(&mut self) -> io::Result<()> {
        if !self.pending.is_empty() {
            self.write_raw("\n[lite-ide] 日志断点：原输出已分段\n".as_bytes())?;
            let pending = std::mem::take(&mut self.pending);
            self.write_raw(&pending)?;
        }
        Ok(())
    }

    #[allow(clippy::disallowed_methods)] // 自己的任务输出轮转
    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        fs::rename(&self.path, rotated(&self.path))?;
        self.file = File::create(&self.path)?;
        self.written = 0;
        crate::prune::rotated(&self.path);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tasksvc-sink-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        d.join("t.log")
    }

    #[test]
    fn 插一行_程序写到一行中间时等它写完再插() {
        let p = tmp("note");
        let mut s = Sink::open(&p, 1 << 20).unwrap();
        s.note("[断点] 一").unwrap();
        s.write(b"abc").unwrap();
        s.note("[断点] 二").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "[断点] 一\nabc", "写到一行中间：先不插");
        s.write(b"def\nghi\n").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "[断点] 一\nabcdef\n[断点] 二\nghi\n", "程序那一行没被劈开，插话紧跟在它后面");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn 没有换行_单块和跨块输出都不能越过硬上限() {
        for chunks in [1, 16] {
            let p = tmp(&format!("hard-{chunks}"));
            let mut s = Sink::open(&p, 64).unwrap();
            for _ in 0..chunks {
                s.write(&vec![b'x'; 4096 / chunks]).unwrap();
                for f in [&p, &rotated(&p)] {
                    assert!(fs::metadata(f).map(|m| m.len() <= 64).unwrap_or(true));
                }
            }
            let current = fs::read_to_string(&p).unwrap();
            assert!(current.contains("上行续接"), "不能把一截输出装成完整的一行");
            assert!(!current.is_empty(), "最后的输出不能只留在 .1");
            let _ = fs::remove_dir_all(p.parent().unwrap());
        }
    }

    #[test]
    fn 无效编码的续字节也不能穿透容量上限() {
        let p = tmp("invalid-utf8");
        let mut s = Sink::open(&p, 64).unwrap();
        for _ in 0..4096 {
            s.write(&[0x80]).unwrap();
            for f in [&p, &rotated(&p)] {
                assert!(fs::metadata(f).map(|m| m.len() <= 64).unwrap_or(true));
            }
        }
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn 单块很多普通行_按完整行轮转_当前文件留着最后一行() {
        let p = tmp("lines");
        let mut s = Sink::open(&p, 128).unwrap();
        s.write(&b"ordinary line\n".repeat(1000)).unwrap();
        assert!(fs::read_to_string(&p).unwrap().ends_with("ordinary line\n"));
        for f in [&p, &rotated(&p)] {
            let v = fs::read_to_string(f).unwrap();
            assert!(v.len() <= 128);
            assert!(v.lines().all(|l| l == "ordinary line"));
        }
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn 程序一直不换行_插话内存有界_结束时不丢最后的插话() {
        let p = tmp("pending");
        let mut s = Sink::open(&p, 1 << 20).unwrap();
        s.write(b"partial output").unwrap();
        for _ in 0..200 {
            s.note(&"注".repeat(1000)).unwrap();
            assert!(s.pending.len() <= PENDING_CAP);
        }
        s.note("最后一条").unwrap();
        s.flush_pending().unwrap();
        assert!(fs::read_to_string(&p).unwrap().contains("最后一条"));
        assert!(s.pending.is_empty());
        s.note(&"字".repeat(PENDING_CAP)).unwrap();
        let text = fs::read_to_string(&p).unwrap();
        assert!(text.contains("省略 131202 字节"), "截掉的字节数要准确");
        s.finish().unwrap();
        let len = fs::metadata(&p).unwrap().len();
        s.note("EOF 后不再写入已释放保护的文件").unwrap();
        assert_eq!(fs::metadata(&p).unwrap().len(), len);
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn 容量分段不能切开中文字符_跨输入块也一样() {
        let p = tmp("utf8");
        let mut s = Sink::open(&p, 128).unwrap();
        let data = "长".repeat(200).into_bytes();
        for part in data.chunks(7) {
            s.write(part).unwrap();
            for f in [&p, &rotated(&p)] {
                if let Ok(v) = fs::read(f) {
                    // 当前输入本身可能停在半个字，只有轮转出去的文件必须是完整 UTF-8。
                    if f != &p { assert!(String::from_utf8(v).is_ok()); }
                }
            }
        }
        assert!(fs::read_to_string(&p).unwrap().ends_with('长'));
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }
}
