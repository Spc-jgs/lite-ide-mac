//! 打开文件时的模式判定 —— 决定走编辑模式还是日志模式。
//!
//! 判据是复合的（ARCHITECTURE.md §1 修正 01）：
//!
//! ```text
//! size > 32MB  ||  估算行数 > 300k  ||  最长行 > 10k  →  日志模式（只读）
//! ```
//!
//! 为什么不能只看文件大小：CodeMirror 6 的瓶颈是**行数与单行长度**，不是字节数。
//! 一个 40MB 的单行 JSON 比 200MB 的多行日志更容易把它拖死 —— 前者是一个
//! 4000 万字符的文本节点，后者只是很多短行。

use memmap2::{Mmap, MmapOptions};
use std::fs::File;
use std::io;
use std::path::Path;

/// 超过这个大小直接判日志模式，不再采样
pub const MAX_EDIT_BYTES: u64 = 32 << 20; // 32MB
/// 估算行数超过这个值走日志模式
pub const MAX_EDIT_LINES: u64 = 300_000;
/// 任一行超过这个长度走日志模式
pub const MAX_EDIT_LINE_LEN: usize = 10_000;
/// 采样多少字节用于估算
const SAMPLE_BYTES: usize = 2 << 20; // 2MB

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// CodeMirror 6，可编辑
    Edit,
    /// 自研 mmap 引擎，只读
    Log,
}

impl Mode {
    pub fn as_str(self) -> &'static str {
        match self {
            Mode::Edit => "edit",
            Mode::Log => "log",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Probe {
    pub size: u64,
    pub mode: Mode,
    /// 估算总行数（采样外推；文件小于采样窗口时就是精确值）
    pub est_lines: u64,
    /// 采样窗口内见到的最长行
    pub max_line_len: usize,
    /// 含 NUL 字节 —— 二进制文件，不给编辑
    pub binary: bool,
    /// 判定依据，用于界面上说明「为什么这个文件是只读的」
    pub reason: &'static str,
}

/// 判定一个文件该用哪种模式打开。只读文件头部，不加载全文。
pub fn probe(path: impl AsRef<Path>) -> io::Result<Probe> {
    let file = File::open(path.as_ref())?;
    let size = file.metadata()?.len();

    if size == 0 {
        return Ok(Probe {
            size: 0,
            mode: Mode::Edit,
            est_lines: 0,
            max_line_len: 0,
            binary: false,
            reason: "空文件",
        });
    }

    // 超大文件不必采样，直接判定
    if size > MAX_EDIT_BYTES {
        return Ok(Probe {
            size,
            mode: Mode::Log,
            est_lines: 0,
            max_line_len: 0,
            binary: false,
            reason: "文件超过 32MB",
        });
    }

    // SAFETY: 只读映射，且上面已确认 size > 0
    let map: Mmap = unsafe { MmapOptions::new().map(&file)? };
    let sample_len = SAMPLE_BYTES.min(map.len());
    let sample = &map[..sample_len];

    // NUL 字节是二进制文件最可靠的信号
    let binary = memchr::memchr(0, sample).is_some();

    let mut lines: u64 = 0;
    let mut max_line_len = 0usize;
    let mut line_start = 0usize;
    for nl in memchr::memchr_iter(b'\n', sample) {
        max_line_len = max_line_len.max(nl - line_start);
        line_start = nl + 1;
        lines += 1;
    }
    // 采样窗口末尾那段残行也要算进最长行（可能整个采样窗口都没有换行）
    max_line_len = max_line_len.max(sample_len - line_start);

    // 按采样比例外推；文件本身没超过采样窗口时 lines 已是精确值
    let est_lines = if sample_len as u64 >= size {
        lines + u64::from(line_start < sample_len)
    } else {
        (lines as f64 * (size as f64 / sample_len as f64)).ceil() as u64
    };

    let (mode, reason) = if binary {
        (Mode::Log, "二进制文件")
    } else if est_lines > MAX_EDIT_LINES {
        (Mode::Log, "行数超过 30 万")
    } else if max_line_len > MAX_EDIT_LINE_LEN {
        (Mode::Log, "存在超过 1 万字符的长行")
    } else {
        (Mode::Edit, "")
    };

    Ok(Probe {
        size,
        mode,
        est_lines,
        max_line_len,
        binary,
        reason,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn temp(name: &str, body: &[u8]) -> std::path::PathBuf {
        let p = std::env::temp_dir().join(format!("probe-test-{name}"));
        let mut f = File::create(&p).unwrap();
        f.write_all(body).unwrap();
        f.sync_all().unwrap();
        p
    }

    #[test]
    fn 普通源码走编辑模式() {
        let p = temp("src", b"fn main() {\n    println!(\"hi\");\n}\n");
        let r = probe(&p).unwrap();
        assert_eq!(r.mode, Mode::Edit);
        assert_eq!(r.est_lines, 3);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 空文件可编辑() {
        let p = temp("empty", b"");
        assert_eq!(probe(&p).unwrap().mode, Mode::Edit);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 超长单行走日志模式_哪怕文件不大() {
        // 这正是「只看文件大小会误判」的场景：才 20KB，但一行装完
        let mut body = vec![b'x'; 20_000];
        body.push(b'\n');
        let p = temp("longline", &body);
        let r = probe(&p).unwrap();
        assert_eq!(r.mode, Mode::Log);
        assert_eq!(r.reason, "存在超过 1 万字符的长行");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 行数太多走日志模式() {
        let mut body = Vec::new();
        for n in 0..400_000 {
            body.extend_from_slice(format!("{n}\n").as_bytes());
        }
        let p = temp("manylines", &body);
        let r = probe(&p).unwrap();
        assert_eq!(r.mode, Mode::Log);
        assert_eq!(r.reason, "行数超过 30 万");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 二进制文件不给编辑() {
        let p = temp("bin", &[0x7f, b'E', b'L', b'F', 0x00, 0x01, 0x02, b'\n']);
        let r = probe(&p).unwrap();
        assert!(r.binary);
        assert_eq!(r.mode, Mode::Log);
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn 采样外推的行数估算大致准确() {
        // 造一个超过采样窗口（2MB）的文件，检查外推没有离谱
        let mut body = Vec::with_capacity(3 << 20);
        let mut n = 0u32;
        while body.len() < (3 << 20) {
            body.extend_from_slice(format!("line {n} some padding text here\n").as_bytes());
            n += 1;
        }
        let p = temp("estimate", &body);
        let r = probe(&p).unwrap();
        let err = (r.est_lines as f64 - n as f64).abs() / n as f64;
        assert!(
            err < 0.05,
            "估算 {} 实际 {}，误差 {:.1}%",
            r.est_lines,
            n,
            err * 100.0
        );
        std::fs::remove_file(p).ok();
    }
}
