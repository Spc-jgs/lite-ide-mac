/**
 * unified diff 解析。
 *
 * 为什么自己解析而不是上 CodeMirror 的 merge 插件：
 * merge 插件要再拉一个包、要两份全文、还得自己跑一遍 diff 算法。
 * 而 `git diff` 已经把结果算好了 —— 我们要做的只是把它**读懂并画好看**。
 * 这条路零新依赖、零算法风险，且 git 的 rename/binary/mode 判断天然继承下来。
 */

export type LineKind = "ctx" | "add" | "del" | "hunk" | "meta";

export interface DiffLine {
  kind: LineKind;
  text: string;
  /** 旧文件行号，新增行没有 */
  oldNo?: number;
  /** 新文件行号，删除行没有 */
  newNo?: number;
  /**
   * 行内改动区间 `[起, 止)`，按 code point 计。
   * 只有配对成功的增删行才有 —— 没有就整行平铺，不猜。
   */
  span?: [number, number];
}

export interface DiffFile {
  /** 新路径（删除的文件则是旧路径） */
  path: string;
  /** 改名时的旧路径 */
  oldPath?: string;
  binary: boolean;
  isNew: boolean;
  isDeleted: boolean;
  lines: DiffLine[];
  adds: number;
  dels: number;
}

/** 解析 `git diff` 的完整输出，可能含多个文件 */
export function parseDiff(raw: string): DiffFile[] {
  const files: DiffFile[] = [];
  if (!raw.trim()) return files;

  const all = raw.split("\n");

  let cur: DiffFile | null = null;
  let oldNo = 0;
  let newNo = 0;

  const push = (l: DiffLine) => cur?.lines.push(l);

  for (let li = 0; li < all.length; li++) {
    const line = all[li];
    if (line.startsWith("diff --git ")) {
      if (cur) files.push(finish(cur));
      cur = {
        path: pathFromHeader(line),
        binary: false,
        isNew: false,
        isDeleted: false,
        lines: [],
        adds: 0,
        dels: 0,
      };
      continue;
    }
    if (!cur) continue;

    if (line.startsWith("new file mode")) {
      cur.isNew = true;
      continue;
    }
    if (line.startsWith("deleted file mode")) {
      cur.isDeleted = true;
      continue;
    }
    if (line.startsWith("rename from ")) {
      cur.oldPath = line.slice(12);
      continue;
    }
    if (line.startsWith("rename to ")) {
      cur.path = line.slice(10);
      continue;
    }
    if (line.startsWith("Binary files") || line.startsWith("GIT binary patch")) {
      cur.binary = true;
      continue;
    }
    // 这几行是噪声，用户不看：index 哈希、--- / +++ 的路径重复
    if (
      line.startsWith("index ") ||
      line.startsWith("--- ") ||
      line.startsWith("+++ ") ||
      line.startsWith("old mode") ||
      line.startsWith("new mode") ||
      line.startsWith("similarity index")
    ) {
      continue;
    }

    if (line.startsWith("@@")) {
      const m = /^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@(.*)$/.exec(line);
      if (m) {
        oldNo = Number(m[1]);
        newNo = Number(m[2]);
        // @@ 后面那截是 git 猜的所属函数名，很有用，保留
        push({ kind: "hunk", text: m[3].trim() });
      } else {
        push({ kind: "hunk", text: "" });
      }
      continue;
    }

    const c = line[0];
    if (c === "+") {
      push({ kind: "add", text: line.slice(1), newNo: newNo++ });
      cur.adds++;
    } else if (c === "-") {
      push({ kind: "del", text: line.slice(1), oldNo: oldNo++ });
      cur.dels++;
    } else if (c === "\\") {
      // "\ No newline at end of file" —— 是真实信息，但不占行号
      push({ kind: "meta", text: line.slice(2) });
    } else if (c === " ") {
      push({ kind: "ctx", text: line.slice(1), oldNo: oldNo++, newNo: newNo++ });
    } else if (line === "" && li < all.length - 1) {
      // git 自己输出的空上下文行是**一个空格**，不是空串。
      // 真正的空串只可能是尾随换行被 split 出来的那一个 —— 它不是内容行，
      // 当成上下文会白吃掉一个行号，后面所有行号全错一位。
      // 但有些工具（编辑器、粘贴板）会把行尾空格削掉，那种 diff 里空串
      // 确实代表空上下文行，所以只把**最后一个**空串当作产物排除。
      push({ kind: "ctx", text: "", oldNo: oldNo++, newNo: newNo++ });
    }
  }
  if (cur) files.push(finish(cur));
  return files;
}

/** `diff --git a/x b/x` → `x`。路径可能带引号（含特殊字符时 git 会转义） */
function pathFromHeader(line: string): string {
  const rest = line.slice(11);
  // 常见情形：两段等长，中点就是分界。带空格的路径靠 " b/" 定位更稳
  const at = rest.lastIndexOf(" b/");
  const b = at >= 0 ? rest.slice(at + 3) : rest;
  return unquote(b);
}

