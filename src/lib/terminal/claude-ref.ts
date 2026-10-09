/**
 * 「发送到终端」（#45）写进终端的那串字：Claude Code 的 `@` 文件引用。
 *
 * 格式不是猜的，出自 Claude Code 2.1.293 自己的解析正则（从安装包里读出来的）：
 *
 * - 行号 `^([^#]+)(?:#L(\d+)(?:-(\d+))?)?…` → `@src/a.ts#L10-20`，单行 `#L10`，结尾那个数**不带** L。
 *   和官方 JetBrains 插件 ⌥⌘K 插入的一样（code.claude.com/docs/en/jetbrains 的例子 `@src/auth.ts#L1-99`）。
 * - 带空白的路径：`@"…"`（`@(?:"([^"\n]…`，它自己的补全也写成这样）；或者 `\ ` 转义（`(?:^|\s)@((?:[^\s\\]|\\ )+)`）。
 *   优先用引号：引号里除了 `"` 什么都能放，`\ ` 只认空格一种转义。
 * - 路径里本身有 `#`：上面那条正则拿 `#` 切路径，这种文件**没法用 @ 引用**（引了会指到 `#` 前面那截）。
 *   退一步给不带 @ 的 `路径:10-20`，claude 读得懂这种写法，会自己去读那个文件。
 *
 * 开头留一个空格：claude 认 `@` 的前提是它前面是行首或空白（`(?:^|\s)@`），而输入框里可能已经有半句话
 * 「看一下」—— 不隔开就粘成 `看一下@src/a.ts`，不再是引用。末尾也留一个，好接着打字（同从 Finder 拖文件进终端）。
 */

/** 选中的一段落在哪几行（1 起，两头都含） */
export interface LineSpan {
  from: number;
  to: number;
}

/**
 * 一段选区换成行范围。`toCol` 是终点在那一行的列（0 起）。
 *
 * 选区终点落在某行**行首**、而且不止一行时，那一行不算：⇧↓ 选整行、三击选行，终点都停在下一行的开头，
 * 人想说的是「这几行」而不是「这几行外加下一行的第 0 个字」。VS Code / IDEA 的「复制引用」也这么算。
 */
export function lineSpan(fromLine: number, toLine: number, toCol: number): LineSpan {
  return { from: fromLine, to: toLine > fromLine && toCol === 0 ? toLine - 1 : toLine };
}

/**
 * 文件路径相对于终端前台进程的目录（claude 按它自己的工作目录解析 `@`）。
 * 不在那个目录底下就给绝对路径 —— `../../x` 能用但难读，绝对路径永远对。
 */
export function refPath(file: string, cwd: string | null): string {
  if (!cwd) return file;
  const base = cwd.endsWith("/") ? cwd : `${cwd}/`;
  return file.startsWith(base) ? file.slice(base.length) : file;
}

function one(path: string, s: LineSpan | null): string {
  const lines = s ? (s.to > s.from ? `${s.from}-${s.to}` : `${s.from}`) : "";
  if (path.includes("#")) {
    const plain = lines ? `${path}:${lines}` : path;
    return /\s/.test(plain) && !plain.includes('"') ? `"${plain}"` : plain;
  }
  const body = lines ? `${path}#L${lines}` : path;
  if (!/\s/.test(body)) return `@${body}`;
  return body.includes('"') ? `@${body.replace(/ /g, "\\ ")}` : `@"${body}"`;
}

/**
 * 整串：没选东西就是整个文件，选了几段就几个引用（多光标各选一段时）。同一个范围只给一次，按行号排。
 */
export function mention(path: string, spans: LineSpan[]): string {
  const seen = new Set<string>();
  const uniq = [...spans]
    .sort((a, b) => a.from - b.from || a.to - b.to)
    .filter((s) => !seen.has(`${s.from}-${s.to}`) && seen.add(`${s.from}-${s.to}`));
  const refs = uniq.length ? uniq.map((s) => one(path, s)) : [one(path, null)];
  return ` ${refs.join(" ")} `;
}
