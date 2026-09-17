/**
 * md-blocks：块级解析器吃行的逻辑。
 *
 * 不起真的 lezer 解析器 —— 这里只验「从哪行吃到哪行、产什么节点」，用一个按行走的
 * 假 `Cx` 就够：`peekLine` 看下一行、`nextLine` 走一行、`addElement` 记下产物。
 * 判据也是假的（以 `LOG` / `+`/`-`/`@@` 开头），被测的是吃行规则，不是正则。
 */
import { makeLogDiffBlocks, eat } from "../src/lib/editor/md-blocks.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const blocks = makeLogDiffBlocks({
  logLine: (t) => t.startsWith("LOG"),
  stackLine: (t) => t.startsWith("\tat "),
  diffKind: (t) => (/^(diff --git |@@|[-+ ])/.test(t) ? "x" : null),
});
const [logP, diffP] = blocks.parseBlock;

/** 一个按行走的假上下文：从第 `at` 行开始，当前行就是 `lines[at]` */
function ctx(lines: string[], at: number) {
  const starts: number[] = [];
  let pos = 0;
  for (const l of lines) {
    starts.push(pos);
    pos += l.length + 1;
  }
  const out: { type: string; fromLine: number; toLine: number }[] = [];
  let i = at;
  const cx = {
    get lineStart() {
      return starts[i];
    },
    peekLine: () => (i + 1 < lines.length ? lines[i + 1] : ""),
    nextLine: () => (i + 1 < lines.length ? (i++, true) : false),
    elt: (type: string, from: number, to: number) => ({ type, from, to }),
    addElement: (e: any) => {
      out.push({
        type: e.type,
        fromLine: starts.findIndex((s, k) => s <= e.from && (k + 1 >= starts.length || starts[k + 1] > e.from)),
        toLine: starts.findIndex((s, k) => s <= e.to && (k + 1 >= starts.length || starts[k + 1] > e.to)),
      });
    },
  };
  const line = {
    get text() {
      return lines[i];
    },
    basePos: 0,
  };
  return { cx, line, out, cur: () => i };
}

// ── 日志块 ──
{
  const lines = ["prose", "LOG 1", "LOG 2", "\tat x.y(Z.java:1)", "LOG 3", "", "LOG lonely", "after"];
  const c = ctx(lines, 1);
  ok(logP.parse(c.cx, c.line) === true, "两行以上的日志起块");
  ok(c.out.length === 1 && c.out[0].type === "LogBlock", "产一个 LogBlock");
  ok(c.out[0].fromLine === 1 && c.out[0].toLine === 4, `块盖 1–4 行（堆栈行续段），实得 ${c.out[0].fromLine}–${c.out[0].toLine}`);
  ok(c.cur() === 5, `吃完停在空行（第 5 行），实得 ${c.cur()}`);
}
{
  const c = ctx(["LOG only one", "prose"], 0);
  ok(logP.parse(c.cx, c.line) === false && c.out.length === 0, "只有一行像日志不起块");
}
{
  const c = ctx(["LOG a", "LOG b"], 0);
  logP.parse(c.cx, c.line);
  ok(c.out[0]?.toLine === 1 && c.cur() === 1, "到文档末尾也能收尾");
}
{
  const c = ctx(["prose", "LOG a", "LOG b"], 1);
  ok(logP.endLeaf!(c.cx, c.line) === true, "正文后面紧接两行日志：打断段落");
  const d = ctx(["prose", "LOG a", "prose"], 1);
  ok(logP.endLeaf!(d.cx, d.line) === false, "只有一行日志：不打断");
}
{
  const c = ctx(["\tat x", "LOG a", "LOG b"], 0);
  ok(logP.parse(c.cx, c.line) === false, "堆栈行不能起块");
}

// ── diff 块 ──
{
  const lines = ["diff --git a/x b/x", "--- a/x", "+++ b/x", "@@ -1 +1 @@", "-a", "+b", " ctx", "", "tail"];
  const c = ctx(lines, 0);
  ok(diffP.parse(c.cx, c.line) === true, "diff --git 起块");
  ok(c.out[0]?.type === "DiffBlock" && c.out[0].fromLine === 0 && c.out[0].toLine === 6, `块盖 0–6 行，实得 ${c.out[0]?.fromLine}–${c.out[0]?.toLine}`);
}
{
  const c = ctx(["@@ -1 +1 @@", "-a", "+b", "prose"], 0);
  diffP.parse(c.cx, c.line);
  ok(c.out[0]?.toLine === 2 && c.cur() === 3, "不像 diff 的行断块，停在那一行上");
}
{
  const c = ctx(["--- a/x", "+++ b/x"], 0);
  ok(diffP.parse(c.cx, c.line) === false, "光有 --- / +++ 不起块（正文里 --- 太常见）");
}

// ── eat：空行断 ──
{
  const c = ctx(["LOG a", "", "LOG b"], 0);
  eat(c.cx, c.line, "X", () => true);
  ok(c.out[0]?.toLine === 0, "空行永远断块，哪怕 cont 说续");
}

console.log(`md-blocks: ${pass} 过, ${fail} 败`);
if (fail) process.exit(1);
