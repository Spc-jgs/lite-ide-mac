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

// ── 关：关掉一格，当前格挪到邻居；关掉最后一格，运行窗收起 ──
const [a, b] = runs.list.map((r) => r.id);
runs.activeId = a;
ops.closeRun(a);
ok(runs.list.length === 1 && runs.activeId === b, "关掉当前格，当前格挪到剩下那个");
ops.closeRun(b);
ok(runs.list.length === 0 && !layout.panel && layout.panelView === "term", "关掉最后一格，运行窗收起、偏好回到终端");

console.log(`${fail === 0 ? "✅" : "❌"} 运行窗状态层：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
