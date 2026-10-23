/**
 * 书签在编辑器里的那一半（docs/TAKEOVER.md G18，IDEA 的 Bookmarks）：F3 在光标那行打 / 取消，行号旁一个黄色的书签。
 * 表在 `state/bookmarks.ts`（存 localStorage、工具窗也读它）。
 *
 * 跟着行走：同断点（`java-debug-gutter.ts`），存成 RangeSet 挂在每行开头，改动时 CM6 替我们映射；映射完连同那一行此刻的字写回表（去抖 400ms）。
 * **不盖住行号**：断点是画在行号上、盖住数字的；书签用 `gutterLineClass` 给那一格加个类，CSS 在数字左边画图标 —— 同一行既有断点又有书签两样都看得见。
 * 任何文件都能打（IDEA 也是），不靠语言服务器
 */
import { EditorView, GutterMarker, ViewPlugin, gutterLineClass } from "@codemirror/view";
import { RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { marksOf, onBookmarks, setMarks, snippet, toggle, type Mark } from "../state/bookmarks";
import { idKeymap } from "./id-keys";

class BmMarker extends GutterMarker {
  override elementClass = "cm-bm-line";
}
const MARK = new BmMarker();

const reset = StateEffect.define<Mark[]>();

const setOf = (state: EditorState, list: Mark[]) =>
  RangeSet.of(
    list.filter((m) => m.line <= state.doc.lines).map((m) => MARK.range(state.doc.line(m.line).from)),
    true,
  );

/** 标记此刻在哪几行、那一行的字 */
function marksIn(state: EditorState, set: RangeSet<BmMarker>): Mark[] {
  const out: Mark[] = [];
  for (const it = set.iter(); it.value; it.next()) {
    const l = state.doc.lineAt(it.from);
    out.push({ line: l.number, text: snippet(l.text) });
  }
  return out;
}

const sameLines = (a: Mark[], b: Mark[]) => a.length === b.length && a.every((m, i) => m.line === b[i].line && m.text === b[i].text);

/** `root`：项目根（没有就是 null，记在草稿本那一格）；`path`：这个编辑器开的文件 */
export function bookmarks(root: string | null, path: string): Extension {
  const field = StateField.define<RangeSet<BmMarker>>({
    create: (s) => setOf(s, marksOf(root, path)),
    update(v, tr) {
      for (const e of tr.effects) if (e.is(reset)) return setOf(tr.state, e.value);
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => gutterLineClass.from(f),
  });

  // 改了正文：映射完的行号 + 那一行的字写回表（去抖：连着打字不必每个键都写一次 localStorage、通知一遍工具窗）
  const writeback = ViewPlugin.fromClass(
    class {
      timer: ReturnType<typeof setTimeout> | undefined;
      off: () => void;
      // 不写成 `constructor(readonly view)`：要编译的 TS 语法，状态层测试在裸 node 里剥类型跑（frontend.md）
      view: EditorView;
      constructor(view: EditorView) {
        this.view = view;
        // 别处改了表（工具窗里删了一条、另一个分屏里打了一个）：照表重画
        this.off = onBookmarks((r, p) => {
          if (r !== (root ?? "") || p !== path) return;
          const want = marksOf(root, path);
          if (!sameLines(want, marksIn(this.view.state, this.view.state.field(field)))) this.view.dispatch({ effects: reset.of(want) });
        });
      }
      update(u: { docChanged: boolean; state: EditorState }) {
        if (!u.docChanged) return;
        clearTimeout(this.timer);
        this.timer = setTimeout(() => setMarks(root, path, marksIn(this.view.state, this.view.state.field(field))), 400);
      }
      destroy() {
        // 关标签前还没写回的那一下也要写：不然最后几个键带来的挪动丢了。
        // 但表里这个路径已经空了的不写：文件刚改名 / 挪走，书签已经跟到新路径（`bookmarks.rename`），这个旧编辑器写回去就在旧路径上复活一份
        if (this.timer !== undefined && marksOf(root, path).length) {
          clearTimeout(this.timer);
          setMarks(root, path, marksIn(this.view.state, this.view.state.field(field)));
        }
        this.off();
      }
    },
  );

  const run = (v: EditorView) => {
    const l = v.state.doc.lineAt(v.state.selection.main.head);
    // 先把映射过的写回去，再按行号切换 —— 不然表里还是挪动前的行号
    setMarks(root, path, marksIn(v.state, v.state.field(field)));
    // 不说话：行号旁那个黄书签出来 / 没了就是回答（IDEA 也不弹提示）
    toggle(root, path, l.number, l.text);
    return true;
  };

  return [
    field,
    writeback,
    idKeymap([{ id: "bookmark-toggle", preventDefault: true, run }]),
    EditorView.baseTheme({
      ".cm-lineNumbers .cm-gutterElement.cm-bm-line": { position: "relative" },
      // 数字左边、行号格子的空白里：14×14 的上游图标缩到 11，不和数字抢地方
      ".cm-lineNumbers .cm-gutterElement.cm-bm-line::before": {
        content: '""',
        position: "absolute",
        left: "1px",
        top: "50%",
        width: "11px",
        height: "11px",
        transform: "translateY(-50%)",
        background: "url(/icons/bookmark.svg) center / contain no-repeat",
      },
    }),
  ];
}
