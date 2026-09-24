import { flatten, dirKindOf, type TreeInput } from "../src/lib/shell/tree-rows.ts";
import type { DirEntry } from "../src/lib/ipc/commands.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) {
    pass++;
  } else {
    fail++;
    console.error("  ✗ " + m);
  }
};
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：得到 ${JSON.stringify(a)}`);

/**
 * 造一个目录项。`gen` 只是给 dims 用的名字标记，测试里 dims 就认它；
 * `chain` 是 Rust 侧探好的单子目录链（不含自己）
 */
const dir = (path: string, gen = false, chain: string[] = []): DirEntry => ({
  name: path.slice(path.lastIndexOf("/") + 1),
  path,
  isDir: true,
  size: 0,
  generated: gen,
  contested: false,
  chain,
});
const file = (path: string): DirEntry => ({
  name: path.slice(path.lastIndexOf("/") + 1),
  path,
  isDir: false,
  size: 1,
  generated: false,
  contested: false,
  chain: [],
});

const tree = (kids: Record<string, DirEntry[]>, open: string[], compact = false): TreeInput => ({
  root: "/p",
  children: new Map(Object.entries(kids)),
  expanded: new Set(["/p", ...open]),
  dims: (it) => it.generated,
  compact,
});

const show = (t: TreeInput) =>
  flatten(t).map((r) => `${"  ".repeat(r.depth)}${r.label}${r.generated ? " (gen)" : ""}${r.srcRoot ? " [源码根]" : ""}`);

// ── 拍平：只走展开的，深度对，没加载的那层断在那里 ──
{
  const kids = {
    "/p": [dir("/p/src"), dir("/p/docs"), file("/p/README.md")],
    "/p/src": [dir("/p/src/a"), file("/p/src/x.ts")],
    "/p/src/a": [file("/p/src/a/y.ts")],
  };
  eq(show(tree(kids, [])), ["src", "docs", "README.md"], "只有根展开时只有第一层");
  eq(show(tree(kids, ["/p/src"])), ["src", "  a", "  x.ts", "docs", "README.md"], "展开 src 插在它自己下面");
  eq(
    show(tree(kids, ["/p/src", "/p/src/a"])),
    ["src", "  a", "    y.ts", "  x.ts", "docs", "README.md"],
    "两层都展开，深度逐层加一",
  );
  eq(show(tree(kids, ["/p/docs"])), ["src", "docs", "README.md"], "展开了但子项还没加载回来：什么都不画，不报错");
}

// ── 生成物是整棵子树的性质 ──
{
  const kids = {
    "/p": [dir("/p/node_modules", true), dir("/p/src")],
    "/p/node_modules": [dir("/p/node_modules/vue")],
    "/p/node_modules/vue": [file("/p/node_modules/vue/index.js")],
  };
  eq(
    show(tree(kids, ["/p/node_modules", "/p/node_modules/vue"])),
    ["node_modules (gen)", "  vue (gen)", "    index.js (gen)", "src"],
    "生成物目录往下每一行都压暗，不只是顶上那一行",
  );
}

// ── 合并：Java 模块，照 IDEA 画 ──
{
  const M = "/p/order-service";
  const J = `${M}/src/main/java`;
  const kids = {
    "/p": [dir(M), file("/p/pom.xml")],
    [M]: [dir(`${M}/src`), file(`${M}/pom.xml`)],
    [`${M}/src`]: [dir(`${M}/src/main`, false, ["java", "com", "demo", "order"]), dir(`${M}/src/test`, false, ["java", "com"])],
    [`${M}/src/main`]: [dir(J, false, ["com", "demo", "order"])],
    [J]: [dir(`${J}/com`, false, ["demo", "order"])],
    [`${J}/com/demo/order`]: [file(`${J}/com/demo/order/OrderService.java`)],
  };
  const open = [M, `${M}/src`, `${M}/src/main`, J, `${J}/com/demo/order`];
  eq(
    show(tree(kids, open, true)),
    [
      "order-service",
      "  src",
      "    main",
      "      java [源码根]",
      "        com.demo.order",
      "          OrderService.java",
      "    test",
      "  pom.xml",
      "pom.xml",
    ],
    "源码根自己占一行，main 不和 java 并；源码根底下的包用点连",
  );

  const rows = flatten(tree(kids, open, true));
  const pkg = rows.find((r) => r.label === "com.demo.order")!;
  eq(pkg.path, `${J}/com/demo/order`, "合并行的 path 是最深那一层 —— 新建、拖进来、改名、删除都落在它上面");
  eq(pkg.name, "order", "name 是最深那一段：重命名只改它");
  eq(pkg.chain, [`${J}/com`, `${J}/com/demo`, `${J}/com/demo/order`], "chain 列出这一行盖住的每一层");

  eq(
    show(tree(kids, [M, `${M}/src`, `${M}/src/main`, J, `${J}/com`], true)),
    ["order-service", "  src", "    main", "      java [源码根]", "        com.demo.order", "    test", "  pom.xml", "pom.xml"],
    "展开与否认的是最深那一层：只展开了 com（链头）不算展开了这一行",
  );

  eq(
    show(tree(kids, open, false)).slice(0, 6),
    ["order-service", "  src", "    main", "      java [源码根]", "        com", "    test"],
    "关掉合并：一层一行，和原来一样",
  );
}

// ── 合并：别的语言用 / 连 ──
{
  const kids = {
    "/p": [dir("/p/web", false, ["src", "components"]), dir("/p/node_modules", true, ["a", "b"])],
    "/p/web/src/components": [file("/p/web/src/components/OrderList.vue")],
  };
  eq(
    show(tree(kids, ["/p/web/src/components"], true)),
    ["web/src/components", "  OrderList.vue", "node_modules (gen)"],
    "不在源码根底下的用 / 连；压暗的不并（哪怕带着链）",
  );
}

// ── 目录角色（画哪种文件夹图标，照 IDEA） ──
{
  const M = "/p/m/src";
  eq(dirKindOf(`${M}/main/java`, false, false), "source", "src/main/java 是源码根");
  eq(dirKindOf(`${M}/test/java`, false, false), "test", "src/test/java 是测试根");
  eq(dirKindOf(`${M}/integrationTest/kotlin`, false, false), "test", "源码集名带 test 的都算测试");
  eq(dirKindOf(`${M}/main/resources`, false, false), "resources", "资源根");
  eq(dirKindOf(`${M}/test/resources`, false, false), "testResources", "测试资源根");
  eq(dirKindOf(`${M}/main/java/com`, true, false), "package", "源码根底下的是包");
  eq(dirKindOf("/p/docs", false, false), "folder", "别的是普通文件夹");
  eq(dirKindOf("/p/target", false, true), "excluded", "自己被判成生成物的画排除图标");

  const kids = {
    "/p": [dir("/p/target", true)],
    "/p/target": [dir("/p/target/classes")],
  };
  const rows = flatten(tree(kids, ["/p/target"]));
  eq(rows.map((r) => r.dirKind), ["excluded", "folder"], "只有 target 本身是排除图标，里面的子目录是普通文件夹（压暗照样继承）");
  eq(rows.map((r) => r.generated), [true, true], "压暗跟着整棵子树");
}

console.log(`${fail === 0 ? "✅" : "❌"} tree-rows：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
