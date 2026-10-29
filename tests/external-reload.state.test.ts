import assert from "node:assert/strict";
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { tabs } = await import("../src/lib/state/tabs.svelte");
const { docs } = await import("../src/lib/state/docs.svelte");
const { fileStamp } = await import("../src/lib/ipc/commands");
const ipc = (globalThis as any).__TAURI_INTERNALS__;
const invoke = ipc.invoke;
const path = "/proj/README.md";
let pass = 0;

async function setup() {
  ipc.invoke = invoke;
  const stamp = await fileStamp(path);
  tabs.list = [{ id: 991, path, name: "README.md", mode: "edit", group: 0, size: 3, content: "OLD", dirty: false, stamp }];
  tabs.activeId = 991;
  (globalThis as any).__mockEditFileOutside(path, "EXTERNAL");
  return tabs.list[0];
}

function holdRead() {
  let release!: () => void, entered!: () => void;
  const gate = new Promise<void>(r => release = r);
  const reading = new Promise<void>(r => entered = r);
  ipc.invoke = async (cmd: string, args: any) => {
    const value = await invoke(cmd, args);
    if (cmd === "read_text" && args.path === path) { entered(); await gate; }
    return value;
  };
  return { reading, release };
}

{
  const t = await setup();
  await docs.checkExternalChanges();
  assert.equal(t.content, "EXTERNAL");
  assert.equal(t.dirty, false);
  pass++;
}
for (const change of ["stash", "undo", "save", "rename", "close", "encoding", "mode"] as const) {
  const t = await setup();
  const h = holdRead();
  const pending = docs.checkExternalChanges();
  await h.reading;
  if (change === "stash") docs.stashDraft(path, "LOCAL");
  if (change === "undo") { docs.edited(t.id, true); docs.edited(t.id, false); }
  if (change === "save") await docs.saveTab(t, "SAVED", { quiet: true });
  if (change === "rename") t.path = "/proj/renamed.md";
  if (change === "close") tabs.list = [{ ...t, content: "REOPENED" }];
  if (change === "encoding") await docs.saveAsEncoding("GBK", false);
  if (change === "mode") t.mode = "log";
  h.release();
  await pending;
  if (change === "stash") {
    assert.equal(t.draft, "LOCAL", "慢重载不得清掉等待期间交回的草稿");
    assert.equal(t.dirty, true);
    assert.equal(t.conflict, true);
  } else if (change === "save") assert.equal(t.content, "SAVED");
  else assert.equal(t.content, "OLD", `${change} 后旧回包不得修改标签`);
  if (change === "close") assert.equal(tabs.list[0].content, "REOPENED");
  pass++;
}
for (const action of ["encoding-reload", "take-disk"] as const) {
  const t = await setup();
  if (action === "take-disk") { t.dirty = true; t.conflict = true; }
  const h = holdRead();
  const pending = action === "take-disk" ? docs.resolveConflict(t, "disk") : docs.reopenWith("GBK");
  await h.reading;
  docs.stashDraft(path, "NEW INPUT");
  h.release();
  await pending;
  assert.equal(t.draft, "NEW INPUT", `${action} 也不能覆盖等待期间的输入`);
  assert.equal(t.dirty, true);
  if (action === "take-disk") assert.equal(t.conflict, true);
  pass++;
}
{
  const t = await setup();
  const h = holdRead();
  const older = docs.checkExternalChanges();
  await h.reading;
  ipc.invoke = invoke;
  (globalThis as any).__mockEditFileOutside(path, "LATEST");
  await docs.checkExternalChanges();
  h.release();
  await older;
  assert.equal(t.content, "LATEST", "旧重载后到也不能倒退正文");
  pass++;
}
ipc.invoke = invoke;
console.log(`✅ 外部重载时序：${pass} 个场景通过`);
process.exit(0);
