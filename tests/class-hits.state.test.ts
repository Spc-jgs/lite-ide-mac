/**
 * ⇧⇧ 的「类」怎么排（`search/class-hits.ts`，docs/TAKEOVER.md G2）：项目里的排在 jar 里的前面；带包名的按包名 + 类名排、高亮只标类名；
 * 通配写法不丢 jdtls 找到的；jdtls 没在跑时按文件名顶上、只认 JVM 的源文件
 */
import { declLine, fileTypes, jarOf, rankTypes } from "../src/lib/search/class-hits";
import type { JavaType } from "../src/lib/ipc/commands";

let pass = 0,
  fail = 0;
const ok = (c: unknown, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const t = (name: string, container: string, path: string | null, kind = 5): JavaType => ({
  name,
  container,
  kind,
  at: { uri: path ? `file://${path}` : `jdt://contents/spring-core-7.0.2.jar/${container}/${name}.class?=x`, path, label: name, line: 3, character: 13, endLine: 3, endCharacter: 20, text: "" },
});

// jdtls 回来的次序是乱的（HashSet）：jar 里那个名字正好是 `Order`，分数比项目里的 `OrderService` 高
const items = [t("Order", "org.springframework.core", null, 11), t("OrderService", "com.demo.order", "/p/OrderService.java"), t("OrderMapper", "com.demo.order", "/p/OrderMapper.java", 11)];
const r = rankTypes(items, "Order", 10);
ok(
  r.map((h) => h.name).join() === "OrderService,OrderMapper,Order" || r.map((h) => h.name).join() === "OrderMapper,OrderService,Order",
  `项目里的整组排在 jar 里的前面：${r.map((h) => h.name).join()}`,
);
ok(r[2].lib === "spring-core-7.0.2.jar" && r[2].path === null && r[2].kind === "接口", `jar 里的：标出 jar 名、没有路径、种类照 jdtls：${JSON.stringify(r[2])}`);
ok(r[0].line === 4 && r[0].col === 14, "行列换成从 1 数（jdtls 从 0 数）");
ok(rankTypes(items, "Order", 2).map((h) => h.lib).join() === ",", "条数不够时先让给项目里的：两个名额都是项目的");

// 带包名：`order.OS` —— 按「包名.类名」排，高亮只标类名那段
const d = rankTypes(items, "order.OS", 10);
ok(d[0]?.name === "OrderService", `带包名的写法认得出：${d.map((h) => h.name).join()}`);
ok(d[0] && d[0].seg.filter((s) => s.hit).map((s) => s.t).join("") === "OS", `高亮只在类名上：${JSON.stringify(d[0]?.seg)}`);

// 通配：rank 不认 `*`，jdtls 认 —— 不能因为我们不认就丢
const w = rankTypes(items, "Ord*Ser", 10);
ok(w.length === 3 && w.every((h) => h.seg.length === 1 && !h.seg[0].hit), `通配的照 jdtls 给的全列、不标高亮：${w.length}`);

ok(jarOf("jdt://contents/my%20lib.jar/a.b/C.class") === "my lib.jar", "jar 名解码");
ok(jarOf("file:///x/A.java") === "", "不是 jdt:// 的给空");

// jdtls 没在跑：按文件名
const f = fileTypes(["web/src/OrderService.java", "web/src/order.ts", "build.gradle.kts", "core/OrderUtil.kt", "README.md"], "order", 10);
ok(f.map((h) => h.name).join() === "OrderService,OrderUtil" || f.map((h) => h.name).join() === "OrderUtil,OrderService", `只认 JVM 的源文件、名字去掉扩展名：${f.map((h) => h.name).join()}`);
ok(f.every((h) => h.kind === "" && h.path && h.line === undefined), "不知道是类还是接口：种类空着，没有行号");
ok(f.find((h) => h.name === "OrderService")?.container === "web/src", "右边那列是目录");

// jar 里的类：jdtls 给的位置是文件开头，开了之后停到声明那行（同 ⌘B）；Javadoc 里提到的不算
const src = "package org.springframework.boot;\n\n/**\n * Class used to bootstrap — see class SpringApplication docs.\n */\npublic class SpringApplication {\n  // class SpringApplication 的注释\n}";
ok(JSON.stringify(declLine(src, "SpringApplication")) === JSON.stringify({ line: 6, col: 14 }), `停在声明那行、光标在名字上：${JSON.stringify(declLine(src, "SpringApplication"))}`);
ok(declLine("interface Map$Entry {}", "Map$Entry")?.line === 1, "名字里带 $ 的（反编译的内部类）当字面量");
ok(declLine("class Other {}", "SpringApplication") === null, "找不到给 null（调用方停在开头）");

console.log(`${fail === 0 ? "✅" : "❌"} ⇧⇧ 找类：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
