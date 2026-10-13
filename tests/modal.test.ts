/**
 * 对话框开着时键盘怎么办（`src/lib/shell/dialog-focus.ts` 的 keyVerdict，背景在 `modal.ts` 头上）：只放对话框自己要的那几个键，
 * 别的（尤其是快捷键）挡掉 —— 原来 ⇧⇧ / ⌘P / ⌃Tab 在压暗的对话框底下照样生效（code review 2026-10-13）。
 * 外加 `takeFocus` 换人时把焦点放回默认按钮（「关闭全部」逐个问，焦点停在「不保存」上那条）。
 */
import { keyVerdict, nudgesFor, takeFocus } from "../src/lib/shell/dialog-focus.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const k = (key: string, m: { meta?: boolean; ctrl?: boolean; alt?: boolean } = {}) => ({
  key, metaKey: !!m.meta, ctrlKey: !!m.ctrl, altKey: !!m.alt,
});

// 对话框自己要的
ok(keyVerdict(k("Enter"), true) === "pass", "回车：按下焦点所在的按钮");
ok(keyVerdict(k(" "), true) === "pass", "空格：同上");
ok(keyVerdict(k("Escape"), true) === "pass", "Esc：取消");
ok(keyVerdict(k("Escape"), false) === "pass", "Esc 焦点不在对话框里也放：取消从哪儿按都安全");
ok(keyVerdict(k("c", { meta: true }), true) === "pass", "⌘C：信任对话框里的配置要能复制");
ok(keyVerdict(k("Tab"), true) === "cycle", "Tab：在按钮之间转圈");

// 快捷键一律挡
for (const [e, name] of [
  [k("p", { meta: true }), "⌘P"],
  [k("w", { meta: true }), "⌘W"],
  [k("s", { meta: true }), "⌘S"],
  [k("Tab", { ctrl: true }), "⌃Tab"],
  [k("r", { ctrl: true }), "⌃R"],
  [k("l", { meta: true, alt: true }), "⌥⌘L"],
  [k("Shift"), "⇧（⇧⇧ 的那一下 keyup）"],
  [k("F2", { meta: true }), "⌘F2"],
] as const) {
  ok(keyVerdict(e, true) === "block", `${name}：对话框开着时不算数`);
}
ok(keyVerdict(k("Enter"), false) === "block", "焦点不在对话框里的回车：会落在被压着的东西上，挡");
ok(keyVerdict(k("a"), true) === "block", "光按字母：挡（没东西可打）");

// 哪些要顶一下
ok(nudgesFor(k("p", { meta: true })) && nudgesFor(k("Tab", { ctrl: true })), "快捷键被挡：顶一下");
ok(!nudgesFor(k("a")) && !nudgesFor(k("Shift")), "光按字母、⇧：不顶（每个字都晃太吵）");

// takeFocus：参数变了（换了问的对象）要把焦点重新给默认按钮
{
  let focused = 0;
  const def = { focus: () => { focused++; } };
  const node = { querySelector: (sel: string) => (sel === "[data-default]" ? def : null) } as unknown as HTMLElement;
  (globalThis as { document?: unknown }).document = { activeElement: null, body: null };
  const act = takeFocus(node, 1);
  await Promise.resolve();
  ok(focused === 1, "出现时：焦点给默认按钮");
  (act as { update?: (v: unknown) => void }).update?.(2);
  await Promise.resolve();
  ok(focused === 2, "换了问的对象（关闭全部的下一个）：焦点再给一次默认按钮");
}

console.log(`${fail === 0 ? "✅" : "❌"} 对话框模态：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
