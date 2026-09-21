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
const { setInvariantSink } = await import("../src/lib/state/invariant");
const { persist } = await import("../src/lib/state/persist.svelte");
const { layout } = await import("../src/lib/state/layout.svelte");
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
// 自检器默认的 sink 是空函数 —— 不装这一条，下面那些 `tabs.audit()` 报了也没人知道
setInvariantSink((m) => ok(false, `不变量：${m}`));

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

// ── 7. 改换行符（issue #37）：只标脏不动内容，保存那一下才带着 eol 写盘，重开读回来是新的 ──
{
  for (const t of [...tabs.list]) tabflow.doClose(t);
  await tabflow.openPath("/proj/README.md");
  const t = tabs.active!;
  ok(t.eol === "LF", `桩里 README 是 LF，实际 ${t.eol}`);
  const ed = mountEditor(t.path, t.content!);
  await docs.setEol("CRLF");
  ok(t.dirty && t.eol === "CRLF", "改换行符 = 标脏 + 换目标，内容不动");
  ok(ed.text() === t.content, "编辑器里的文本没被碰");
  await docs.save(ed.text());
  ok(!t.dirty, "保存后不脏");
  ok((await readText(t.path)).eol === "CRLF", "写盘带着 CRLF：重读探出来的就是 CRLF");
  await docs.setEol("CRLF");
  ok(!t.dirty, "选了和现在一样的不标脏 —— 否则圆点亮了却没东西可存");
  // 改成 LF 再「切走切回」：编辑器交回一份和磁盘一样的文本，圆点不能灭（review 2026-09-20）
  await docs.setEol("LF");
  ed.leave();
  ok(t.dirty && t.eol === "LF", `切走（交回草稿）之后仍然脏，实际 dirty=${t.dirty}`);
  const ed2 = mountEditor(t.path, t.content!);
  ok(t.dirty, "切回来仍然脏");
  await docs.save(ed2.text());
  ok(!t.dirty && !t.fmt, "保存后清干净");
  ed2.leave();
  tabflow.requestClose(t.id);
}

