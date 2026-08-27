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
//! 注意「原始」二字：这里**不做任何编码转换**。文件是什么字节就传什么字节，
//! 由前端拿探测出来的编码标签交给 `TextDecoder` 去解。
//!
//! 早先这里做过 `from_utf8_lossy`，结果是一份 GBK 日志在到达前端之前就被
//! 烙上了 U+FFFD，前端再按 GBK 解那串 U+FFFD 的 UTF-8 字节，屏幕上就是
//! 满篇「锟斤拷」。**转换必须发生在知道编码的那一层，而这里不知道。**

/// 单行返回上限。超长行（例如一整行 100MB 的 JSON）截断到此长度，
/// 否则一个块就能把 IPC 和渲染一起拖垮。
pub const MAX_LINE_BYTES: usize = 64 * 1024;

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
        let len = clipped.len() as u32;
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

#[cfg(test)]
mod tests {
    use super::*;

    /// 按线格式解回原始字节，供「字节必须原样透传」这类断言使用
    fn decode_raw(buf: &[u8]) -> (u64, Vec<Vec<u8>>) {
        let first = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
        let mut pos = 12 + count * 4;
        let mut lines = Vec::with_capacity(count);
        for i in 0..count {
            let len = u32::from_le_bytes(buf[12 + i * 4..16 + i * 4].try_into().unwrap()) as usize;
            lines.push(buf[pos..pos + len].to_vec());
            pos += len;
        }
        assert_eq!(pos, buf.len(), "payload 长度与长度表对不上");
        (first, lines)
    }

    /// 按线格式解回字符串（内容本来就是 UTF-8 时用）
    fn decode(buf: &[u8]) -> (u64, Vec<String>) {
        let first = u64::from_le_bytes(buf[0..8].try_into().unwrap());
        let count = u32::from_le_bytes(buf[8..12].try_into().unwrap()) as usize;
        let mut pos = 12 + count * 4;
        let mut lines = Vec::with_capacity(count);
        for i in 0..count {
            let len = u32::from_le_bytes(buf[12 + i * 4..16 + i * 4].try_into().unwrap()) as usize;
            lines.push(String::from_utf8(buf[pos..pos + len].to_vec()).unwrap());
            pos += len;
        }
        assert_eq!(pos, buf.len(), "payload 长度与长度表对不上");
        (first, lines)
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

    #[test]
    fn 超长行被截断() {
        let huge = vec![b'x'; MAX_LINE_BYTES * 2];
        let (_, lines) = decode(&encode(0, &[&huge]));
        assert_eq!(lines[0].len(), MAX_LINE_BYTES);
    }
}
