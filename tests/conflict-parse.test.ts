import { parseConflicts, compose, unresolved, type ConflictBlock } from "../src/lib/git/conflict.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const TXT = `前面正常
<<<<<<< HEAD
我的第一行
我的第二行
=======
他们的第一行
>>>>>>> feature/x
中间正常
<<<<<<< HEAD
A
=======
B
>>>>>>> feature/x
末尾正常`;

const bs = parseConflicts(TXT);
ok(bs.length === 5, `普通/冲突/普通/冲突/普通 = 5 段，实得 ${bs.length}`);
ok(bs[0].kind === "plain" && bs[0].lines.join() === "前面正常", "首段普通文本");
const c0 = bs[1] as ConflictBlock;
ok(c0.kind === "conflict", "第二段是冲突");
ok(c0.ours.join("|") === "我的第一行|我的第二行", "ours 内容 " + c0.ours.join("|"));
ok(c0.theirs.join("|") === "他们的第一行", "theirs 内容");
ok(c0.oursLabel === "HEAD" && c0.theirsLabel === "feature/x", `标签 ${c0.oursLabel}/${c0.theirsLabel}`);
ok(c0.base === null, "非 diff3 风格没有 base");
ok(unresolved(bs) === 2, "两个冲突都还没决定");

// 原样往返：一个都不选，compose 出来必须和原文一字不差
ok(compose(bs) === TXT, "未决定时 compose 必须原样还原");

// 选择
(bs[1] as ConflictBlock).pick = "ours";
(bs[3] as ConflictBlock).pick = "theirs";
ok(unresolved(bs) === 0, "都决定了");
const r = compose(bs);
ok(r.includes("我的第一行") && !r.includes("他们的第一行"), "第一个冲突取了 ours");
ok(r.includes("B") && !r.includes("\nA\n"), "第二个冲突取了 theirs");
ok(!r.includes("<<<<<<<") && !r.includes(">>>>>>>") && !r.includes("======="), "标记清干净了");

// 都要
const bs2 = parseConflicts(TXT);
(bs2[1] as ConflictBlock).pick = "both";
const r2 = compose(bs2);
ok(r2.indexOf("我的第二行") < r2.indexOf("他们的第一行"), "「都要」保持 ours 在前");

// diff3 风格
const D3 = `<<<<<<< HEAD
mine
||||||| merged common ancestors
base line
=======
yours
>>>>>>> other`;
const b3 = parseConflicts(D3);
const c3 = b3[0] as ConflictBlock;
ok(c3.base?.join() === "base line", "diff3 的 base 段");
ok(c3.ours.join() === "mine" && c3.theirs.join() === "yours", "diff3 两侧");
c3.pick = "base";
ok(compose(b3) === "base line", "可以取共同祖先");

// 没有配对结束标记 —— 当普通文本，别把好文件解析坏
const BAD = `讲冲突的文档：
<<<<<<< 这一行只是举例
后面没有配对的结束标记`;
const bb = parseConflicts(BAD);
ok(bb.every(b => b.kind === "plain"), "无配对时不该产生冲突块");
ok(compose(bb) === BAD, "无配对时内容原样保留");

// 没有冲突的普通文件
const P = "line1\nline2\n";
ok(compose(parseConflicts(P)) === P, "普通文件原样往返（含末尾换行）");
ok(unresolved(parseConflicts(P)) === 0, "普通文件没有待决");

console.log(`\n${fail === 0 ? "✅" : "❌"} 冲突解析：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
