import { diffLines, MAX_LINES } from "../src/lib/git/linediff.ts";
import { changedLines } from "../src/lib/git/diff.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const show = (m: Map<number, string> | null) => (m ? JSON.stringify([...m]) : "null");

// ── 三种标记，和 changedLines 同一套语义 ──

// 改：一行换成另一行
{
  const m = diffLines("c1\nold\nc3\n", "c1\nnew\nc3\n");
  ok(m?.size === 1 && m.get(2) === "mod", `改一行 → 第 2 行 mod，实得 ${show(m)}`);
}
// 加：中间插两行
{
  const m = diffLines("c1\nc2\n", "c1\nn1\nn2\nc2\n");
  ok(m?.get(2) === "add" && m.get(3) === "add" && m.size === 2, `插两行 → 2、3 add，实得 ${show(m)}`);
}
// 删：标在缺口下面那一行
{
  const m = diffLines("c1\ngone1\ngone2\nc4\n", "c1\nc4\n");
  ok(m?.size === 1 && m.get(2) === "del", `删两行 → 第 2 行 del，实得 ${show(m)}`);
}
// 删到文件开头：标第 1 行
{
  const m = diffLines("gone\nc2\n", "c2\n");
  ok(m?.get(1) === "del" && m.size === 1, `开头删 → 第 1 行 del，实得 ${show(m)}`);
}
// 没变
{
  const m = diffLines("a\nb\n", "a\nb\n");
  ok(m !== null && m.size === 0, `没变要给空表，实得 ${show(m)}`);
}
// 追加到末尾（掐后缀时前缀已经吃光，后缀不能再越过它）
{
  const m = diffLines("a\nb\n", "a\nb\nc\n");
  // "a\nb\n" 拆成 [a,b,""]，"a\nb\nc\n" 拆成 [a,b,c,""]：多出来的是第 3 行 c
  ok(m?.size === 1 && m.get(3) === "add", `末尾追加 → 第 3 行 add，实得 ${show(m)}`);
}
// 重复行：前缀/后缀都相同、中间只差一行的情形，掐两头不能吃过头
{
  const m = diffLines("x\nx\nx\n", "x\nx\nx\nx\n");
  ok(m?.size === 1 && m.get(4) === "add", `重复行里加一行 → 只标一行，实得 ${show(m)}`);
}
// 两处改动，中间隔着相同的行
{
  const m = diffLines("1\n2\n3\n4\n5\n", "1\nB\n3\n4\nE\n");
  ok(m?.get(2) === "mod" && m.get(5) === "mod" && m.size === 2, `两处改 → 2、5 mod，实得 ${show(m)}`);
}
// 一段删除紧跟一段新增，行数不等：全算 mod（同 changedLines）
{
  const m = diffLines("1\na\n3\n", "1\nb\nc\n3\n");
  ok(m?.get(2) === "mod" && m.get(3) === "mod" && m.size === 2, `1 删 2 加 → 2、3 mod，实得 ${show(m)}`);
}

// ── 和 git 的口径对一下：同一份改动，从 unified diff 提取的结果要一样 ──
{
  const fromDiff = changedLines(`diff --git a/x b/x
@@ -1,5 +1,5 @@
 1
-2
+B
 3
 4
-5
+E
`);
  const fromLive = diffLines("1\n2\n3\n4\n5\n", "1\nB\n3\n4\nE\n");
  ok(show(fromDiff) === show(fromLive), `两条路口径要一致：diff=${show(fromDiff)} live=${show(fromLive)}`);
}

// ── 上限 ──
{
  const big = "x\n".repeat(MAX_LINES / 2 + 1);
  ok(diffLines(big, big + "y\n") === null, "行数超上限要给 null，不能硬算");
}
{
  // D 超上限：两份完全不同的长文本（掐不掉两头）
  const a = Array.from({ length: 3000 }, (_, i) => `a${i}`).join("\n");
  const b = Array.from({ length: 3000 }, (_, i) => `b${i}`).join("\n");
  ok(diffLines(a, b) === null, "差异行数超 MAX_D 要给 null");
}

console.log(`${fail === 0 ? "✅" : "❌"} 实时改动行：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
