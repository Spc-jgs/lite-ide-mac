/**
 * 「跟随尾部」开着时，这一刻要不要把视图贴到最新一行。
 *
 * 原来只要跟随开着，行数一变就贴底：服务每秒几十行日志，人往上翻堆栈，每来一行就被拽回最底下 —— 跟随开着的时候根本没法往上看。
 * 现在照聊天应用流式输出的做法（Claude 桌面端、终端都是这样）：**人往上滚就松开，滚回底部附近再跟上**，跟随这个开关本身不变。
 *
 * 判「人要往上看」看的是**意图**（滚轮往上、↑ / PageUp、滚动条往上拖），不是「离底部多远」：
 * 新行密的时候，人往上滚的那几像素和程序贴底挤在同一帧里，程序贴完量到的距离是 0，只按距离判就把人那一下吃掉了。
 * 跟上要两样都满足：这一次是往下滚的、离底部不超过 `SLACK`。程序自己贴底也是「往下、距离 0」，所以不会被它误判成松开。
 */

/** 离底部多近算「回到底部了」：滚轮一格大约 40–100px，留一格以内的余量 */
export const SLACK = 40;

export type ScrollSample = {
  /** 这次滚动之前 / 之后的 scrollTop */
  prevTop: number;
  top: number;
  /** 滚完之后离底部还有多远（scrollHeight - scrollTop - clientHeight） */
  distance: number;
};

/** 一次滚动之后还跟不跟 */
export function pinAfterScroll(pinned: boolean, s: ScrollSample): boolean {
  // 往上了、而且离开了底部：人在往上看（拖滚动条走的是这条，滚轮和键盘在 wantsUp 那里已经先松开了）。
  // 往上了但还贴着底部：内容变短、浏览器把 scrollTop 夹回来了（换过滤条件），不是人动的
  if (s.top < s.prevTop) return s.distance <= 1 ? pinned : false;
  if (s.top > s.prevTop) return s.distance <= SLACK ? true : pinned;
  return pinned;
}

/** 人明确往上看的输入：滚轮往上、↑、PageUp、Home。这些一来就松开，不等滚动事件 */
export function wantsUp(e: { type: string; deltaY?: number; key?: string }): boolean {
  if (e.type === "wheel") return (e.deltaY ?? 0) < 0;
  if (e.type === "keydown") return e.key === "ArrowUp" || e.key === "PageUp" || e.key === "Home";
  return false;
}