// ── 8. 分屏（issue #35 第 ① 步，docs/SPLIT.md 第 3 节）：状态层自己就能验的那些规则 ──
{
  const names = () => tabs.list.map((t) => `${t.name}@${t.group}`).join(" ");
  const shownNames = () => tabs.shown.map((id) => (id === null ? "∅" : tabs.byId(id)!.name)).join("|");
  for (const t of [...tabs.list]) tabflow.doClose(t);
  await tabflow.openPath("/proj/README.md");
  await tabflow.openPath("/proj/pom.xml");
  await tabflow.openPath("/proj/package.json");
  const readme = tabs.byPath("/proj/README.md")!;
  const pom = tabs.byPath("/proj/pom.xml")!;
  const pkg = tabs.byPath("/proj/package.json")!;
  ok(!tabs.split && tabs.shown.length === 1 && tabs.activeId === pkg.id, "三个标签开在单栏，活动的是最后开的");

  // 移到另一组 = 向右分屏；原组正显示它 → 原组落到邻居；焦点跟到右组
  ok(tabs.moveToOther(pkg.id), "单栏时「移到另一组」就是向右分屏");
  ok(tabs.split && shownNames() === "pom.xml|package.json", `左组落到邻居、右组显示它，实际 ${shownNames()}`);
  ok(tabs.activeId === pkg.id && tabs.activeGroup === 1, "焦点跟到右组");
  ok(tabs.active === pkg, "tabs.active 语义不变：还是光标所在的那个");
  ok(pkg.group === 1 && readme.group === 0 && pom.group === 0, `分组 ${names()}`);
  tabs.audit("分屏");

  // 打开文件落在焦点组
  await tabflow.openPath("/proj/Cargo.toml");
  const cargo = tabs.byPath("/proj/Cargo.toml")!;
  ok(cargo.group === 1 && tabs.shown[1] === cargo.id, "新开的落在焦点组（右）并显示");
  // 已在另一组开着的文件被再打开：焦点跳过去，不开第二份
  await tabflow.openPath("/proj/README.md");
  ok(tabs.list.filter((t) => t.path === readme.path).length === 1, "同一文件只有一份");
  ok(tabs.activeId === readme.id && tabs.activeGroup === 0 && tabs.shown[0] === readme.id, "焦点跳到左组的那个");
  tabs.audit("跨组打开");

  // 只有一个标签的组不许再分出去
  tabs.moveToGroup(pom.id, 1);
  ok(readme.group === 0 && pom.group === 1, "pom 挪到右组");
  ok(!tabs.canMove(readme.id) && !tabs.moveToOther(readme.id), "左组只剩 readme，不许挪 —— 挪走原组就空了");
  ok(readme.group === 0 && tabs.split, "拒绝之后什么都没变");

  // 焦点在另一组时，关掉右组正显示的：右组落到邻居，焦点不动
  tabs.focusGroup(1);
  ok(tabs.activeId === tabs.shown[1], "focusGroup 让活动标签变成那组显示的");
  tabs.focusGroup(0);
  const rightShown = tabs.shownIn(1)!;
  tabflow.doClose(rightShown);
  ok(tabs.shown[1] !== null && tabs.shownIn(1)!.group === 1, "右组换成邻居");
  ok(tabs.activeId === readme.id, "焦点组没变，活动标签还是 readme");
  tabs.audit("关另一组的");

  // 预览按组：左右各能有一个
  await tabflow.openPath("/proj/src/main.py", { preview: true });
  ok(tabs.active!.preview && tabs.activeGroup === 0, "预览开在焦点组（左）");
  tabs.focusGroup(1);
  await tabflow.openPath("/proj/src/long.ts", { preview: true });
  ok(tabs.previewIn(0) !== null && tabs.previewIn(1) !== null, "两组各一个预览，互不顶掉");
  await tabflow.openPath("/proj/src/OrderService.java", { preview: true });
  ok(tabs.previewIn(1)!.path.endsWith("OrderService.java") && tabs.previewIn(0)!.path.endsWith("main.py"), "右组的预览被顶掉，左组的不动");
  tabs.audit("预览按组");
  // 把左组的预览挪进已经有预览的右组：挪动是显式动作，挪过去就不再是预览（review 2026-09-21）
  const leftPreview = tabs.previewIn(0)!;
  tabs.moveToGroup(leftPreview.id, 1);
  ok(!leftPreview.preview && leftPreview.group === 1, "挪过去的那个不再是预览");
  ok(tabs.inGroup(1).filter((t) => t.preview).length === 1, "右组仍然只有一个预览");
  tabs.audit("挪预览");
  tabs.moveToGroup(leftPreview.id, 0); // 挪回去，别影响下面几段的标签数

  // 右组关到空 → 收成单栏，全归组 0，焦点落到左组显示的
  for (const t of tabs.inGroup(1)) tabflow.doClose(t);
  ok(!tabs.split && tabs.shown.length === 1, `右组空了就收起，实际 shown=${shownNames()}`);
  ok(tabs.list.every((t) => t.group === 0), `全归组 0：${names()}`);
  ok(tabs.activeId === tabs.shown[0] && tabs.active !== null, "焦点落到剩下那组显示的");
  tabs.audit("收起");

  // 合并分屏：钉住的排前面（左钉、右钉、左余、右余）
  tabs.setPinned(readme.id, true);
  tabs.moveToGroup(tabs.list.find((t) => t.id !== readme.id)!.id, 1);
  const rightOne = tabs.shownIn(1)!;
  tabs.setPinned(rightOne.id, true);
  ok(tabs.inGroup(1)[0].id === rightOne.id, "右组钉住的排右组最左");
  tabs.unsplit();
  ok(!tabs.split, "合并了");
  ok(tabs.list[0].id === readme.id && tabs.list[1].id === rightOne.id, `合并后钉住的在最前：${names()}`);
  ok(tabs.list.every((t) => t.group === 0), "合并后全在组 0");
  ok(tabs.activeId === rightOne.id, "合并保住焦点组显示的那个");
  tabs.audit("合并");

  for (const t of [...tabs.list]) {
    t.pinned = false;
    tabflow.doClose(t);
  }
  ok(tabs.list.length === 0 && tabs.shown[0] === null && tabs.activeId === null, "全关掉：单栏、没有显示的");
}

