import { EditorState } from "@codemirror/state";
import { SearchQuery } from "@codemirror/search";
import { countMatches, countLabel, MAX_COUNT } from "../src/lib/editor/search-count.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const st = (doc: string, at = 0) =>
  EditorState.create({ doc, selection: { anchor: at } });
const q = (search: string, o: Partial<{ caseSensitive: boolean; regexp: boolean; wholeWord: boolean }> = {}) =>
  new SearchQuery({ search, ...o });

// ── 总数 ───────────────────────────────────────────────────────

const 三个 = st("foo bar foo baz foo");
ok(countMatches(三个, q("foo")).total === 3, "数得出 3 个");
ok(countMatches(三个, q("nope")).total === 0, "一个都没有时是 0");
ok(countMatches(st("Foo foo FOO"), q("foo")).total === 3, "默认不区分大小写");
ok(countMatches(st("Foo foo FOO"), q("foo", { caseSensitive: true })).total === 1, "区分大小写");
ok(countMatches(st("foo foobar"), q("foo", { wholeWord: true })).total === 1, "全词匹配");
ok(countMatches(st("a1 b22 c333"), q("\\d+", { regexp: true })).total === 3, "正则");

// ── 「第几个」：查找会把光标停在匹配的起点上 ─────────────────────

/*
 * `foo bar foo baz foo` 里三个 foo 的起点是 0 / 8 / 16。
 * 判据是「第一个起点 >= 光标」，所以光标正好停在某个起点上时算它自己，
 * 停在它中间或后面就算下一个。
 */
ok(countMatches(st("foo bar foo baz foo", 0), q("foo")).index === 1, "光标在第 1 个的起点上");
ok(countMatches(st("foo bar foo baz foo", 8), q("foo")).index === 2, "光标在第 2 个的起点上");
ok(countMatches(st("foo bar foo baz foo", 16), q("foo")).index === 3, "光标在第 3 个的起点上");
ok(countMatches(st("foo bar foo baz foo", 4), q("foo")).index === 2, "光标在两个之间，算后面那个");
/*
 * 光标在最后一个匹配的**后面**：没有「起点 >= 光标」的匹配了，index 是 0。
 * 显示成 `·/3` —— 这一档必须和「第 3 个」分开，
 * 不然按 ↓ 绕回第 1 个时那个数会从 3 直接跳到 1，中间那一下没有交代。
 */
ok(countMatches(st("foo bar foo baz foo", 19), q("foo")).index === 0, "光标在全部匹配之后");

// ── 闸 ─────────────────────────────────────────────────────────

const 很多 = st("x".repeat(MAX_COUNT + 50));
const c = countMatches(很多, q("x"));
ok(c.capped === true, "命中到上限要标 capped");
ok(c.total === MAX_COUNT, `到上限就停在 ${MAX_COUNT}，不继续数`);

// 4MB 以上不数。造 5MB 字符串比造一个真文件便宜得多
const 巨大 = st("y".repeat(5 << 20));
const big = countMatches(巨大, q("y"));
ok(big.skipped === true, "超过 4MB 直接不数");
ok(big.total === 0, "不数的时候不能给一个假数字");

// 无效正则不能崩
const 坏正则 = q("[unclosed", { regexp: true });
ok(countMatches(st("abc"), 坏正则).skipped === true, "无效正则要跳过，不能抛");

// ── 显示 ───────────────────────────────────────────────────────

ok(countLabel(q(""), countMatches(st("abc"), q(""))) === "", "没输入时不显示任何东西");
ok(countLabel(q("foo"), countMatches(三个, q("foo"))) === "1/3", "3/1 的形状");
ok(countLabel(q("nope"), countMatches(三个, q("nope"))) === "无匹配", "没匹配要说出来");
ok(countLabel(坏正则, countMatches(st("abc"), 坏正则)) === "无效", "坏正则显示「无效」");
ok(countLabel(q("y"), big) === "", "太大不数时留白，而不是显示 0");
ok(countLabel(q("x"), c) === `1/${MAX_COUNT}+`, "到上限要带 +");
ok(
  countLabel(q("foo"), countMatches(st("foo bar foo baz foo", 19), q("foo"))) === "·/3",
  "光标不在任何匹配上时用 ·，不是 0",
);

console.log(`${fail === 0 ? "✅" : "❌"} 查找计数：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
