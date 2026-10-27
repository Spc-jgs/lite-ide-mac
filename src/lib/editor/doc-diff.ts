/**
 * 整篇换内容时（外面改了文件重读、本地历史里「换成这一版」），算出**按行的最小改动**交给 CM6，而不是 `{ from: 0, to: 全长 }` 一笔替换。
 *
 * 为什么：断点挂在行首、靠 CM6 照改动映射跟着行走。行号上的标记（`GutterMarker`）的映射方式是 `MapMode.TrackBefore` ——
 * **它前面那个字符被删了，标记就没了**（读 @codemirror/state 的 `mapPos` 确认）。一笔替换整篇，除了第 1 行，所有断点前面的字符都被删了：
 * 全没了，只剩第 1 行的（2026-10-21 验热替换时撞见：验收脚本在外面重写了 Money.java）。
 *
 * 所以改动的**形状**要照这条规矩来，不只是「范围小」：
 * - 一行的内容变了：只换这一行的内容、不碰它的换行 —— 这行的断点在起点上、留着；下一行前面的换行没被删、也留着
 * - 多出来的行删掉：删「前一行末尾的换行 + 这几行」，后面那行前面的换行还在
 * - 新加的行：插在前一行末尾（「换行 + 内容」），后面那行的断点在插入点之后、跟着往下挪
 * - 文件最开头插 / 删没有「前一行」可挂，第 1 行的断点会跟着挪 / 没掉 —— Java 文件第 1 行是 package，没人在那儿下断点
 *
 * 先剥掉首尾相同的行（外面的改动多半只碰一小段），中间按行做最长公共子序列：开头加一个 import、末尾加一个方法这种两处的改动拆成两笔。
 * 一段里旧的几行和新的几行先一对一配对（当成「这行改了」，断点留着 —— 格式化器重排一段、行数没变时一个都不丢），多出来的才删 / 插。
 * 中间太大（行数乘积超过 [`MAX_CELLS`]）就把中间当成一段 —— 首尾照样留着
 */

export interface Change {
  from: number;
  to: number;
  insert: string;
}

/**
 * 中间那段做 LCS 的表最多这么多格（Uint16，8 MB）。超了的是格式化器整篇重排那种，逐行对齐也没多大意义。
 * 实测（2026-10-22，M 系列）：2000 行的文件开头、中间、末尾各改一处（剥完首尾中间还是 2000 × 2001，贴着上限）10.6ms；
 * 1500 × 1500 是 3ms；超上限退回整段 0.4ms。只在外面改了文件重读、本地历史恢复时跑一次，不在打字的路上
 */
const MAX_CELLS = 4_000_000;

/** `before` 改成 `after` 的改动，位置都按 `before` 算、从前往后、互不重叠（CM6 的 `changes` 数组就要这种）。一样的话是空数组 */
export function lineChanges(before: string, after: string): Change[] {
  // CM6 的文档里没有 \r（`doc.toString()` 一律 \n 连行），而 `after` 可能是盘上 / 本地历史里的原文、带 CRLF。按 \n 切的话每行尾巴挂着 \r，
  // 插进去时 CM6 把单个 \r 也当成换行 —— 每行后面多出一个空行（2026-10-27 history.sh：GBK + CRLF 的文件「换成这一版」撞见）。先统一成 \n
  after = after.replace(/\r\n?/g, "\n");
  if (before === after) return [];
  const a = before.split("\n");
  const b = after.split("\n");
  // 旧文的每行起点；行尾 = 起点 + 长度（不含换行）
  const start = [0];
  for (const l of a) start.push(start[start.length - 1] + l.length + 1);
  const end = (k: number) => start[k] + a[k].length;
  const out: Change[] = [];

  /** 旧的 [i0, i1) 行换成新的 [j0, j1) 行 */
  const hunk = (i0: number, i1: number, j0: number, j1: number) => {
    const pair = Math.min(i1 - i0, j1 - j0);
    for (let k = 0; k < pair; k++) if (a[i0 + k] !== b[j0 + k]) out.push({ from: start[i0 + k], to: end(i0 + k), insert: b[j0 + k] });
    const i = i0 + pair;
    const j = j0 + pair;
    if (i < i1) {
      // 删 [i, i1)：带上前一行末尾的换行；没有前一行（文件开头）就带上最后一行后面的
      out.push(i > 0 ? { from: end(i - 1), to: end(i1 - 1), insert: "" } : { from: 0, to: start[i1], insert: "" });
    } else if (j < j1) {
      // 插新的 [j, j1) 在旧的第 i 行前面：挂在前一行末尾；没有前一行就插在开头
      const lines = b.slice(j, j1);
      out.push(i > 0 ? { from: end(i - 1), to: end(i - 1), insert: "\n" + lines.join("\n") } : { from: 0, to: 0, insert: lines.join("\n") + "\n" });
    }
  };

  let lo = 0;
  while (lo < a.length && lo < b.length && a[lo] === b[lo]) lo++;
  let ea = a.length;
  let eb = b.length;
  while (ea > lo && eb > lo && a[ea - 1] === b[eb - 1]) {
    ea--;
    eb--;
  }
  const n = ea - lo;
  const m = eb - lo;
  if (n === 0 || m === 0 || n * m > MAX_CELLS) {
    hunk(lo, ea, lo, eb);
    return out;
  }

  // 行换成整数再比（字符串比较每次从头比到不同处）
  const ids = new Map<string, number>();
  const id = (s: string) => {
    let v = ids.get(s);
    if (v === undefined) ids.set(s, (v = ids.size));
    return v;
  };
  const x = Int32Array.from({ length: n }, (_, i) => id(a[lo + i]));
  const y = Int32Array.from({ length: m }, (_, j) => id(b[lo + j]));
  // L[i][j] = x[i..] 和 y[j..] 的最长公共子序列长度。n·m ≤ 4M 时 min(n, m) ≤ 2000，Uint16 装得下
  const w = m + 1;
  const L = new Uint16Array((n + 1) * w);
  for (let i = n - 1; i >= 0; i--)
    for (let j = m - 1; j >= 0; j--) L[i * w + j] = x[i] === y[j] ? L[(i + 1) * w + j + 1] + 1 : Math.max(L[(i + 1) * w + j], L[i * w + j + 1]);

  // 从前往后走：相同的行跳过，不同的攒成一段，碰到下一行相同时交出去
  let i = 0;
  let j = 0;
  let hi = 0;
  let hj = 0;
  while (i < n && j < m) {
    if (x[i] === y[j]) {
      hunk(lo + hi, lo + i, lo + hj, lo + j);
      i++;
      j++;
      hi = i;
      hj = j;
    } else if (L[(i + 1) * w + j] >= L[i * w + j + 1]) i++;
    else j++;
  }
  hunk(lo + hi, lo + n, lo + hj, lo + m);
  return out;
}
