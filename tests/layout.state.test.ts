import { layout } from "../src/lib/state/layout.svelte";
import { toLayout, type Layout } from "../src/lib/state/session";

let pass = 0, fail = 0;
function ok(c: unknown, message: string) {
  if (c) pass++;
  else { fail++; console.error(`  ✗ ${message}`); }
}

const views: Layout["sideView"][] = ["files", "git", "maven", "gradle", "structure", "db", "agent", "scratch"];
for (const view of views) {
  layout.sidebar = false;
  layout.sidebarWidth = 317;
  layout.toggleSide(view);
  ok(layout.sidebar && layout.sideView === view, `${view}：收起时点导轨展开对应视图`);
  layout.toggleSide(view);
  ok(!layout.sidebar, `${view}：点当前亮着的视图收起`);
  layout.toggleSide(view);
  ok(layout.sidebar && layout.sidebarWidth === 317, `${view}：再展开保留宽度`);
}

layout.showSide("files");
layout.toggleSide("maven");
ok(layout.sidebar && layout.sideView === "maven", "点另一视图是切换，不把侧栏关掉");
layout.showSide("maven");
ok(layout.sidebar, "程序要求显示当前视图时保持展开，showSide 的语义不变");
layout.toggleSide("scratch");
ok(layout.sideShown(false), "无项目时草稿仍能展开");
layout.toggleSide("scratch");
ok(!layout.sideShown(false), "无项目时再次点草稿也能收起");

// 工具窗有不同的工作量：切换后各自记住高度，最大化不覆盖正常尺寸。
layout.resetSizes();
layout.panelView = "git";
layout.panelHeight = 340;
layout.panelView = "term";
ok(layout.panelHeight === 260, "未调整的终端仍使用默认高度");
layout.panelHeight = 220;
layout.panelView = "git";
ok(layout.panelHeight === 340, "Git 恢复自己的高度");
layout.panelMaximized = true;
layout.paneSizes.gitDetail = 420;
const saved = toLayout(JSON.parse(JSON.stringify(layout.snapshot())));
layout.restore(saved);
ok(layout.panelHeight === 340 && layout.paneSizes.gitDetail === 420, "会话恢复工具窗和内部尺寸");
ok(!layout.panelMaximized, "最大化为瞬态，重开保持正常布局");
const bad = toLayout({ paneSizes: { gitDetail: NaN, debugFrames: 1e8, unknown: 100 }, toolHeights: { git: 1, unknown: 100 } });
ok(!("gitDetail" in (bad.paneSizes ?? {})) && bad.paneSizes?.debugFrames === 1200 && !("unknown" in (bad.paneSizes ?? {})), "损坏和未知分栏尺寸在存储边界清理");
ok(bad.toolHeights?.git === 180 && !("unknown" in (bad.toolHeights ?? {})), "工具窗高度限定已知工具和范围");
layout.resetSizes();
ok(layout.panelHeight === 260 && Object.keys(layout.paneSizes).length === 0 && layout.sidebarWidth === 240, "恢复布局清掉尺寸偏好，保留其他工作状态");

console.log(`${fail ? "❌" : "✅"} 侧栏导轨切换：${pass} 通过，${fail} 失败`);
process.exit(fail ? 1 : 0);
