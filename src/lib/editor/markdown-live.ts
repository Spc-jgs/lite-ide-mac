/**
 * Obsidian 式的 Markdown live preview。
 *
 * 核心行为：**光标所在的行显示原始语法，其余行渲染**。
 * 这样既能所见即所得，又随时能看到并编辑真正的标记 ——
 * 而磁盘上永远是纯 .md，没有任何私有格式（PLAN 里定的红线）。
 *
 * 实现方式是 CodeMirror 的 decoration，不是把文档转成 HTML：
 * 文档模型自始至终是那份 Markdown 源码，渲染只是显示层的事。
 */

import { syntaxTree } from "@codemirror/language";
import { type Extension, type Range } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";

/** 把标记字符藏起来（`**`、`#`、`` ` `` 之类） */
const hide = Decoration.replace({});

/** 无序列表的 `-` / `*` 换成圆点 */
class BulletWidget extends WidgetType {
  override toDOM(): HTMLElement {
    const s = document.createElement("span");
    s.className = "cm-md-bullet";
    s.textContent = "•";
    return s;
  }
  override eq(): boolean {
    return true;
  }
}

/** `---` 分隔线 */
class RuleWidget extends WidgetType {
  override toDOM(): HTMLElement {
    const s = document.createElement("span");
    s.className = "cm-md-rule";
    return s;
  }
  override eq(): boolean {
    return true;
  }
}

const line = {
  h1: Decoration.line({ class: "cm-md-h1" }),
  h2: Decoration.line({ class: "cm-md-h2" }),
  h3: Decoration.line({ class: "cm-md-h3" }),
  h4: Decoration.line({ class: "cm-md-h4" }),
  quote: Decoration.line({ class: "cm-md-quote" }),
  code: Decoration.line({ class: "cm-md-codeblock" }),
};

const mark = {
  strong: Decoration.mark({ class: "cm-md-strong" }),
  em: Decoration.mark({ class: "cm-md-em" }),
  strike: Decoration.mark({ class: "cm-md-strike" }),
  code: Decoration.mark({ class: "cm-md-code" }),
  link: Decoration.mark({ class: "cm-md-link" }),
};

const HEADING_LINE: Record<string, Decoration> = {
  ATXHeading1: line.h1,
  ATXHeading2: line.h2,
  ATXHeading3: line.h3,
  ATXHeading4: line.h4,
  ATXHeading5: line.h4,
  ATXHeading6: line.h4,
};

const INLINE_MARK: Record<string, Decoration> = {
  StrongEmphasis: mark.strong,
  Emphasis: mark.em,
  Strikethrough: mark.strike,
  InlineCode: mark.code,
};

