import { detectEol, detectIndent, indentLabel } from "../src/lib/editor/indent.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// ── 换行符 ──
ok(detectEol("a\nb\n") === "LF", "LF");
ok(detectEol("a\r\nb\r\n") === "CRLF", "CRLF");
ok(detectEol("a\rb\r") === "CR", "老 Mac 的 CR");
ok(detectEol("a\nb\r\n") === "混用", "混用要说出来，不能猜一个");
ok(detectEol("单行没有换行") === "LF", "没有换行按 LF");
ok(detectEol("") === "LF", "空文件按 LF");

// ── 缩进 ──
ok(detectIndent("a\n  b\n    c\n  d\n") === 2, "2 空格");
ok(detectIndent("a\n    b\n        c\n    d\n") === 4, "4 空格");
ok(detectIndent("a\n\tb\n\t\tc\n") === "tab", "制表符");
ok(detectIndent("a\nb\nc\n") === null, "没缩进过的行看不出来");
ok(detectIndent("") === null, "空文件");
// 深层嵌套骗不了「相邻差」：绝对缩进 8 的行最多，但相邻差全是 2
{
  const t = "fn\n  a\n    b\n      c\n        d1\n        d2\n        d3\n        d4\n      e\n    f\n  g\n";
  ok(detectIndent(t) === 2, `深层嵌套仍是 2，实得 ${detectIndent(t)}`);
}
// 对齐用的差 1 不算缩进单位
ok(detectIndent("a\n    b\n     c\n    d\n") === 4, "差 1 的对齐行不算");
// 制表符文件里混几行空格对齐：制表符赢
ok(detectIndent("a\n\tb\n\tc\n\t d\n\te\n  f\n") === "tab", "制表符为主就是 Tab");
// 全是同一级（没有相邻差）：按那一级的宽度
ok(detectIndent("a\n\n    b\n\n    c\n") === 4, "只有一级缩进按宽度算");
// 空行不算：它的缩进是随手的
ok(detectIndent("a\n  b\n      \n  c\n") === 2, "空白行不参与");
// 并列时取小的：4 空格文件里连跳两级的行
ok(detectIndent("a\n    b\n            c\n") === 4, "并列取小的");

// ── 标签 ──
ok(indentLabel(4) === "4 空格" && indentLabel("tab") === "Tab" && indentLabel(null) === "—", "状态栏文字");

console.log(`${fail === 0 ? "✅" : "❌"} 缩进与换行符判据：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
