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

type View = {
  state: {
    doc: { toString(): string; length: number; line(n: number): { from: number }; lineAt(pos: number): { number: number; from: number } };
    selection: { main: { head: number } };
  };
  dispatch(tr: unknown): void;
  focus(): void;
};

/*
 * ── 按「名字」找控件 ──
 *
 * smoke 原来走 AX 树，按「角色 + 名字」找元素；这里照抄同一套名字，脚本里的「Git 改动」「提交 (」不用改写。
 * 名字的次序和 WebKit 给 AX 的一样：aria-label 优先，其次看得见的文字，再其次 title。
 * 角色按 ARIA 算、不按标签名：`<button role="menuitem">` 是菜单项不是按钮（smoke 文件头记过这个坑）
 */
type Match = "exact" | "prefix" | "contains";
const ROLES: Record<string, string> = {
  button: 'button:not([role]), [role="button"]',
  menuitem: '[role="menuitem"]',
  treeitem: '[role="treeitem"]',
  any: 'button, [role="button"], [role="menuitem"], [role="treeitem"], [role="option"], [role="tab"]',
};
const nameOf = (el: Element) =>
  (el.getAttribute("aria-label") || (el as HTMLElement).innerText?.trim() || el.getAttribute("title") || "").trim();
// 画出来了才算：display:none 的、还没挂上的不算（AX 树里也没有它们）
const shown = (el: Element) => (el as HTMLElement).getClientRects().length > 0;
const hit = (n: string, want: string, m: Match) =>
  m === "exact" ? n === want : m === "prefix" ? n.startsWith(want) : n.includes(want);

function find(name: string, role = "button", match: Match = "exact"): HTMLElement | null {
  const sel = ROLES[role];
  if (!sel) throw new Error(`不认识的角色：${role}`);
  for (const el of document.querySelectorAll<HTMLElement>(sel)) if (shown(el) && hit(nameOf(el), name, match)) return el;
  return null;
}

/** "Mod-Shift-f" / "Escape" / "Shift-F10" → 一次 keydown。Mod 在 macOS 上就是 ⌘ */
function keyEvent(combo: string): KeyboardEvent {
  const parts = combo.split("-");
  let key = parts.pop()!;
  const has = (m: string) => parts.includes(m);
  if (key.length === 1 && has("Shift")) key = key.toUpperCase();
  const code = key.length === 1 && /[a-z]/i.test(key) ? `Key${key.toUpperCase()}` : key;
  return new KeyboardEvent("keydown", {
    key, code, bubbles: true, cancelable: true,
    metaKey: has("Mod") || has("Meta"), shiftKey: has("Shift"), altKey: has("Alt"), ctrlKey: has("Ctrl"),
  });
}

