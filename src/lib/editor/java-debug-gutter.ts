/**
 * 断点和执行点（docs/IDEA.md 第 18 节），照 IDEA 新 UI 的默认样子：**断点画在行号上**（点行号下 / 取消，红点盖住那个数字），
 * 停下来时那一整行标成执行点（停在这儿的那一帧亮、选了下面的帧淡一档）。⌘F8 在光标那行下 / 取消。
 *
 * 断点跟着行走：存成 RangeSet 挂在每行开头，改动时 CM6 替我们映射（上面插了三行，断点跟着下移三行）；映射完写回 `debug/breakpoints.ts`
 * （去抖 400ms —— 那边每次变了都要过一次 IPC 告诉 Rust）。
 *
 * 不靠语言服务器：下在哪一行就是哪一行。「这一行能不能停」（空行、注释）要等调试器挂上、按编译出来的行号表才知道，这一版不画「没对上」的样子。
 */
import { Decoration, EditorView, GutterMarker, ViewPlugin, gutterLineClass, keymap, lineNumberMarkers, lineNumbers, type DecorationSet } from "@codemirror/view";
import { Prec, RangeSet, StateEffect, StateField, type EditorState, type Extension } from "@codemirror/state";
import { linesOf, onBreakpoints, setLines, toggle } from "../debug/breakpoints";
import { execPoint, onExecPoint } from "../debug/exec-point";

class BpMarker extends GutterMarker {
  override eq() {
    return true;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = "cm-bp";
    el.title = "断点（点一下取消，⌘F8）";
    return el;
  }
}
const BP = new BpMarker();

/** 换成这些行（从 1 数）：外面（另一个分屏、调试开始前同步）改了表 */
const resetBps = StateEffect.define<number[]>();

function bpSet(state: EditorState, lines: number[]): RangeSet<GutterMarker> {
  const doc = state.doc;
  return RangeSet.of(
    lines.filter((l) => l <= doc.lines).map((l) => BP.range(doc.line(l).from)),
    true,
  );
}

function linesIn(state: EditorState, set: RangeSet<GutterMarker>): number[] {
  const out: number[] = [];
  const it = set.iter();
  for (; it.value; it.next()) out.push(state.doc.lineAt(it.from).number);
  return [...new Set(out)];
}

const execEffect = StateEffect.define<{ line: number; top: boolean } | null>();

const execLine = Decoration.line({ class: "cm-exec-line" });
const execLineUnder = Decoration.line({ class: "cm-exec-line under" });
class ExecGutter extends GutterMarker {
  override elementClass = "cm-exec-gutter";
}

export function javaDebugGutter(root: string, path: string): Extension {
  const bps = StateField.define<RangeSet<GutterMarker>>({
    create: (s) => bpSet(s, linesOf(root, path)),
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
      },
    }),
    ViewPlugin.define((view) => {
      let timer: ReturnType<typeof setTimeout> | null = null;
      const flush = () => {
        timer = null;
        setLines(root, path, linesIn(view.state, view.state.field(bps)));
      };
      // 表变了（点了行号、⌘F8、另一个分屏里改的）：照表重画。自己映射完写回去的那一次，表和画面本来就一样，重画一次无妨
      const offBps = onBreakpoints((p) => {
        if (p !== path) return;
        const want = linesOf(root, path);
        const have = linesIn(view.state, view.state.field(bps));
        if (want.length !== have.length || want.some((l, i) => l !== have[i])) view.dispatch({ effects: resetBps.of(want) });
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
      // 执行点：整行淡蓝（IDEA 的 Execution point），选了下面的帧时更淡
      ".cm-exec-line": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent) !important" },
      ".cm-exec-line.under": { backgroundColor: "color-mix(in srgb, var(--accent) 14%, transparent) !important" },
      ".cm-gutterElement.cm-exec-gutter": { backgroundColor: "color-mix(in srgb, var(--accent) 28%, transparent)", color: "var(--text)" },
    }),
  ];
}
