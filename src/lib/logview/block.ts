/**
 * 解码 Rust 侧的线格式二进制块。
 *
 * 布局（小端）：
 *   [u64 firstLine][u32 count][u32 len_0 .. len_{count-1}][payload UTF-8]
 *
 * 截过的行（Rust 侧 `MAX_LINE_BYTES`，64KB）：`len` 的最高位是 1，这段字节后面跟一个 `u64` = 这一行原本多少字节。
 * 读出来放进 `more`（还有多少字节没显示），界面在行尾说出来 —— 原来只截不说（logengine/src/block.rs 头上有来龙去脉）
 */

const CLIPPED = 0x8000_0000;

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
  /** 第 i 行还有多少字节没显示；一行都没截过时是 null（绝大多数块，省一个千元素的数组） */
  more: number[] | null;
}

/** 「还有多少没显示」的那句话。数字要真：KB 以下按字节说，往上一位小数 */
export function cutNote(bytes: number): string {
  // 中文单位后面直接接字，英文单位后面空一格（「500 字节没显示」「1.5 KB 没显示」）
  const size =
    bytes < 1024 ? `${bytes} 字节` : bytes < 1024 * 1024 ? `${(bytes / 1024).toFixed(1)} KB ` : `${(bytes / 1024 / 1024).toFixed(1)} MB `;
  // 说完怎么看：行详情（#64）取得到整行
  return `…这一行还有 ${size}没显示，双击看整行`;
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

  let more: number[] | null = null;

  for (let i = 0; i < count; i++) {
    const raw = view.getUint32(12 + i * 4, true);
    // `>>> 0`：位运算的结果是有符号 32 位，最高位一置上就成了负数
    const len = (raw & ~CLIPPED) >>> 0;
    lines[i] = decoder.decode(bytes.subarray(pos, pos + len));
    pos += len;
    if (raw & CLIPPED) {
      const full = Number(view.getBigUint64(pos, true));
      pos += 8;
      (more ??= new Array(count).fill(0))[i] = full - len;
    }
  }
  return { firstLine, lines, more };
}
