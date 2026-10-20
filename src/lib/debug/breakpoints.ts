/**
 * 断点（docs/IDEA.md 第 18 节）：项目根 → 文件 → 行（从 1 数）。写的是编辑器（行号上点一下、⌘F8），读的是编辑器和调试开始前的同步 ——
 * 两边都是懒加载的块，这张表单独一个小文件（同 `run/test-outcomes.ts` 的分法），谁都不用把对方拽进来。
 *
 * **存进 localStorage、跨重启留着**：IDEA 的断点是存在工作区里的，调一个问题常常要好几天、重启好几次。
 * 一行改动之后断点跟着行走（编辑器里的 RangeSet 映射，`java-debug-gutter.ts`），走完再写回这里。
 * 每次变了也告诉 Rust（`debug_breakpoints`）：正在调试的会话当场下到 JVM 里；没在调试的话 Rust 只是记着，下次挂上时照着下。
 */
import { debugBreakpoints } from "../ipc/debug";

const KEY = "lite-ide.breakpoints";
type Table = Record<string, Record<string, number[]>>;

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

export function linesOf(root: string, path: string): number[] {
  return load()[root]?.[path] ?? [];
}

/** 这个文件的断点换成 `lines`。和原来一样就什么都不做（编辑器每次改动都会报一遍） */
export function setLines(root: string, path: string, lines: number[]) {
  const t = load();
  const next = [...new Set(lines.filter((l) => Number.isInteger(l) && l > 0))].sort((a, b) => a - b);
  const old = t[root]?.[path] ?? [];
  if (old.length === next.length && old.every((l, i) => l === next[i])) return;
  t[root] ??= {};
  if (next.length) t[root][path] = next;
  else delete t[root][path];
  save();
  void debugBreakpoints(root, path, next).catch(() => {});
  for (const f of subs) f(path);
}

export function toggle(root: string, path: string, line: number) {
  const cur = linesOf(root, path);
  setLines(root, path, cur.includes(line) ? cur.filter((l) => l !== line) : [...cur, line]);
}

/** 调试开始前：把这个项目的断点全部交给 Rust（重启之后 Rust 那边是空的） */
export async function syncAll(root: string) {
  const files = load()[root] ?? {};
  await Promise.all(Object.entries(files).map(([p, l]) => debugBreakpoints(root, p, l).catch(() => [])));
}

/** 这个项目一共几个断点（调试面板里说一句「没有断点」用） */
export function countOf(root: string): number {
  return Object.values(load()[root] ?? {}).reduce((n, l) => n + l.length, 0);
}

export function onBreakpoints(f: (path: string) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
