/**
 * 日志归类（docs/PATTERNS.md）里点一类 → 过滤框里的条件。纯函数，`tests/pattern-query.test.ts` 测。
 *
 * 模板是「常量 + `<*>`」：`<*> INFO [http-nio-<*>-exec-<*>] c.d.Order - 下单 userId=<*> sku=A-<*>`。
 * 把 `<*>` 之间的常量词**加引号**当成 AND 条件（过滤框的语法，`query.ts`）：这一类的每一行都含着这些段，别的类基本不会全含。
 * 不新造一种「按类号筛」的过滤：那要给每一行记类号（1GB 日志 36MB），而这条路复用了现成的过滤、高亮、只看命中 / 全文、F3。
 * 代价是**超集**：理论上别的行也可能恰好含着全部这几段。界面上那条说的是「按这个模板过滤」，不是「这一类的第几行」。
 */

/** 一段里有几个字（字母、数字、汉字） */
const letters = (s: string) => (s.match(/[\p{L}\p{N}]/gu) ?? []).length;

/** 最多取几个词：一行 JSON 的模板能切出几十个，全塞进过滤框既看不清、也没必要 —— 挑最长的几个，按原来的先后排 */
const MAX_SEGS = 6;

export function templateQuery(template: string): string {
  /*
   * **按词取，不按整段取**：模板是切词之后用单个空格拼回去的，原行里的空白不一定是一个 —— Logback 把级别补齐成五格（`INFO  [`），
   * 整段加引号的 `"INFO [http-nio-"` 在原行里一次都对不上，点了一类筛出来是空的（第一版的测试拿 `replace("  ", " ")` 把这个盖住了）。
   * 词和词之间的空白本来就不带信息。带引号的词不要：过滤框的引号里不能再有引号（没有转义）
   */
  const segs: { text: string; at: number }[] = [];
  let at = 0;
  for (const raw of template.split("<*>")) {
    for (const w of raw.split(/\s+/)) {
      at++;
      if (w && !w.includes('"') && letters(w) >= 2) segs.push({ text: w, at });
    }
  }
  const seen = new Set<string>();
  const uniq = segs.filter((x) => !seen.has(x.text) && seen.add(x.text));
  const pick = [...uniq].sort((a, b) => b.text.length - a.text.length).slice(0, MAX_SEGS);
  // 一律加引号：段里有空格要它；以 `-` 开头的不加会被当成「排除」，以 `/` 开头的会被当成正则
  return uniq
    .filter((x) => pick.includes(x))
    .map((x) => `"${x.text}"`)
    .join(" ");
}

/** 模板切成「常量 / 变量」两种段，画的时候变量压暗 */
export function templateParts(template: string): { text: string; wild: boolean }[] {
  const out: { text: string; wild: boolean }[] = [];
  template.split("<*>").forEach((s, i) => {
    if (i > 0) out.push({ text: "<*>", wild: true });
    if (s) out.push({ text: s, wild: false });
  });
  return out;
}

/**
 * 画在列表里的模板：**去掉开头那几个没有字的词**（`<*>-<*>-<*> <*>:<*>:<*>.<*>` 这类时间戳形状、纯数字的序号）。
 * 每一类都以它们开头，一行四成宽度花在一样的东西上，真正区分各类的话被挤到省略号后面（2026-10-14 在浏览器里一眼看出来的）。
 * 只改画法：过滤条件照旧从完整模板来，tooltip 给全文
 */
export function templateShown(template: string): string {
  const words = template.split(" ");
  let i = 0;
  while (i < words.length - 1 && letters(words[i].replaceAll("<*>", "")) === 0) i++;
  return words.slice(i).join(" ");
}

/** 变化的说法：涨跌不到这个倍数算「持平」—— 两天的日志不会一模一样，±50% 以内多半是业务量的起伏，不值得标出来 */
const FLAT = 1.5;

/**
 * 对比时一类的变化怎么说（Rust 侧 `change_rank` 按同一套排的序）：基准那边没有 → 新出现；这边没有 → 没了；
 * 两边都有 → 按**频率**（次数 / 那边一共多少行）比，不按次数比 —— 今天的日志长了一倍，每一类次数都翻倍，那不是变化
 */
export function changeLabel(count: number, base: number, lines: number, baseLines: number): { text: string; kind: "new" | "gone" | "up" | "down" | "flat" } {
  if (base === 0) return { text: "新出现", kind: "new" };
  if (count === 0) return { text: "已消失", kind: "gone" };
  const r = count / Math.max(1, lines) / (base / Math.max(1, baseLines));
  if (r >= FLAT) return { text: `×${r >= 10 ? Math.round(r) : r.toFixed(1)}`, kind: "up" };
  if (r <= 1 / FLAT) return { text: `÷${1 / r >= 10 ? Math.round(1 / r) : (1 / r).toFixed(1)}`, kind: "down" };
  return { text: "持平", kind: "flat" };
}
