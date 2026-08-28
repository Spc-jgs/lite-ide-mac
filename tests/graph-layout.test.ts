import { layout } from "../src/lib/git/graph.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const C = (sha: string, ...parents: string[]) => ({ sha, parents });

// 1. 一条直线：所有提交都在泳道 0
const g1 = layout([C("d", "c"), C("c", "b"), C("b", "a"), C("a")]);
ok(g1.width === 1, "直线只该占一条泳道，实得 " + g1.width);
ok(g1.rows.every(r => r.lane === 0), "直线全在泳道 0");
ok(g1.rows[0].outs.join() === "0", "第一行往下走泳道 0");
ok(g1.rows[3].outs.length === 0, "根提交没有父，不往下走");
ok(g1.rows.every(r => r.through.length === 0), "直线上不该有路过的泳道");

// 2. 合并提交：m 有两个父，岔出第二条泳道；到 a 处两条汇合
//    m → (b, c)，b → a，c → a，a 是根
const g2 = layout([C("m", "b", "c"), C("b", "a"), C("c", "a"), C("a")]);
ok(g2.width === 2, "一次合并该用两条泳道，实得 " + g2.width);
ok(g2.rows[0].lane === 0 && g2.rows[0].outs.length === 2, "合并行往下岔出两条");
ok(g2.rows[0].outs.includes(1), "第二个父要开新泳道");
ok(g2.rows[1].lane === 0, "b 在泳道 0");
ok(g2.rows[2].lane === 1, "c 在泳道 1");
// b 这行，泳道 1（等 c）应当路过
ok(g2.rows[1].through.includes(1), "b 那行泳道 1 该是路过状态");
// a 这行：泳道 0 和 1 都在等 a，其中一条汇入
ok(g2.rows[3].ins.length === 1, `a 处应有 1 条汇入，实得 ${g2.rows[3].ins.length}`);

// 3. 空槽复用：分支结束后腾出的位置该被后来的提交拿去，而不是一直往右长
//    x →(a,b) ; b→c ; a→c ; c→d ; d 独立
const g3 = layout([C("x", "a", "b"), C("a", "c"), C("b", "c"), C("c", "d"), C("d")]);
ok(g3.width === 2, "合并回来后不该继续加宽，实得 " + g3.width);

// 4. 分叉：两条独立的头，各占一条泳道
const g4 = layout([C("h1", "base"), C("h2", "base"), C("base")]);
ok(g4.width === 2, "两个头两条泳道");
ok(g4.rows[0].lane === 0 && g4.rows[1].lane === 1, "两个头分居两条泳道");
ok(g4.rows[2].ins.length === 1, "base 处两条汇合");

// 5. 空输入不崩，宽度至少为 1（渲染要用它算宽度）
const g5 = layout([]);
ok(g5.rows.length === 0 && g5.width === 1, "空输入");

// 6. 父提交不在列表里（limit 截断的边界）—— 线该继续往下走，不能崩
const g6 = layout([C("only", "missing-parent")]);
ok(g6.rows[0].outs.join() === "0", "父不在列表里时线仍向下延伸");

// 7. 章鱼合并（三个父）
const g7 = layout([C("o", "p1", "p2", "p3"), C("p1"), C("p2"), C("p3")]);
ok(g7.rows[0].outs.length === 3, `章鱼合并该岔出 3 条，实得 ${g7.rows[0].outs.length}`);
ok(g7.width === 3, "三个父三条泳道");

console.log(`\n${fail === 0 ? "✅" : "❌"} 泳道布局：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
