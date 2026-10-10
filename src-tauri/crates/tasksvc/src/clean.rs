//! 任务输出写进日志之前洗一遍：剥掉终端控制序列、`\r\n` 收成 `\n`、单独的 `\r` 当换行。
//!
//! 第 0 步实测管道下 Spring Boot / vite 的颜色码都是 0（TASKS.md 第 10 节），这一层是**兜底**：
//! 总有工具不看是不是终端、强开颜色（`FORCE_COLOR`、`--color=always`），日志视图不认 ANSI，原样写进去就是满屏 `[32m`。
//!
//! 输出是一块一块读的，一个序列可能正好切在两块中间（`\x1b[3` | `2m`），所以它是个**带状态**的过滤器，
//! 状态跨块保留 —— 按块各洗各的，切口两边都会漏出半截。

#[derive(Clone, Copy, PartialEq, Debug)]
enum St {
    Text,
    /// 刚看到 ESC
    Esc,
    /// `ESC [` 之后，等终止字节（0x40–0x7E）
    Csi,
    /// `ESC ]` 之后（设标题、超链接），到 BEL 或 `ESC \` 为止
    Osc,
    /// OSC 里看到了 ESC，下一个是 `\` 就结束
    OscEsc,
}

pub struct Cleaner {
    st: St,
    /// 上一块以 `\r` 结尾：要看下一个字节才知道它是 `\r\n` 的前半，还是单独的 `\r`
    cr: bool,
}

impl Default for Cleaner {
    fn default() -> Self {
        Self::new()
    }
}

impl Cleaner {
    pub fn new() -> Self {
        Cleaner { st: St::Text, cr: false }
    }

    /// 洗一块，结果追加到 `out`
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            match self.st {
                St::Text => {
                    if self.cr {
                        self.cr = false;
                        // `\r\n` → `\n`；单独的 `\r`（进度条回到行首重画）→ 换一行，每次重画都留下来，比挤成一行好读
                        out.push(b'\n');
                        if b == b'\n' {
                            continue;
                        }
                    }
                    match b {
                        0x1b => self.st = St::Esc,
                        b'\r' => self.cr = true,
                        _ => out.push(b),
                    }
                }
                St::Esc => {
                    self.st = match b {
                        b'[' => St::Csi,
                        b']' => St::Osc,
                        // 其余是两字节的序列（`ESC 7`、`ESC =` …），吃掉这一个就回到正文
                        _ => St::Text,
                    }
                }
                St::Csi => {
                    if (0x40..=0x7e).contains(&b) {
                        self.st = St::Text;
                    }
                }
                St::Osc => match b {
                    0x07 => self.st = St::Text,
                    0x1b => self.st = St::OscEsc,
                    _ => {}
                },
                St::OscEsc => self.st = if b == b'\\' { St::Text } else { St::Osc },
            }
        }
    }

    /// 输出结束：攥着的那个 `\r` 落成换行
    pub fn finish(&mut self, out: &mut Vec<u8>) {
        if self.cr {
            self.cr = false;
            out.push(b'\n');
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(chunks: &[&[u8]]) -> String {
        let mut c = Cleaner::new();
        let mut out = Vec::new();
        for ch in chunks {
            c.feed(ch, &mut out);
        }
        c.finish(&mut out);
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn 颜色码剥掉_正文留着() {
        assert_eq!(all(&[b"\x1b[32m\x1b[1mVITE\x1b[22m v6\x1b[39m ok\n"]), "VITE v6 ok\n");
    }

    /// 这条测试存在的理由：按块各洗各的，切口两边会漏出 `[3` 和 `2m`
    #[test]
    fn 序列切在两块中间也剥得干净() {
        assert_eq!(all(&[b"a\x1b[3", b"2mb\x1b", b"[0mc\n"]), "abc\n");
    }

    #[test]
    fn 超链接和标题这类_osc_也剥() {
        assert_eq!(all(&[b"\x1b]0;title\x07x\x1b]8;;http://a\x1b\\link\x1b]8;;\x1b\\\n"]), "xlink\n");
    }

    #[test]
    fn crlf_收成一个换行_切在两块中间也一样() {
        assert_eq!(all(&[b"a\r\nb\r", b"\nc\n"]), "a\nb\nc\n");
    }

    #[test]
    fn 单独的回车当换行_进度条每次重画都留下() {
        assert_eq!(all(&[b"10%\r20%\r30%\n"]), "10%\n20%\n30%\n");
        assert_eq!(all(&[b"tail\r"]), "tail\n", "结尾攥着的回车在 finish 时落下");
    }

    #[test]
    fn 中文不受影响() {
        assert_eq!(all(&["\x1b[31m错误\x1b[0m：端口".as_bytes(), "被占\n".as_bytes()]), "错误：端口被占\n");
    }
}
