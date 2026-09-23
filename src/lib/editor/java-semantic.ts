/**
 * Java 的声明上色：方法 / 构造器声明蓝、字段声明紫、static 字段紫斜体（照 IDEA）。
 *
 * # 为什么是 ViewPlugin 而不是语法标签
 *
 * `@lezer/java` 把类名、方法名、字段、参数、局部变量的声明全标成一个 `Definition`。
 * 按父节点分开本该是一条路径规则的事，但 `@lezer/highlight` 的规则合并有 bug，
 * 带路径的规则压不过语法包那条裸 `Definition` —— 来龙去脉在 `java-highlight.ts` 头上。
 * 于是在这里自己走树、按父节点判，画成 mark。
 *
 * 类名从 `HIGHLIGHT_SPEC` 那份 `HighlightStyle` 里取（`style([tag])`），色值仍只有一份。
 *
 * # 为什么是 `Prec.high`
 *
 * CM6 约定优先级高的 decoration 生成**内层** DOM。语法着色那层已经给这些词套了一个
 * 「正文色」的 span；我们的 span 在它里面，自己的 color 盖住继承下来的 —— 不用去比
 * 两条 CSS 规则谁在样式表里更靠后（同一张表里，正文色那条恰好写在后面，会赢）。
 *
 * # 只走可视区域
 *
 * 声明只看自己和父节点，不需要全文信息，所以只遍历 `visibleRanges`。
 */
import { ViewPlugin, Decoration, type DecorationSet, type EditorView, type ViewUpdate } from "@codemirror/view";
import { syntaxTree } from "@codemirror/language";
import { Prec, RangeSetBuilder } from "@codemirror/state";
import { tags as t } from "@lezer/highlight";
import { ideaHighlightStyle } from "./theme-idea-dark";
import { javaMarks, type MarkKind } from "./java-scope";

const TAG: Record<MarkKind, Parameters<typeof ideaHighlightStyle.style>[0][number]> = {
  fnDecl: t.definition(t.function(t.variableName)),
  field: t.definition(t.propertyName),
  staticField: t.constant(t.propertyName),
};
const DECO = Object.fromEntries(
  (Object.keys(TAG) as MarkKind[]).map((k) => [k, Decoration.mark({ class: ideaHighlightStyle.style([TAG[k]]) ?? "" })]),
) as Record<MarkKind, Decoration>;

function build(view: EditorView): DecorationSet {
  const b = new RangeSetBuilder<Decoration>();
  const tree = syntaxTree(view.state);
  const text = (a: number, z: number) => view.state.sliceDoc(a, z);
  for (const r of view.visibleRanges) {
    for (const m of javaMarks(tree, text, r.from, r.to)) b.add(m.from, m.to, DECO[m.kind]);
  }
  return b.finish();
}

export const javaSemantic = Prec.high(
  ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view);
      }
      update(u: ViewUpdate) {
        if (u.docChanged || u.viewportChanged || syntaxTree(u.state) !== syntaxTree(u.startState)) {
          this.decorations = build(u.view);
        }
      }
    },
    { decorations: (v) => v.decorations },
  ),
);
