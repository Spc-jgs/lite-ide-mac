/**
 * 对话框（app.css「对话框」那段）一出来就把焦点交给它的默认按钮（`data-default`）：回车 = 那个按钮；焦点也离开了编辑器 ——
 * 不然字照样打进被压暗的正文里。关掉之后焦点还回去：关脏标签时点了「取消」，人是要接着打字的。
 * `Confirms.svelte` 和 `RemoteBars.svelte` 两处用，`use:takeFocus`
 */
export function takeFocus(node: HTMLElement) {
  const prev = document.activeElement as HTMLElement | null;
  queueMicrotask(() => node.querySelector<HTMLButtonElement>("[data-default]")?.focus({ preventScroll: true }));
  return {
    destroy() {
      const now = document.activeElement;
      if (prev?.isConnected && (now === null || now === document.body || !now.isConnected)) prev.focus({ preventScroll: true });
    },
  };
}
