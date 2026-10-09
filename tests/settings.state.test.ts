/**
 * 设置（issue #44 第 3 步）的状态层：启动时迁旧偏好、切开关、⌘=。IPC 走 `mock/settings.ts`（和 `pnpm dev` 同一份桩）。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { settings } = await import("../src/lib/state/settings.svelte");
const { LEGACY_MOVED_KEY } = await import("../src/lib/state/settings-view");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const settle = () => new Promise((r) => setTimeout(r, 10));

// ── 交给 Rust 失败了：不记「迁过了」，下次启动再交 ──
{
  localStorage.setItem("lite-ide.minimap", "0");
  const internals = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
  const real = internals.invoke;
  internals.invoke = (c, a) => (c === "adopt_ui_state" ? Promise.reject(new Error("模拟失败")) : real(c, a));
  await settings.init();
  internals.invoke = real;
  ok(localStorage.getItem(LEGACY_MOVED_KEY) === null, "交失败了：不能记迁过了，不然这几个旧偏好就永远丢了");
  ok(settings.v.minimap === true, "交失败了：照常用 Rust 那边的值（默认），应用照常起来");
}

// ── 升级后第一次启动：旧偏好交给 Rust，迁过记一笔，旧键不删 ──
{
  localStorage.setItem("lite-ide.minimap", "0");
  localStorage.setItem("lite-ide.git-grouped", "1");
  localStorage.setItem("lite-ide.editorFont", "16");
  await settings.init();
  ok(settings.v.minimap === false && settings.v.gitGrouped === true, "旧的缩略图、分组设置迁过来了");
  ok(settings.v.editorFontSize === 16, `旧字号 16 迁过来还是 16（换算成偏移）：${settings.v.editorFontSize}`);
  ok(localStorage.getItem(LEGACY_MOVED_KEY) === "1", "记了迁过了");
  ok(localStorage.getItem("lite-ide.minimap") === "0", "旧键不删：回退到旧版本时偏好还在");
}

// ── 切开关、⌘= 走 Rust，用它回的那份 ──
{
  settings.toggle("editor.minimap");
  await settle();
  ok(settings.v.minimap === true, "切缩略图");
  settings.toggle("tree.compact");
  await settle();
  ok(settings.v.treeCompact === false, "切文件树紧凑");
  settings.zoom(2);
  await settle();
  ok(settings.v.editorFontSize === 18, `⌘= 两格：${settings.v.editorFontSize}`);
  for (let i = 0; i < 30; i++) settings.zoom(1);
  await settle();
  ok(settings.v.editorFontSize === 28, `到顶夹住：${settings.v.editorFontSize}`);
  settings.zoom(null);
  await settle();
  ok(settings.v.editorFontSize === settings.v.editorFontBase, "⌘0 回到基础字号");
}

console.log(`${fail === 0 ? "✅" : "❌"} 设置（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
