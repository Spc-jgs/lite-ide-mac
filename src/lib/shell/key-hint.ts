/** 导轨在入口包，键位表按需读；悬停/聚焦后订阅生效键位，不复制默认值。 */
export function keyHint(node: HTMLElement, initial: { id: string; label: string }) {
  let current = initial;
  let keys: typeof import("../state/keymap") | undefined;
  let off: (() => void) | undefined;
  let dead = false, loading = false;
  function update(value = current) {
    current = value;
    const accel = keys?.byId(current.id)?.accel;
    node.title = current.label + (accel ? ` · ${accel}` : "");
  }
  async function load() {
    if (keys || loading || !current.id) return;
    loading = true;
    // 提示加载失败时仍保留动作名称，下次悬停重试，不打断主要操作。
    let m: typeof import("../state/keymap");
    try { m = await import("../state/keymap"); }
    catch { loading = false; return; }
    if (dead) return;
    keys = m;
    off = m.onOverrides(() => update());
    update();
  }
  update();
  node.addEventListener("pointerenter", load);
  node.addEventListener("focus", load);
  return { update, destroy() { dead = true; off?.(); node.removeEventListener("pointerenter", load); node.removeEventListener("focus", load); } };
}
