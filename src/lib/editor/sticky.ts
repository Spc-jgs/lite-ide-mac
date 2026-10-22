/**
 * 粘性行（docs/TAKEOVER.md G19，IDEA 的 Sticky Lines、VS Code 的 Sticky Scroll）：滚到一个长方法中间时，它所在的类、方法、if / for / try
 * 的那一行钉在编辑器顶上 —— 不用往回翻也知道自己在哪。点一行跳过去。
 *
 * 靠 CM6 手上的语法树（Lezer），不问语言服务器：Java、TS、Python… 有语法树的都有，jdtls 没起也有。
 * 「哪些算一层」按节点名认（下面的 `SCOPE`），一张表管几种语言 —— 名字是读 `@lezer/java` 的节点表定的，TS / Python 的同名规则正好对上。
 *
 * 纯的部分（给一行，算出要钉哪几行）在 `sticky-scope.ts`、单独测（tests/sticky.test.ts）；画在编辑器上的是这个文件。
 * 设置里能关（`editor.stickyLines`）
 */
import { EditorView, ViewPlugin, type ViewUpdate } from "@codemirror/view";
import { highlightingFor, syntaxTree } from "@codemirror/language";
import { highlightTree } from "@lezer/highlight";
import { settings } from "../state/settings.svelte";
import { stickyLines } from "./sticky-scope";

/** 一行带着语法着色画成 DOM（用编辑器自己的高亮，和正文同色） */
function renderLine(view: EditorView, line: number): HTMLElement {
  const { state } = view;
  const l = state.doc.line(line);
  const el = document.createElement("div");
  el.className = "cm-sticky-line";
  const num = document.createElement("span");
  num.className = "cm-sticky-num";
  num.textContent = String(line);
  el.append(num);
  const code = document.createElement("span");
  code.className = "cm-sticky-code";
  let at = l.from;
  const put = (to: number, cls: string) => {
    if (to <= at) return;
    const s = document.createElement("span");
    if (cls) s.className = cls;
    s.textContent = state.doc.sliceString(at, to);
    code.append(s);
    at = to;
  };
  highlightTree(syntaxTree(state), { style: (tags) => highlightingFor(state, tags) }, (from, to, cls) => {
    put(Math.max(from, l.from), "");
    put(Math.min(to, l.to), cls);
  }, l.from, l.to);
  put(l.to, "");
  el.append(code);
  el.title = `跳到第 ${line} 行`;
  el.addEventListener("mousedown", (e) => {
    e.preventDefault();
    view.dispatch({ selection: { anchor: l.from + (l.text.length - l.text.trimStart().length) }, effects: EditorView.scrollIntoView(l.from, { y: "start" }) });
    view.focus();
  });
  return el;
}

/**
 * 画在编辑器顶上的那一层。读布局（视口顶上是第几行）走 `requestMeasure`：CM6 不许在 update 里读布局（frontend.md）。
 * 滚动单独听 `scroll` —— 视口里的小幅滚动不一定有 `viewportChanged`
 */
export const stickyLinesExt = ViewPlugin.fromClass(
  class {
    view: EditorView;
    box: HTMLElement;
    shown = "";
    off: () => void;
    // 滚动当场算、当场画：滚动事件不在 CM6 的更新里，读布局是允许的；少等一帧，也不靠帧回调（自动化浏览器的后台标签里帧回调一次都不回）
    onScroll = () => this.write(this.read(this.view), this.view);
    constructor(view: EditorView) {
      this.view = view;
      this.box = document.createElement("div");
      this.box.className = "cm-sticky";
      this.box.setAttribute("aria-hidden", "true");
      view.dom.append(this.box);
      view.scrollDOM.addEventListener("scroll", this.onScroll, { passive: true });
      this.off = settings.subscribe(() => this.measure());
      this.measure();
    }
    update(u: ViewUpdate) {
      // 文档、视口、尺寸变了：在 CM6 的更新里不许读布局，交给 requestMeasure（frontend.md）
      if (u.docChanged || u.viewportChanged || u.geometryChanged) this.measure();
    }
    measure() {
      this.view.requestMeasure({ read: (v) => this.read(v), write: (m, v) => this.write(m, v) });
    }
    read(v: EditorView) {
      if (!settings.v.editorStickyLines) return null;
      const box = v.scrollDOM.getBoundingClientRect();
      const lh = v.defaultLineHeight;
      // 视口顶上那一行；钉上去的几行会盖住正文，再往下数那么多行重算一次（钉的行数跟着变时以第二次为准）
      const lineAt = (y: number) => v.state.doc.lineAt(v.lineBlockAtHeight(y - v.documentTop).from).number;
      const first = stickyLines(v.state, lineAt(box.top));
      const lines = first.length ? stickyLines(v.state, lineAt(box.top + first.length * lh)) : first;
      const gutter = v.dom.querySelector(".cm-gutters")?.getBoundingClientRect().width ?? 0;
      // 贴着滚动区的上沿，不是编辑器的上沿：查找面板（⌘F）开着时它在滚动区上面，贴编辑器上沿就把面板盖住了
      const dom = v.dom.getBoundingClientRect();
      return { lines, gutter, left: box.left - dom.left, top: box.top - dom.top };
    }
    write(m: { lines: number[]; gutter: number; left: number; top: number } | null, v: EditorView) {
      const lines = m?.lines ?? [];
      if (m) {
        this.box.style.left = `${m.left}px`;
        this.box.style.top = `${m.top}px`;
        this.box.style.setProperty("--sticky-gutter", `${m.gutter}px`);
      }
      const key = lines.join(",");
      if (key === this.shown) return;
      this.shown = key;
      this.box.replaceChildren(...lines.map((n) => renderLine(v, n)));
    }
    destroy() {
      this.view.scrollDOM.removeEventListener("scroll", this.onScroll);
      this.off();
      this.box.remove();
    }
  },
);

export const stickyTheme = EditorView.baseTheme({
  ".cm-sticky": { position: "absolute", top: "0", right: "0", zIndex: "3", pointerEvents: "auto", background: "var(--content-solid)", boxShadow: "0 1px 0 var(--border-soft)" },
  ".cm-sticky:empty": { display: "none" },
  ".cm-sticky-line": { display: "flex", whiteSpace: "pre", fontFamily: "var(--code-font)", fontSize: "var(--editor-font-size)", lineHeight: "1.5", cursor: "pointer", overflow: "hidden" },
  ".cm-sticky-line:hover": { background: "var(--hover)" },
  ".cm-sticky-num": { flex: "none", width: "var(--sticky-gutter, 48px)", paddingRight: "12px", textAlign: "right", color: "var(--text-faint)", boxSizing: "border-box" },
  ".cm-sticky-code": { overflow: "hidden", textOverflow: "ellipsis" },
});
