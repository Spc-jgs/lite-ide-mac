/**
 * 整篇换内容时的按行最小改动（`editor/doc-diff.ts`）：结果文本一个字不差；**断点跟着没动的行走、不丢** ——
 * 用的是真的 `GutterMarker`（它的映射方式 `TrackBefore`：前面那个字符被删，标记就没了）和 CM6 真的映射，不是照着它的规矩另写一份
 */
import { EditorState, RangeSet } from "@codemirror/state";
import { GutterMarker } from "@codemirror/view";
import { lineChanges } from "../src/lib/editor/doc-diff.ts";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

class Bp extends GutterMarker {}

/** 在这些行（从 1 数）下断点，把 before 换成 after，返回结果文本和断点落在哪几行（按那一行的内容报，好读） */
function run(before: string, after: string, lines: number[], whole = false) {
  const s = EditorState.create({ doc: before });
  const set = RangeSet.of(lines.map((l) => new Bp().range(s.doc.line(l).from)), true);
  const changes = whole ? [{ from: 0, to: before.length, insert: after }] : lineChanges(before, after);
  const t = s.update({ changes });
  const at: string[] = [];
  for (const it = set.map(t.changes).iter(); it.value; it.next()) at.push(t.state.doc.lineAt(it.from).text.trim());
  return { text: t.state.doc.toString(), at };
}
const same = (a: string[], b: string[]) => JSON.stringify(a) === JSON.stringify(b);

const JAVA = `package shop;

import java.util.List;

public class Money {
    public String show(long cents) {
        String s = fmt(cents);
        return s;
    }

    private String fmt(long c) {
        return c / 100 + "." + c % 100;
    }
}
`;
// 断点下在「String s = …」（第 7 行）和「return c / 100 …」（第 12 行）
const BPS = [7, 12];

{
  // 演示原来的样子：一笔替换整篇，第 1 行以外的断点全没了（不演示的话，下面这些看着像可有可无）
  const r = run(JAVA, JAVA.replace("import java.util.List;", "import java.util.List;\nimport java.util.Map;"), BPS, true);
  ok(same(r.at, []), `整篇一笔替换：断点全丢（这正是要修的）：${r.at}`);
}

{
  // 外面开头加一个 import、末尾加一个方法：两处改动，中间的断点原地不动（行号跟着下移）
  const after = JAVA.replace("import java.util.List;", "import java.util.List;\nimport java.util.Map;").replace(
    "        return c / 100 + \".\" + c % 100;\n    }\n",
    "        return c / 100 + \".\" + c % 100;\n    }\n\n    void extra() {}\n",
  );
  const r = run(JAVA, after, BPS);
  ok(r.text === after, "两处改动：结果一字不差");
  ok(same(r.at, ["String s = fmt(cents);", 'return c / 100 + "." + c % 100;']), `两处改动：断点还在原来那两行上：${r.at}`);
  ok(lineChanges(JAVA, after).length === 2, `拆成两笔：${JSON.stringify(lineChanges(JAVA, after))}`);
}

{
  // 断点那一行自己被改了（格式化器加了空格）、紧挨着它的下一行也改了：两个断点都留着
  const after = JAVA.replace("String s = fmt(cents);", "String s = fmt( cents );").replace("return s;", "return  s;");
  const r = run(JAVA, after, [7, 8]);
  ok(r.text === after, "改了内容的行：结果一字不差");
  ok(same(r.at, ["String s = fmt( cents );", "return  s;"]), `改了内容的行：断点留在那一行：${r.at}`);
}

{
  // 断点前面删了几行、后面插了几行：断点那一行没动，断点就在
  const after = JAVA.replace("import java.util.List;\n\n", "").replace("    private String", "    // 新加的注释\n    // 两行\n    private String");
  const r = run(JAVA, after, BPS);
  ok(r.text === after, "删 + 插：结果一字不差");
  ok(same(r.at, ["String s = fmt(cents);", 'return c / 100 + "." + c % 100;']), `删 + 插：断点还在：${r.at}`);
}

