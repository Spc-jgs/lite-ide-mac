// 堆栈行 → 源文件（src/lib/logview/stack-frame.ts）。和 ⌘Click 一个原则：认不准就不画链接。
import { stackFrame, frameResolver, withLink } from "../src/lib/logview/stack-frame.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const linkText = (line: string) => {
  const f = stackFrame(line);
  return f ? line.slice(f.from, f.to) : null;
};

// ── 解析 ──
{
  const l = "\tat com.liteide.OrderService.persist(OrderService.java:142)";
  const f = stackFrame(l);
  ok(f?.suffix === "com/liteide/OrderService.java" && f.line === 142, `普通帧：${JSON.stringify(f)}`);
  ok(linkText(l) === "OrderService.java:142", `链接画在「文件:行号」上：${linkText(l)}`);

  const inner = stackFrame("    at com.a.Outer$Inner.lambda$run$0(Outer.java:10)");
  ok(inner?.suffix === "com/a/Outer.java" && inner.line === 10, `内部类 / lambda 用括号里的文件名：${JSON.stringify(inner)}`);

  const mod = stackFrame("\tat java.base/java.lang.Thread.run(Thread.java:833)");
  ok(mod?.suffix === "java/lang/Thread.java", `模块前缀去掉：${mod?.suffix}`);
  const app = stackFrame("\tat app//com.demo.Main.main(Main.java:7)");
  ok(app?.suffix === "com/demo/Main.java", `类加载器前缀去掉：${app?.suffix}`);

  const init = stackFrame("\tat com.demo.Order.<init>(Order.java:5)");
  ok(init?.suffix === "com/demo/Order.java" && init.line === 5, `构造器帧 <init>：${JSON.stringify(init)}`);

  const kt = stackFrame("\tat com.demo.Svc.run(Svc.kt:12)");
  ok(kt?.suffix === "com/demo/Svc.kt", "Kotlin 帧");

  const jarTail = "\tat org.springframework.aop.Foo.bar(Foo.java:88) ~[spring-aop-5.3.jar:5.3.1]";
  ok(stackFrame(jarTail)?.line === 88 && linkText(jarTail) === "Foo.java:88", "logback 追加的 jar 尾巴不影响");

  const noPkg = stackFrame("\tat Main.main(Main.java:3)");
  ok(noPkg?.suffix === "Main.java", `默认包：${noPkg?.suffix}`);
}

// ── 不是能跳的帧 ──
{
  ok(stackFrame("\tat java.lang.Thread.run(Native Method)") === null, "Native Method 没有行号");
  ok(stackFrame("\tat com.a.B.m(Unknown Source)") === null, "Unknown Source 没有行号");
  ok(stackFrame("\t... 42 more") === null, "... N more");
  ok(stackFrame("Caused by: java.io.IOException: boom") === null, "Caused by");
  ok(stackFrame("2026-09-23 10:00:00 INFO at startup (Main.java:3)") === null, "正文里碰巧有 at 和括号不算（要行首的 at）");
  ok(stackFrame("\tat com.a.B.m(B.java:0)") === null, "行号 0 不跳");
}

// ── 在索引里坐实：只认唯一 ──
{
  const files = [
    "order-api/src/main/java/com/liteide/OrderService.java",
    "order-core/src/main/java/com/demo/Dup.java",
    "order-web/src/main/java/com/demo/Dup.java",
    "Main.java",
  ];
  const find = frameResolver(files);
  ok(find("com/liteide/OrderService.java") === files[0], "唯一命中 → 那一份");
  ok(find("java/lang/Thread.java") === null, "JDK 的类不在项目里 → 不画");
  ok(find("com/demo/Dup.java") === null, "两个模块各有一份 → 认不准，不画");
  ok(find("Main.java") === "Main.java", "项目根下的默认包");
  // 后缀要按整段目录比：「xcom/liteide/...」不该被 com/liteide 命中
  const find2 = frameResolver(["src/xcom/liteide/OrderService.java"]);
  ok(find2("com/liteide/OrderService.java") === null, "后缀按目录边界比，不按字符");
}

// ── 切分段 ──
{
  const parts = [
    { text: "\tat com.a.B.m(", cls: "msg" as const },
    { text: "B.java:1", cls: "msg" as const },
    { text: ")", cls: "msg" as const },
  ];
  const f = stackFrame("\tat com.a.B.m(B.java:1)")!;
  const one = withLink([{ text: "\tat com.a.B.m(B.java:1)", cls: "msg" }], f.from, f.to);
  ok(one.map((p) => `${p.cls}:${p.text}`).join("|") === "msg:\tat com.a.B.m(|link:B.java:1|msg:)", `一段切三段：${JSON.stringify(one)}`);
  const multi = withLink(parts, f.from, f.to);
  ok(multi.map((p) => p.text).join("") === parts.map((p) => p.text).join(""), "切完拼回去还是原文");
  ok(multi.filter((p) => p.cls === "link").map((p) => p.text).join("") === "B.java:1", "跨段也只标中间那截");
}

console.log(`堆栈帧：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
