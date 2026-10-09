/**
 * 测试通道的前端那半（Rust 那半是 `src-tauri/src/testbridge.rs`，协议写在它头上）。
 *
 * 把几个状态对象和几样「测试要读要做的事」挂在 `window.__lite` 上，测试用 `eval` 指令在页面里调。
 * **只在 `VITE_TEST_BRIDGE=1` 的测试构建里存在**：main.ts 里是 `if (import.meta.env.VITE_TEST_BRIDGE === "1") await import(...)`，
 * 正式构建里条件是常量假，整句在语法层面就没了 —— 和浏览器桩（mock-ipc）同一个保证，不靠 tree-shaking
 * （frontend.md「别靠 tree-shaking 保证开发桩不进产物」）。
 */
import { tabs } from "../state/tabs.svelte";
import { project } from "../state/project.svelte";
import { settings } from "../state/settings.svelte";
import { tabflow } from "../state/tabflow.svelte";
import { docs } from "../state/docs.svelte";
import { nav } from "../state/nav.svelte";
import { terms } from "../state/terms.svelte";

/*
 * **模块级的副作用，给 CI 当哨兵**（ci.yml「开发桩不许进产物」）：正式产物里出现这个串 = 测试钩子漏进去了。
 * 必须是模块级的 —— 函数体里的串会随函数一起被 shake 掉，那种哨兵永远不响（mock-ipc 那次的教训）
 */
(globalThis as Record<string, unknown>).__liteHooks = "lite-test-hooks-loaded";

type View = { state: { doc: { toString(): string }; selection: { main: { head: number } } }; dispatch(tr: unknown): void };

export function installTestHooks() {
  const editor = (): View | null =>
    (document.querySelector(".cm-content") as unknown as { cmTile?: { view?: View } } | null)?.cmTile?.view ?? null;
  (window as unknown as Record<string, unknown>).__lite = {
    tabs,
    project,
    settings,
    tabflow,
    docs,
    nav,
    terms,
    /** 活动编辑器里的全文；没有编辑器是 null */
    text: () => editor()?.state.doc.toString() ?? null,
    /**
     * 在活动编辑器的光标处插入文字。走 CM6 事务（`userEvent: "input.type"`，和打字一样触发脏标记、自动保存），
     * 不是按键 —— 按键要发给「最前面的应用」，那正是测试通道要躲开的
     */
    type: (text: string) => {
      const v = editor();
      if (!v) throw new Error("没有活动编辑器");
      const p = v.state.selection.main.head;
      v.dispatch({ changes: { from: p, insert: text }, selection: { anchor: p + text.length }, userEvent: "input.type" });
    },
    /** 焦点在哪：标签名 + class，没有焦点（落在 body 上）就是 "body" */
    focus: () => {
      const a = document.activeElement;
      if (!a || a === document.body) return "body";
      return `${a.tagName.toLowerCase()}${a.className && typeof a.className === "string" ? "." + a.className.trim().split(/\s+/).join(".") : ""}`;
    },
    /** :root 上某个 CSS 变量此刻的值 */
    css: (name: string) => getComputedStyle(document.documentElement).getPropertyValue(name).trim(),
  };
}
