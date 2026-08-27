/**
 * 解码 Rust 侧的线格式二进制块。
 *
 * 布局（小端）：
 *   [u64 firstLine][u32 count][u32 len_0 .. len_{count-1}][payload UTF-8]
 */

/**
 * 解码器按编码标签缓存。
 *
 * 日志模式不在 Rust 侧解码，而是把探测到的编码标签交给 WebView 自己的
 * TextDecoder —— WebKit 原生就带 GBK / Big5 / Shift_JIS 这些表，
 * 让 Rust 再解一遍等于把整块字节多搬一次，而这条路径是 60fps 的滚动主路径。
 *
 * 缓存是因为 TextDecoder 的构造对 CJK 编码不便宜，而滚动时每块都要用。
 */
const decoders = new Map<string, TextDecoder>();

function decoderFor(label: string): TextDecoder {
  const key = label || "utf-8";
  let d = decoders.get(key);
  if (!d) {
    try {
      d = new TextDecoder(key);
    } catch {
      // 标签不认识就退回 UTF-8，绝不让一个坏标签把整个日志视图打空
      d = new TextDecoder("utf-8");
    }
    decoders.set(key, d);
  }
  return d;
}

export interface Block {
  firstLine: number;
  lines: string[];
}

export function decodeBlock(buf: ArrayBuffer, encoding = "utf-8"): Block {
  const view = new DataView(buf);
  // 行号远小于 2^53，转 Number 安全
  const firstLine = Number(view.getBigUint64(0, true));
  const count = view.getUint32(8, true);

  const decoder = decoderFor(encoding);
  const lines: string[] = new Array(count);
  const bytes = new Uint8Array(buf);
  let pos = 12 + count * 4;

  for (let i = 0; i < count; i++) {
    const len = view.getUint32(12 + i * 4, true);
    lines[i] = decoder.decode(bytes.subarray(pos, pos + len));
    pos += len;
  }
  return { firstLine, lines };
}
