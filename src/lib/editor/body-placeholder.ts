/**
 * 「正文为空」时的一行灰字（空草稿里放快捷键提示，启动直接落在草稿里之后
 * 空态卡片上那两条就搬到这儿）。
 *
 * 不用 CM6 自带的 `placeholder()`：它只在**整个文档**为空时出现，而草稿从来
 * 不是空的 —— 头上有六行锚点 frontmatter（折着）。这里判的是「头之后没有字」，
 * 灰字挂在正文起点那一行上，敲第一个字就没了。样式借 CM 自己的 `.cm-placeholder`，
 * 和它的 `placeholder()` 长一样。
 */
import { Decoration, EditorView, ViewPlugin, WidgetType, type DecorationSet, type ViewUpdate } from "@codemirror/view";
import { splitFrontmatter } from "../state/frontmatter";

class Hint extends WidgetType {
  constructor(readonly text: string) {
    super();
  }
  override eq(o: Hint) {
    return o.text === this.text;
  }
  toDOM() {
    const el = document.createElement("span");
    el.className = "cm-placeholder";
    el.setAttribute("aria-hidden", "true");
    el.textContent = this.text;
    return el;
  }
  override ignoreEvent() {
    return false;
  }
}

function build(view: EditorView, text: string): DecorationSet {
  const doc = view.state.doc.toString();
  const [, body] = splitFrontmatter(doc);
  if (doc.slice(body).trim() !== "") return Decoration.none;
  // 正文起点可能落在文档末尾（头后面什么都没有）；widget 挂在那个位置，side 1 = 光标在它前面
  const pos = Math.min(body, doc.length);
  return Decoration.set([Decoration.widget({ widget: new Hint(text), side: 1 }).range(pos)]);
}

export function bodyPlaceholder(text: string) {
  return ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view, text);
      }
      update(u: ViewUpdate) {
        if (u.docChanged) this.decorations = build(u.view, text);
      }
    },
    { decorations: (v) => v.decorations },
  );
}
