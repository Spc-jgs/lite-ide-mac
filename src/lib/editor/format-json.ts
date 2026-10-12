/**
 * ⌥⌘L：格式化 JSON（IDEA「重新格式化代码」的键位；先只做 JSON —— 2026-10-12 走查：随手粘一段接口返回、日志里抠出来的
 * 单行 JSON 是草稿里最常见的东西，粘进来还是一长行，Sublime 用户在这一步几乎都装了 Pretty JSON）。
 *
 * # 只重排空白，值一个字节都不动
 *
 * 顺手的写法 `JSON.stringify(JSON.parse(s), null, 2)` 会**悄悄改数据**，两处：
 * - **大整数丢精度**：JS 的数字是双精度，超过 2^53 的整数被改掉 —— `12345678901234567890` → `12345678901234567000`。
 *   订单号、雪花 ID 正好是这么长，格式化完数字变了还不报错；
 * - **键被重排**：像整数的键排到最前（`{"b":1,"2":0}` → `{"2":0,"b":1}`）。
 * 所以这里按字符流走：字符串、数字、字面量原样照抄，只在括号、逗号、冒号前后换行缩进。`JSON.parse` 只用来判合不合法。
 *
 * # 格式化哪一段
 *
 * 有选区：选中的那几段。没有选区，按这个次序找第一个是 JSON 的：整份文件（`.json`）→ 光标所在的 ```` ```json ```` 围栏
 * （粘贴时自动包的就是这种，见 `paste-fence.ts`）→ 光标这一行从第一个 `{` / `[` 起到行尾（「接口返回：{…}」这种笔记）。
 * 都不是就说清楚、什么都不改 —— 不猜（尾逗号、单引号那种「像 JSON」的不认，同 `paste-fence.ts` 的判据）。
 */
import type { EditorState, StateCommand, TransactionSpec } from "@codemirror/state";

const WS = new Set([" ", "\t", "\n", "\r"]);
const DELIM = new Set([",", ":", "{", "}", "[", "]", '"']);

/** 合法 JSON 才返回 null，否则返回原因（给人看的） */
export function jsonError(src: string): string | null {
  try {
    JSON.parse(src);
    return null;
  } catch (e) {
    return e instanceof Error ? e.message : String(e);
  }
}

/**
 * 把一段**已知合法**的 JSON 重排成每层缩进 `unit`，第二行起再加一层 `base`（片段在一行中间时，后面的行跟着这一行的缩进走）。
 * 空的 `{}` `[]` 不展开
 */
export function prettyJson(src: string, unit = "  ", base = ""): string {
  let out = "";
  let depth = 0;
  let i = 0;
  const n = src.length;
  const nl = () => "\n" + base + unit.repeat(depth);
  while (i < n) {
    const c = src[i];
    if (WS.has(c)) {
      i++;
    } else if (c === '"') {
      // 字符串原样照抄：认转义，`\"` 不是结尾
      let j = i + 1;
      while (j < n && src[j] !== '"') j += src[j] === "\\" ? 2 : 1;
      out += src.slice(i, j + 1);
      i = j + 1;
    } else if (c === "{" || c === "[") {
      const close = c === "{" ? "}" : "]";
      let k = i + 1;
      while (k < n && WS.has(src[k])) k++;
      if (src[k] === close) {
        out += c + close;
        i = k + 1;
      } else {
        depth++;
        out += c + nl();
        i++;
      }
    } else if (c === "}" || c === "]") {
      depth--;
      out += nl() + c;
      i++;
    } else if (c === ",") {
      out += "," + nl();
      i++;
    } else if (c === ":") {
      out += ": ";
      i++;
    } else {
      // 数字、true / false / null：一直抄到下一个分隔符，一个字符都不改（大整数原样留着）
      let j = i;
      while (j < n && !WS.has(src[j]) && !DELIM.has(src[j])) j++;
      out += src.slice(i, j);
      i = j;
    }
  }
  return out;
}

/** 对象或数组（一个数字、一个字符串也是合法 JSON，但没什么可格式化的） */
function isJsonBlock(s: string): boolean {
  const t = s.trim();
  return (t.startsWith("{") || t.startsWith("[")) && jsonError(t) === null;
}

