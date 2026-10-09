/**
 * 跨文件替换浮层的纯函数（#42 第 3 步）：勾选 → 要替换哪些、编辑怎么落到文本上、列表怎么摊平给虚拟滚动。
 * 拎出来是为了在裸 node 里测 —— 这几样错一个，替换的就不是你勾的那些。
 */
import type { ReplaceEdit, ReplaceFile, ReplaceScan } from "../ipc/commands";

/**
 * 一处命中的身份：文件 + 行 + 列。**不用下标**：重新扫一次（浮层关了再开、开关变了）下标会挪，
 * 而「你取消勾选的那一处」应该还是那一处 —— 行列在文件没变时是稳的，文件变了那一处本来也就不是那一处了
 */
export const hitKey = (rel: string, h: { line: number; col: number }) => `${rel}:${h.line}:${h.col}`;

/** 勾了的那些 → 给 Rust 的 picks（文件 + 第几处）。一处都没勾的文件不带 */
export function buildPicks(scan: ReplaceScan, unchecked: ReadonlySet<string>): { rel: string; hits: number[] }[] {
  const out: { rel: string; hits: number[] }[] = [];
  for (const f of scan.files) {
    const hits = f.hits.flatMap((h, i) => (unchecked.has(hitKey(f.rel, h)) ? [] : [i]));
    if (hits.length) out.push({ rel: f.rel, hits });
  }
  return out;
}

/** 按钮上那句「替换 N 处 · M 个文件」 */
export function counts(scan: ReplaceScan, unchecked: ReadonlySet<string>): { hits: number; files: number } {
  const p = buildPicks(scan, unchecked);
  return { hits: p.reduce((n, x) => n + x.hits.length, 0), files: p.length };
}

/** 一个文件的勾选状态：全勾 / 全没勾 / 勾了一部分（文件那一行的复选框画成半勾） */
export function fileState(f: ReplaceFile, unchecked: ReadonlySet<string>): "all" | "none" | "some" {
  const off = f.hits.filter((h) => unchecked.has(hitKey(f.rel, h))).length;
  return off === 0 ? "all" : off === f.hits.length ? "none" : "some";
}

/** 点文件那一行的复选框：全勾 → 全不勾，其余（半勾、全不勾）→ 全勾 */
export function toggleFile(f: ReplaceFile, unchecked: ReadonlySet<string>): Set<string> {
  const next = new Set(unchecked);
  const allOn = fileState(f, unchecked) === "all";
  for (const h of f.hits) {
    const k = hitKey(f.rel, h);
    if (allOn) next.add(k);
    else next.delete(k);
  }
  return next;
}

/**
 * 把 Rust 给的编辑落到文本上（UTF-16 下标，原文里的位置，按位置升序、互不重叠）。
 * **从后往前改**：从前往后的话，前一处改了长度，后面那些下标就全歪了
 */
export function applyEdits(text: string, edits: readonly ReplaceEdit[]): string {
  let out = text;
  for (const e of [...edits].sort((a, b) => b.from - a.from)) {
    out = out.slice(0, e.from) + e.insert + out.slice(e.to);
  }
  return out;
}

/** 列表里的一行：文件头，或者一处命中。摊平了给虚拟滚动 —— 5000 处一次全画出来，DOM 节点就是几万个 */
export type Row = { kind: "file"; fi: number } | { kind: "hit"; fi: number; hi: number };

export function flatten(scan: ReplaceScan, collapsed: ReadonlySet<string>): Row[] {
  const out: Row[] = [];
  scan.files.forEach((f, fi) => {
    out.push({ kind: "file", fi });
    if (!collapsed.has(f.rel)) f.hits.forEach((_, hi) => out.push({ kind: "hit", fi, hi }));
  });
  return out;
}

/**
 * 虚拟滚动：只画看得见的那一段（上下各多画几行，滚得快时不露白）。
 * 行高固定，所以「第几行在哪」是乘法，不用量
 */
export function visibleRange(total: number, rowH: number, scrollTop: number, viewH: number, overscan = 8): [number, number] {
  const first = Math.max(0, Math.floor(scrollTop / rowH) - overscan);
  const last = Math.min(total, Math.ceil((scrollTop + viewH) / rowH) + overscan);
  return [first, last];
}

/**
 * 一处命中画成一行：前文 + ~~旧的~~ + 新的 + 后文（VS Code 搜索面板的样子，一眼看得出改了什么）。
 * 前文太长就收成「…」，让改的那一段出现在看得见的地方（同 `snippet`）。
 * `hit` / `after` 是 Rust 给的「改前那一行 + 命中区间」和「改后那一行 + 换上去的区间」，都是 UTF-16 下标
 */
export function inlineDiff(
  hit: { text: string; spans: [number, number][] },
  after: { text: string; spans: [number, number][] } | undefined,
  before = 28,
): { pre: string; old: string; neu: string | null; post: string } {
  const lead = hit.text.length - hit.text.trimStart().length;
  const [a, b] = hit.spans[0] ?? [hit.text.length, hit.text.length];
  const s = Math.max(lead, a - before);
  const pre = (s > lead ? "…" : "") + hit.text.slice(s, a);
  const old = hit.text.slice(a, b);
  // 还没拿到「改后」（替换串刚改、预览在路上）：只画旧的，不画一个错的新的
  if (!after) return { pre, old, neu: null, post: hit.text.slice(b) };
  const [x, y] = after.spans[0] ?? [a, a];
  return { pre, old, neu: after.text.slice(x, y), post: after.text.slice(y) };
}