{
  // 删掉的正是断点那一行：断点跟着没了（IDEA 也是），别的照旧
  const after = JAVA.replace("        String s = fmt(cents);\n", "");
  const r = run(JAVA, after, BPS);
  ok(r.text === after && same(r.at, ['return c / 100 + "." + c % 100;']), `删掉断点那一行：只丢那一个：${r.at}`);
}

{
  // 末尾的换行：加上、去掉、文件末尾追加、整篇清空、从空到有
  const cases: [string, string][] = [
    ["a\nb", "a\nb\n"],
    ["a\nb\n", "a\nb"],
    ["a\nb", "a\nb\nc"],
    ["a\nb\nc", "a\nb"],
    ["a\nb", ""],
    ["", "a\nb"],
    ["x\na\nb", "a\nb"],
    ["a\nb", "x\na\nb"],
    ["a", "b"],
  ];
  for (const [x, y] of cases) ok(run(x, y, []).text === y, `${JSON.stringify(x)} → ${JSON.stringify(y)}：结果一字不差（得到 ${JSON.stringify(run(x, y, []).text)}）`);
  // 末尾追加一行：最后一行上的断点不挪到新行上
  ok(same(run("a\nb", "a\nb\nc", [2]).at, ["b"]), "末尾追加一行：断点留在原来的最后一行");
}

{
  // 乱改一千次：结果都对、改动从前往后不重叠；所有没被改的行上的断点都在。
  // 第 1 行固定不动（像 Java 的 package）：文件开头的插 / 删没有「前一行」可挂，紧跟着的那行断点保不住 —— 那是 TrackBefore 的规矩、doc-diff.ts 头上写着

  let seed = 7;
  const rnd = (k: number) => ((seed = (seed * 1103515245 + 12345) % 2147483648) % k);
  const words = ["int a;", "int b;", "", "}", "return x;", "// c"];
  let bad = 0;
  let lost = 0;
  for (let t = 0; t < 1000; t++) {
    const a = ["package p;", ...Array.from({ length: rnd(30) }, () => words[rnd(words.length)] + rnd(4))];
    const b = [...a];
    for (let e = rnd(5); e >= 0; e--) {
      const p = 1 + rnd(b.length);
      const op = rnd(3);
      if (op === 0 && b.length > 2) b.splice(Math.min(p, b.length - 1), 1);
      else if (op === 1) b.splice(p, 0, "新" + rnd(100));
      else if (b.length > 1) b[Math.min(p, b.length - 1)] += "~";
    }
    const before = a.join("\n");
    const after = b.join("\n");
    const ch = lineChanges(before, after);
    if (ch.some((c, i) => c.from > c.to || (i > 0 && c.from < ch[i - 1].to))) bad++;
    // 断点下在每一行（第 1 行除外）；没被改、没被删的行（内容在新旧两边都只出现一次、认得出来）都该在 ——
    // 「挪了位置」的行（[X, Y] → [Y, X]）按行比只能对上一个，这里的随机改动只有删、插、改，不造那种
    const r = run(before, after, a.map((_, i) => i + 1).slice(1));
    if (r.text !== after) bad++;
    const unique = (s: string, xs: string[]) => xs.filter((v) => v === s).length === 1;
    for (const l of a.slice(1)) if (unique(l, a) && unique(l, b) && !r.at.includes(l.trim())) lost++;
  }
  ok(bad === 0, `乱改一千次：结果都对、改动不重叠（错 ${bad} 次）`);
  ok(lost === 0, `乱改一千次：没动的行上的断点一个没丢（丢了 ${lost} 个）`);
}

{
  // 中间太大（行数乘积超过上限）：退回中间整段，首尾的断点照样留着，结果照样对
  const head = "head\n";
  const tail = "\ntail";
  const mid = (p: string) => Array.from({ length: 2500 }, (_, i) => p + i).join("\n");
  const before = head + mid("a") + tail;
  const after = head + mid("b") + tail;
  const t0 = performance.now();
  const r = run(before, after, [1, 2502]);
  const ms = performance.now() - t0;
  ok(r.text === after && same(r.at, ["head", "tail"]), `中间 2500 × 2500 行整段换：首尾断点都在（${ms.toFixed(0)}ms）：${r.at}`);
}

console.log(`${fail === 0 ? "✅" : "❌"} 整篇换内容时的按行最小改动：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
