import { findLogSegments } from "../src/lib/logview/parse.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const J = (n: number, lvl = "INFO") => `2026-08-24 14:03:2${n}.442 ${lvl}  [main] c.l.Svc - 第 ${n} 条`;

// 三行成段，两行不成
ok(findLogSegments([J(1), J(2), J(3)]).length === 1, "三行像日志的是一段");
ok(findLogSegments([J(1), J(2)]).length === 0, "两行不算");
ok(findLogSegments(["ERROR 这不是日志", "还是 ERROR", "ERROR 三行"]).length === 0, "只有级别词、没有时间戳的不算");

// 段边界：前后有 prose
const s1 = findLogSegments(["记一笔", "", J(1), J(2), J(3), "", "然后呢"]);
ok(s1.length === 1 && s1[0].from === 2 && s1[0].to === 4, `prose 断段、段尾空行剪掉：${JSON.stringify(s1)}`);
ok(s1[0].fmt === "java" && s1[0].count === 3, `格式 java、计数 3：${JSON.stringify(s1)}`);

// 空行和堆栈续行不断段
const s2 = findLogSegments([J(1), "", J(2, "ERROR"), "\tat com.x.Y.z(Y.java:1)", "\tat com.x.Y.w(Y.java:2)", J(3)]);
ok(s2.length === 1 && s2[0].from === 0 && s2[0].to === 5 && s2[0].count === 3, `空行 / 堆栈不断段：${JSON.stringify(s2)}`);

// 段尾的堆栈剪掉：段以真日志结尾
const s3 = findLogSegments([J(1), J(2), J(3, "ERROR"), "\tat a.b(c:1)", "正文"]);
ok(s3[0].to === 2, `段尾堆栈剪掉：${JSON.stringify(s3)}`);

// 两段
const s4 = findLogSegments([J(1), J(2), J(3), "中间一句话", J(4), J(5), J(6)]);
ok(s4.length === 2 && s4[1].from === 4, `prose 隔开的是两段：${JSON.stringify(s4)}`);

// 别的格式
const L = (n: number) => `time=2026-08-24T14:03:2${n}Z level=info msg="x${n}"`;
ok(findLogSegments([L(1), L(2), L(3)])[0]?.fmt === "logfmt", "logfmt 也认");
ok(findLogSegments([]).length === 0, "空输入");

console.log(`${fail === 0 ? "✅" : "❌"} 日志段：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
