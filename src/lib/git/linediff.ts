import type { ChangeKind } from "./diff";

/**
 * 在前端算「编辑器里这份文本相对 HEAD 那份，哪些行动过」（issue #33 ④）。
 *
 * 以前的标记来自 `git diff`，反映的是**磁盘上那份**：打字不动，保存了才跟着变。
 * IDEA / VS Code 都是实时的 —— 它们把 HEAD 版本拿到前端，每次改动都重新比一遍。
 * 这里也这么做：`head` 打开文件时取一次（`git show HEAD:path`），`live` 是编辑器里
 * 此刻的文本，比的结果和 `changedLines`（从 unified diff 提取）**同一套标记语义**：
 * 改的 mod、加的 add、纯删除标在缺口下面那一行 del。
 *
 * # 算法
 *
 * 按行做 Myers 最短编辑脚本（O((N+M)·D)，D 是差异行数）。先掐掉两头相同的行 ——
 * 编辑时绝大多数改动集中在一处，掐完中间那段常常只有几行，Myers 在上面几乎不花时间。
 *
 * # 上限
 *
 * 两条：行数（两边加起来）和 D。超了返回 `null`，调用方按「不标」处理。
 * 不是为了性能预算好看 —— Myers 的内存是 O(D²)：D = 10000 时要存 1 亿个整数。
 * 一份几万行的文件被整个重写（换行符风格一换就是这样）正好撞上这个，
 * 而这种文件上「哪行改了」本来也没有信息量：全改了。
 */
export const MAX_LINES = 60_000;
export const MAX_D = 2_000;

export function diffLines(head: string, live: string): Map<number, ChangeKind> | null {
  const a = head.split("\n");
  const b = live.split("\n");
  if (a.length + b.length > MAX_LINES) return null;

  // 掐两头。后缀不能越过前缀 —— 两边长度不同时，先掐掉的前缀那段不能再被后缀数一次
  let p = 0;
  const maxP = Math.min(a.length, b.length);
  while (p < maxP && a[p] === b[p]) p++;
  let q = 0;
  const maxQ = maxP - p;
  while (q < maxQ && a[a.length - 1 - q] === b[b.length - 1 - q]) q++;

  const n = a.length - p - q;
  const m = b.length - p - q;
  const out = new Map<number, ChangeKind>();
  if (n === 0 && m === 0) return out;

  const ops = myers(a, b, p, n, m);
  if (!ops) return null;

  /*
   * 把编辑脚本折成标记，规则同 `changedLines`：
   * 一段删除紧跟一段新增 → mod；只新增 → add；只删除 → 标缺口下面那一行。
   */
  let newNo = p; // 已经走到 live 的第几行（1-based 里的「上一行」）
  let i = 0;
  while (i < ops.length) {
    if (ops[i] === EQ) {
      newNo++;
      i++;
      continue;
    }
    let dels = 0;
    while (i < ops.length && ops[i] === DEL) {
      dels++;
      i++;
    }
    let adds = 0;
    while (i < ops.length && ops[i] === ADD) {
      adds++;
      i++;
    }
    if (adds > 0) {
      const kind: ChangeKind = dels > 0 ? "mod" : "add";
      for (let k = 0; k < adds; k++) out.set(++newNo, kind);
    } else {
      out.set(Math.max(1, newNo + 1), "del");
    }
  }
  return out;
}

const EQ = 0, DEL = 1, ADD = 2;

/**
 * 标准 Myers：正向找最短路径，每一层 D 存一份 V，回溯得到编辑脚本。
 * `a[p..p+n)` 对 `b[p..p+m)`。超过 `MAX_D` 给 null。
 *
 * 这里没有用 linear-space 的分治版：那版代码量三倍，而 D 已经被上限压住了。
 */
function myers(a: string[], b: string[], p: number, n: number, m: number): number[] | null {
  const max = Math.min(n + m, MAX_D);
  const off = max;
  const trace: Int32Array[] = [];
  let v = new Int32Array(2 * max + 2);
  v[off + 1] = 0;
  for (let d = 0; d <= max; d++) {
    const snap = new Int32Array(v);
    trace.push(snap);
    for (let k = -d; k <= d; k += 2) {
      let x: number;
      if (k === -d || (k !== d && snap[off + k - 1] < snap[off + k + 1])) {
        x = snap[off + k + 1]; // 往下：b 多一行（add）
      } else {
        x = snap[off + k - 1] + 1; // 往右：a 多一行（del）
      }
      let y = x - k;
      while (x < n && y < m && a[p + x] === b[p + y]) {
        x++;
        y++;
      }
      v[off + k] = x;
      if (x >= n && y >= m) return backtrack(trace, n, m, off);
    }
  }
  return null;
}

