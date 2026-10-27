/**
 * 运行窗（#48 第 3 步）的状态层：跑、重跑、停、关、退出事件。跑在裸 node 里，IPC 走浏览器桩（mock/tasks.ts）。
 * 进程那半（整组停、端口空）在 tasksvc 自己的测试里；这里测的是前端独有的那半 —— 它错了，表现是格子状态说谎、
 * 重跑多出一格、⌃R 按下去没反应。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { FILES } = await import("../src/lib/dev/mock/data");
const { project } = await import("../src/lib/state/project.svelte");
const { layout } = await import("../src/lib/state/layout.svelte");
const { overlay } = await import("../src/lib/state/overlay.svelte");
const { runs } = await import("../src/lib/state/runs.svelte");
const ops = await import("../src/lib/state/runs-ops");

let pass = 0;
let fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));

project.root = "/proj";
FILES["/proj/.lite-ide/tasks.json"] = `[
  { "name": "后端", "command": "mvn spring-boot:run" },
  { "name": "fail-boot", "command": "mvn -q spring-boot:run" },
  { "name": "quick-web", "command": "pnpm dev" },
  { "name": "linger-boot", "command": "./gradlew bootRun" },
]`;

// ── ⌃R 一个都没跑过：开任务列表，不是没反应 ──
ops.rerunLast();
ok(overlay.taskPicker, "⌃R 在一次都没跑过时该打开任务列表");
overlay.taskPicker = false;

// ── 跑：一格、在跑、运行窗亮出来 ──
await ops.runTask("后端");
ok(runs.list.length === 1 && runs.list[0].name === "后端" && runs.list[0].status === "running", `跑起来一格：${JSON.stringify(runs.list)}`);
ok(layout.panel && layout.panelView === "run", "运行窗要亮出来");
// 在运行窗**开着**的时候问快照：放在最后（全关之后）问的话 panelView 早回到 term 了，这条断言就是空的（验红时发现的）
ok(layout.snapshot().panelView === "term", "运行窗不进快照（任务跟着进程死，重启没东西可恢复）");
ok(runs.activeId === runs.list[0].id, "新跑的那格是当前格");
ok(ops.last["/proj"] === "后端", "记下这个项目最近跑的");
ok(FILES[runs.list[0].log]?.includes("mvn spring-boot:run"), "输出文件的路径交回来了（桩里能读到）");

// ── 重跑同名的：原地换一格（位置不跳、不多一格），id 换新的 ──
const first = runs.list[0].id;
await ops.runTask("fail-boot");
await ops.runTask("后端");
ok(runs.list.length === 2, `重跑不该多出一格：${runs.list.map((r) => r.name)}`);
ok(runs.list[0].name === "后端" && runs.list[0].id !== first, "重跑的那格原地换成新的一次（新 id）");
ok(runs.list[1].name === "fail-boot", "别的格子位置不动");

// ── 连按两次 ⌃R：第二下不再发 task_run（重跑要等旧的软停，人会按急）—— code review 2026-10-10 ──
{
  const inner = (window as unknown as { __TAURI_INTERNALS__: { invoke: (c: string, a?: unknown) => Promise<unknown> } }).__TAURI_INTERNALS__;
  const orig = inner.invoke;
  let calls = 0;
  inner.invoke = (c, a) => {
    if (c === "task_run") calls++;
    return orig(c, a);
  };
  await Promise.all([ops.runTask("后端"), ops.runTask("后端")]);
  inner.invoke = orig;
  ok(calls === 1, `连按两次只该发一次 task_run：发了 ${calls} 次`);
  ok(runs.list.filter((r) => r.name === "后端").length === 1, "还是一格");
  await ops.runTask("后端");
  ok(runs.list.filter((r) => r.name === "后端").length === 1 && runs.list[0].status === "running", "起完之后再按照常能重跑");
}

// ── 退出事件：自己非零退出 → 失败；桩 0.9 秒后让 fail-boot 退出 ──
await sleep(1100);
const fb = runs.list.find((r) => r.name === "fail-boot");
ok(fb?.status === "failed" && fb.code === 1, `fail-boot 应该是失败、退出码 1：${JSON.stringify(fb)}`);

// ── 停：先「正在停」，等它收尾报退出 → 「已停止」（我们停的不算失败） ──
const be = runs.list[0];
await ops.stopRun(be.id);
ok(runs.list[0].status === "stopping", `停了之后先是「正在停」：${runs.list[0].status}`);
await sleep(800);
ok(runs.list[0].status === "stopped", `收尾完了是「已停止」，不是失败：${runs.list[0].status}`);

// ── ⌃R 跑最近那个（后端） ──
ops.rerunLast();
await sleep(50);
ok(runs.list[0].name === "后端" && runs.list[0].status === "running", "⌃R 重跑了最近跑过的「后端」");

// ── 软停中再按一次：立刻强杀 ──
await ops.stopRun(runs.list[0].id);
await ops.stopRun(runs.list[0].id);
await sleep(50);
ok(runs.list[0].status === "stopped", `软停中再按一次应该立刻结束：${runs.list[0].status}`);

// ── 退得快的：退出事件比 task_stop 的回包先到（vite 收到 SIGINT 几毫秒就退）。回包回来不能再把「已停止」改回「正在停」——
//    那样就再也没有事件来改它，格子永远停在「正在停」（第 5 步 tasks-real.sh 在真 .app 上撞见的）──
await ops.runTask("quick-web");
const qw = runs.list.find((r) => r.name === "quick-web")!;
await ops.stopRun(qw.id);
await sleep(50);
ok(runs.list.find((r) => r.id === qw.id)?.status === "stopped", `退得快的停完是「已停止」：${runs.list.find((r) => r.id === qw.id)?.status}`);
ops.closeRun(qw.id);

// ── 端口被占（第 4 步）：fail-boot 失败时 8080 被别的程序占着 → 那格带上「谁占着」；「结束它并重跑」→ 端口空了、原地重跑起来 ──
const fb2 = runs.list.find((r) => r.name === "fail-boot")!;
ok(fb2.holder?.port === 8080 && fb2.holder.command === "java" && fb2.holder.ours === null, `失败那格带上占端口的：${JSON.stringify(fb2.holder)}`);
await ops.freePortAndRerun(fb2.id);
const fb3 = runs.list.find((r) => r.name === "fail-boot")!;
ok(fb3.id !== fb2.id && fb3.holder === null && fb3.status === "running", `结束占用者后原地重跑、卡片收起：${JSON.stringify(fb3)}`);
await sleep(1100);
ok(runs.list.find((r) => r.name === "fail-boot")?.status === "running", "端口空了，这次没再失败");
ops.dismissHolder(fb3.id);

// ── 上次没停干净（第 4 步）：问到了出卡片；结束它们 → 卡片收起、账上划掉，再问就没了 ──
(globalThis as { __mockStale?: unknown[] }).__mockStale = [{ pgid: 4242, name: "后端", command: "mvn spring-boot:run" }];
await ops.checkStale("/proj");
ok(runs.stale?.root === "/proj" && runs.stale.list.length === 1, `上次没停干净的出卡片：${JSON.stringify(runs.stale)}`);
await ops.resolveStale(true);
ok(runs.stale === null, "点了「结束它们」卡片收起");
await ops.checkStale("/proj");
ok(runs.stale === null, "处理过了，再问不该再出");
await ops.checkStale("/别的项目");
ok(runs.stale === null, "别的项目的不出");

// ── Services（docs/IDEA.md 第 12 节）：认出服务；服务跑起来进 Services、不进运行窗；⌘F2 的目标；关格子时当前格不落到服务上 ──
await ops.detectServices("/proj");
ok(runs.services === 2, `认出桩里的两个 Spring Boot 服务：${runs.services}`);
await ops.detectServices("/别的项目");
ok(runs.services === 0, `不是当前项目的回包不算数（问之前先清零）：${runs.services}`);
await ops.detectServices("/proj");
{
  const before = runs.activeId;
  await ops.runTask("OrderServiceApplication");
  const svc = runs.list.find((r) => r.name === "OrderServiceApplication")!;
  ok(svc?.service === true && layout.panelView === "services" && layout.panel, `服务跑起来亮的是 Services：${JSON.stringify(svc)} ${layout.panelView}`);
  ok(runs.activeId === before && !runs.plain().some((r) => r.id === svc.id), "服务不进运行窗的标签、不抢运行窗的当前格");
  ok(runs.alive(true) === 1 && runs.alive() === runs.plain().filter((r) => r.status === "running" || r.status === "stopping").length, "角标各数各的");
  ok(layout.snapshot().panelView === "term", "Services 不进快照（同运行窗）");

  // 当前格是运行窗最后一格、它右边紧挨着一个服务：关掉它，当前格要挪到运行窗里的邻居，不能挪到服务上（按整张表的下标挑就会）
  await ops.runTask("quick-web");
  const qw2 = runs.list.find((r) => r.name === "quick-web")!;
  ok(runs.list[runs.list.length - 1].id === qw2.id && runs.list[runs.list.length - 2].id === svc.id, `摆成「…, 服务, quick-web」：${runs.list.map((r) => r.name)}`);
  ops.closeRun(qw2.id);
  ok(runs.activeId !== svc.id && runs.plain().some((r) => r.id === runs.activeId), `关掉之后当前格还在运行窗里：${runs.activeId}（服务是 ${svc.id}）`);

  // ⌘F2（不带 id）：Services 开着 → 停那边选中的服务，不是运行窗的当前格
  layout.panelView = "services";
  layout.panel = true;
  ops.focusService(svc.id);
  const plainBefore = runs.list.find((r) => r.id === runs.activeId)!.status;
  await ops.stopRun();
  ok(runs.list.find((r) => r.id === svc.id)?.status === "stopping", `⌘F2 停的是选中的服务：${runs.list.find((r) => r.id === svc.id)?.status}`);
  ok(runs.list.find((r) => r.id === runs.activeId)?.status === plainBefore, "运行窗的当前格没被动");
  await sleep(800);
  ops.focusService(null);
  ops.closeRun(svc.id);
  layout.panelView = "run";
}

// ── 关：关掉一格，当前格挪到邻居；关掉最后一格，运行窗收起 ──
const [a, b] = runs.list.map((r) => r.id);
runs.activeId = a;
ops.closeRun(a);
ok(runs.list.length === 1 && runs.activeId === b, "关掉当前格，当前格挪到剩下那个");
ops.closeRun(b);
ok(runs.list.length === 0 && !layout.panel && layout.panelView === "term", "关掉最后一格，运行窗收起、偏好回到终端");

// ── 停完还占着端口（#59）：「已停止」照常先出来，一秒后同一格摆卡片；「结束进程」只结束它、不重新运行；被重跑换掉的那格不摆 ──
await ops.runTask("linger-boot");
const lg = runs.list.find((r) => r.name === "linger-boot")!.id;
await ops.stopRun(lg);
await sleep(700);
const lgr = () => runs.list.find((r) => r.id === lg);
ok(lgr()?.status === "stopped" && !lgr()?.holder, `已停止先出来、卡片还没到：${lgr()?.status} ${JSON.stringify(lgr()?.holder)}`);
// 桩：停后 600ms 退出、再一秒报占用
await sleep(1100);
ok(lgr()?.lingering === true && lgr()?.holder?.port === 8080, `一秒后同一格摆上「仍被占用」的卡片：${JSON.stringify(lgr()?.holder)}`);
const before = runs.list.length;
await ops.freeLingering(lg);
ok(!lgr()?.holder && runs.list.length === before && lgr()?.status === "stopped", "结束进程：卡片收起、没有重新运行");
await ops.runTask("linger-boot");
const lg2 = runs.list.find((r) => r.name === "linger-boot")!.id;
await ops.stopRun(lg2);
await sleep(650);
await ops.runTask("linger-boot");
await sleep(1000);
const now = runs.list.find((r) => r.name === "linger-boot")!;
ok(now.id !== lg2 && now.status === "running" && !now.holder, `停完马上重跑：卡片不摆到新的那格上（${now.status} ${JSON.stringify(now.holder)}）`);
await ops.stopRun(now.id);
await sleep(1700);

console.log(`${fail === 0 ? "✅" : "❌"} 运行窗状态层：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