// ── 9. 分屏进快照、从快照回来（issue #35 第 ② 步）：走的是切项目那条真路（beforeRootChange → afterRootChange） ──
{
  const shownNames = () => tabs.shown.map((id) => (id === null ? "∅" : tabs.byId(id)!.name)).join("|");
  for (const t of [...tabs.list]) tabflow.doClose(t);
  await tabflow.openPath("/proj/README.md");
  await tabflow.openPath("/proj/pom.xml");
  await tabflow.openPath("/proj/package.json");
  await tabflow.openPath("/proj/Cargo.toml");
  tabs.moveToGroup(tabs.byPath("/proj/package.json")!.id, 1);
  tabs.moveToGroup(tabs.byPath("/proj/Cargo.toml")!.id, 1);
  tabs.show(tabs.byPath("/proj/pom.xml")!.id);      // 左组显示 pom，焦点在左
  layout.splitRatio = 0.35;
  ok(tabs.split && shownNames() === "pom.xml|Cargo.toml" && tabs.activeGroup === 0, `摆好：${shownNames()}`);

  const snap = persist.snapshot();
  ok(snap.tabs.filter((t) => t.group === 1).map((t) => t.path.split("/").pop()).join() === "package.json,Cargo.toml", "右组两个打了 group: 1");
  ok(snap.tabs.filter((t) => t.shown).map((t) => t.path.split("/").pop()).join() === "pom.xml,Cargo.toml", "两组各一个 shown");
  ok(snap.tabs[snap.active].path.endsWith("pom.xml"), "active 指着焦点组显示的");
  ok(snap.layout.splitRatio === 0.35, "分隔线位置进快照");

  // 落盘、关掉、再从项目快照里回来
  persist.restoring = false;
  persist.beforeRootChange("/proj");
  for (const t of [...tabs.list]) tabflow.doClose(t);
  ok(tabs.list.length === 0 && !tabs.split, "关干净了");
  await persist.afterRootChange("/proj");
  ok(tabs.list.length === 4, `四个都回来了，实际 ${tabs.list.length}`);
  ok(tabs.split && shownNames() === "pom.xml|Cargo.toml", `分屏回来了，显示的还是那两个：${shownNames()}`);
  ok(tabs.active?.name === "pom.xml" && tabs.activeGroup === 0, "焦点在左组的 pom");
  ok(tabs.inGroup(1).map((t) => t.name).join() === "package.json,Cargo.toml", "右组顺序照旧");
  tabs.audit("分屏恢复");

  // 右组的文件都不在了 → 单栏回来
  const raw = JSON.parse(localStorage.getItem("lite-ide.session:/proj")!);
  raw.tabs = raw.tabs.map((t: { path: string }) => (t.path.endsWith("package.json") || t.path.endsWith("Cargo.toml") ? { ...t, path: t.path + ".gone" } : t));
  localStorage.setItem("lite-ide.session:/proj", JSON.stringify(raw));
  for (const t of [...tabs.list]) tabflow.doClose(t);
  await persist.afterRootChange("/proj");
  ok(tabs.list.length === 2 && !tabs.split && tabs.list.every((t) => t.group === 0), `右组全丢 → 单栏，实际 ${tabs.list.length} 个 split=${tabs.split}`);
  ok(tabs.active?.name === "pom.xml", "焦点还在 pom");
  tabs.audit("分屏恢复·右组全丢");
  persist.restoring = true;
  for (const t of [...tabs.list]) tabflow.doClose(t);
}

console.log(`${fail === 0 ? "✅" : "❌"} 状态层（tabflow / docs / files）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
