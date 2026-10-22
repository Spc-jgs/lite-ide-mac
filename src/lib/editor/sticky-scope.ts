/**
 * 粘性行（docs/TAKEOVER.md G19）里不碰编辑器的那一半：给一行，算出它在哪几层里、要钉哪几行。单独一个文件在裸 node 里测（tests/sticky.test.ts）——
 * 画的那一半（`sticky.ts`）引着设置的状态，裸 node 里起不来。
 */
import type { EditorState } from "@codemirror/state";
import { syntaxTree } from "@codemirror/language";
import type { SyntaxNode } from "@lezer/common";

/** 一层：声明（类、接口、枚举、方法、构造器、函数）和带块的控制流（if / for / while / switch / try / catch / finally / synchronized / with） */
const SCOPE =
  /^(Class|Interface|Enum|AnnotationType|Record|Method|Constructor|Function)(Declaration|Definition)$|^(If|While|For|EnhancedFor|Switch|Do|Synchronized|Try|TryWithResources|With)Statement$|^(CatchClause|FinallyClause)$/;

/** 最多钉几行。IDEA 默认 5、VS Code 默认 5；编辑区本来就不高，4 行之后挡住的正文比带来的信息多。超了留里面的（离你近的那几层） */
export const STICKY_MAX = 4;

/**
 * 一层的「头」在第几行：声明跳过前面的注解和修饰符（`@Override` 单独一行时，钉的是方法签名那行，不是 `@Override`）。
 * `Modifiers` 结束的地方往后第一个不是空白的字所在的行
 */
function headLine(state: EditorState, n: SyntaxNode): number {
  let from = n.from;
  const mods = n.getChild("Modifiers");
  if (mods && mods.from === n.from) {
    const after = state.doc.sliceString(mods.to, Math.min(n.to, mods.to + 200));
    const skip = after.length - after.trimStart().length;
    from = mods.to + (skip === after.length ? 0 : skip);
  }
  return state.doc.lineAt(from).number;
}

/**
 * 第 `top` 行（视口顶上那一行）在哪几层里：从外到里的「头」的行号。只要头在 `top` 上面、身子还盖着 `top` 的那几层；
 * 头和 `top` 是同一行的不算（它自己就露着）。超过 `max` 留里面的
 */
export function stickyLines(state: EditorState, top: number, max = STICKY_MAX): number[] {
  if (top < 2 || top > state.doc.lines) return [];
  const pos = state.doc.line(top).from;
  const out: number[] = [];
  let from: SyntaxNode | null = null;
  for (let n: SyntaxNode | null = syntaxTree(state).resolveInner(pos, 1); n; from = n, n = n.parent) {
    if (!SCOPE.test(n.type.name)) continue;
    // 在 catch / finally 里：try 那一块已经过去了，钉 catch 那行就够（IDEA 也不钉 try）
    if (/^Try/.test(n.type.name) && from && /^(CatchClause|FinallyClause)$/.test(from.type.name)) continue;
    let head = headLine(state, n);
    // 在 else 那一支里：钉 `} else {` 那行，不钉 `if (…)` —— 不然看着像在 if 那一支里
    if (n.type.name === "IfStatement" && from) {
      for (let c = n.firstChild; c && c.from < from.from; c = c.nextSibling) if (c.type.name === "else") head = state.doc.lineAt(c.from).number;
    }
    const end = state.doc.lineAt(n.to).number;
    if (head < top && end >= top && !out.includes(head)) out.push(head);
  }
  return out.reverse().slice(-max);
}

