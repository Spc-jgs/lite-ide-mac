/**
 * Java 补全（docs/IDEA.md 第 3 步）里不碰编辑器的那一半：LSP 的 snippet 换成 CM6 的、补全种类换成图标、行列换成偏移。
 * 拆出来是为了在裸 node 里测（tests/java-complete.test.ts）—— 接线在 java-lsp.ts。
 */
import type { Text } from "@codemirror/state";
import type { JavaEdit } from "../ipc/commands";

/**
 * LSP 的 snippet → CM6 `snippet()` 的模板。
 *
 * 两边大体同一种写法（`${1:name}`、`${0}` 是最后光标停的地方），差在三处：
 * - LSP 可以省掉花括号（`$1`、`$0`），CM6 只认带花括号的 —— 不换的话 `$0` 原样插进代码里
 * - LSP 的选择项 `${1|a,b|}`，CM6 没有，取第一个当默认文字
 * - LSP 用 `\$` `\}` `\\` 转义；CM6 里单独的 `$` 本来就是字面量，去掉反斜杠就行
 */
export function lspSnippet(text: string): string {
  return text
    .replace(/\$\{(\d+)\|([^,|}]*)[^}]*\|\}/g, (_, n, first) => `\${${n}:${first}}`)
    .replace(/(?<!\\)\$(\d+)/g, (_, n) => `\${${n}}`)
    .replace(/\\([$}\\])/g, "$1");
}

/** LSP 的 CompletionItemKind → CM6 的 `type`（决定图标，theme-idea-dark.ts 里按它上色） */
export function kindType(kind: number): string {
  switch (kind) {
    case 2:
    case 4:
      return "method";
    case 3:
      return "function";
    case 5:
    case 10:
      return "property";
    case 6:
      return "variable";
    case 7:
      return "class";
    case 8:
      return "interface";
    case 9:
      return "namespace";
    case 13:
      return "enum";
    case 14:
      return "keyword";
    case 20:
    case 21:
      return "constant";
    case 25:
      return "type";
    default:
      return "text";
  }
}

/** LSP 的行列（从 0 数、列按 UTF-16）→ 文档里的偏移。越界夹回来 */
export function offsetOf(doc: Text, line: number, ch: number): number {
  if (line >= doc.lines) return doc.length;
  const l = doc.line(line + 1);
  return Math.min(l.from + ch, l.to);
}

/**
 * resolve 回来的 additionalTextEdits 怎么和补全正文一起做。补全落在第 `line` 行的 `[col, endCol)`（列从 0 数、按 UTF-16）：
 * - `before`：补全那一处前面的 —— 上面的行、同一行在 `col` 之前结束的（自动 import 都在这里），先做，再把补全的范围挪一下
 * - `span`：同一行上、和补全范围重叠的**删除**，并进补全的范围里一笔换掉。这是后缀补全的：`name.length().var` 选 `var`，
 *   jdtls 给一笔「删 `name.length().var`」（从表达式开头到光标，读 1.61 `CompletionResolveHandler` / `PostfixTemplateEngine` 的字节码），
 *   正文那段换的是 `var` —— 两笔重叠，分开做哪一笔先都不对。没有就是 null
 * 别的（补全后面的、重叠但不是删除的）不做：没见 jdtls 给过，猜着做会把人的代码改坏
 */
export function splitEdits(edits: JavaEdit[], line: number, col: number, endCol: number): { before: JavaEdit[]; span: [number, number] | null } {
  const before: JavaEdit[] = [];
  let span: [number, number] | null = null;
  for (const e of edits) {
    if (e.endLine < line || (e.endLine === line && e.endCharacter <= col)) before.push(e);
    else if (e.line === line && e.endLine === line && e.text === "" && e.character <= col && e.endCharacter >= col) {
      span = [Math.min(span?.[0] ?? col, e.character), Math.max(span?.[1] ?? endCol, e.endCharacter)];
    }
  }
  return { before, span };
}

/**
 * jdtls 给的顺序是它的相关度（类型对得上的、近处的在前）；CM6 先按「和敲的字的匹配度」排，匹配度一样才看别的。
 * 按名次给一点加分：前面的略高 —— 只够在匹配度相近时排出先后，盖不过「完全匹配 vs 隔着几个字匹配」那种差距
 */
export function boostOf(index: number, total: number): number {
  if (total <= 1) return 0;
  return Math.round(20 - (40 * index) / (total - 1));
}
