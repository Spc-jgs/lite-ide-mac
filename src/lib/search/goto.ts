/**
 * ⌘P 里的 Goto Anything（issue #43）：把输入拆成「找哪个文件」和「到文件里的哪儿」。
 *
 * Sublime 的写法，一个框走到底：
 *
 * | 输入 | 意思 |
 * |---|---|
 * | `order@listMember` | 文件名模糊匹配 `order`，再在那个文件的符号里找 `listMember` |
 * | `order:42` / `order:42:7` | 那个文件的第 42 行（第 7 列） |
 * | `@listMember` | 当前文件的符号 |
 * | `:42` | 当前文件的第 42 行 |
 *
 * 拎成纯函数是为了在裸 node 里测 —— 判据全在「什么算分隔符」上，而那几条边界（`foo@2x.png`、
 * `@types/node`）在界面上点是点不全的。
 */

export type Goto =
  /** 普通搜索，整串照原样去找文件 / 内容 / 操作 */
  | { kind: "plain" }
  /** `file` 为空 = 当前文件 */
  | { kind: "symbol"; file: string; sym: string }
  /** `line` 为 null = 冒号已经打了、数字还没打（等后半段，列表别跳） */
  | { kind: "line"; file: string; line: number | null; col: number | null };

/*
 * 符号部分只认标识符字符。这是区分「分隔符」和「文件名里本来就有的 @」的唯一依据：
 * `icon@2x.png` 的 `@` 后面带着 `.`，不是符号；`@types/node` 的后面带着 `/`，也不是。
 * 代价：打到 `icon@2x` 那一刻会被当成在找符号 `2x` —— 再打一个 `.` 就回来了，只是一瞬间
 */
// 分隔符总是**最后一个** @：符号部分不含 @、又锚在串尾，前面的 @ 后面跟不出一段纯标识符。
// 所以 `node_modules/@types/x@foo` 前半截里的 @ 自然留在文件名里，不用另判
const SYM = /^(.*)@([\w$]*)$/;
// 行列只认数字。`a:b` 不是跳行（那更像是在搜正文里的 `a:b`）
const LINE = /^(.*?):(\d*)(?::(\d*))?$/;

export function parseGoto(q: string): Goto {
  const s = SYM.exec(q);
  if (s) return { kind: "symbol", file: s[1].trim(), sym: s[2] };
  const l = LINE.exec(q);
  if (l && !l[1].includes(":")) {
    const num = (t: string | undefined) => (t ? Number(t) : null);
    return { kind: "line", file: l[1].trim(), line: num(l[2]), col: num(l[3]) };
  }
  return { kind: "plain" };
}
