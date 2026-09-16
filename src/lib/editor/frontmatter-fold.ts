/**
 * 草稿文件头（锚点 frontmatter，M10）在编辑器里默认**折起来**。
 *
 * 为什么折而不是藏：文件里那几行是真的（用 Sublime 打开也看得见），编辑器不该
 * 假装它们不存在；但每次打开草稿先看六行 YAML 也不对 —— 那是元数据，不是笔记。
 * 折叠是 CM6 现成的：一行 `---…` 占位，点开就能看、能改。Obsidian 的 Properties
 * 折起来也是这个样子。
 *
 * 只在**挂载时**折一次，之后人展开了就展开了 —— 这是显示状态，不是文档状态。
 * 光标如果还停在 0（刚打开、没有跳转目标），挪到头下面：新草稿 ⌘N 之后
 * 第一个字得落在正文里，不能落进折叠块。
 */
import { foldEffect } from "@codemirror/language";
import type { EditorView } from "@codemirror/view";
import { splitFrontmatter } from "../state/frontmatter";

/** 有头就折起来、把停在 0 的光标挪到正文起点；返回折了没有 */
export function foldFrontmatter(view: EditorView): boolean {
  const text = view.state.doc.toString();
  const [anchor, body] = splitFrontmatter(text);
  if (!anchor || body === 0) return false;
  // 折的范围：从文件开头到收尾 `---` 的行尾（不含头后面那个空行）。
  // 从 0 起而不是从第一行的 `---` 之后：留着那三个横杠的话 markdown-live 会把它
  // 画成一条分隔线，折叠占位符就掉到第二个视觉行上，头看着像两行
  const closing = text.lastIndexOf("---", body - 1);
  if (closing <= 0) return false;
  const from = 0;
  const to = closing + 3;
  const effects = [foldEffect.of({ from, to })];
  const sel = view.state.selection.main;
  if (sel.empty && sel.head === 0) {
    view.dispatch({ effects, selection: { anchor: Math.min(body, text.length) } });
  } else {
    view.dispatch({ effects });
  }
  return true;
}
