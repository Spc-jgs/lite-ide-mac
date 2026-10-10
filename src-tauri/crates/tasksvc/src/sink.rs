//! 任务输出落盘：一个任务一个文件，**到上限就轮转**。
//!
//! 输出没有上限（一个开着几天的 dev server 能吐几个 GB），rust.md 那条「起子进程先问输出有没有上限，没有就设闸」——
//! 这里的闸不是截断（截断等于让日志骗人），是轮转：当前文件写到 `cap` 就在下一个换行处改名成 `.1`、另开一份。
//! 盘上最多 `2 × cap`。日志视图本来就认轮转（`logengine::LogFile::refresh` 看 inode 换没换，smoke ⑱）。
//!
//! 每次跑之前，上一次的输出挪成 `.1` —— 停完想回头看上一次为什么挂，还在。

use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

pub struct Sink {
    path: PathBuf,
    file: File,
    written: u64,
    cap: u64,
    /// 写过的最后一个字节是换行（轮转只在行首做）
    at_line_start: bool,
}

pub fn rotated(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(".1");
    PathBuf::from(s)
}

impl Sink {
    /// 开一份新的。已经有上一次的就挪成 `.1`（再上一次的 `.1` 被覆盖：只留一份）
    pub fn open(path: &Path, cap: u64) -> io::Result<Sink> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        if fs::symlink_metadata(path).is_ok() {
            fs::rename(path, rotated(path))?;
        }
        // 上一行刚把同名的挪走了，这里是新文件；`create` 的截断语义在这儿正是要的
        let file = File::create(path)?;
        Ok(Sink { path: path.to_path_buf(), file, written: 0, cap: cap.max(1), at_line_start: true })
    }

    /// 写一块（已经洗过的）。**每块写完就落到文件里**（不经缓冲）：看日志的那一边要跟得上。
    ///
    /// 轮转放在**写之前**判：已经过线、又正好在行首，先换一份新的再写这一块。反过来（写完这块发现过线、在末尾切开再换）
    /// 第一版就是那么写的 —— 一块就把全部输出读完的时候，整块进了 `.1`，当前那份是空的，日志视图跟着一个空文件，人什么都看不见。
    pub fn write(&mut self, data: &[u8]) -> io::Result<()> {
        let mut data = data;
        if self.written >= self.cap && !data.is_empty() {
            if self.at_line_start {
                self.rotate()?;
            } else if let Some(i) = data.iter().position(|&b| b == b'\n') {
                // 过线时停在一行中间：先把这一行写完再换，不把一行劈成两半
                self.file.write_all(&data[..=i])?;
                self.rotate()?;
                data = &data[i + 1..];
            }
            // 这一块里一个换行都没有（超长的一行还没完）：先写着，等它的换行
        }
        self.file.write_all(data)?;
        self.written += data.len() as u64;
        if let Some(&last) = data.last() {
            self.at_line_start = last == b'\n';
        }
        Ok(())
    }

    fn rotate(&mut self) -> io::Result<()> {
        self.file.flush()?;
        fs::rename(&self.path, rotated(&self.path))?;
        self.file = File::create(&self.path)?;
        self.written = 0;
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

    /// 第一版的形状：一块就跨过上限时「写完这块、在末尾切开再换」，整块进了 `.1`，当前那份是空的 ——
    /// 日志视图跟着一个空文件。集成测试里要看最后一块碰不碰巧跨线，这里确定地造出来
    #[test]
    fn 一块就跨过上限_这块留在当前那份里() {
        let p = tmp("cross");
        let mut s = Sink::open(&p, 10).unwrap();
        s.write(b"0123456789abc\n").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "0123456789abc\n", "当前那份被轮转空了");
        // 下一块才换：已经过线、又在行首
        s.write(b"next\n").unwrap();
        assert_eq!(fs::read_to_string(&p).unwrap(), "next\n");
        assert_eq!(fs::read_to_string(rotated(&p)).unwrap(), "0123456789abc\n");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }

    #[test]
    fn 过线时停在一行中间_写完这行再换() {
        let p = tmp("mid");
        let mut s = Sink::open(&p, 5).unwrap();
        s.write(b"abcdefg").unwrap();
        s.write(b"hi\nnew\n").unwrap();
        assert_eq!(fs::read_to_string(rotated(&p)).unwrap(), "abcdefghi\n", "一行被劈开了");
        assert_eq!(fs::read_to_string(&p).unwrap(), "new\n");
        let _ = fs::remove_dir_all(p.parent().unwrap());
    }
}
