/**
 * 图片预览的纯逻辑（src/lib/image/zoom.ts，docs/TAKEOVER.md G26）：适应怎么算、± 跳到哪一档、信息栏怎么写。
 */
import { MAX, MIN, PAD, bytes, fitScale, kind, percent, pinch, step } from "../src/lib/image/zoom.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

// 适应：小图原大（不放大成马赛克），大图按短的那一边缩，四周留 PAD
eq(fitScale(16, 16, 800, 600), 1, "16px 图标不放大");
eq(fitScale(2000, 1000, 1000 + 2 * PAD, 2000), 0.5, "宽超了按宽缩");
eq(fitScale(1000, 4000, 4000, 2000 + 2 * PAD), 0.5, "高超了按高缩");
eq(fitScale(0, 0, 800, 600), 1, "还没量出尺寸时当原大");
ok(fitScale(100000, 100000, 800, 600) >= MIN, "再大也不缩过下限");

// ± 按档跳：从一档出发到相邻一档；从两档之间（适应、捏合出来的）出发，跳到那一侧最近的一档 —— 不会原地不动
eq(step(1, 1), 1.5, "100% → 150%");
eq(step(1, -1), 0.75, "100% → 75%");
eq(step(0.6, 1), 0.75, "60%（适应出来的）放大到 75%");
eq(step(0.6, -1), 0.5, "60% 缩小到 50%");
eq(step(MAX, 1), MAX, "到顶不再大");
eq(step(MIN, -1), MIN, "到底不再小");
// 来回按一下回到原处（乘固定倍数做不到这一点）
eq(step(step(1, 1), -1), 1, "放大再缩小回到 100%");

// 捏合：往上推（deltaY < 0）放大，夹在上下限里
ok(pinch(1, -10) > 1 && pinch(1, 10) < 1, "捏合方向");
eq(pinch(MAX, -1000), MAX, "捏合不过上限");

eq(percent(1), "100%", "整数百分比");
eq(percent(0.05), "5%", "小比例去掉 .0");
eq(percent(0.0625), "6.3%", "小于 10% 留一位");
eq(bytes(98), "98 B", "字节");
eq(bytes(2048), "2 KB", "KB 去掉 .0");
eq(bytes(1536 * 1024), "1.5 MB", "MB");
eq(kind("image/svg+xml"), "SVG", "svg+xml");
eq(kind("image/x-icon"), "ICO", "x-icon");
eq(kind("image/png"), "PNG", "png");

console.log(fail === 0 ? `✅ 图片缩放：${pass} 通过，0 失败` : `❌ 图片缩放：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