function build(view: EditorView): DecorationSet {
  const { state } = view;
  const decos: Range<Decoration>[] = [];

  // 光标（或选区）覆盖到的行要显示源码 —— 这是整个交互的关键
  const raw = new Set<number>();
  for (const r of state.selection.ranges) {
    const a = state.doc.lineAt(r.from).number;
    const b = state.doc.lineAt(r.to).number;
    for (let n = a; n <= b; n++) raw.add(n);
  }
  const isRaw = (pos: number) => raw.has(state.doc.lineAt(pos).number);

  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const name = node.name;

        // ── 整行级
        const lineDeco = HEADING_LINE[name];
        if (lineDeco) {
          decos.push(lineDeco.range(state.doc.lineAt(node.from).from));
          return;
        }
        // 围栏代码块：整块加底色。围栏行本身也留着 ——
        // 那三个反引号是块的边界，藏了反而看不出范围
        if (name === "FencedCode" || name === "CodeBlock") {
          const a = state.doc.lineAt(node.from).number;
          const b = state.doc.lineAt(node.to).number;
          for (let n = a; n <= b; n++) {
            decos.push(line.code.range(state.doc.line(n).from));
          }
          return;
        }
        if (name === "Blockquote") {
          const a = state.doc.lineAt(node.from).number;
          const b = state.doc.lineAt(node.to).number;
          for (let n = a; n <= b; n++) {
            decos.push(line.quote.range(state.doc.line(n).from));
          }
          return;
        }

        // 以下都只在「非光标行」才改写外观
        if (isRaw(node.from)) return;

        // ── 需要藏起来的标记
        if (
          name === "HeaderMark" ||
          name === "EmphasisMark" ||
          name === "CodeMark" ||
          name === "StrikethroughMark" ||
          name === "QuoteMark"
        ) {
          // 藏掉标记本身，顺带把它后面那个空格也吃掉（`# ` / `> `）
          let end = node.to;
          if ((name === "HeaderMark" || name === "QuoteMark") && state.doc.sliceString(end, end + 1) === " ") {
            end += 1;
          }
          if (end > node.from) decos.push(hide.range(node.from, end));
          return;
        }

        if (name === "ListMark") {
          const text = state.doc.sliceString(node.from, node.to);
          // 有序列表保留数字，无序的换成圆点
          if (text === "-" || text === "*" || text === "+") {
            decos.push(
              Decoration.replace({ widget: new BulletWidget() }).range(node.from, node.to),
            );
          }
          return;
        }

        if (name === "HorizontalRule") {
          decos.push(Decoration.replace({ widget: new RuleWidget() }).range(node.from, node.to));
          return;
        }

        // ── 行内样式
        const inline = INLINE_MARK[name];
        if (inline) {
          decos.push(inline.range(node.from, node.to));
          return;
        }

        // ── 链接：显示文字，藏掉 (url)
        if (name === "Link") {
          const text = state.doc.sliceString(node.from, node.to);
          const close = text.indexOf("](");
          if (close > 0) {
            decos.push(hide.range(node.from, node.from + 1)); // [
            decos.push(mark.link.range(node.from + 1, node.from + close));
            decos.push(hide.range(node.from + close, node.to)); // ](...)
          }
        }
      },
    });
  }

  // decoration 必须按位置排序；iterate 是前序遍历，父子节点顺序不保证递增
  return Decoration.set(decos, true);
}

const livePreviewPlugin = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = build(view);
    }
    update(u: ViewUpdate) {
      // 光标一动就要重算 —— 「当前行显示源码」全靠这个
      if (u.docChanged || u.selectionSet || u.viewportChanged) {
        this.decorations = build(u.view);
      }
    }
  },
  { decorations: (v) => v.decorations },
);

const livePreviewTheme = EditorView.theme({
  ".cm-md-h1": { fontSize: "1.7em", fontWeight: "700", lineHeight: "1.6" },
  ".cm-md-h2": { fontSize: "1.42em", fontWeight: "700", lineHeight: "1.6" },
  ".cm-md-h3": { fontSize: "1.2em", fontWeight: "600", lineHeight: "1.6" },
  ".cm-md-h4": { fontSize: "1.08em", fontWeight: "600" },
  ".cm-md-h1, .cm-md-h2, .cm-md-h3, .cm-md-h4": { color: "#dfe1e5" },
  ".cm-md-strong": { fontWeight: "700", color: "#dfe1e5" },
  ".cm-md-em": { fontStyle: "italic" },
  ".cm-md-strike": { textDecoration: "line-through", color: "#6f737b" },
  ".cm-md-code": {
    fontFamily: "var(--code-font)",
    background: "#2b2d30",
    color: "#6aab73",
    padding: "0.1em 0.35em",
    borderRadius: "3px",
  },
  ".cm-md-link": { color: "#548af7", textDecoration: "underline" },
  ".cm-md-quote": {
    borderLeft: "3px solid #393b40",
    paddingLeft: "12px",
    color: "#9da0a8",
    fontStyle: "italic",
  },
  ".cm-md-codeblock": { background: "#26282e", fontFamily: "var(--code-font)" },
  ".cm-md-bullet": { color: "#cf8e6d" },
  ".cm-md-rule": {
    display: "inline-block",
    width: "100%",
    borderTop: "1px solid #393b40",
    verticalAlign: "middle",
  },
});

/** Markdown live preview 扩展集 */
export const markdownLivePreview: Extension = [livePreviewPlugin, livePreviewTheme];
