/**
 * 终端输出里哪些字能点：网址、项目里的文件路径（带行号）、Java 堆栈帧。
 *
 * # 为什么
 *
 * 在内置终端里跑 mvn / gradle / 测试，报错里的 `Foo.java:[42,10]`、异常堆栈、
 * `http://localhost:8080` 原来都是死字，得自己去 ⌘P 敲文件名、再 ⌘L 敲行号。IDEA 的运行窗口、
 * iTerm、VSCode 的终端都能点 —— 这是很强的肌肉记忆（2026-09-23 交互习惯那轮）。
 *
 * # 路径只认坐实了的
 *
 * 和 ⌘Click、日志堆栈帧同一条：**有下划线就一定开得对**。路径要在 ⌘P 那份文件索引里找得到
 * （绝对路径先去掉项目根前缀，相对路径按项目根、再按终端的起始目录试）；堆栈帧走
 * `logview/stack-frame.ts` 那套包路径后缀匹配、只认唯一。找不到的不画 —— 项目外的文件、
 * 已经删了的、jar 里的类。
 *
 * 纯函数，不碰 xterm：列号换算（中文一个字占两格）在 Terminal.svelte。
 * 零 import —— 堆栈帧的识别由调用方经 `ctx.frameAt` 给（测试直接跑 .ts，相对 import 解析不了）。
 */

export type TermLink =
  | { start: number; end: number; kind: "url"; url: string }
  | { start: number; end: number; kind: "file"; rel: string; line?: number };

export interface LinkCtx {
  /** 项目根（绝对路径，无尾斜杠）；没开项目就是 null —— 那时路径一个都不认 */
  root: string | null;
  /** 终端起始目录相对项目根的部分（`""` = 就在根上）；不在项目里就是 null */
  cwdRel: string | null;
  /** 这个相对路径在不在项目的文件索引里 */
  has: (rel: string) => boolean;
  /**
   * 这一行是不是一帧能坐实的堆栈：`stackFrame` + `frameResolver`（logview/stack-frame.ts），
   * 给出「文件:行号」那一段的位置和对应的文件；jar 里的、认不准的就是 null
   */
  frameAt: (text: string) => { from: number; to: number; rel: string; line: number } | null;
}

const URL_RE = /\bhttps?:\/\/[^\s<>"'`]+/g;
/**
 * 文件路径 + 可选的行列，覆盖几种常见的写法：
 * - `Foo.java:42` / `Foo.java:42:10`（javac、tsc、rustc、eslint）
 * - `Foo.java:[42,10]`（maven 的 `[ERROR]` 行）
 * - `Foo.kt: (42, 10)`（gradle 的 kotlin 编译器；前面常带 `file://`）
 * - 光有路径不带行号也算（`git status` 列出来的那种）
 * 路径至少要有一个 `/` 或者就是一个带扩展名的文件名；扩展名是白名单，免得把 `1.5.2` 这种当成文件
 */
const EXT = "java|kt|kts|scala|groovy|gradle|xml|ya?ml|properties|sql|ts|tsx|js|jsx|mjs|cjs|py|rs|go|md|json|toml|vue|svelte|css|scss|html|sh";
const PATH_RE = new RegExp(
  `(?:file://)?((?:\\.{1,2}/|/)?[\\w.\\-]+(?:/[\\w.\\-]+)*\\.(?:${EXT}))\\b(?::\\[(\\d+)(?:,\\d+)?\\]|:(\\d+)(?::\\d+)?|:?\\s?\\((\\d+),\\s*\\d+\\))?`,
  "g",
);

/** 网址末尾常粘着句号、括号、引号 —— 那是句子的，不是网址的 */
function trimUrl(u: string): string {
  return u.replace(/[.,;:!?)\]}>'"]+$/, "");
}

/**
 * `a/b/../c` → `a/c`。和 `editor/jump.ts` 里那个是同一个函数，**有意抄了一份**：两个文件都要
 * 零相对 import（测试直接跑 .ts，相对 import 不带扩展名解析不了），抽成公共模块两边的测试都跑不起来。
 * 改一处记得改另一处。
 */
function normalize(p: string): string {
  const out: string[] = [];
  for (const seg of p.split("/")) {
    if (!seg || seg === ".") continue;
    if (seg === "..") out.pop();
    else out.push(seg);
  }
  return out.join("/");
}

/** 一段路径在项目里对应哪个文件（相对项目根），坐实不了就是 null */
function resolvePath(p: string, ctx: LinkCtx): string | null {
  if (!ctx.root) return null;
  if (p.startsWith("/")) {
    if (!p.startsWith(`${ctx.root}/`)) return null; // 项目外的文件：没法同步确认它在不在
    const rel = p.slice(ctx.root.length + 1);
    return ctx.has(rel) ? rel : null;
  }
  // 相对路径：先按终端的起始目录（mvn 在子模块里跑，报的是相对子模块的路径），再按项目根
  const tries = ctx.cwdRel ? [normalize(`${ctx.cwdRel}/${p}`), normalize(p)] : [normalize(p)];
  for (const t of tries) if (t && ctx.has(t)) return t;
  return null;
}

export function findTermLinks(text: string, ctx: LinkCtx): TermLink[] {
  const out: TermLink[] = [];
  const taken = (a: number, z: number) => out.some((l) => a < l.end && z > l.start);

  for (const m of text.matchAll(URL_RE)) {
    const url = trimUrl(m[0]);
    out.push({ start: m.index, end: m.index + url.length, kind: "url", url });
  }

  // 堆栈帧：一行最多一帧，而且帧里的「文件:行号」光按文件名找不到（没有包路径）
  const f = ctx.frameAt(text);
  if (f && !taken(f.from, f.to)) out.push({ start: f.from, end: f.to, kind: "file", rel: f.rel, line: f.line });

  for (const m of text.matchAll(PATH_RE)) {
    const start = m.index;
    const end = start + m[0].length;
    if (taken(start, end)) continue;
    const rel = resolvePath(m[1], ctx);
    if (!rel) continue;
    const ln = Number(m[2] ?? m[3] ?? m[4]);
    out.push({ start, end, kind: "file", rel, line: ln > 0 ? ln : undefined });
  }
  return out.sort((a, b) => a.start - b.start);
}