/** 去掉两头空白之后的区间 */
function trimmed(doc: string, from: number, to: number): { from: number; to: number } {
  while (from < to && WS.has(doc[from])) from++;
  while (to > from && WS.has(doc[to - 1])) to--;
  return { from, to };
}

export type JsonSpan = { from: number; to: number };

/** 没有选区时，光标 `pos` 处该格式化哪一段；找不到返回 null */
export function jsonSpanAt(doc: string, pos: number): JsonSpan | null {
  // 整份
  if (isJsonBlock(doc)) return trimmed(doc, 0, doc.length);
  // 光标所在的 ```json 围栏
  const lines = doc.split("\n");
  let off = 0;
  let open: number | null = null; // 围栏正文的起点
  for (const line of lines) {
    const end = off + line.length;
    const fence = /^\s*```/.test(line);
    if (open === null && fence && /^\s*```\s*json\s*$/i.test(line)) {
      open = end + 1;
    } else if (open !== null && fence) {
      if (pos >= open - 1 && pos <= end) {
        const body = doc.slice(open, Math.max(open, off - 1));
        if (isJsonBlock(body)) return trimmed(doc, open, Math.max(open, off - 1));
        return null;
      }
      open = null;
    }
    off = end + 1;
  }
  // 光标这一行：从第一个 { / [ 起
  const ls = doc.lastIndexOf("\n", pos - 1) + 1;
  const le = doc.indexOf("\n", pos);
  const line = doc.slice(ls, le < 0 ? doc.length : le);
  const at = line.search(/[{[]/);
  if (at >= 0 && isJsonBlock(line.slice(at))) return trimmed(doc, ls + at, ls + line.length);
  return null;
}

/** 一段所在那一行的行首缩进：格式化出来的第二行起跟着它走 */
function baseIndent(doc: string, from: number): string {
  const ls = doc.lastIndexOf("\n", from - 1) + 1;
  return /^[ \t]*/.exec(doc.slice(ls, from))?.[0] ?? "";
}

/**
 * 算出要做的改动（纯函数，测试直接调）。`unit` 是这个文件的缩进单位。
 * 返回 `{ spec }` = 要套用的事务；`{ same: true }` = 已经是这个格式；`{ error }` = 给人看的原因
 */
export function formatJsonChanges(
  state: EditorState,
  unit: string,
): { spec: TransactionSpec } | { same: true } | { error: string } {
  const doc = state.doc.toString();
  const sel = state.selection.ranges.filter((r) => !r.empty);
  let spans: JsonSpan[];
  if (sel.length > 0) {
    spans = [];
    for (const r of sel) {
      const span = trimmed(doc, r.from, r.to);
      const err = jsonError(doc.slice(span.from, span.to));
      if (err !== null) return { error: `选中的不是合法的 JSON：${err}` };
      spans.push(span);
    }
  } else {
    const span = jsonSpanAt(doc, state.selection.main.head);
    if (!span) return { error: "光标这里没有 JSON —— 选中那一段再按 ⌥⌘L（只认合法的 JSON 对象 / 数组）" };
    spans = [span];
  }
  const changes = spans
    .map((s) => ({ from: s.from, to: s.to, insert: prettyJson(doc.slice(s.from, s.to), unit, baseIndent(doc, s.from)) }))
    .filter((c) => c.insert !== doc.slice(c.from, c.to));
  if (changes.length === 0) return { same: true };
  return { spec: { changes, scrollIntoView: true, userEvent: "input.format" } };
}

/** CM6 命令。`unit` 每次现取（状态栏能改缩进）；结果怎么告诉人由调用方定（编辑器模块不碰通知） */
export function formatJson(unitOf: (s: EditorState) => string, say: (ok: boolean, msg: string) => void): StateCommand {
  return ({ state, dispatch }) => {
    const r = formatJsonChanges(state, unitOf(state));
    if ("error" in r) say(false, r.error);
    else if ("same" in r) say(true, "JSON 已经是这个格式了");
    else dispatch(state.update(r.spec));
    return true;
  };
}
