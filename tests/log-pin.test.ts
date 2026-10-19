/**
 * 跟随尾部时人往上翻不能被拽回去（src/lib/logview/pin.ts）。原来行数一变就贴底：服务一直在打日志，往上看堆栈每来一行就被拽回最底下。
 */
import { pinAfterScroll, wantsUp, SLACK } from "../src/lib/logview/pin.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// 程序自己贴底（新行来了）：往下、距离 0 —— 照旧跟着
ok(pinAfterScroll(true, { prevTop: 1000, top: 1400, distance: 0 }), "程序贴底那次滚动不算人松开");

// 人滚轮往上：wantsUp 当场松开；紧跟着那次滚动事件只往上挪了 30px，还在 SLACK 以内 —— 不能因为「离底部很近」又接回去
ok(wantsUp({ type: "wheel", deltaY: -12 }), "滚轮往上 = 要往上看");
ok(!pinAfterScroll(false, { prevTop: 1400, top: 1370, distance: 30 }), "松开之后往上那一点点滚动不能把跟随接回来（只按距离判就会）");

// 拖滚动条往上（没有滚轮 / 键盘事件，只有滚动）：离开底部就松开
ok(!pinAfterScroll(true, { prevTop: 1400, top: 600, distance: 800 }), "拖滚动条往上离开底部：松开");

// 往下翻回底部附近：跟上；往下了但还很远：不跟
ok(pinAfterScroll(false, { prevTop: 600, top: 1380, distance: SLACK - 10 }), "往下滚回底部附近：跟上");
ok(!pinAfterScroll(false, { prevTop: 600, top: 900, distance: 500 }), "往下了但离底部还远：还不跟");

// 内容变短、浏览器把 scrollTop 夹回来（往上、但贴着底部）：不是人动的，跟随不变
ok(pinAfterScroll(true, { prevTop: 1400, top: 800, distance: 0 }), "内容变短被夹回来不算松开");

// 键盘：↑ / PageUp / Home 是往上看；↓ / End / 滚轮往下不是
ok(wantsUp({ type: "keydown", key: "ArrowUp" }) && wantsUp({ type: "keydown", key: "PageUp" }) && wantsUp({ type: "keydown", key: "Home" }), "↑ / PageUp / Home 松开");
ok(!wantsUp({ type: "keydown", key: "ArrowDown" }) && !wantsUp({ type: "keydown", key: "End" }) && !wantsUp({ type: "wheel", deltaY: 8 }), "↓ / End / 滚轮往下不松开");

console.log(`${fail === 0 ? "✅" : "❌"} 跟随尾部不抢滚动：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
