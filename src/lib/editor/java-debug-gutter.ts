/**
 * 断点和执行点（docs/IDEA.md 第 18 节），照 IDEA 新 UI 的默认样子：**断点画在行号上**（点行号下 / 取消，红点盖住那个数字），
 * 停下来时那一整行标成执行点（停在这儿的那一帧亮、选了下面的帧淡一档）。⌘F8 在光标那行下 / 取消。
 *
 * 断点跟着行走：存成 RangeSet 挂在每行开头，改动时 CM6 替我们映射（上面插了三行，断点跟着下移三行）；映射完写回 `debug/breakpoints.ts`
 * （去抖 400ms —— 那边每次变了都要过一次 IPC 告诉 Rust）。
 *
 * 不靠语言服务器：下在哪一行就是哪一行。「这一行能不能停」（空行、注释）要等调试器挂上、按编译出来的行号表才知道，这一版不画「没对上」的样子。
 *
 * **条件断点**（IDEA 的 Condition）：右键行号上的断点，填一个 Java 表达式，为真才停（`orderId == 42`）。条件挂在标记上、跟着行一起走；
 * 有条件的红点里多一个「?」（IDEA 是同一个意思的小记号）。条件由 java-debug 在目标 JVM 里算。
 */
import { Decoration, EditorView, GutterMarker, ViewPlugin, gutterLineClass, keymap, lineNumberMarkers, lineNumbers, type DecorationSet } from "@codemirror/view";
import { Prec, RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { bpsOf, onBreakpoints, setBps, setCond, toggle, type Bp } from "../debug/breakpoints";
import { imeKey } from "../shell/ime";
import { execPoint, onExecPoint } from "../debug/exec-point";

class BpMarker extends GutterMarker {
  // 不写成 `constructor(readonly cond)`：那是要编译的 TS 语法，状态层测试在裸 node 里剥类型跑，剥不掉它
  readonly cond: string;
  constructor(cond: string) {
    super();
    this.cond = cond;
  }
  override eq(o: BpMarker) {
    return o.cond === this.cond;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = this.cond ? "cm-bp cond" : "cm-bp";
    el.title = this.cond ? `条件断点：${this.cond} 为真才停（右键改条件，点一下取消）` : "断点（点一下取消，⌘F8；右键加条件）";
    return el;
  }
}

/** 换成这些断点：外面（另一个分屏、调试开始前同步、改了条件）改了表 */
const resetBps = StateEffect.define<Bp[]>();

function bpSet(state: EditorState, list: Bp[]): RangeSet<BpMarker> {
  const doc = state.doc;
  return RangeSet.of(
    list.filter((b) => b.line <= doc.lines).map((b) => new BpMarker(b.cond ?? "").range(doc.line(b.line).from)),
    true,
  );
}

/** 标记此刻在哪几行、各带什么条件（同一行两个的话后面的算数，`setBps` 会去重） */
function bpsIn(state: EditorState, set: RangeSet<BpMarker>): Bp[] {
  const out: Bp[] = [];
  for (const it = set.iter(); it.value; it.next()) {
    const line = state.doc.lineAt(it.from).number;
    out.push(it.value.cond ? { line, cond: it.value.cond } : { line });
  }
  return out;
}

const same = (a: Bp[], b: Bp[]) => a.length === b.length && a.every((x, i) => x.line === b[i].line && (x.cond ?? "") === (b[i].cond ?? ""));

/**
 * 改条件的小框：挂在鼠标那儿（body 上，按屏幕坐标摆 —— frontend.md「container-type 的元素里 fixed 不以窗口为参照」）。
 * 回车确定、Esc 不改（`done(null)`）；清空就是普通断点
 */
function editCond(x: number, y: number, cur: string, done: (cond: string | null) => void) {
  const box = document.createElement("div");
  box.className = "popup cm-bp-cond";
  box.style.cssText = `position:fixed;left:${Math.min(x, innerWidth - 380)}px;top:${y + 8}px;z-index:70;width:360px;padding:10px 12px`;
  const title = document.createElement("div");
  title.className = "t";
  title.textContent = "条件（Java 表达式，为真才停）";
  const input = document.createElement("input");
  input.className = "in";
  input.value = cur;
  input.placeholder = "比如 orderId == 42 或 name.startsWith(\"VIP\")";
  input.spellcheck = false;
  const hint = document.createElement("div");
  hint.className = "h";
  hint.textContent = "↵ 确定 · Esc 不改 · 清空就是普通断点";
  box.append(title, input, hint);
  document.body.append(box);
  let closed = false;
  const close = (commit: boolean) => {
    if (closed) return;
    closed = true;
    box.remove();
    document.removeEventListener("mousedown", outside, true);
    done(commit ? input.value.trim() : null);
  };
  const outside = (e: MouseEvent) => {
    if (!box.contains(e.target as Node)) close(true);
  };
  input.addEventListener("keydown", (e) => {
    if (imeKey(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      close(true);
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      close(false);
    }
  });
  // 点外面 = 确定（同 IDEA 的断点属性框：点开就生效）
  setTimeout(() => document.addEventListener("mousedown", outside, true));
  input.focus();
  input.select();
}

const execEffect = StateEffect.define<{ line: number; top: boolean } | null>();

const execLine = Decoration.line({ class: "cm-exec-line" });
const execLineUnder = Decoration.line({ class: "cm-exec-line under" });
class ExecGutter extends GutterMarker {
  override elementClass = "cm-exec-gutter";
}

export function javaDebugGutter(root: string, path: string): Extension {
  const bps = StateField.define<RangeSet<BpMarker>>({
    create: (s) => bpSet(s, bpsOf(root, path)),
    update(v, tr) {
      for (const e of tr.effects) if (e.is(resetBps)) return bpSet(tr.state, e.value);
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => lineNumberMarkers.from(f),
  });

  const exec = StateField.define<DecorationSet>({
    create(s) {
      const p = execPoint();
      return p && p.path === path && p.line <= s.doc.lines ? Decoration.set((p.top ? execLine : execLineUnder).range(s.doc.line(p.line).from)) : Decoration.none;
    },
    update(v, tr) {
      for (const e of tr.effects)
        if (e.is(execEffect)) {
          const p = e.value;
          return p && p.line <= tr.state.doc.lines ? Decoration.set((p.top ? execLine : execLineUnder).range(tr.state.doc.line(p.line).from)) : Decoration.none;
        }
      return tr.docChanged ? v.map(tr.changes) : v;
    },
    provide: (f) => [
      EditorView.decorations.from(f),
      // 行号那一格也染上（同 IDEA：执行点那一行连行号底下都是蓝的）
      gutterLineClass.from(f, (d) => {
        const marks: ReturnType<GutterMarker["range"]>[] = [];
        const it = d.iter();
        for (; it.value; it.next()) marks.push(new ExecGutter().range(it.from));
        return RangeSet.of(marks);
      }),
    ],
  });

  const atCursor = (view: EditorView) => {
    toggle(root, path, view.state.doc.lineAt(view.state.selection.main.head).number);
    return true;
  };

  return [
    bps,
    exec,
    lineNumbers({
      domEventHandlers: {
        // 点行号：下 / 取消断点。按下就算（不等抬起），同 IDEA；拖选正文从行号上开始的那种不受影响 —— 那是在正文里按下的
        mousedown(view, line, ev) {
          if ((ev as MouseEvent).button !== 0) return false;
          toggle(root, path, view.state.doc.lineAt(line.from).number);
          return true;
        },
        // 右键：改这一行断点的条件（没有断点就新下一个带条件的）。同 IDEA：右键断点出属性框
        contextmenu(view, line, ev) {
          const n = view.state.doc.lineAt(line.from).number;
          const cur = bpsOf(root, path).find((b) => b.line === n);
          const e = ev as MouseEvent;
          e.preventDefault();
          editCond(e.clientX, e.clientY, cur?.cond ?? "", (cond) => {
            // 没断点的行、又没填条件：什么都不做（右键一下不该凭空多一个断点）
            if (cond !== null && (cur || cond)) setCond(root, path, n, cond);
            // 焦点还给编辑器：小框一摘，焦点落在 body 上，接着打的字没地方去（浏览器里验时撞见的）
            view.focus();
          });
          return true;
        },
      },
    }),
    ViewPlugin.define((view) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      const flush = () => {
        timer = null;
        setBps(root, path, bpsIn(view.state, view.state.field(bps)));
      };
      // 表变了（点了行号、⌘F8、另一个分屏里改的）：照表重画。自己映射完写回去的那一次，表和画面本来就一样，重画一次无妨
      const offBps = onBreakpoints((p) => {
        if (p !== path) return;
        const want = bpsOf(root, path);
        if (!same(want, bpsIn(view.state, view.state.field(bps)))) view.dispatch({ effects: resetBps.of(want) });
      });
      const offExec = onExecPoint(() => {
        const p = execPoint();
        view.dispatch({ effects: execEffect.of(p && p.path === path ? { line: p.line, top: p.top } : null) });
      });
      return {
        update(u) {
          if (!u.docChanged || u.startState.field(bps) === u.state.field(bps)) return;
          if (timer) clearTimeout(timer);
          timer = setTimeout(flush, 400);
        },
        destroy() {
          if (timer) {
            clearTimeout(timer);
            flush();
          }
          offBps();
          offExec();
        },
      };
    }),
    Prec.high(keymap.of([{ key: "Mod-F8", preventDefault: true, run: atCursor }])),
    EditorView.baseTheme({
      // 行号格里的红点：14 的图标居中，盖住数字（IDEA 新 UI 的「断点画在行号上」）
      ".cm-lineNumbers .cm-gutterElement": { cursor: "pointer" },
      ".cm-bp": { display: "inline-block", width: "14px", height: "14px", verticalAlign: "middle", background: "url(/icons/breakpoint.svg) center / 14px 14px no-repeat" },
      // 条件断点：红点里一个白色的「?」
      ".cm-bp.cond": { position: "relative" },
      ".cm-bp.cond::after": { content: '"?"', position: "absolute", inset: "0", textAlign: "center", lineHeight: "14px", fontSize: "10px", fontWeight: "700", color: "#fff", fontFamily: "var(--ui-font)" },
      // 执行点：整行淡蓝（IDEA 的 Execution point），选了下面的帧时更淡
      ".cm-exec-line": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent) !important" },
      ".cm-exec-line.under": { backgroundColor: "color-mix(in srgb, var(--accent) 14%, transparent) !important" },
      ".cm-gutterElement.cm-exec-gutter": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent)", color: "var(--text)" },
    }),
  ];
}
