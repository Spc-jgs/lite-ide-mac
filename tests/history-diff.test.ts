/**
 * 本地历史的差异（src/lib/git/linediff.ts 的 unifiedDiff）：生成的 unified diff 要是一份**能打回去**的补丁 ——
 * 拿它从旧的推出新的，必须一字不差；差异视图用的解析器读回来，每块的行数要对得上（错一个行号，画出来就错一片）。
 */
import { unifiedDiff } from "../src/lib/git/linediff.ts";
import { parseDiff } from "../src/lib/git/diff.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

/** 照 unified diff 把旧文本推成新文本（独立实现，只信块头的起始行和每行的前缀） */
function apply(oldText: string, diff: string): string {
  const a = oldText === "" ? [] : oldText.split("\n");
  const out: string[] = [];
  let ai = 0;
  const lines = diff.split("\n");
  for (let i = 0; i < lines.length; i++) {
    const h = /^@@ -(\d+),(\d+) \+(\d+),(\d+) @@/.exec(lines[i]);
    if (!h) continue;
    const start = Number(h[2]) === 0 ? Number(h[1]) : Number(h[1]) - 1;
    while (ai < start) out.push(a[ai++]);
    for (i++; i < lines.length && /^[ +-]/.test(lines[i]) && !lines[i].startsWith("@@"); i++) {
      const c = lines[i][0], t = lines[i].slice(1);
      if (c === " ") { if (a[ai] !== t) throw new Error(`上下文对不上：第 ${ai + 1} 行「${a[ai]}」≠「${t}」`); out.push(t); ai++; }
      else if (c === "-") { if (a[ai] !== t) throw new Error(`删的不是这一行：第 ${ai + 1} 行`); ai++; }
      else out.push(t);
    }
    i--;
  }
  while (ai < a.length) out.push(a[ai++]);
  return out.join("\n");
}

// ── 手算核对过的一例：第 5 行改、第 21 行删、末尾加一行 → 三块（21 和 31 之间隔 9 行 > 2×3，不并）──
const L = (n: number) => Array.from({ length: n }, (_, i) => `line ${i + 1}`);
const old = L(30);
const now = [...old];
now[4] = "line 5 改了";
now.splice(20, 1);
now.push("line 31 新加的");
const d = unifiedDiff(old.join("\n"), now.join("\n"), "src/A.java")!;
ok(JSON.stringify(d.split("\n").filter((l) => l.startsWith("@@"))) === JSON.stringify(["@@ -2,7 +2,7 @@", "@@ -18,7 +18,6 @@", "@@ -28,3 +27,4 @@"]), `块头：${d.split("\n").filter((l) => l.startsWith("@@")).join(" | ")}`);
const f = parseDiff(d);
ok(f.length === 1 && f[0].path === "src/A.java", "差异视图的解析器认出一个文件");
const kinds = f[0].lines.map((l) => l.kind);
ok(kinds.filter((k) => k === "add").length === 2 && kinds.filter((k) => k === "del").length === 2 && kinds.filter((k) => k === "hunk").length === 3, `解析回来两加两删三块：${kinds.join(",")}`);
// 包起来：打补丁时抛了（块头的行号错了，上下文对不上）也要报成一条失败、走到最后的汇总 —— 不包的话错版本「没输出」，看着像没红（验红时撞见的）
let back = "";
try { back = apply(old.join("\n"), d); } catch (e) { back = `抛了：${e}`; }
ok(back === now.join("\n"), `打回去得到新的：${back.slice(0, 80)}`);
ok(unifiedDiff("a\nb", "a\nb", "x") === "", "一样给空串");
ok(unifiedDiff("", "新文件\n第二行", "x")!.includes("@@ -0,0 +1,2 @@"), `旧的是空文件：${unifiedDiff("", "新文件\n第二行", "x")}`);

// ── 随机：300 组，打回去都得到新的（种子固定，红了能复现）──
let seed = 20261020;
const rnd = (n: number) => ((seed = (seed * 1103515245 + 12345) & 0x7fffffff) % n);
for (let t = 0; t < 300; t++) {
  const base = Array.from({ length: rnd(40) }, () => ["a", "b", "c", "", "x y", "}"][rnd(6)]);
  const next = [...base];
  for (let e = rnd(6); e > 0; e--) {
    const k = rnd(3), at = rnd(next.length + 1);
    if (k === 0) next.splice(at, 0, `new${rnd(9)}`);
    else if (k === 1 && next.length) next.splice(Math.min(at, next.length - 1), 1);
    else if (next.length) next[Math.min(at, next.length - 1)] = `chg${rnd(9)}`;
  }
  const o = base.join("\n"), n = next.join("\n");
  const diff = unifiedDiff(o, n, "r.txt");
  let got = "";
  try { got = diff === "" ? o : apply(o, diff!); } catch (e) { got = `抛了：${e}`; }
  if (got !== n) { ok(false, `第 ${t} 组打回去不对：\n旧 ${JSON.stringify(o)}\n新 ${JSON.stringify(n)}\n${diff}`); break; }
  pass++;
}

console.log(`${fail === 0 ? "✅" : "❌"} 本地历史的差异：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
