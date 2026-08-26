/**
 * 轻量模糊匹配 —— ⌘P 与随处搜索的排序依据。
 *
 * 放在前端做是有意为之：每敲一个字符都往 Rust 跑一趟，IPC 往返会让输入发木。
 * 几万条路径在内存里跑一遍子序列匹配是微秒级的。
 *
 * 打分偏好（从强到弱）：
 * 1. 匹配落在文件名而不是目录名上 —— 找文件时脑子里想的是文件名
 * 2. 连续匹配成段
 * 3. 落在词首（`/`、`-`、`_`、`.` 之后，或驼峰的大写处）
 * 4. 整体越靠前越好
 */

export interface Match {
  /** 越大越好 */
  score: number;
  /** 命中字符在原串中的下标，用于高亮 */
  positions: number[];
}

const isBoundary = (s: string, i: number): boolean => {
  if (i === 0) return true;
  const prev = s[i - 1];
  if (prev === "/" || prev === "-" || prev === "_" || prev === "." || prev === " ") return true;
  // 驼峰：小写后接大写
  return prev === prev.toLowerCase() && s[i] !== s[i].toLowerCase();
};

/**
 * 贪心子序列匹配。返回 null 表示不匹配。
 *
 * 不做全局最优（那要动态规划，几万条路径上不划算）——
 * 贪心 + 词首优先的启发式，在实际路径上排序质量已经够好。
 */
export function fuzzyMatch(text: string, query: string): Match | null {
  if (!query) return { score: 0, positions: [] };

  const lowText = text.toLowerCase();
  const lowQuery = query.toLowerCase();
  const positions: number[] = [];

  let ti = 0;
  for (let qi = 0; qi < lowQuery.length; qi++) {
    const c = lowQuery[qi];
    // 优先找词首位置上的匹配，找不到再退回最近的任意匹配
    let found = -1;
    for (let k = ti; k < lowText.length; k++) {
      if (lowText[k] !== c) continue;
      if (found < 0) found = k;
      if (isBoundary(text, k)) {
        found = k;
        break;
      }
    }
    if (found < 0) return null;
    positions.push(found);
    ti = found + 1;
  }

  // ── 打分
  let score = 0;
  const slash = text.lastIndexOf("/");
  for (let i = 0; i < positions.length; i++) {
    const p = positions[i];
    score += 10;
    if (p > slash) score += 12; // 落在文件名而非目录上
    if (isBoundary(text, p)) score += 8;
    if (i > 0 && p === positions[i - 1] + 1) score += 10; // 连续
  }
  // 越靠前越好；路径越短越好（同样匹配时偏向浅层文件）
  score -= positions[0] * 0.4;
  score -= text.length * 0.05;
  return { score, positions };
}

export interface Ranked<T> {
  item: T;
  score: number;
  positions: number[];
}

/** 对候选集打分排序，返回前 limit 条 */
export function rank<T>(
  items: T[],
  query: string,
  key: (t: T) => string,
  limit: number,
): Ranked<T>[] {
  const out: Ranked<T>[] = [];
  for (const item of items) {
    const m = fuzzyMatch(key(item), query);
    if (m) out.push({ item, score: m.score, positions: m.positions });
  }
  out.sort((a, b) => b.score - a.score);
  return out.slice(0, limit);
}

/** 把命中位置切成 [普通, 命中, 普通, ...]，供高亮渲染 */
export function segments(text: string, positions: number[]): { t: string; hit: boolean }[] {
  if (positions.length === 0) return [{ t: text, hit: false }];
  const out: { t: string; hit: boolean }[] = [];
  let cursor = 0;
  for (let i = 0; i < positions.length; ) {
    const start = positions[i];
    let end = start + 1;
    // 合并连续命中，少产生几个 span
    while (i + 1 < positions.length && positions[i + 1] === end) {
      end++;
      i++;
    }
    i++;
    if (start > cursor) out.push({ t: text.slice(cursor, start), hit: false });
    out.push({ t: text.slice(start, end), hit: true });
    cursor = end;
  }
  if (cursor < text.length) out.push({ t: text.slice(cursor), hit: false });
  return out;
}
