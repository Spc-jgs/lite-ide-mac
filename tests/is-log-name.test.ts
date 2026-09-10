import { isLogName } from "../src/lib/logview/is-log-name.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

// ── 该认出来的 ─────────────────────────────────────────────────

ok(isLogName("app.log"), ".log");
ok(isLogName("/var/log/app.log"), "全路径也要认");
ok(isLogName("catalina.out"), "catalina.out —— Tomcat 天天见");
ok(isLogName("nohup.out"), "nohup.out");
ok(isLogName("stderr.err"), ".err");
ok(isLogName("/private/var/log/system.log"), "system.log 靠 .log 就中了");
ok(isLogName("messages"), "没有扩展名的 messages");
ok(isLogName("syslog"), "syslog");
/*
 * **这一条才是 `lastIndexOf("/")` 的存在理由。**
 * 没有扩展名的那几个走的是 `^(messages|syslog|dmesg)$`，两头都锚死 ——
 * 不先切出文件名，一个全路径永远匹配不上。
 * （下面那两条「目录名带 log 不算数」看着像在测这个，其实不是：
 *   `/` 不在 `[\w-]` 里，那两条靠正则自己就挡住了，去掉这一步照样绿。）
 */
ok(isLogName("/var/log/syslog"), "全路径下的 syslog");
ok(isLogName("APP.LOG"), "大小写不敏感");

// 轮转出来的那几种形状 —— 查历史日志时打开的正是这些
ok(isLogName("app.log.1"), "轮转：数字后缀");
ok(isLogName("app.log.2026-09-10"), "轮转：日期后缀带连字符");
ok(isLogName("app.2026-09-10.log"), "轮转：日期在中间");
ok(isLogName("app.log.gz"), "压缩的也认（内容是二进制，probe 会自己判成日志）");

// ── 不该认的：判错一个，按钮就又回到了「到处都在」 ──────────────

ok(!isLogName("App.svelte"), "组件文件");
ok(!isLogName("checkout.ts"), "名字里含 out，但 out 不是扩展名");
ok(!isLogName("layout.tsx"), "同上，layout");
ok(!isLogName("rollout.md"), "同上，rollout");
ok(!isLogName("notes.txt"), ".txt 可能是日志，但更可能不是 —— 不收");
ok(!isLogName("data.json"), ".json 不收");
ok(!isLogName("Cargo.toml"), "配置文件");
ok(!isLogName(""), "空串不能崩，也不能算日志");

// 目录名里带 log 不算数（这两条测的是正则的锚定，不是 basename —— 见上）
ok(!isLogName("/Users/me/logs/App.svelte"), "目录叫 logs 不代表文件是日志");
ok(!isLogName("/Users/me/app.log/index.ts"), "目录叫 app.log 也不算");

console.log(`${fail === 0 ? "✅" : "❌"} 日志文件名判据：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
