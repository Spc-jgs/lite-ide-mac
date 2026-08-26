/**
 * 解码 Rust 侧的线格式二进制块。
 *
 * 布局（小端）：
 *   [u64 firstLine][u32 count][u32 len_0 .. len_{count-1}][payload UTF-8]
 */

const decoder = new TextDecoder("utf-8");

export interface Block {
  firstLine: number;
  lines: string[];
}

export function decodeBlock(buf: ArrayBuffer): Block {
  const view = new DataView(buf);
  // 行号远小于 2^53，转 Number 安全
  const firstLine = Number(view.getBigUint64(0, true));
  const count = view.getUint32(8, true);

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
