/**
 * 对话框（app.css「对话框」那段）一出来就把焦点交给它的默认按钮（`data-default`）：回车 = 那个按钮；焦点也离开了编辑器 ——
 * 不然字照样打进被压暗的正文里。关掉之后焦点还回去：关脏标签时点了「取消」，人是要接着打字的。
 * `Confirms.svelte` 和 `RemoteBars.svelte` 两处用，`use:takeFocus`
 */
export function takeFocus(node: HTMLElement, _who?: unknown) {
  const prev = document.activeElement as HTMLElement | null;
  const toDefault = () => queueMicrotask(() => node.querySelector<HTMLButtonElement>("[data-default]")?.focus({ preventScroll: true }));
  toDefault();
  return {
    /**
     * 同一个对话框换了问的对象（参数变了，`use:takeFocus={id}`）：焦点重新给默认按钮。
     * 同一拍里换人时块不重建、上面那句不会再跑，焦点就停在上一下按过的按钮上 —— 「关闭全部」里那就是「不保存」
     * （code review 2026-10-13）。`prev` 不动：关掉之后还回去的仍是对话框出现之前的那个地方
     */
    update: toDefault,
    destroy() {
      const now = document.activeElement;
      if (prev?.isConnected && (now === null || now === document.body || !now.isConnected)) prev.focus({ preventScroll: true });
    },
  };
}

type KeyLike = Pick<KeyboardEvent, "key" | "metaKey" | "ctrlKey" | "altKey">;

/**
 * 对话框开着时，这一个键怎么办（纯函数，`tests/modal.test.ts` 直接测；背景见 `modal.ts` 头上）：
 * - `pass`：对话框自己要的 —— 回车 / 空格（按下焦点所在的按钮）、Esc（取消，Confirms 的 onKey）、⌘C / ⌘A（信任对话框里那段配置要能复制）
 * - `cycle`：Tab / ⇧Tab，在对话框的按钮之间转圈，不让焦点走出去（WebKit 默认 Tab 不停在按钮上，除非系统开了「键盘导航」）
 * - `block`：别的全部挡掉；焦点不在对话框里（`inDialog` 假）时除了 Esc 都挡 —— 那一下回车会落在被压着的东西上
 */
export function keyVerdict(e: KeyLike, inDialog: boolean): "pass" | "cycle" | "block" {
  const mod = e.metaKey || e.ctrlKey || e.altKey;
  // Esc 是「取消」，取消从哪儿按都安全；Confirms 的 onKey 去点那个 data-dismiss
  if (e.key === "Escape" && !mod) return "pass";
  if (!inDialog) return "block";
  if (e.key === "Tab" && !e.metaKey && !e.ctrlKey && !e.altKey) return "cycle";
  if (!mod && (e.key === "Enter" || e.key === " ")) return "pass";
  if (e.metaKey && !e.ctrlKey && !e.altKey && (e.key === "c" || e.key === "a")) return "pass";
  return "block";
}

/** 被挡下的键里，哪些要「顶一下」：带 ⌘ / ⌃ 的（人是想用快捷键）。光按字母不顶 —— 每个字都晃一下太吵 */
export function nudgesFor(e: KeyLike): boolean {
  return e.metaKey || e.ctrlKey;
}
