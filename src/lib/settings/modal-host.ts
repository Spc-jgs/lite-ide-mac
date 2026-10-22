/**
 * 设置对话框、运行配置对话框的「挂上去 + 真模态」（docs/SETTINGS.md 第 13 节）。两个都是懒块，由菜单动作在用到时挂到 body 上 ——
 * 不放进 Confirms 那一层：那一层的对话框只有按钮，模态守卫（`dialog-focus.ts` 的 `keyVerdict`）只放回车、空格、Esc、⌘C ⌘A，
 * 输入框里一个字都打不进去；这里的对话框是表单。
 *
 * 守卫的分法：
 * - **对话框里面的键**照常给它的输入框，在对话框的根上截住冒泡（组件自己的 `onkeydown` 调 `stopPropagation`），App 的全局按键收不到
 * - **对话框外面的**点击和按键挡掉、顶一下（`modal.ts` 的 `nudgeDialog`，和 Confirms 的对话框同一个反馈）；Esc 从哪儿按都是取消
 * - **菜单**那条路：`setAsking(true)` 之后 `dialogUp()` 认得出它（`.layer .dialog`），`menu-actions` 门口照旧挡
 */
import { mount, unmount, type Component } from "svelte";
import { nudgeDialog, setAsking } from "../shell/modal";

/** 挂上一个对话框组件，返回关掉它的函数。组件要接一个 `onclose`（取消、确定之后调）；不管从哪条路关的，关完都调 `onClosed` */
export function openModal<P extends { onclose: () => void }>(C: Component<P>, props: Omit<P, "onclose">, onClosed?: () => void): () => void {
  const prev = document.activeElement as HTMLElement | null;
  const host = document.createElement("div");
  document.body.appendChild(host);
  let closed = false;
  const dialog = () => host.querySelector<HTMLElement>(".dialog");

  function close() {
    if (closed) return;
    closed = true;
    for (const k of KEYS) window.removeEventListener(k, guardKey, true);
    for (const m of MICE) window.removeEventListener(m, guardPointer, true);
    setAsking(false);
    unmount(app);
    host.remove();
    if (prev?.isConnected) prev.focus({ preventScroll: true });
    onClosed?.();
  }

  function guardKey(e: KeyboardEvent) {
    const d = dialog();
    if (!d || d.contains(e.target as Node)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type !== "keydown") return;
    if (e.key === "Escape") close();
    else if (e.metaKey || e.ctrlKey) nudgeDialog(d);
  }

  /** 点在对话框外面：挡掉、顶一下。只放过标题栏上拖窗口的那几块（同 Confirms 的 guardPointer） */
  function guardPointer(e: MouseEvent) {
    const d = dialog();
    const t = e.target as Element | null;
    if (!d || !t || d.contains(t) || t.hasAttribute?.("data-tauri-drag-region")) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type === "pointerdown") nudgeDialog(d);
  }

  const KEYS = ["keydown", "keyup"] as const;
  const MICE = ["pointerdown", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"] as const;
  for (const k of KEYS) window.addEventListener(k, guardKey, true);
  for (const m of MICE) window.addEventListener(m, guardPointer, true);
  setAsking(true);
  const app = mount(C, { target: host, props: { ...props, onclose: close } as unknown as P });
  return close;
}
