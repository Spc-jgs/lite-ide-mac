/**
 * 草稿本（没有项目的那个窗口，docs/SCRATCH.md 第七节）恢复时，哪些标签不再摆回标签栏。
 *
 * 「在草稿本新建」每按一次多一张（当前那张空的除外），用一周就是几十个标签 —— 草稿本会变成第二个乱糟糟的 Sublime。
 * 所以一张草稿**三天没改过**，下次打开草稿本时就不摆了：文件还在，草稿列表里、⌘P 里都找得到。标签栏放这几天的，列表放全部的。
 *
 * 不摆的只有「草稿本里、三天没改过的草稿」。这几样一律留着：
 * - **有没保存的内容的**（快照里带 `draft`）—— 「旧」不是丢东西的理由；
 * - 钉住的；
 * - 不是草稿的（草稿本里随手开着的日志、散文件）；
 * - 不知道多久没改的（草稿列表里没有它、或者取不到修改时间）—— 拿不准就留着，猜错的方向得是「多留一个标签」
 *
 * 「多久没改」看**盘上的修改时间**（`fileStamp`，只问 `mayCollapse` 的那几张），不看会话快照里的 `stamp`：
 * 快照只给**有没保存内容**的标签存时间戳（`session.ts` 里 stamp 跟着 draft 走），干净的草稿标签根本没有 ——
 * 第一版照快照的写，状态测试走真的 `session.parse`，它把时间戳丢了，一张都收不起来（2026-10-13）
 */
import type { TabSnap } from "./session";

export const SCRATCHBOOK_KEEP_DAYS = 3;

/**
 * 这一张**有没有可能**被收起：是草稿、没钉住、没有没保存的字。只有这几张要去盘上问修改时间 ——
 * 调用方据此只问它们（原来是先列整个草稿目录：每一份都打开读首行，用几个月几百份时首屏要先等它，code review 2026-10-13）
 */
export function mayCollapse(t: TabSnap, isScratch: (path: string) => boolean): boolean {
  return isScratch(t.path) && !t.pinned && t.draft === undefined;
}

export function keepInScratchbook(
  t: TabSnap,
  now: number,
  isScratch: (path: string) => boolean,
  mtimeOf: (path: string) => number | undefined,
): boolean {
  if (!mayCollapse(t, isScratch)) return true;
  const m = mtimeOf(t.path);
  if (!m) return true;
  return now - m < SCRATCHBOOK_KEEP_DAYS * 24 * 3600 * 1000;
}
