/**
 * 工具窗的编号键（`shell/tool-keys.ts`，docs/TAKEOVER.md G7）：按一下开、开着再按收起；没有内容可显示时说一句、不开空壳；
 * ⌘5 调试找的是「调试着起的、还在跑的」那一格，服务去 Services、别的去运行窗
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { layout } = await import("../src/lib/state/layout.svelte");
const { runs } = await import("../src/lib/state/runs.svelte");
const { notify } = await import("../src/lib/state/notify.svelte");
const { git } = await import("../src/lib/state/git.svelte");
const { toggleToolKey } = await import("../src/lib/shell/tool-keys");

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

// 问题：没开 → 开；开着 → 收起
layout.panel = false;
toggleToolKey("tw-problems");
ok(layout.panel && layout.panelView === "problems", "⌘6：开问题工具窗");
toggleToolKey("tw-problems");
ok(!layout.panel, "⌘6 再按一下：收起");
// 开着的是别的工具窗：切过来，不是收起
layout.panelView = "term";
layout.panel = true;
toggleToolKey("tw-problems");
ok(layout.panel && layout.panelView === "problems", "终端开着时按 ⌘6：切到问题，不是收起");

// 运行：一次都没跑过 → 说一句、不开空壳
layout.panel = false;
runs.list = [];
toggleToolKey("tw-run");
ok(!layout.panel && notify.toast?.text.includes("还没跑过任务"), `⌘4 没跑过：不开、说为什么（${notify.toast?.text}）`);

// Git：不是仓库 → 说一句
git.repo = null;
toggleToolKey("tw-git");
ok(!layout.panel && notify.toast?.text.includes("不是 Git 仓库"), "⌘9 不是仓库：不开、说为什么");

// 调试：找调试着起、还在跑的那一格（最近的那个）；跑完了的不算
const tab = (id: number, over: Record<string, unknown>) =>
  ({ id, root: "/proj", name: `t${id}`, command: "", log: "", status: "running", code: null, holder: null, service: false, startedAt: 0, tests: null, debug: null, http: null, ...over }) as never;
const dbg = { run: 0, phase: "paused", note: "", thread: null, reason: "", frames: [], totalFrames: 0, threads: [] };
// 跑完了的那一格放在后面：只按「最近的」挑的话会挑到它
runs.list = [tab(1, { debug: dbg }), tab(2, { debug: dbg, status: "done" }), tab(3, {})];
runs.activeId = 3;
toggleToolKey("tw-debug");
ok(layout.panel && layout.panelView === "run" && runs.activeId === 1, `⌘5：切到调试着、还在跑的那一格（1），不是更新但跑完了的（2）也不是没调试的（3）：${runs.activeId}`);
runs.list = [tab(3, {})];
layout.panel = false;
toggleToolKey("tw-debug");
ok(!layout.panel && notify.toast?.text.includes("没有在调试"), "⌘5 没有在调试的：说一句");

console.log(`${fail === 0 ? "✅" : "❌"} 工具窗的编号键：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
