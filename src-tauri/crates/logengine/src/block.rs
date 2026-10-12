//! 传给前端的紧凑二进制块编码。
//!
//! 走 `tauri::ipc::Response` 直接落成 ArrayBuffer，绕开 JSON 序列化 ——
//! 1000 行的量级差异是 15ms 对 1ms，而 60fps 单帧预算只有 16ms
//! （见 docs/ARCHITECTURE.md §3.4）。
//!
//! 线格式（全部小端）：
//! ```text
//! [u64 first_line][u32 count][u32 len_0 .. len_{count-1}][payload]
//! ```
//! payload 是 count 段**原始字节**，按 len 依次切分，不含行尾换行符。
//!
//! **截过的行**（超过 [`MAX_LINE_BYTES`]）：`len` 的最高位置 1（[`CLIPPED`]，低 31 位照旧是这段的字节数），
//! 这段字节后面紧跟一个 `u64`：这一行**原本**有多少字节。界面拿它在行尾说「还有多少没显示」——
//! 原来只截不说，50MB 的单行 JSON 只显示前 64KB、一个字的提示都没有（2026-10-12 核「大文件能不能只读打开」时发现；
//! 修法照 Codex 截给模型的那套：截了要说、给真数字，docs/research/codex.md 主题 3）
//!
//! 注意「原始」二字：这里**不做任何编码转换**。文件是什么字节就传什么字节，
//! 由前端拿探测出来的编码标签交给 `TextDecoder` 去解。
//!
//! 早先这里做过 `from_utf8_lossy`，结果是一份 GBK 日志在到达前端之前就被
//! 烙上了 U+FFFD，前端再按 GBK 解那串 U+FFFD 的 UTF-8 字节，屏幕上就是
//! 满篇「锟斤拷」。**转换必须发生在知道编码的那一层，而这里不知道。**

/// 单行返回上限。超长行（例如一整行 100MB 的 JSON）截断到此长度，
/// 否则一个块就能把 IPC 和渲染一起拖垮。
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// 长度表里「这一行截过」的标记位。截过的段最长 [`MAX_LINE_BYTES`]，远用不到这一位
pub const CLIPPED: u32 = 1 << 31;

