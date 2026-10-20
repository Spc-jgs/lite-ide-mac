/**
 * 断点（docs/IDEA.md 第 18 节）：项目根 → 文件 → 断点（行从 1 数；带 `cond` 的是条件断点）。写的是编辑器（行号上点一下、⌘F8、右键改条件），
 * 读的是编辑器和调试开始前的同步 —— 两边都是懒加载的块，这张表单独一个小文件（同 `run/test-outcomes.ts` 的分法），谁都不用把对方拽进来。
 *
 * **存进 localStorage、跨重启留着**：IDEA 的断点是存在工作区里的，调一个问题常常要好几天、重启好几次。
 * 一行改动之后断点跟着行走（编辑器里的 RangeSet 映射，`java-debug-gutter.ts`；**条件挂在标记上一起走**），走完再写回这里。
 * 每次变了也告诉 Rust（`debug_breakpoints`）：正在调试的会话当场下到 JVM 里；没在调试的话 Rust 只是记着，下次挂上时照着下。
 *
 * 存的格式兼容第一版：那时一个断点就是一个行号（数字），读到数字当成没有条件的断点 —— 已经下好的断点不会因为升级丢掉。
 */
import { debugBreakpoints } from "../ipc/debug";

export interface Bp {
  line: number;
  /** 条件（Java 表达式，为真才停）。没有就是普通断点 */
  cond?: string;
}

const KEY = "lite-ide.breakpoints";
type Stored = number | { line: number; cond?: string };
type Table = Record<string, Record<string, Stored[]>>;

let table: Table | null = null;
const subs = new Set<(path: string) => void>();

function load(): Table {
  if (table) return table;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    table = v && typeof v === "object" ? (v as Table) : {};
  } catch {
    table = {};
  }
  return table;
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(table));
  } catch {
    /* 写不下（满了）就只在这次运行里有：断点不该把别的东西挤掉 */
  }
}

const norm = (s: Stored): Bp | null => {
  const b = typeof s === "number" ? { line: s } : s;
  if (!b || !Number.isInteger(b.line) || b.line <= 0) return null;
  const cond = typeof b.cond === "string" && b.cond.trim() ? b.cond.trim() : undefined;
  return cond ? { line: b.line, cond } : { line: b.line };
};

export function bpsOf(root: string, path: string): Bp[] {
  return (load()[root]?.[path] ?? []).map(norm).filter((b): b is Bp => !!b);
}

export function linesOf(root: string, path: string): number[] {
  return bpsOf(root, path).map((b) => b.line);
}

/** 这个文件的断点换成这些（同一行只留一个，后面的算数）。和原来一样就什么都不做（编辑器每次改动都会报一遍） */
export function setBps(root: string, path: string, bps: Bp[]) {
  const t = load();
  const byLine = new Map<number, Bp>();
  for (const b of bps) {
    const n = norm(b);
    if (n) byLine.set(n.line, n);
  }
  const next = [...byLine.values()].sort((a, b) => a.line - b.line);
  const old = bpsOf(root, path);
  if (old.length === next.length && old.every((b, i) => b.line === next[i].line && (b.cond ?? "") === (next[i].cond ?? ""))) return;
  t[root] ??= {};
  // 普通断点照旧存成数字：第一版读得懂、文件也小
  if (next.length) t[root][path] = next.map((b) => (b.cond ? b : b.line));
  else delete t[root][path];
  save();
  void debugBreakpoints(root, path, next.map((b) => b.line), next.map((b) => b.cond ?? "")).catch(() => {});
  for (const f of subs) f(path);
}

/** 只给行号（行号上点一下、测试通道）：还留着的那几行，条件照旧 */
export function setLines(root: string, path: string, lines: number[]) {
  const had = new Map(bpsOf(root, path).map((b) => [b.line, b.cond]));
  setBps(root, path, lines.map((line) => ({ line, cond: had.get(line) })));
}

export function toggle(root: string, path: string, line: number) {
  const cur = bpsOf(root, path);
  setBps(root, path, cur.some((b) => b.line === line) ? cur.filter((b) => b.line !== line) : [...cur, { line }]);
}

/** 改一个断点的条件（空的 = 变回普通断点）。那一行没有断点就顺手下一个 */
export function setCond(root: string, path: string, line: number, cond: string) {
  const cur = bpsOf(root, path).filter((b) => b.line !== line);
  setBps(root, path, [...cur, { line, cond }]);
}

/** 调试开始前：把这个项目的断点全部交给 Rust（重启之后它那边是空的） */
export async function syncAll(root: string) {
  const files = Object.keys(load()[root] ?? {});
  await Promise.all(
    files.map((p) => {
      const b = bpsOf(root, p);
      return debugBreakpoints(root, p, b.map((x) => x.line), b.map((x) => x.cond ?? "")).catch(() => []);
    }),
  );
}

/** 这个项目一共几个断点（调试面板里说一句「没有断点」用） */
export function countOf(root: string): number {
  return Object.keys(load()[root] ?? {}).reduce((n, p) => n + bpsOf(root, p).length, 0);
}

export function onBreakpoints(f: (path: string) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
