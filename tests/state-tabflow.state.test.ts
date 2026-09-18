/**
 * 状态层（tabflow / docs / files / worktree）的回归测试 —— 跑在裸 node 里，
 * runes 由 `tests/runes/hooks.mjs` 编译，IPC 走 `mock-ipc.ts`（和 `pnpm dev` 同一份桩）。
 *
 * 这层原来零测试：09-03 的两个丢数据 bug、M25 切标签丢未保存、09-09 `stashed` 回滚
 * 全出在这儿，靠间歇红的 smoke 兜。这里每一条都是那些 bug 的形状。
 *
 * 「编辑器」在这儿是一个 20 行的替身：它和 `Editor.svelte` 讲同一套契约 ——
 * `docs.onEditorLive(path, get)` 交出「读此刻文本」的能力，切走时 `docs.stashDraft`
 * 交回草稿。测的不是编辑器，是状态层拿到这些之后做对没有。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
// 桩装好之后再 import：这些模块顶层就会碰 localStorage / invoke
const { tabflow } = await import("../src/lib/state/tabflow.svelte");
const { tabs } = await import("../src/lib/state/tabs.svelte");
const { docs } = await import("../src/lib/state/docs.svelte");
const { files } = await import("../src/lib/state/files.svelte");
const { worktree } = await import("../src/lib/state/worktree.svelte");
const { project } = await import("../src/lib/state/project.svelte");
const { readText, scratchDir } = await import("../src/lib/ipc/commands");
// App 启动时拿一次草稿目录（`isScratch` 靠它，没到位一律算「不是」—— 猜错的方向必须是留下）
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
const disk = async (p: string) => (await readText(p)).content;

/** 编辑器替身：挂上 = 交出「此刻文本」；切走 = 交回草稿 */
function mountEditor(path: string, text: string) {
  let live = text;
  docs.onEditorLive(path, () => live);
  return {
    type(s: string) {
      live += s;
      const t = tabs.byPath(path);
      if (t) t.dirty = live !== (t.content ?? "");
    },
    leave() {
      docs.onEditorLive(path, null);
      docs.stashDraft(path, live);
    },
    text: () => live,
  };
}

// 项目根：桩里的 /proj
await tabflow.openPath("/proj");
ok(project.root === "/proj", "打开目录 = 设项目根");

// ── 1. 「保存并关闭」写的是编辑器此刻的内容（09-03 第一个丢数据 bug） ──
{
  await tabflow.openPath("/proj/README.md");
  const t = tabs.active!;
  const ed = mountEditor(t.path, t.content!);
  ed.type("\n我刚打的字");
  ok(t.dirty, "打了字标签要脏");
  tabflow.requestClose(t.id);
  await new Promise((r) => setTimeout(r, 20));
  ok(tabflow.pendingClose?.id === t.id, "脏标签关闭要先问");
  await tabflow.resolveClose("save");
  ok((await disk("/proj/README.md")).endsWith("我刚打的字"), "写进盘的是编辑器里此刻的文本，不是磁盘原文也不是旧草稿");
  ok(tabs.byPath("/proj/README.md") === null, "保存并关闭之后标签关掉了");
}

// ── 2. 切走再切回来，字和脏标记都在（M25 / 09-03 第三个） ──
{
  await tabflow.openPath("/proj/vite.config.ts");
  const a = tabs.active!;
  const ed = mountEditor(a.path, a.content!);
  ed.type("\n// 没保存");
  ed.leave();
  await tabflow.openPath("/proj/pom.xml");
  ok(tabs.active?.path === "/proj/pom.xml", "切到了别的标签");
  const back = tabs.byPath("/proj/vite.config.ts")!;
  ok(back.dirty, "切走之后脏标记还在");
  ok(back.draft?.endsWith("// 没保存"), "切走之后草稿还在");
  ok(docs.liveText(back).endsWith("// 没保存"), "切回来该保存的文本是草稿那份");
  // 收拾：丢弃
  tabflow.requestClose(back.id);
  await new Promise((r) => setTimeout(r, 20));
  await tabflow.resolveClose("discard");
  tabflow.requestClose(tabs.byPath("/proj/pom.xml")!.id);
}

// ── 3. 另存为：写成了才换路径，草稿搬走、头脱掉 ──
{
  await tabflow.newScratch();
  const s = tabs.active!;
  ok(project.isScratch(s.path), "⌘N 建出来的是草稿");
  const oldPath = s.path;
  const ed = mountEditor(s.path, s.content!);
  ed.type("毕业的正文");
  (globalThis as { prompt?: (m: string, d: string) => string }).prompt = () => "/proj/docs/grad.md";
  await worktree.saveAs();
  ok(tabs.active?.path === "/proj/docs/grad.md", "另存为之后标签指向新路径");
  ok(!tabs.active?.dirty, "另存为之后不脏");
  const body = await disk("/proj/docs/grad.md");
  ok(body.startsWith("毕业的正文"), `毕业时锚点头要脱掉，实际开头：${JSON.stringify(body.slice(0, 20))}`);
  ok(await readText(oldPath).then((r) => r.content.startsWith("// 桩里没有")), "旧草稿进了废纸篓（桩里读不到了）");
  ed.leave();
  tabflow.requestClose(tabs.active!.id);
}

// ── 4. 另存为写失败：标签还指着原来那份 ──
{
  await tabflow.openPath("/proj/Cargo.toml");
  const t = tabs.active!;
  (globalThis as { prompt?: (m: string, d: string) => string }).prompt = () => "/nonexistent-dir/x.toml";
  await worktree.saveAs();
  ok(tabs.active?.path === "/proj/Cargo.toml", "写失败时标签不换路径");
  tabflow.requestClose(t.id);
}

// ── 5. 最近文件：打开就记、改名跟着走、废纸篓摘掉 ──
{
  await tabflow.openPath("/proj/package.json");
  await tabflow.openPath("/proj/.env");
  ok(files.recent[0] === "/proj/.env" && files.recent[1] === "/proj/package.json", "最近文件最新在前");
  await worktree.renameOpenTabs("/proj/.env", "/proj/.env.local", false, null);
  ok(files.recent[0] === "/proj/.env.local", "改名后最近列表原位换成新路径");
  worktree.closeTabsUnder("/proj/.env.local", false);
  ok(!files.recent.includes("/proj/.env.local"), "进废纸篓的从最近里摘掉");
  tabflow.requestClose(tabs.byPath("/proj/package.json")!.id);
}

// ── 6. 启动进草稿：有空草稿就复用，不再新建 ──
{
  for (const t of [...tabs.list]) tabflow.doClose(t);
  ok(tabs.list.length === 0, "清空标签");
  await tabflow.launchScratch();
  const first = tabs.active!.path;
  ok(project.isScratch(first), "启动落在草稿里");
  // 模拟「退出再开」：标签表清空但文件还在（doClose 会把空草稿丢掉，那是另一条规则）
  tabs.remove(tabs.active!.id);
  await tabflow.launchScratch();
  ok(tabs.active!.path === first, "第二次启动复用同一份空草稿，不新建");
}

console.log(`${fail === 0 ? "✅" : "❌"} 状态层（tabflow / docs / files）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
