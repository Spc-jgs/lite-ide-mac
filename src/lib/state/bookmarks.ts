/**
 * 书签（docs/TAKEOVER.md G18 的另一半，IDEA 的 Bookmarks）：项目根 → 文件 → 书签（行从 1 数，带打书签时那一行的字，列表里给人认）。
 * 编辑器里 F3 在光标那行打 / 取消（`editor/bookmarks-ext.ts`，标记跟着行走），⌘F3 开底部的「书签」工具窗看全部、点一行跳过去。
 *
 * **存 localStorage、跨重启留着、不往项目里写**（同断点 `debug/breakpoints.ts`：IDEA 的书签存在工作区里，不进仓库）。
 * 没开项目的窗口（草稿本）记在 `""` 那一格。不是 Svelte 的状态：编辑器扩展和工具窗两个懒块各自订阅，这个小文件谁都不拽谁
 */

export interface Mark {
  line: number;
  /** 打书签时（和每次跟着行走写回时）那一行的字，去掉两头空白、截 120 字 */
  text: string;
}
export interface Entry extends Mark {
  path: string;
}

const KEY = "lite-ide.bookmarks";
type Table = Record<string, Record<string, Mark[]>>;

let table: Table | null = null;
const subs = new Set<(root: string, path: string) => void>();

function load(): Table {
  if (table) return table;
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? "{}");
    table = v && typeof v === "object" && !Array.isArray(v) ? (v as Table) : {};
  } catch {
    table = {};
  }
  return table;
}

function save() {
  try {
    localStorage.setItem(KEY, JSON.stringify(table));
  } catch {
    /* 写不下就只在这次运行里有 */
  }
}

/** 一行字 → 存进书签的那个样子 */
export const snippet = (s: string) => {
  const t = s.trim();
  return t.length > 120 ? `${t.slice(0, 120)}…` : t;
};

/** 这个文件的书签（读回来的认不得的丢掉，按行排） */
export function marksOf(root: string | null, path: string): Mark[] {
  const list = load()[root ?? ""]?.[path] ?? [];
  return list.filter((m) => m && Number.isInteger(m.line) && m.line > 0).map((m) => ({ line: m.line, text: typeof m.text === "string" ? m.text : "" }));
}

/** 这个项目全部的书签：按文件路径、行号排 */
export function allOf(root: string | null): Entry[] {
  const files = load()[root ?? ""] ?? {};
  return Object.keys(files)
    .sort()
    .flatMap((path) => marksOf(root, path).map((m) => ({ path, ...m })))
    .sort((a, b) => (a.path === b.path ? a.line - b.line : a.path < b.path ? -1 : 1));
}

/** 整个换掉这个文件的书签（编辑器里跟着行走之后写回、工具窗里删一条）。同一行两个只留一个；空了就把这个文件摘掉 */
export function setMarks(root: string | null, path: string, list: Mark[]) {
  const t = load();
  const r = root ?? "";
  const seen = new Set<number>();
  const clean = list.filter((m) => m.line > 0 && !seen.has(m.line) && seen.add(m.line)).sort((a, b) => a.line - b.line);
  const before = JSON.stringify(t[r]?.[path] ?? []);
  if (before === JSON.stringify(clean)) return;
  if (clean.length) (t[r] ??= {})[path] = clean;
  else if (t[r]) {
    delete t[r][path];
    if (!Object.keys(t[r]).length) delete t[r];
  }
  save();
  for (const f of subs) f(r, path);
}

/** 这一行有就取消、没有就打上。返回打上了没有 */
export function toggle(root: string | null, path: string, line: number, text: string): boolean {
  const cur = marksOf(root, path);
  const has = cur.some((m) => m.line === line);
  setMarks(root, path, has ? cur.filter((m) => m.line !== line) : [...cur, { line, text: snippet(text) }]);
  return !has;
}

/** 文件 / 目录改名、挪走了（改名、移动类、文件树里拖、改名）：书签跟过去。`from` 是目录时它底下的全跟着走 */
export function rename(root: string | null, from: string, to: string) {
  const files = load()[root ?? ""] ?? {};
  for (const p of Object.keys(files)) {
    if (p !== from && !p.startsWith(`${from}/`)) continue;
    const list = marksOf(root, p);
    setMarks(root, p, []);
    setMarks(root, to + p.slice(from.length), list);
  }
}

/** 书签变了（哪个项目的哪个文件）。返回退订 */
export function onBookmarks(f: (root: string, path: string) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/** 测试用：丢掉缓存、下次重读 localStorage */
export function _reset() {
  table = null;
}
