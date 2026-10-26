/**
 * 测试文件行号旁的 ▶（docs/IDEA.md 第 17 节）：顶层的测试类一个、每个测试方法一个（`模块 [test 类#方法]`，走任务那套）。
 * 跑完换成 ✓ / ✗（`run/test-outcomes.ts`），同 IDEA。⌃⇧R 跑光标所在的那个方法（光标在第一个测试方法上面就跑整个类），⌃⇧D 调试着跑。
 * **点 ▶ 出一个小菜单「运行 / 调试」**（同 IDEA：点了不直接跑，菜单里两项各带键位 —— 菜单是人学键位的地方）。
 * 只认 `src/test/java/` 底下的文件；认测试按注解（`java-tests.ts`），不靠语言服务器 —— jdtls 没起好也能跑
 */
import { EditorView, GutterMarker, ViewPlugin, gutter } from "@codemirror/view";
import { Prec, RangeSet, StateEffect, StateField, type Extension } from "@codemirror/state";
import { moduleOf, testTargets, testTaskName, type TestTarget } from "./java-tests";
import { onOutcomes, outcomeOf } from "../run/test-outcomes";
import { mount, unmount } from "svelte";
import { idKeymap } from "./id-keys";

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
    el.title = `${this.outcome === "passed" ? "上次通过 · " : this.outcome === "failed" ? "上次没过 · " : ""}运行（⌃⇧R）/ 调试（⌃⇧D）${what}`;
    return el;
  }
}

function markers(view: EditorView) {
  const doc = view.state.doc;
  return RangeSet.of(
    view.state.field(targetsField).filter((t) => t.line <= doc.lines).map((t) => new RunMarker(t, outcomeOf(t.cls, t.method)).range(doc.line(t.line).from)),
  );
}

async function run(root: string, module: string, t: TestTarget, debug = false, cover = false) {
  if (debug) {
    // 调试走 debug-ops：先把断点全交给 Rust、没断点时说一句（同 Services 里点虫子）
    const { debugTask } = await import("../debug/debug-ops");
    await debugTask(testTaskName(module, t), root);
    return;
  }
  const { runTask } = await import("../state/runs-ops");
  await runTask(testTaskName(module, t, cover), root);
}

const short = (t: TestTarget) => (t.method ? `${t.cls.slice(t.cls.lastIndexOf(".") + 1)}.${t.method}` : t.cls.slice(t.cls.lastIndexOf(".") + 1));

/** ▶ 的小菜单：右键菜单那个壳（定位、键盘、点外面关、钳进视口都在它里面），挂在 body 上 —— 编辑器在 container-type 的岛里，fixed 不以窗口为参照 */
async function menu(view: EditorView, x: number, y: number, root: string, module: string, t: TestTarget) {
  const { default: ContextMenu } = await import("../shell/ContextMenu.svelte");
  const host = document.createElement("div");
  document.body.append(host);
  let m: ReturnType<typeof mount> | null = null;
  const close = () => {
    if (!m) return;
    void unmount(m);
    m = null;
    host.remove();
    view.focus();
  };
  m = mount(ContextMenu, {
    target: host,
    props: {
      x,
      y,
      // 不印顶上那行名字：两项里都写着了（ui.md 第六条「同一个信息只印一遍」）
      label: "测试",
      items: [
        { label: `运行 '${short(t)}'`, keys: "⌃⇧R", run: () => void run(root, module, t) },
        { label: `调试 '${short(t)}'`, keys: "⌃⇧D", run: () => void run(root, module, t, true) },
        // IDEA 的 Run with Coverage（docs/TAKEOVER.md G15）：跑完行号旁标出哪些行跑到了，底部「覆盖率」工具窗列每个文件
        { label: `带覆盖率运行 '${short(t)}'`, run: () => void run(root, module, t, false, true) },
      ],
      onclose: close,
    },
  });
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
        mousedown(view, line, ev) {
          const n = view.state.doc.lineAt(line.from).number;
          const t = view.state.field(targetsField).find((x) => x.line === n);
          if (!t) return false;
          const r = (ev.target as HTMLElement).getBoundingClientRect();
          ev.preventDefault();
          void menu(view, r.left, r.bottom + 2, root, module, t);
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
      idKeymap([
        {
          id: "run-test",
          preventDefault: true,
          run: (view) => {
            const t = targetAt(view);
            if (t) void run(root, module, t);
            return !!t;
          },
        },
        {
          id: "debug-test",
          preventDefault: true,
          run: (view) => {
            const t = targetAt(view);
            if (t) void run(root, module, t, true);
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