function unquote(s: string): string {
  if (!s.startsWith('"') || !s.endsWith('"')) return s;
  try {
    return JSON.parse(s);
  } catch {
    return s.slice(1, -1);
  }
}

/** 收尾：跑一遍行内差异配对 */
function finish(f: DiffFile): DiffFile {
  pairInline(f.lines);
  return f;
}

/**
 * 行内改动高亮。
 *
 * 只处理最常见也最有价值的一种情形：**一段删除后面紧跟等长的一段新增**。
 * 这就是「改了这几行」。1:1 配对之后，砍掉公共前缀和公共后缀，
 * 中间剩下的那一小截才是真正改的东西。
 *
 * 长度不等就不猜 —— 猜错的高亮比没有高亮更误导。
 */
function pairInline(lines: DiffLine[]) {
  let i = 0;
  while (i < lines.length) {
    if (lines[i].kind !== "del") {
      i++;
      continue;
    }
    let d = i;
    while (d < lines.length && lines[d].kind === "del") d++;
    let a = d;
    while (a < lines.length && lines[a].kind === "add") a++;
    const dels = d - i;
    const adds = a - d;
    if (dels > 0 && dels === adds) {
      for (let k = 0; k < dels; k++) {
        markSpan(lines[i + k], lines[d + k]);
      }
    }
    i = a > i ? a : i + 1;
  }
}

function markSpan(del: DiffLine, add: DiffLine) {
  const o = [...del.text];
  const n = [...add.text];
  let p = 0;
  while (p < o.length && p < n.length && o[p] === n[p]) p++;
  let s = 0;
  while (s < o.length - p && s < n.length - p && o[o.length - 1 - s] === n[n.length - 1 - s]) s++;
  // 整行都变了就别标了，标了等于没标，还多一层视觉噪声
  const oLen = o.length - p - s;
  const nLen = n.length - p - s;
  if (p === 0 && s === 0) return;
  if (oLen > 0) del.span = expand(o, p, p + oLen);
  if (nLen > 0) add.span = expand(n, p, p + nLen);
}

/**
 * 把区间向外扩到词边界。
 *
 * 前后缀法在有重复字符时会切在词中间：`300` → `5000` 的**最小**编辑是
 * 把 `3` 换成 `50`（公共后缀 `00;` 被保留）。结果没错，但人读起来是错的 ——
 * 谁都会说「这里从 300 改成了 5000」。所以只要区间的端点落在一个词里，
 * 就把它扩到整个词。
 */
function expand(cs: string[], a: number, b: number): [number, number] {
  const word = (c: string | undefined) => !!c && /[\w$\u4e00-\u9fff]/.test(c);
  let s = a;
  let e = b;
  if (word(cs[s])) while (s > 0 && word(cs[s - 1])) s--;
  if (word(cs[e - 1])) while (e < cs.length && word(cs[e])) e++;
  return [s, e];
}

/** 一行按 span 切成三段，供模板直接渲染 */
export function segs(l: DiffLine): [string, string, string] {
  if (!l.span) return [l.text, "", ""];
  const cs = [...l.text];
  const [a, b] = l.span;
  return [cs.slice(0, a).join(""), cs.slice(a, b).join(""), cs.slice(b).join("")];
}

// ─────────────────────── 双栏对照 ───────────────────────

/**
 * 双栏对照里的一行。左右两侧各自可能为空 —— 纯新增的行左边没有对应，
 * 纯删除的行右边没有对应，用 `null` 表示那一格是空的（画成灰底占位）。
 */
export interface SideRow {
  kind: "ctx" | "change" | "hunk" | "meta";
  left: DiffLine | null;
  right: DiffLine | null;
  /** hunk / meta 行的文本 */
  text?: string;
}

/**
 * 把 unified diff 摊成左右两栏。
 *
 * 规则很简单，麻烦的是**对齐**：
 * - 上下文行两边都放，天然对齐
 * - 一段删除后面跟一段新增，是「改了这几行」—— 按下标 1:1 摆，
 *   多出来的那侧另起一行、对面留空
 * - 只有删除或只有新增，对面整段留空
 *
 * 为什么不按行内容做相似度匹配再对齐：那需要一遍 LCS，而 git 已经把
 * 增删分好组了，按顺序摆就已经是人读起来最顺的形态。多一层猜测反而会
 * 在重构类改动上把顺序打乱。
 */
