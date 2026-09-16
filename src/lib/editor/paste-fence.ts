/**
 * 粘贴 JSON 自动包一层 ```` ```json ```` 围栏（M10 ②b）。
 *
 * 调试笔记里粘一坨 JSON 是常事，而 markdown 眼里它是一段 prose：`{` `"` 都是普通字符，
 * live preview 还会把里面的 `_` 认成强调。包进围栏之后有三样是白捡的：等宽、能折叠
 * （foldGutter 认 FencedCode）、markdown-live 整块加底色。
 *
 * **只在三个条件同时成立时动手**，别的情况原样粘：
 * 1. 粘的东西整体是合法 JSON（`JSON.parse` 过），而且是对象或数组 —— 一个数字、一个
 *    字符串也是合法 JSON，但那是人在句子里粘个值；
 * 2. 光标在一行的开头，且那一行是空的 —— 粘在句子中间的不该被拆成三行；
 * 3. 光标不在围栏里 —— 已经在 ```` ``` ```` 里了再包一层就是双层。
 *
 * 不做「像 JSON 但解析不过」的猜测（尾逗号、单引号）：猜错的代价是把一段正文包进
 * 代码块，比不包难受。不做别的格式（堆栈、XML）：先看 JSON 这一种用得多不多。
 *
 * 文件里写进去的就是标准 markdown 围栏，没有私有标记。
 */
import { syntaxTree } from "@codemirror/language";
import type { EditorState, Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

/** 粘的东西该不该包：整体是 JSON 对象 / 数组，并且不是一行里的小值 */
export function looksLikeJsonBlock(text: string): boolean {
  const t = text.trim();
  if (!(t.startsWith("{") || t.startsWith("[")) || t.length < 2) return false;
  try {
    const v = JSON.parse(t);
    return typeof v === "object" && v !== null;
  } catch {
    return false;
  }
}

/** 光标那一行是不是空行，且不在围栏代码块里 */
function atBlankLineOutsideFence(state: EditorState): boolean {
  const sel = state.selection.main;
  if (!sel.empty) return false;
  const line = state.doc.lineAt(sel.head);
  if (line.text.trim() !== "") return false;
  let node = syntaxTree(state).resolveInner(sel.head, -1);
  while (node) {
    if (node.name === "FencedCode" || node.name === "CodeBlock") return false;
    if (!node.parent) break;
    node = node.parent;
  }
  return true;
}

/** 包好的文本。压成两空格缩进的漂亮格式 —— 粘进来的多半是一行压扁的 */
export function fenced(text: string): string {
  let body = text.trim();
  try {
    body = JSON.stringify(JSON.parse(body), null, 2);
  } catch {
    /* looksLikeJsonBlock 已经保证能 parse，这里只是兜底 */
  }
  return "```json\n" + body + "\n```";
}

export const pasteFence: Extension = EditorView.domEventHandlers({
  paste(event, view) {
    const text = event.clipboardData?.getData("text/plain");
    if (!text || !looksLikeJsonBlock(text) || !atBlankLineOutsideFence(view.state)) return false;
    event.preventDefault();
    const line = view.state.doc.lineAt(view.state.selection.main.head);
    const insert = fenced(text);
    // 整行替换（那一行本来就是空的），光标停在围栏后面
    view.dispatch({
      changes: { from: line.from, to: line.to, insert },
      selection: { anchor: line.from + insert.length },
      userEvent: "input.paste",
    });
    return true;
  },
});
