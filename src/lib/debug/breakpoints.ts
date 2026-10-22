/**
 * 断点（docs/IDEA.md 第 18 节）：项目根 → 文件 → 断点（行从 1 数；带 `cond` 的是条件断点）。写的是编辑器（行号上点一下、⌘F8、右键改条件），
 * 读的是编辑器和调试开始前的同步 —— 两边都是懒加载的块，这张表单独一个小文件（同 `run/test-outcomes.ts` 的分法），谁都不用把对方拽进来。
 *
 * **存进 localStorage、跨重启留着**：IDEA 的断点是存在工作区里的，调一个问题常常要好几天、重启好几次。
 * 一行改动之后断点跟着行走（编辑器里的 RangeSet 映射，`java-debug-gutter.ts`；**条件挂在标记上一起走**），走完再写回这里。
 * 每次变了也告诉 Rust（`debug_breakpoints`）：正在调试的会话当场下到 JVM 里；没在调试的话 Rust 只是记着，下次挂上时照着下。
 *
 * 存的格式兼容第一版：那时一个断点就是一个行号（数字），读到数字当成没有条件的断点 —— 已经下好的断点不会因为升级丢掉。
 *
 * 2026-10-22 起（docs/TAKEOVER.md G14）每个断点还可以带：`hit` 第几次走到才停（IDEA 的 Pass count）、`log` 日志断点（不停，打一句进运行窗的输出）、
 * `off` 停用（留着、画成空心，不交给调试器 —— IDEA 里点断点列表的勾就是这个，调一个问题时临时关掉几个比删了再找回来省事）
 */
import { debugBreakpoints, debugExceptions } from "../ipc/debug";

export interface Bp {
  line: number;
  /** 条件（Java 表达式，为真才停）。没有就是普通断点 */
  cond?: string;
  /** 第几次走到才停（正整数的字符串，同 java-debug 的 hitCondition） */
  hit?: string;
  /** 日志断点：不停，把这句打进运行窗的输出，`{表达式}` 换成值 */
  log?: string;
  /** 停用：留着、不交给调试器 */
  off?: true;
}

const KEY = "lite-ide.breakpoints";
type Stored = number | Bp;
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

const str = (v: unknown) => (typeof v === "string" && v.trim() ? v.trim() : undefined);

/** 读回来的一个断点：认得的字段留下、空的去掉（存的时候不留空字段：老版本读得懂、文件也小）。命中次数只留正整数（java-debug 直接 parseInt） */
export const norm = (s: Stored): Bp | null => {
  const b = typeof s === "number" ? { line: s } : s;
  if (!b || !Number.isInteger(b.line) || b.line <= 0) return null;
  const out: Bp = { line: b.line };
  const cond = str(b.cond);
  const hit = str(b.hit);
  const log = str(b.log);
  if (cond) out.cond = cond;
  if (hit && /^[1-9]\d{0,8}$/.test(hit)) out.hit = hit;
  if (log) out.log = log;
  if (b.off === true) out.off = true;
  return out;
};

/** 悬停在断点上（行号栏、断点列表）说清它是什么样的断点 */
export function bpTitle(b: Omit<Bp, "line">): string {
  const what: string[] = [];
  if (b.off) what.push("已停用（不停）");
  if (b.log) what.push(`日志断点：不停，打「${b.log}」`);
  if (b.cond) what.push(`${b.cond} 为真才${b.log ? "打" : "停"}`);
  if (b.hit) what.push(`第 ${b.hit} 次走到才${b.log ? "打" : "停"}`);
  return `${what.length ? what.join("；") : "断点"}（点一下取消，⌘F8；右键改属性）`;
}

/** 两个断点一样不一样（行之外的那几样也比） */
export const sameBp = (a: Bp, b: Bp) => a.line === b.line && (a.cond ?? "") === (b.cond ?? "") && (a.hit ?? "") === (b.hit ?? "") && (a.log ?? "") === (b.log ?? "") && !!a.off === !!b.off;

/** 交给 Rust 的那几个（停用的不交） */
function send(root: string, path: string, bps: Bp[]) {
  const on = bps.filter((b) => !b.off);
  return debugBreakpoints(root, path, on.map((b) => b.line), on.map((b) => b.cond ?? ""), on.map((b) => b.hit ?? ""), on.map((b) => b.log ?? ""));
}

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
  if (old.length === next.length && old.every((b, i) => sameBp(b, next[i]))) return;
  t[root] ??= {};
  // 普通断点照旧存成数字：第一版读得懂、文件也小
  if (next.length) t[root][path] = next.map((b) => (b.cond || b.hit || b.log || b.off ? b : b.line));
  else delete t[root][path];
  save();
  void send(root, path, next).catch(() => {});
  for (const f of subs) f(path);
}

