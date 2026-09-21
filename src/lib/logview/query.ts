/**
 * 日志过滤框里那一串字的语法（2026-09-21）。
 *
 * 原来整个框就是一个子串。查问题时最常见的却是「订单号 **且** ERROR」「所有 timeout
 * **除了** healthcheck」—— 一次只能筛一个词，剩下的靠眼睛。语法照 grep / GitHub 搜索
 * 的习惯，不发明新的：
 *
 * ```text
 * a b            两个都得有（AND）
 * -c             不能有
 * "x y"          带空格的整串
 * /re/  -/re/    正则（和引号一样，里面的空格不拆）
 * ```
 *
 * 大小写由过滤条上那个 Aa 统一管，正则也跟着它，不另开 `/i`。
 *
 * **Rust 侧 `logengine::query` 是同一套语法，两边各写一份**：引擎在那边按字节比对，
 * 高亮在这边按字符串画，中间隔着 IPC 和编码转换，共用不了代码。
 * `tests/log-query.test.ts` 和 `query.rs` 的测试用同一组用例，改语法两边一起改。
 */

export type Term = { kind: "lit"; text: string } | { kind: "re"; source: string };

export interface Query {
  include: Term[];
  exclude: Term[];
  /** 有一条正则写坏了（JS 的 RegExp 编不过）。过滤框标红，但其余条件照常生效 */
  bad: boolean;
}

/** 空查询：什么都不筛 */
export const EMPTY_QUERY: Query = { include: [], exclude: [], bad: false };

/**
 * 切词：空白分隔；`"…"` 和 `/…/` 里的空白不算分隔。引号 / 斜杠没闭合就吃到末尾 ——
 * 边打边搜时大半时间它就是没闭合的，不能因此把整串当坏数据。
 */
function tokenize(q: string): string[] {
  const out: string[] = [];
  let i = 0;
  const n = q.length;
  while (i < n) {
    while (i < n && /\s/.test(q[i])) i++;
    if (i >= n) break;
    const start = i;
    // 取反前缀先吃掉，剩下的部分才看是不是引号 / 正则
    if (q[i] === "-" && i + 1 < n && !/\s/.test(q[i + 1])) i++;
    const open = q[i];
    if (open === '"' || open === "/") {
      const close = q.indexOf(open, i + 1);
      i = close < 0 ? n : close + 1;
    } else {
      while (i < n && !/\s/.test(q[i])) i++;
    }
    out.push(q.slice(start, i));
  }
  return out;
}

export function parseQuery(q: string): Query {
  const include: Term[] = [];
  const exclude: Term[] = [];
  let bad = false;
  for (const raw of tokenize(q)) {
    let tok = raw;
    // 单独一个 `-` 是字面量（日志里有 `-` 分隔符的多得是），带东西的才是取反
    const neg = tok.length > 1 && tok.startsWith("-");
    if (neg) tok = tok.slice(1);
    let term: Term;
    if (tok.length >= 2 && tok.startsWith("/") && tok.endsWith("/")) {
      const source = tok.slice(1, -1);
      if (source === "") continue; // `//` 什么都不筛
      try {
        new RegExp(source);
      } catch {
        bad = true;
        continue;
      }
      term = { kind: "re", source };
    } else {
      if (tok.startsWith('"') && tok.endsWith('"') && tok.length >= 2) tok = tok.slice(1, -1);
      else if (tok.startsWith('"')) tok = tok.slice(1); // 没闭合的引号：正在打
      if (tok === "") continue;
      term = { kind: "lit", text: tok };
    }
    (neg ? exclude : include).push(term);
  }
  return { include, exclude, bad };
}

export function isEmptyQuery(q: Query): boolean {
  return q.include.length === 0 && q.exclude.length === 0;
}

function termRegExp(t: Term, caseSensitive: boolean): RegExp {
  const flags = caseSensitive ? "g" : "gi";
  return t.kind === "re"
    ? new RegExp(t.source, flags)
    : new RegExp(t.text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"), flags);
}

/** 一行合不合条件：所有 include 都命中、任一 exclude 都不命中。桩和测试用；真过滤在 Rust */
export function matchLine(text: string, q: Query, caseSensitive: boolean): boolean {
  for (const t of q.include) if (!termRegExp(t, caseSensitive).test(text)) return false;
  for (const t of q.exclude) if (termRegExp(t, caseSensitive).test(text)) return false;
  return true;
}

/**
 * 高亮：把 `text` 切成交替的 [普通, 命中, 普通, 命中, …] 段（奇数位是命中），
 * 和原来 `parse.ts` 的 `highlight` 同一个约定，LogView 那段模板不用动。
 * 只画 include，exclude 的词本来就不该出现在命中行里。重叠的命中合并成一段。
 */
export function highlightQuery(text: string, q: Query, caseSensitive: boolean): string[] {
  if (q.include.length === 0 || text === "") return [text];
  const spans: [number, number][] = [];
  for (const t of q.include) {
    const re = termRegExp(t, caseSensitive);
    for (const m of text.matchAll(re)) {
      if (m[0].length === 0) continue; // 空匹配（`a*` 在非 a 的位置）没东西可画；matchAll 自己会往前走
      spans.push([m.index, m.index + m[0].length]);
    }
  }
  if (spans.length === 0) return [text];
  spans.sort((a, b) => a[0] - b[0]);
  const out: string[] = [];
  let pos = 0;
  let [s, e] = spans[0];
  for (let i = 1; i <= spans.length; i++) {
    const next = spans[i];
    if (next && next[0] <= e) {
      e = Math.max(e, next[1]);
      continue;
    }
    out.push(text.slice(pos, s), text.slice(s, e));
    pos = e;
    if (next) [s, e] = next;
  }
  out.push(text.slice(pos));
  return out;
}