function backtrack(trace: Int32Array[], n: number, m: number, off: number): number[] {
  const ops: number[] = [];
  let x = n, y = m;
  for (let d = trace.length - 1; d >= 0; d--) {
    const v = trace[d];
    const k = x - y;
    let prevK: number;
    if (k === -d || (k !== d && v[off + k - 1] < v[off + k + 1])) prevK = k + 1;
    else prevK = k - 1;
    const prevX = v[off + prevK];
    const prevY = prevX - prevK;
    // 先退掉对角线（相同行），再退那一步编辑
    while (x > prevX && y > prevY) {
      ops.push(EQ);
      x--;
      y--;
    }
    if (d > 0) {
      if (x === prevX) ops.push(ADD); // 往下那一步：b 的一行
      else ops.push(DEL); // 往右那一步：a 的一行
    }
    x = prevX;
    y = prevY;
  }
  ops.reverse();
  return ops;
}

/**
 * 两份文本 → 一段 git 格式的 unified diff（本地历史用，docs/IDEA.md 第 21 节）：交给差异视图（`parseDiff` / `DiffView`）画，
 * 双栏、着色、「N 行未改动」折叠都是现成的，不为本地历史另写一个渲染。一样给 ""；太大（同 `diffLines` 的两条上限）给 null。
 * 每块前后带 `ctx` 行上下文，两块挨得比 2×ctx 近就并成一块（同 git）
 */
export function unifiedDiff(oldText: string, newText: string, path: string, ctx = 3): string | null {
  if (oldText === newText) return "";
  // 空文件是 0 行（git 的 `-0,0`），不是「一个空行」：`"".split` 会给 [""]
  const a = oldText === "" ? [] : oldText.split("\n");
  const b = newText === "" ? [] : newText.split("\n");
  if (a.length + b.length > MAX_LINES) return null;
  let p = 0;
  const maxP = Math.min(a.length, b.length);
  while (p < maxP && a[p] === b[p]) p++;
  let q = 0;
  const maxQ = maxP - p;
  while (q < maxQ && a[a.length - 1 - q] === b[b.length - 1 - q]) q++;
  const mid = myers(a, b, p, a.length - p - q, b.length - p - q);
  if (!mid) return null;
  const ops = [...Array<number>(p).fill(EQ), ...mid, ...Array<number>(q).fill(EQ)];
  // 每个操作对应的旧行 / 新行下标（从 0 数）
  const rows: { op: number; ai: number; bi: number }[] = [];
  let ai = 0, bi = 0;
  for (const op of ops) {
    rows.push({ op, ai, bi });
    if (op !== ADD) ai++;
    if (op !== DEL) bi++;
  }
  const out = [`diff --git a/${path} b/${path}`, `--- a/${path}`, `+++ b/${path}`];
  let i = 0;
  while (i < rows.length) {
    if (rows[i].op === EQ) {
      i++;
      continue;
    }
    // 一块：从这处改动往前 ctx 行起，往后吃到「连着 2×ctx 行没改」为止
    const start = Math.max(0, i - ctx);
    let end = i;
    let quiet = 0;
    for (let j = i; j < rows.length; j++) {
      if (rows[j].op === EQ) quiet++;
      else {
        quiet = 0;
        end = j;
      }
      if (quiet > 2 * ctx) break;
    }
    const stop = Math.min(rows.length, end + 1 + ctx);
    const block = rows.slice(start, stop);
    const oldN = block.filter((r) => r.op !== ADD).length;
    const newN = block.filter((r) => r.op !== DEL).length;
    // 行数是 0 时起始行写成「前一行」（git 的约定：-0,0 表示这一侧什么都没有）
    const oldS = oldN ? block[0].ai + 1 : block[0].ai;
    const newS = newN ? block[0].bi + 1 : block[0].bi;
    out.push(`@@ -${oldS},${oldN} +${newS},${newN} @@`);
    for (const r of block) out.push(r.op === EQ ? ` ${a[r.ai]}` : r.op === DEL ? `-${a[r.ai]}` : `+${b[r.bi]}`);
    i = stop;
  }
  return out.join("\n") + "\n";
}