export function toSideBySide(lines: DiffLine[]): SideRow[] {
  const out: SideRow[] = [];
  let i = 0;
  while (i < lines.length) {
    const l = lines[i];
    if (l.kind === "hunk" || l.kind === "meta") {
      out.push({ kind: l.kind, left: null, right: null, text: l.text });
      i++;
      continue;
    }
    if (l.kind === "ctx") {
      out.push({ kind: "ctx", left: l, right: l });
      i++;
      continue;
    }
    // 收一段连续的删除 + 紧随其后的一段连续新增
    const dels: DiffLine[] = [];
    while (i < lines.length && lines[i].kind === "del") dels.push(lines[i++]);
    const adds: DiffLine[] = [];
    while (i < lines.length && lines[i].kind === "add") adds.push(lines[i++]);
    const n = Math.max(dels.length, adds.length);
    for (let k = 0; k < n; k++) {
      out.push({ kind: "change", left: dels[k] ?? null, right: adds[k] ?? null });
    }
  }
  return out;
}

/**
 * 变更块的起止行下标，供「上一处 / 下一处改动」跳转用。
 * 连续的变更行算一块 —— 跳转应该以「一处改动」为单位，不是一行一跳。
 */
export function changeBlocks(rows: { kind: string }[]): number[] {
  const starts: number[] = [];
  for (let i = 0; i < rows.length; i++) {
    const isChange = rows[i].kind === "change" || rows[i].kind === "add" || rows[i].kind === "del";
    const prevChange =
      i > 0 && (rows[i - 1].kind === "change" || rows[i - 1].kind === "add" || rows[i - 1].kind === "del");
    if (isChange && !prevChange) starts.push(i);
  }
  return starts;
}

/**
 * 连续多少行空白之后，把斜纹底换成纯色。
 *
 * 一两行斜纹是「对面本来就没东西」的提示，正合适。但一口气新增五行 import 时，
 * 对面那一整块连续斜纹的视觉重量会盖过旁边真正的代码 ——
 * 人先看到的是纹理，不是绿色的新增行。IDEA 在这种位置用的是一层很淡的纯色。
 */
export const FLAT_BLANK_RUN = 3;

/**
 * 标出「哪些行落在一段够长的空白里」，左右各一份。
 *
 * 放在这里而不是组件里，是为了能单测 —— 这是纯数据变换，
 * 而「第 N 行到底该不该换成纯色」正是最容易在改动里悄悄错掉的那种细节。
 */
export function blankRuns(rows: SideRow[]): { left: boolean[]; right: boolean[] } {
  const mark = (pick: (r: SideRow) => DiffLine | null) => {
    const out = new Array<boolean>(rows.length).fill(false);
    let run = 0;
    // 多跑一格（i === rows.length）好让结尾那一段也被结算
    for (let i = 0; i <= rows.length; i++) {
      const r = i < rows.length ? rows[i] : null;
      // hunk / meta 是跨四列的整行，不参与空白块 —— 它天然把上下两段隔开
      if (r && r.kind !== "hunk" && r.kind !== "meta" && !pick(r)) {
        run++;
        continue;
      }
      if (run > FLAT_BLANK_RUN) for (let k = i - run; k < i; k++) out[k] = true;
      run = 0;
    }
    return out;
  };
  return { left: mark((r) => r.left), right: mark((r) => r.right) };
}

// ─────────────────── 改动行标记 ───────────────────

export type ChangeKind = "add" | "mod" | "del";

/**
 * 从 unified diff 里提取「新文件的第几行被改动了」，供编辑器缩略图 / 行标做标记。
 *
 * 三种情形分开：
 * - 一段删除紧跟一段新增 → 这几行是**改的**（mod）
 * - 只有新增 → **加的**（add）
 * - 只有删除 → 新文件里没有对应的行，标在**缺口处**（del）。
 *   标在缺口的下一行是有意的：用户看到的是「这里少了点东西」，
 *   而缺口下面那一行就是缺口所在的位置。
 */
export function changedLines(raw: string): Map<number, ChangeKind> {
  const out = new Map<number, ChangeKind>();
  for (const f of parseDiff(raw)) {
    if (f.binary) continue;
    const lines = f.lines;
    let i = 0;
    /** 已经走到新文件的第几行 —— 纯删除时靠它定位缺口 */
    let lastNew = 0;
    while (i < lines.length) {
      const k = lines[i].kind;
      if (k === "ctx") {
        lastNew = lines[i].newNo ?? lastNew;
        i++;
        continue;
      }
      if (k !== "del" && k !== "add") {
        i++;
        continue;
      }
      const dels: DiffLine[] = [];
      while (i < lines.length && lines[i].kind === "del") dels.push(lines[i++]);
      const adds: DiffLine[] = [];
      while (i < lines.length && lines[i].kind === "add") adds.push(lines[i++]);

      if (adds.length > 0) {
        const kind: ChangeKind = dels.length > 0 ? "mod" : "add";
        for (const a of adds) {
          if (a.newNo !== undefined) {
            out.set(a.newNo, kind);
            lastNew = a.newNo;
          }
        }
      } else if (dels.length > 0) {
        // 纯删除：标在缺口下面那一行；缺口在文件开头时标第 1 行
        out.set(Math.max(1, lastNew + 1), "del");
      }
    }
  }
  return out;
}