export function installTestHooks() {
  const editor = (): View | null =>
    (document.querySelector(".cm-content") as unknown as { cmTile?: { view?: View } } | null)?.cmTile?.view ?? null;
  const mustEditor = () => {
    const v = editor();
    if (!v) throw new Error("没有活动编辑器");
    return v;
  };
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
      const v = mustEditor();
      const p = v.state.selection.main.head;
      v.dispatch({ changes: { from: p, insert: text }, selection: { anchor: p + text.length }, userEvent: "input.type" });
    },
    /** 整份换成这段文字（⌘A ⌘V 的效果，`input.paste`）。smoke 原来真走剪贴板，那条路要按键 */
    setText: (text: string) => {
      const v = mustEditor();
      v.dispatch({ changes: { from: 0, to: v.state.doc.length, insert: text }, selection: { anchor: text.length }, userEvent: "input.paste" });
    },
    /** 光标放到第 line 行第 col 列（都从 1 起），并让编辑器拿到焦点 —— 后面的 key() 要发给它 */
    caret: (line: number, col: number) => {
      const v = mustEditor();
      v.dispatch({ selection: { anchor: v.state.doc.line(line).from + col - 1 } });
      v.focus();
    },
    /** 选中从 (l1, c1) 到 (l2, c2)，行列都从 1 起（#45 发送到终端读选区） */
    select: (l1: number, c1: number, l2: number, c2: number) => {
      const v = mustEditor();
      const at = (l: number, c: number) => v.state.doc.line(l).from + c - 1;
      v.dispatch({ selection: { anchor: at(l1, c1), head: at(l2, c2) } });
      v.focus();
    },
    /**
     * 当前那个终端里的字，读 xterm 解析好的缓冲区（Terminal.svelte 在测试构建里交出来的），一行一行。
     * **不读 DOM**：测试应用的窗口被别的窗口挡住时页面是 hidden，帧回调停了，xterm 不往 DOM 上画 ——
     * 读 DOM 的第一版在 smoke ㉔ 里间歇红，那时字其实早进了 shell（2026-10-09）
     */
    termText: () => {
      const reg = (window as unknown as { __liteXterm?: Map<number, { buffer: { active: { length: number; getLine(i: number): { translateToString(trim: boolean): string } | undefined } } }> }).__liteXterm;
      const t = terms.activeId === null ? undefined : reg?.get(terms.activeId);
      if (!t) return "";
      const b = t.buffer.active;
      const out: string[] = [];
      for (let i = 0; i < b.length; i++) out.push(b.getLine(i)?.translateToString(true) ?? "");
      return out.join("\n").trimEnd();
    },
    /** 光标此刻在哪：`"行:列"`（都从 1 起）。跳转类的断言读它 */
    where: () => {
      const v = mustEditor();
      const h = v.state.selection.main.head;
      const l = v.state.doc.lineAt(h);
      return `${l.number}:${h - l.from + 1}`;
    },
    /** 按名字点一下（`el.click()`，和 AX 的「按下」一样只触发 click）。没找到返回 false，不抛 —— 脚本拿它当判断 */
    click: (name: string, role = "button", match: Match = "exact") => {
      const el = find(name, role, match);
      el?.click();
      return !!el;
    },
    /** 有没有这么一个看得见的控件 */
    exists: (name: string, role = "any", match: Match = "contains") => !!find(name, role, match),
    /** 界面上看不看得见这段文字：正文、控件名、输入框里的值、占位符都算 */
    has: (text: string) => {
      if (document.body.innerText.includes(text)) return true;
      for (const el of document.querySelectorAll<HTMLElement>("[aria-label], input, textarea"))
        if (shown(el) && [el.getAttribute("aria-label"), (el as HTMLInputElement).value, el.getAttribute("placeholder")].some((s) => s?.includes(text)))
          return true;
      return false;
    },
    /** 点 CSS 选择器命中的第一个（文件树的行按 `data-path` 点最稳：名字会重，路径不会） */
    tap: (selector: string) => {
      const el = document.querySelector<HTMLElement>(selector);
      el?.click();
      return !!el;
    },
    /** 在元素正中开右键菜单（`contextmenu` 事件，带坐标 —— 菜单要按它定位） */
    rightClick: (selector: string) => {
      const el = document.querySelector<HTMLElement>(selector);
      if (!el) return false;
      const r = el.getBoundingClientRect();
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, clientX: r.left + r.width / 2, clientY: r.top + r.height / 2 }));
      return true;
    },
    /**
     * 往焦点所在的元素发一次按键（页面内的 KeyboardEvent，不经过系统）。所以只够得着**网页自己接的键**
     * （keymap.ts 里 owner 是 key / cm6 的）；归菜单的键 AppKit 先吃掉，网页本来就收不到 —— 那些走 `menu` 指令
     */
    key: (combo: string) => {
      (document.activeElement ?? document.body).dispatchEvent(keyEvent(combo));
    },
    /**
     * 往输入框里填字：给了选择器就填那个（先让它拿焦点），没给就填焦点所在的那个（浮层弹出来自己会把焦点放进输入框）。
     * 直接赋 `value` Svelte 收不到（它听 input 事件），所以用原型上的 setter 赋值再补一个 input 事件
     */
    fill: (text: string, selector?: string) => {
      if (selector) document.querySelector<HTMLElement>(selector)?.focus();
      const el = document.activeElement;
      if (!(el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement)) throw new Error(`焦点不在输入框上（在 ${el?.tagName}）`);
      Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value")!.set!.call(el, text);
      el.dispatchEvent(new Event("input", { bubbles: true }));
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
