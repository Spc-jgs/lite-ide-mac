/**
 * 查找面板上那个「第几个 / 共几个」。
 *
 * 单独一个文件、**不碰 `@codemirror/view`** —— 和 `jump.ts` / `jump-ext.ts`
 * 是同一个分法。`view` 那个包在模块加载时就要 `document`，混进来的话
 * 这段纯逻辑就再也不能在 node 里直接跑了，而它正是这里唯一值得测的东西。
 */

import type { SearchQuery } from "@codemirror/search";
import type { EditorState } from "@codemirror/state";

/** 命中数上限：显示成 `999+`。人不会去数第 1000 个 */
export const MAX_COUNT = 999;
/** 超过这个大小不数。扫全文这件事在 4MB 上就已经不是「顺手」了 */
export const MAX_COUNT_BYTES = 4 << 20;

export interface Counted {
  /** 光标所在的是第几个（1 起）。不在任何一个匹配上时是 0 */
  index: number;
  total: number;
  /** 到上限截断了 —— 显示要带个 `+` */
  capped: boolean;
  /** 文档太大或查询无效，压根没数 */
  skipped: boolean;
}

/**
 * 数一遍匹配，顺便算出光标现在落在第几个上。
 *
 * **一趟扫完，不是数两遍。** 「共几个」和「第几个」分开算的话要扫两次全文，
 * 而它们要的是同一串位置。判「第几个」用的是 `from >= 光标的 from`：
 * 查找会把光标停在匹配的**起点**上，而按下一个之后光标落在新匹配的起点，
 * 所以「第一个起点不早于光标的匹配」就是当前这个。
 */
export function countMatches(state: EditorState, query: SearchQuery): Counted {
  const none: Counted = { index: 0, total: 0, capped: false, skipped: false };
  if (!query.valid) return { ...none, skipped: true };
  if (state.doc.length > MAX_COUNT_BYTES) return { ...none, skipped: true };

  const at = state.selection.main.from;
  const cursor = query.getCursor(state);
  let total = 0;
  let index = 0;
  for (;;) {
    const next = cursor.next();
    if (next.done) break;
    total++;
    if (index === 0 && next.value.from >= at) index = total;
    if (total >= MAX_COUNT) return { index, total, capped: true, skipped: false };
  }
  return { index, total, capped: false, skipped: false };
}

/** 显示成 `3/17`。没有匹配、没数、还没输入时给不同的字 */
export function countLabel(q: SearchQuery, c: Counted): string {
  if (!q.search) return "";
  if (c.skipped) return q.valid ? "" : "无效";
  if (c.total === 0) return "无匹配";
  return `${c.index || "·"}/${c.total}${c.capped ? "+" : ""}`;
}
