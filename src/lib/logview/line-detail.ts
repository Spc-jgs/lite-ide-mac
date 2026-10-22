/**
 * 日志的行详情（#64）：比窗口宽的长行，列表里只看得到开头（行高固定才有 60fps 的虚拟滚动，折行就不定高了）。
 * 双击一行（或回车）在底下展开整行：折行显示，行里的 JSON 排好版。这里是不碰界面的那半：解整行、找 JSON
 */

/** `log_line_full` 回的二进制：前 8 字节是这一行的真长度（小端），后面是原文（可能截过）。空的 = 没有这一行 */
export function decodeLineFull(buf: ArrayBuffer, encoding: string): { text: string; bytes: number; total: number } | null {
  if (buf.byteLength < 8) return null;
  const total = Number(new DataView(buf).getBigUint64(0, true));
  const body = new Uint8Array(buf, 8);
  let dec: TextDecoder;
  try {
    dec = new TextDecoder(encoding);
  } catch {
    dec = new TextDecoder("utf-8");
  }
  // 截过的那种按字节截、可能切在半个字符上：流式解码把末尾那半个留着不出（不出乱码），完整的那种照常
  const text = dec.decode(body, { stream: body.byteLength < total });
  return { text, bytes: body.byteLength, total };
}

/** 从 `s[i]`（`{` 或 `[`）开始，配对的那个括号在哪；字符串里的括号不算。配不上 → -1 */
function matchEnd(s: string, i: number): number {
  let depth = 0;
  let inStr = false;
  for (let j = i; j < s.length; j++) {
    const c = s[j];
    if (inStr) {
      if (c === "\\") j++;
      else if (c === '"') inStr = false;
    } else if (c === '"') inStr = true;
    else if (c === "{" || c === "[") depth++;
    else if (c === "}" || c === "]") {
      depth--;
      if (depth === 0) return j;
    }
  }
  return -1;
}

/** 最多试这么多个开括号：一行 1MB 全是括号的时候，每个都去配对、解析会卡住界面 */
const MAX_TRIES = 40;

/**
 * 行里最长的那段 JSON（对象或数组，至少两个字符以外有东西 —— `[]` `{}` `[INFO]` 这种不算）。
 * 日志里常见的是 `… payload={"a":1} cost=3ms`：前面的前缀、JSON、后面的尾巴分开给。没有 → null
 */
export function extractJson(s: string): { before: string; value: unknown; after: string } | null {
  let best: { from: number; to: number; value: unknown } | null = null;
  let tries = 0;
  for (let i = 0; i < s.length && tries < MAX_TRIES; i++) {
    const c = s[i];
    if (c !== "{" && c !== "[") continue;
    tries++;
    const end = matchEnd(s, i);
    if (end < 0) continue;
    if (best && end - i <= best.to - best.from) continue;
    try {
      const value = JSON.parse(s.slice(i, end + 1));
      // 只要「有结构的」：空对象、空数组、`[INFO]` 这种解不出来的不算；数组里得有对象或数组，`[1,2]` 这种排版没意义
      const rich = Array.isArray(value) ? value.some((v) => v !== null && typeof v === "object") : Object.keys(value as object).length > 0;
      if (!rich) continue;
      best = { from: i, to: end, value };
      // 整段都吃进去了的话，后面的开括号都在它里面，不用再试
      i = end;
    } catch {
      // 不是 JSON（`[main]`、`{}` 占位符）：接着找下一个
    }
  }
  return best ? { before: s.slice(0, best.from), value: best.value, after: s.slice(best.to + 1) } : null;
}

/** 字节数说成人话（和 block.ts 的「还有 49.9 MB 没显示」一个口径） */
export function sizeOf(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)} KB`;
  return `${(n / 1024 / 1024).toFixed(1)} MB`;
}
