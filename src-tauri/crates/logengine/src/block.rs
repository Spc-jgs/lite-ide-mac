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
//! payload 是 count 段 UTF-8 字节，按 len 依次切分，不含行尾换行符。

/// 单行返回上限。超长行（例如一整行 100MB 的 JSON）截断到此长度，
/// 否则一个块就能把 IPC 和渲染一起拖垮。
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// 把若干行原始字节编码成线格式。
///
/// 非 UTF-8 字节按 U+FFFD 替换 —— 日志文件本来就可能被截断在半个字符上，
/// 任何情况下都不该 panic。
pub fn encode(first_line: u64, raw_lines: &[&[u8]]) -> Vec<u8> {
    let count = raw_lines.len() as u32;
    let header = 8 + 4 + 4 * count as usize;
    // 预估：多数日志行在 200 字节上下
    let mut out = Vec::with_capacity(header + count as usize * 200);

    out.extend_from_slice(&first_line.to_le_bytes());
    out.extend_from_slice(&count.to_le_bytes());
    // 长度表先占位，payload 编完再回填（转换后的长度可能与原始不同）
    out.resize(header, 0);

    for (i, raw) in raw_lines.iter().enumerate() {
        let trimmed = trim_eol(raw);
        let clipped = &trimmed[..trimmed.len().min(MAX_LINE_BYTES)];
        let before = out.len();
        match std::str::from_utf8(clipped) {
            // 合法 UTF-8：直接拷贝，无分配
            Ok(s) => out.extend_from_slice(s.as_bytes()),
            // 非法字节：lossy 转换，U+FFFD 替换
            Err(_) => out.extend_from_slice(String::from_utf8_lossy(clipped).as_bytes()),
        }
        let len = (out.len() - before) as u32;
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

    /// 按线格式解回来，供断言使用
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

    #[test]
    fn 非法字节不_panic_且被替换() {
        let bad: &[u8] = &[b'a', 0xff, 0xfe, b'b', b'\n'];
        let (_, lines) = decode(&encode(0, &[bad]));
        assert!(lines[0].starts_with('a') && lines[0].ends_with('b'));
        assert!(lines[0].contains('\u{FFFD}'), "非法字节应替换为 U+FFFD");
    }

    #[test]
    fn 超长行被截断() {
        let huge = vec![b'x'; MAX_LINE_BYTES * 2];
        let (_, lines) = decode(&encode(0, &[&huge]));
        assert_eq!(lines[0].len(), MAX_LINE_BYTES);
    }
}