/** 只给行号（行号上点一下、测试通道）：还留着的那几行，别的属性照旧 */
export function setLines(root: string, path: string, lines: number[]) {
  const had = new Map(bpsOf(root, path).map((b) => [b.line, b]));
  setBps(root, path, lines.map((line) => ({ ...had.get(line), line })));
}

/** 改一个断点的属性（属性小窗、断点列表的勾）。那一行没有断点就顺手下一个 */
export function setBp(root: string, path: string, bp: Bp) {
  setBps(root, path, [...bpsOf(root, path).filter((b) => b.line !== bp.line), bp]);
}

/** 这个项目全部的断点：文件 → 断点（断点列表用） */
export function allOf(root: string): [string, Bp[]][] {
  return Object.keys(load()[root] ?? {})
    .sort()
    .map((p) => [p, bpsOf(root, p)] as [string, Bp[]])
    .filter(([, b]) => b.length > 0);
}

export function toggle(root: string, path: string, line: number) {
  const cur = bpsOf(root, path);
  setBps(root, path, cur.some((b) => b.line === line) ? cur.filter((b) => b.line !== line) : [...cur, { line }]);
}

/** 改一个断点的条件（空的 = 变回普通断点），别的属性照旧。那一行没有断点就顺手下一个 */
export function setCond(root: string, path: string, line: number, cond: string) {
  const cur = bpsOf(root, path).find((b) => b.line === line);
  setBp(root, path, { ...cur, line, cond });
}

/**
 * 异常断点（G14，IDEA 的 Java Exception Breakpoints）：抛出未捕获的 / 被捕获的异常时停，`types` 只看这几种（类的全名，空 = 全部）。
 * 按项目存，同断点跨重启留着。默认全关 —— IDEA 默认开着「未捕获的」，但 Spring 的 Web 服务里异常基本都被框架接住了，开着也几乎不停；
 * 「被捕获的」不加类型能在启动时停几百次（框架自己抛了又接住）。开不开交给人，界面上把这句说出来
 */
export interface ExcBp {
  uncaught: boolean;
  caught: boolean;
  types: string[];
}
const EKEY = "lite-ide.exception-breakpoints";
const NO_EXC: ExcBp = { uncaught: false, caught: false, types: [] };

function excTable(): Record<string, ExcBp> {
  try {
    const v = JSON.parse(localStorage.getItem(EKEY) ?? "{}");
    return v && typeof v === "object" ? (v as Record<string, ExcBp>) : {};
  } catch {
    return {};
  }
}

export function excOf(root: string): ExcBp {
  const e = excTable()[root];
  if (!e) return { ...NO_EXC };
  const types = Array.isArray(e.types) ? e.types.filter((t) => typeof t === "string" && t.trim()).map((t) => t.trim()) : [];
  return { uncaught: e.uncaught === true, caught: e.caught === true, types };
}

export function setExc(root: string, e: ExcBp) {
  const t = excTable();
  const types = e.types.map((x) => x.trim()).filter(Boolean);
  if (e.uncaught || e.caught || types.length) t[root] = { uncaught: e.uncaught, caught: e.caught, types };
  else delete t[root];
  try {
    localStorage.setItem(EKEY, JSON.stringify(t));
  } catch {
    /* 写不下就只在这次运行里有 */
  }
  void debugExceptions(root, e.uncaught, e.caught, types).catch(() => {});
}

/** 调试开始前：把这个项目的断点、异常断点全部交给 Rust（重启之后它那边是空的） */
export async function syncAll(root: string) {
  const files = Object.keys(load()[root] ?? {});
  const e = excOf(root);
  await Promise.all([
    ...files.map((p) => send(root, p, bpsOf(root, p)).catch(() => [])),
    e.uncaught || e.caught ? debugExceptions(root, e.uncaught, e.caught, e.types).catch(() => {}) : Promise.resolve(),
  ]);
}

/** 这个项目一共几个断点（调试面板里说一句「没有断点」用） */
export function countOf(root: string): number {
  return Object.keys(load()[root] ?? {}).reduce((n, p) => n + bpsOf(root, p).length, 0);
}

export function onBreakpoints(f: (path: string) => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}