/// 把若干行原始字节编码成线格式。
///
/// 字节原样透传，不做编码转换（原因见模块注释）。前端的 `TextDecoder`
/// 默认就是非 fatal 的，遇到解不出的字节自己会替换成 U+FFFD ——
/// 该在哪一层做替换，就在哪一层做。
pub fn encode(first_line: u64, raw_lines: &[&[u8]]) -> Vec<u8> {
    let count = raw_lines.len() as u32;
    let header = 8 + 4 + 4 * count as usize;
    // 预估：多数日志行在 200 字节上下
    let mut out = Vec::with_capacity(header + count as usize * 200);

    out.extend_from_slice(&first_line.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    // 长度表先占位，payload 边写边回填
    out.resize(header, 0);

    for (i, raw) in raw_lines.iter().enumerate() {
        let trimmed = trim_eol(raw);
        let clipped = &trimmed[..trimmed.len().min(MAX_LINE_BYTES)];
        out.extend_from_slice(clipped);
        let mut len = clipped.len() as u32;
        if clipped.len() < trimmed.len() {
            len |= CLIPPED;
            out.extend_from_slice(&(trimmed.len() as u64).to_le_bytes());
        }
        let slot = 12 + i * 4;
        out[slot..slot + 4].copy_from_slice(&len.to_le_bytes());
    }
    out
}

/// 去掉行尾的 `\n` 与 `\r`（兼容 CRLF 的日志）。
#[inline]
fn trim_eol(line: &[u8]) -> &[u8] {
    let mut end = line.len();
    if end > 0 && line[end - 1] == b'\n' {
        end -= 1;
    }
    if end > 0 && line[end - 1] == b'\r' {
        end -= 1;
    }
    &line[..end]
}

/// 按线格式解回：原始字节 + 截过的行原本多长（没截是 None）。和前端 `block.ts` 的 `decodeBlock` 读法一致。
/// **测试里解线格式只用这一份**：lib.rs 的测试原来自己手写了一份，不认 [`CLIPPED`] —— 格式加了截断标记之后，
/// 谁在那边加一条长行的测试，长度带着最高位被当成 2GB、切片越界 panic，看着像引擎坏了（code review 2026-10-12）
#[cfg(test)]
pub(crate) fn decode_full(buf: &[u8]) -> (u64, Vec<(Vec<u8>, Option<u64>)>) {
    let first = u64::from_le_bytes(buf[0..8].try_into().unwrap());
    let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
    let mut pos = 12 + count * 4;
    let mut lines = Vec::with_capacity(count);
    for i in 0..count {
        let raw = u32::from_le_bytes(buf[12 + i * 4..16 + i * 4].try_into().unwrap());
        let len = (raw & !CLIPPED) as usize;
        let bytes = buf[pos..pos + len].to_vec();
        pos += len;
        let full = (raw & CLIPPED != 0).then(|| {
            let f = u64::from_le_bytes(buf[pos..pos + 8].try_into().unwrap());
            pos += 8;
            f
        });
        lines.push((bytes, full));
    }
    assert_eq!(pos, buf.len(), "payload 长度与长度表对不上");
    (first, lines)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 按线格式解回原始字节，供「字节必须原样透传」这类断言使用
    fn decode_raw(buf: &[u8]) -> (u64, Vec<Vec<u8>>) {
        let (first, lines) = decode_full(buf);
        (first, lines.into_iter().map(|(b, _)| b).collect())
    }

    /// 按线格式解回字符串（内容本来就是 UTF-8 时用）
    fn decode(buf: &[u8]) -> (u64, Vec<String>) {
        let (first, lines) = decode_raw(buf);
        (first, lines.into_iter().map(|b| String::from_utf8(b).unwrap()).collect())
    }

    #[test]
    fn 往返一致() {
        let raw: Vec<&[u8]> = vec![b"first", b"second\n", b"third\r\n"];
        let (first, lines) = decode(&encode(42, &raw));
        assert_eq!(first, 42);
        assert_eq!(lines, vec!["first", "second", "third"]);
    }

    #[test]
    fn 空块() {
        let (first, lines) = decode(&encode(0, &[]));
        assert_eq!(first, 0);
        assert!(lines.is_empty());
    }

    #[test]
    fn 中文按字节长度正确切分() {
        let raw: Vec<&[u8]> = vec!["订单超时\n".as_bytes(), "重试第 2 次".as_bytes()];
        let (_, lines) = decode(&encode(0, &raw));
        assert_eq!(lines, vec!["订单超时", "重试第 2 次"]);
    }

    /// 非 UTF-8 字节必须**原样**送到前端，不能在这里替换成 U+FFFD。
    ///
    /// 这条是冲着「GBK 日志满篇锟斤拷」那个 bug 去的：在这一层做 lossy 转换，
    /// 等于把前端按正确编码解码的可能性提前掐死。
    #[test]
    fn 非法utf8字节要原样透传而不是替换() {
        let bad: &[u8] = &[b'a', 0xff, 0xfe, b'b', b'\n'];
        let buf = encode(0, &[bad]);
        let (_, raws) = decode_raw(&buf);
        assert_eq!(raws[0], vec![b'a', 0xff, 0xfe, b'b'], "字节被改动了");
    }

    /// 一整行 GBK 中文的往返：字节数和内容都必须一字不差
    #[test]
    fn gbk字节往返不变() {
        // "订单失败" 的 GBK 编码
        let gbk: &[u8] = &[0xB6, 0xA9, 0xB5, 0xA5, 0xCA, 0xA7, 0xB0, 0xDC, b'\n'];
        let (_, raws) = decode_raw(&encode(7, &[gbk]));
        assert_eq!(raws[0], &gbk[..gbk.len() - 1], "GBK 字节在传输中被改了");
    }

    /// 超长行截到上限，**并且带上原本多长** —— 界面靠它说「还有多少没显示」。前后两行不受影响（标记位和尾巴的 8 字节没把切分弄乱）
    #[test]
    fn 超长行被截断_带着原长() {
        let huge = vec![b'x'; MAX_LINE_BYTES * 2 + 5];
        let mut with_eol = huge.clone();
        with_eol.extend_from_slice(b"\r\n");
        let (_, lines) = decode_full(&encode(0, &[b"before", &with_eol, b"after"]));
        assert_eq!(lines[0], (b"before".to_vec(), None));
        assert_eq!(lines[1].0.len(), MAX_LINE_BYTES);
        assert_eq!(lines[1].1, Some(huge.len() as u64), "原长要是去掉换行之后的整行长度");
        assert_eq!(lines[2], (b"after".to_vec(), None));
    }

    /// 正好到上限不算截过：差一个字节就会给一行完整的内容误报「还有 0 字节没显示」
    #[test]
    fn 正好到上限不算截过() {
        let exact = vec![b'y'; MAX_LINE_BYTES];
        let (_, lines) = decode_full(&encode(0, &[&exact]));
        assert_eq!(lines[0], (exact, None));
    }
}
