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

/*
 * 词首判定。**这是整个函数的热点** —— 内层扫描对每一个候选字符都调它一次，
 * 候选字符数是「文本长度 × query 长度」量级。
 *
 * 原来写的是 `prev === prev.toLowerCase() && s[i] !== s[i].toLowerCase()`，
 * 每次比较分配两个单字符串：5 万条路径上 fuzzyMatch 整体 13.4ms，
 * 大头就在这里（对照：5 万次 indexOf 只要 0.9ms）。改成码点比较之后不再分配。
 *
 * 非 ASCII 退回原来的字符串写法：那条路极罕见（路径里出现西里尔/希腊大写字母），
 * 不值得为它把语义改掉 —— 码点版把「任何非 ASCII 大写」都当成非大写，
 * 跟原来的 toLowerCase 判断并不等价。
 */
const isBoundary = (s: string, i: number): boolean => {
  if (i === 0) return true;
  const prev = s.charCodeAt(i - 1);
  // 展开成比较链而不是 SEP.has(prev)：这里是最内层，实测 5 万条路径上
  // 「handler」一次击键 9.9ms → 7.3ms，一个 Set 查找就值这么多
  if (prev === 47 || prev === 45 || prev === 95 || prev === 46 || prev === 32) return true; // / - _ . 空格
  const cur = s.charCodeAt(i);
  // 驼峰：非大写之后接一个大写
  if (prev < 128 && cur < 128) return !(prev >= 65 && prev <= 90) && cur >= 65 && cur <= 90;
  const p = s[i - 1];
  const c = s[i];
  return p === p.toLowerCase() && c !== c.toLowerCase();
};

/**
 * 「每个 query 字符最右能落在哪」的暂存区。
 *
 * 模块级复用而不是每次 new：`rank` 会对**每一条候选**调一次 fuzzyMatch，
 * 5 万条就是 5 万个数组。JS 单线程且这里没有 await，复用是安全的。
 */
let capBuf = new Int32Array(64);

/**
 * 贪心子序列匹配。返回 null 表示不匹配。
 *
 * 两趟：
 *
 * 1. **从右往左**贪心，算出每个 query 字符「最右可以落在哪还不耽误后面」。
 *    这一趟同时就是匹配性判定 —— 配不完就是真的不匹配。
 * 2. 从左往右按「词首优先」挑位置，但**不许越过第一趟给的上界**。
 *
 * 第二趟的上界是这次重写的重点。原来只有第二趟，词首优先会让它跳到一个
 * 更靠右的词首上，跳过去发现剩下的 query 配不完了，就直接返回 null ——
 * 于是 `readme` 搜不到 `README.md`（跳到 `.md` 的 m 上，回头找不到 e），
 * `notify` 搜不到 `notify.svelte.ts`。明明存在的匹配被判成不匹配，
 * 而界面上只写「没有匹配」，根本看不出是算法跳过头了。
 *
 * 有了上界，第二趟保证成功，也保证只在原来会失败的候选上产生新结果 ——
 * 原来能匹配上的，选出的位置和分数一个字节都不变（它们选的位置本来就
 * 满足「后面配得完」，也就必然在上界之内）。
 *
 * 仍然不做全局最优：那要动态规划，几万条路径上不划算。
 */
export function fuzzyMatch(text: string, query: string): Match | null {
  if (!query) return { score: 0, positions: [] };

  const lowText = text.toLowerCase();
  const lowQuery = query.toLowerCase();
  const n = lowText.length;
  const q = lowQuery.length;
  if (q > n) return null;

  // ── 第一趟：从右往左，算每个 query 字符的最右可行位置
  if (capBuf.length < q) capBuf = new Int32Array(q * 2);
  const cap = capBuf;
  let tj = n - 1;
  for (let qi = q - 1; qi >= 0; qi--) {
    const c = lowQuery.charCodeAt(qi);
    while (tj >= 0 && lowText.charCodeAt(tj) !== c) tj--;
    if (tj < 0) return null;
    cap[qi] = tj--;
  }

  // ── 第二趟：从左往右，词首优先，上界卡住
  const positions: number[] = new Array(q);
  let ti = 0;
  for (let qi = 0; qi < q; qi++) {
    const c = lowQuery.charCodeAt(qi);
    const hi = cap[qi];
    // 优先找词首位置上的匹配，找不到再退回最近的任意匹配
    let found = -1;
    for (let k = ti; k <= hi; k++) {
      if (lowText.charCodeAt(k) !== c) continue;
      if (found < 0) found = k;
      if (isBoundary(text, k)) {
        found = k;
        break;
      }
    }
    /*
     * 按第一趟的构造，hi 上一定坐着一个 c，所以这里走不到。
     * 留着是因为**走到了就是崩**：found = -1 会让下面 isBoundary(text, -1)
     * 读到 undefined 直接抛，而这是搜索面板，不该因为一个候选炸掉整个列表。
     * 代价是每个 query 字符一次分支判断，量级上看不见。
     */
    if (found < 0) return null;
    positions[qi] = found;
    ti = found + 1;
  }

  // ── 打分
  let score = 0;
  const slash = text.lastIndexOf("/");
  for (let i = 0; i < q; i++) {
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
  if (limit <= 0) return [];
  const out: Ranked<T>[] = [];

  /*
   * limit 远小于候选集时走有界插入：⌘P 只要前 40 条，为它排 5 万条纯属白烧
   * （实测全量 sort 5.7ms，占 16ms 单帧预算的三分之一）。
   *
   * 反过来 limit 和候选集一个量级时，插入的 splice 会退化成 O(n·limit)，
   * 那种情况（「把匹配的都给我」）直接全量排更快。
   */
  const bounded = limit * 4 < items.length;
  let worst = -Infinity;

  for (const item of items) {
    const m = fuzzyMatch(key(item), query);
    if (!m) continue;
    const r = { item, score: m.score, positions: m.positions };
    if (!bounded) {
      out.push(r);
      continue;
    }
    // 已经满了而且分数还不如最后一名：同分也丢 —— 同分靠后的本来就该排在后面
    if (out.length === limit && m.score <= worst) continue;
    /*
     * 插到第一个「分数比它低」的位置之前，也就是同分的一律排在已有项之后。
     * 这一条是**必须的**：Array.sort 在 V8 里是稳定的，⌘P 的候选按目录序来，
     * 同分时打乱顺序表现为「同一个 query 两次结果不一样」，而人是靠
     * 肌肉记忆按第几条的。朴素的 top-K 堆没有这个性质。
     */
    let lo = 0;
    let hi = out.length;
    while (lo < hi) {
      const mid = (lo + hi) >> 1;
      if (out[mid].score >= m.score) lo = mid + 1;
      else hi = mid;
    }
    out.splice(lo, 0, r);
    if (out.length > limit) out.pop();
    if (out.length === limit) worst = out[limit - 1].score;
  }

  if (bounded) return out;
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
