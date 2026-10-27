/**
 * 标签上限（docs/TAKEOVER.md I10，IDEA 的 Tab limit）：超过 `editor.tabLimit` 关最久没看的那个。
 * 这条的「丢数据形状」是关错了：关掉改过没保存的、固定的；或者按「最早开的」关而不是「最久没看的」——
 * 那样你一直在来回看的那个文件，只因为是第一个开的，就被关了。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { tabflow } = await import("../src/lib/state/tabflow.svelte");
const { tabs } = await import("../src/lib/state/tabs.svelte");
const { settings } = await import("../src/lib/state/settings.svelte");
const { notify } = await import("../src/lib/state/notify.svelte");
const { project } = await import("../src/lib/state/project.svelte");
const { scratchDir } = await import("../src/lib/ipc/commands");
project.scratchRoot = await scratchDir();

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

await tabflow.openPath("/proj");
// 上限的修剪在懒块里、开完标签之后才跑（入口只留一句 `ops().then`）：等它落地
const settle = () => new Promise((r) => setTimeout(r, 0));
const open = async (p: string) => {
  await tabflow.openPath(p, { preview: false });
  await settle();
};
const names = () => tabs.list.map((t) => t.path.slice(t.path.lastIndexOf("/") + 1)).join(",");
const hints: string[] = [];
const hint0 = notify.hint.bind(notify);
notify.hint = (text: string, ms?: number) => {
  hints.push(text);
  hint0(text, ms);
};
const reset = () => {
  for (const t of [...tabs.list]) tabflow.doClose(t);
  tabflow.closed = [];
};

settings.v = { ...settings.v, editorTabLimit: 3 };

{
  await open("/proj/README.md");
  await open("/proj/Cargo.toml");
  await open("/proj/.gitignore");
  ok(tabs.list.length === 3, `没到上限一个不关：${names()}`);
  await open("/proj/.env");
  ok(tabs.list.length === 3 && !tabs.byPath("/proj/README.md"), `第 4 个：关掉最久没看的 README：${names()}`);
  ok(tabflow.closed.includes("/proj/README.md"), "关掉的能 ⇧⌘T 找回来");
  ok(hints.length === 1 && hints[0].includes("README.md") && hints[0].includes("3"), `说一句为什么少了一个：${hints[0]}`);
  reset();
}

{
  await open("/proj/README.md");
  await open("/proj/Cargo.toml");
  await open("/proj/.gitignore");
  tabs.show(tabs.byPath("/proj/README.md")!.id);
  await open("/proj/.env");
  ok(!!tabs.byPath("/proj/README.md") && !tabs.byPath("/proj/Cargo.toml"), `按最久没看、不按最早打开：刚看过的 README 留着、关 Cargo.toml：${names()}`);
  reset();
}

{
  await open("/proj/README.md");
  await open("/proj/Cargo.toml");
  await open("/proj/.gitignore");
  tabs.byPath("/proj/README.md")!.dirty = true;
  tabs.setPinned(tabs.byPath("/proj/Cargo.toml")!.id, true);
  await open("/proj/.env");
  ok(!!tabs.byPath("/proj/README.md") && !!tabs.byPath("/proj/Cargo.toml") && !tabs.byPath("/proj/.gitignore"), `改过没保存的、固定的不关，关的是下一个：${names()}`);
  tabs.byPath("/proj/.env")!.dirty = true;
  await open("/proj/api/body.json");
  ok(tabs.list.length === 4, `能关的都不能关：超着，不替人丢东西（${tabs.list.length} 个）：${names()}`);
  ok(hints.length === 1, `那句话一次会话只说一次（说了 ${hints.length} 次）`);
  tabs.byPath("/proj/README.md")!.dirty = false;
  tabs.byPath("/proj/.env")!.dirty = false;
  reset();
}

{
  settings.v = { ...settings.v, editorTabLimit: 0 };
  for (const p of ["/proj/README.md", "/proj/Cargo.toml", "/proj/.gitignore", "/proj/.env", "/proj/api/body.json"]) await open(p);
  ok(tabs.list.length === 5, `0 = 不限：${tabs.list.length} 个`);
  reset();
}

console.log(`${fail === 0 ? "✅" : "❌"} 标签上限：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
