/**
 * 测试文件行号旁的 ▶（docs/IDEA.md 第 17 节）：顶层的测试类一个、每个测试方法一个，点了跑它（`模块 [test 类#方法]`，走任务那套）。
 * 跑完换成 ✓ / ✗（`run/test-outcomes.ts`），同 IDEA。⌃⇧R 跑光标所在的那个方法（光标在第一个测试方法上面就跑整个类）。
 * 只认 `src/test/java/` 底下的文件；认测试按注解（`java-tests.ts`），不靠语言服务器 —— jdtls 没起好也能跑
 */
import { EditorView, GutterMarker, ViewPlugin, gutter, keymap } from "@codemirror/view";
import { Prec, RangeSet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { moduleOf, testTargets, testTaskName, type TestTarget } from "./java-tests";
import { onOutcomes, outcomeOf } from "../run/test-outcomes";

const refresh = StateEffect.define<null>();

const targetsField = StateField.define<TestTarget[]>({
  create: (s) => testTargets(s.doc.toString()),
  update: (v, tr) => (tr.docChanged ? testTargets(tr.state.doc.toString()) : v),
});

class RunMarker extends GutterMarker {
  constructor(
    readonly t: TestTarget,
    readonly outcome: string | undefined,
  ) {
    super();
  }
  override eq(o: RunMarker) {
    return o.t.cls === this.t.cls && o.t.method === this.t.method && o.outcome === this.outcome;
  }
  override toDOM() {
    const el = document.createElement("span");
    el.className = `cm-test-run ${this.outcome ?? "idle"}`;
    const what = this.t.method ? `${this.t.cls.slice(this.t.cls.lastIndexOf(".") + 1)}.${this.t.method}` : this.t.cls.slice(this.t.cls.lastIndexOf(".") + 1);
    el.title = `${this.outcome === "passed" ? "上次通过 · " : this.outcome === "failed" ? "上次没过 · " : ""}运行 ${what}（⌃⇧R）`;
    return el;
  }
}

function markers(view: EditorView) {
  const doc = view.state.doc;
  return RangeSet.of(
    view.state.field(targetsField).filter((t) => t.line <= doc.lines).map((t) => new RunMarker(t, outcomeOf(t.cls, t.method)).range(doc.line(t.line).from)),
  );
}

async function run(root: string, module: string, t: TestTarget) {
  const { runTask } = await import("../state/runs-ops");
  await runTask(testTaskName(module, t), root);
}

/** 光标所在的那个：往上找最近的测试方法；在第一个测试方法上面（类声明那儿）就是整个类 */
function targetAt(view: EditorView): TestTarget | null {
  const ts = view.state.field(targetsField);
  const line = view.state.doc.lineAt(view.state.selection.main.head).number;
  let hit: TestTarget | null = ts.find((t) => t.method === null) ?? null;
  for (const t of ts) if (t.method && t.line <= line) hit = t;
  return hit;
}

export function javaTestGutter(root: string, path: string): Extension {
  const module = moduleOf(root, path);
  if (module === null) return [];
  return [
    targetsField,
    gutter({
      class: "cm-testGutter",
      markers,
      lineMarkerChange: (u) => u.docChanged || u.transactions.some((tr) => tr.effects.some((e) => e.is(refresh))),
      domEventHandlers: {
        mousedown(view, line) {
          const n = view.state.doc.lineAt(line.from).number;
          const t = view.state.field(targetsField).find((x) => x.line === n);
          if (!t) return false;
          void run(root, module, t);
          return true;
        },
      },
    }),
    // 跑完换图标：结果表变了就让这个编辑器重画一遍行号旁那一列
    ViewPlugin.define((view) => {
      const off = onOutcomes(() => view.dispatch({ effects: refresh.of(null) }));
      return { destroy: off };
    }),
    Prec.high(
      keymap.of([
        {
          key: "Ctrl-Shift-r",
          preventDefault: true,
          run: (view) => {
            const t = targetAt(view);
            if (t) void run(root, module, t);
            return !!t;
          },
        },
      ]),
    ),
    EditorView.baseTheme({
      ".cm-testGutter .cm-gutterElement": { padding: "0 2px", display: "flex", alignItems: "center", justifyContent: "center" },
      // 图标：IDEA 的 run（绿三角）和 error（红圈）本来就带色，当背景图画
      ".cm-test-run": { width: "14px", height: "14px", cursor: "pointer", backgroundSize: "14px 14px", backgroundRepeat: "no-repeat", backgroundPosition: "center" },
      ".cm-test-run.idle": { backgroundImage: "url(/icons/run.svg)" },
      // 通过：现成的单色勾、mask 染绿（不为一个图标再下载一份 IDEA 的 testPassed）
      ".cm-test-run.passed": { background: "var(--git-added)", maskImage: "url(/icons/check.svg)", maskSize: "14px 14px", maskRepeat: "no-repeat", maskPosition: "center", WebkitMaskImage: "url(/icons/check.svg)", WebkitMaskSize: "14px 14px", WebkitMaskRepeat: "no-repeat", WebkitMaskPosition: "center" },
      ".cm-test-run.failed": { backgroundImage: "url(/icons/error.svg)" },
    }),
  ];
}
