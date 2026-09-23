// 终端输出里的链接（src/lib/terminal/links.ts）。原则同 ⌘Click：路径坐实不了就不画。
import { findTermLinks, type LinkCtx } from "../src/lib/terminal/links.ts";
import { frameResolver, stackFrame } from "../src/lib/logview/stack-frame.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const FILES = [
  "order-api/src/main/java/com/demo/OrderService.java",
  "order-api/pom.xml",
  "web/src/app.ts",
  "README.md",
];
const resolveFrame = frameResolver(FILES);
const ctx: LinkCtx = {
  root: "/Users/me/proj",
  cwdRel: "",
  has: (r) => FILES.includes(r),
  frameAt: (t) => {
    const f = stackFrame(t);
    const rel = f && resolveFrame(f.suffix);
    return f && rel ? { from: f.from, to: f.to, rel, line: f.line } : null;
  },
};
const links = (t: string, c: LinkCtx = ctx) => findTermLinks(t, c);
const texts = (t: string, c: LinkCtx = ctx) => links(t, c).map((l) => t.slice(l.start, l.end));

// ── 网址 ──
{
  const t = "Tomcat started on http://localhost:8080/api (see https://example.com/docs).";
  ok(texts(t).join("|") === "http://localhost:8080/api|https://example.com/docs", `网址，末尾的括号和句号不算：${texts(t)}`);
  ok(links(t)[0].kind === "url", "kind = url");
}

// ── maven / javac / tsc 的报错 ──
{
  const mvn = "[ERROR] /Users/me/proj/order-api/src/main/java/com/demo/OrderService.java:[42,10] cannot find symbol";
  const l = links(mvn)[0];
  ok(l?.kind === "file" && l.rel === "order-api/src/main/java/com/demo/OrderService.java" && l.line === 42, `maven 的绝对路径:[行,列]：${JSON.stringify(l)}`);
  ok(mvn.slice(l.start, l.end).endsWith(":[42,10]"), "下划线盖住行列那一截");

  const javac = "order-api/src/main/java/com/demo/OrderService.java:17: error: ';' expected";
  ok(links(javac)[0]?.line === 17, "javac 的 相对路径:行");

  const tsc = "web/src/app.ts:3:5 - error TS2322";
  ok(links(tsc)[0]?.rel === "web/src/app.ts" && links(tsc)[0]?.line === 3, "tsc 的 路径:行:列");

  const kt = "e: file:///Users/me/proj/order-api/src/main/java/com/demo/OrderService.java: (8, 3): Unresolved reference";
  ok(links(kt)[0]?.line === 8, `gradle kotlin 的 file:// + (行, 列)：${JSON.stringify(links(kt)[0])}`);

  const bare = "\tmodified:   order-api/pom.xml";
  ok(links(bare)[0]?.rel === "order-api/pom.xml" && links(bare)[0]?.line === undefined, "git status 里不带行号的路径");
}

// ── 相对路径按终端起始目录试 ──
{
  const sub: LinkCtx = { ...ctx, cwdRel: "order-api" };
  const t = "src/main/java/com/demo/OrderService.java:5: warning";
  ok(links(t, sub)[0]?.rel === "order-api/src/main/java/com/demo/OrderService.java", "在子模块里跑，路径相对子模块");
  ok(links(t)[0] === undefined, "在项目根跑同一行：找不到就不画");
}

// ── 堆栈帧 ──
{
  const t = "\tat com.demo.OrderService.persist(OrderService.java:142)";
  const l = links(t)[0];
  ok(l?.rel === "order-api/src/main/java/com/demo/OrderService.java" && l.line === 142, `堆栈帧按包路径坐实：${JSON.stringify(l)}`);
  ok(links(t).length === 1, "同一段不画两次（帧和路径两条规则都可能认它）");
  ok(links("\tat org.springframework.Foo.bar(Foo.java:9)").length === 0, "jar 里的帧不画");
}

// ── 不画的 ──
{
  ok(links("/tmp/out.java:3").length === 0, "项目外的绝对路径不画（没法同步确认它在不在）");
  ok(links("src/Missing.java:3").length === 0, "索引里没有的不画");
  ok(links("upgraded to 1.5.2 and 2.0.json-ish").length === 0, "版本号不是文件");
  ok(links("README.md", { ...ctx, root: null }).length === 0, "没开项目时路径一个都不认");
  ok(texts("see README.md for details").join() === "README.md", "项目根下的文件名本身");
}

console.log(`终端链接：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
