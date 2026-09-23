// Java 单文件符号表（src/lib/editor/java-scope.ts）：字段使用处上色 + 作用域遮蔽。
// 这层的原则是「判不准就不上色」，所以一半断言是「这里**不该**紫」。
import { javaLanguage } from "@codemirror/lang-java";
import { javaMarks, modelFor } from "../src/lib/editor/java-scope.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

/** 第 nth 次出现的 word 被标成什么（undefined = 没上色） */
function kindsIn(src: string) {
  const tree = javaLanguage.parser.parse(src);
  const marks = javaMarks(tree, (a, z) => src.slice(a, z), 0, src.length);
  return (word: string, nth = 0) => {
    const re = new RegExp(`(?<![\\w$])${word.replace(/[$]/g, "\\$")}(?![\\w$])`, "g");
    let m: RegExpExecArray | null = null;
    for (let k = 0; k <= nth; k++) m = re.exec(src);
    if (!m) throw new Error(`没有第 ${nth} 个 ${word}`);
    return marks.find((x) => x.from === m!.index)?.kind;
  };
}

// ── 字段 vs 局部 ──
{
  const k = kindsIn(`class C {
  int x; static int S; OrderRepository repo;
  void a() { use(x); use(S); repo.save(); }
  void b(int x) { use(x); }
  void c() { int x = 1; use(x); }
  void d() { { int x = 1; } use(x); }
  void e() { for (int x = 0; ; ) { use(x); } use(x); }
  void f() { items.forEach(x -> use(x)); use(x); }
  void g() { try { } catch (Exception x) { use(x); } use(x); }
}`);
  ok(k("x", 1) === "field", "方法里用字段 → 紫");
  ok(k("S", 1) === "staticField", "static 字段使用处 → 斜体紫");
  ok(k("repo", 1) === "field", "方法调用的接收者是字段 → 紫");
  ok(k("x", 3) === undefined, "参数遮住字段");
  ok(k("x", 5) === undefined, "局部变量遮住字段");
  ok(k("x", 7) === "field", "里层块的局部变量不漏到外层");
  ok(k("x", 9) === undefined && k("x", 10) === "field", "for 变量只在循环里遮");
  ok(k("x", 12) === undefined && k("x", 13) === "field", "lambda 参数只在 lambda 里遮");
  ok(k("x", 15) === undefined && k("x", 16) === "field", "catch 参数只在 catch 里遮");
}

// ── 语法包不认的写法：宁可漏 ──
{
  const k = kindsIn(`class C {
  int x;
  void a(Object o) { if (o instanceof Foo x) { use(x); } }
  void b(Object o) { if (!(o instanceof Foo x)) return; use(x); }
  void c() { use(x); }
}`);
  // x 的出现次序：0 字段声明、1/2 方法 a 的模式变量与使用、3/4 方法 b 的、5 方法 c 的
  ok(k("x", 2) === undefined, "instanceof 模式变量遮住字段（语法包把它当错误节点）");
  ok(k("x", 4) === undefined, "流敏感的模式变量：if 之后也遮");
  ok(k("x", 5) === "field", "别的方法不受影响");
}

// ── 匿名类 / 内部类 ──
{
  const k = kindsIn(`class C {
  int x; int y;
  void m() {
    int y = 0;
    new Thread() { int x; void run() { use(x); use(y); } };
    use(x);
  }
  class Inner { void n() { use(x); } }
}`);
  ok(k("x", 2) === "field", "匿名类的方法里：匿名类自己的字段");
  ok(k("y", 2) === undefined, "匿名类里用外层方法的局部变量（捕获）→ 不紫");
  ok(k("x", 3) === "field", "匿名类的字段不漏到外层方法（外层的 x 还是字段）");
  ok(k("x", 4) === "field", "内部类里用外部类的字段 → 紫");
}

// ── 限定访问 q.m ──
{
  const k = kindsIn(`class C {
  static int COUNT; String name;
  void m(Order order) {
    use(order.id);
    use(this.name);
    use(this.COUNT);
    use(System.out);
    use(Integer.MAX_VALUE);
    use(Map.Entry.class);
    use(Outer.Inner.NAME);
    java.util.List.of();
    use(a[0].len);
    use(get().val);
    use(C.this.name);
  }
}`);
  ok(k("id") === "field", "变量.成员 → 实例字段紫");
  ok(k("name", 1) === "field", "this.字段 → 紫");
  ok(k("COUNT", 1) === "staticField", "this.static字段 → 斜体（查得到声明）");
  ok(k("out") === "staticField", "类型.小写成员 → 语言规定是 static");
  ok(k("MAX_VALUE") === "staticField", "类型.常量 → 斜体紫");
  ok(k("System") === undefined && k("Integer") === undefined, "类型名本身不上色");
  ok(k("Entry") === undefined, "类型.类型（嵌套类）→ 不上色");
  ok(k("Inner") === undefined && k("NAME") === "staticField", "Outer.Inner.NAME：嵌套类不上色、常量斜体");
  ok(k("java") === undefined && k("util") === undefined && k("List") === undefined, "包名一段一段都不上色");
  ok(k("len") === "field" && k("val") === "field", "表达式.成员 → 实例字段");
  ok(k("name", 2) === "field", "Outer.this.字段 → 紫");
  ok(k("order", 1) === undefined, "参数当接收者 → 不紫");
}

// ── 不是名字的 Identifier ──
{
  const k = kindsIn(`@Named(value = "a")
class C {
  int run; int value;
  void m() { Runnable r = this::run; run(); foo.run(); }
}`);
  ok(k("run", 1) === undefined, "方法引用 this::run 不是字段");
  ok(k("run", 2) === undefined && k("run", 3) === undefined, "方法名不是字段（哪怕同名字段存在）");
  ok(k("value") === undefined, "注解参数名不是字段");
}

// ── 枚举 / 初始化块 / 字段初始化 ──
{
  const k = kindsIn(`enum E {
  A, B;
  private int w;
  static int s;
  static { s = 1; }
  { w = 2; }
  int t = w + 1;
  E() { w = 1; use(A); }
}`);
  ok(k("s", 1) === "staticField", "static 块里用 static 字段");
  ok(k("w", 1) === "field", "实例初始化块里用字段");
  ok(k("w", 2) === "field", "字段初始化表达式里用别的字段");
  ok(k("w", 3) === "field" && k("A", 1) === "staticField", "枚举构造器里：字段紫、枚举常量斜体");
}

// ── 声明类型（第 3 步成员跳转要用）──
{
  const src = `class C {
  private OrderRepository repo;
  private List<Item> items;
  private int n;
  void m(Order order) {
    Customer c = order.customer;
    var v = c;
    for (Item it : items) { use(it); }
    try (Conn conn = open()) { use(conn); }
    use(repo); use(items); use(n); use(order); use(c); use(v);
  }
}`;
  const tree = javaLanguage.parser.parse(src);
  const m = modelFor(tree, (a, z) => src.slice(a, z));
  const typeAt = (w: string, nth: number) => {
    // 整词匹配：按子串找的话 `it` 先撞上 `items`、`v` 撞上 `void`
    const re = new RegExp(`(?<![\\w$])${w}(?![\\w$])`, "g");
    let hit: RegExpExecArray | null = null;
    for (let k = 0; k <= nth; k++) hit = re.exec(src);
    const at = hit!.index;
    const node = tree.resolveInner(at, 1);
    const r = m.resolve(node, w);
    return r?.kind === "field" ? r.info.type : r?.kind === "local" ? r.type : "<没解析到>";
  };
  ok(typeAt("repo", 1) === "OrderRepository", `字段类型：${typeAt("repo", 1)}`);
  ok(typeAt("items", 2) === "List", `泛型取外层：${typeAt("items", 2)}`);
  ok(typeAt("n", 1) === null, `基本类型为 null：${typeAt("n", 1)}`);
  ok(typeAt("order", 2) === "Order", `参数类型：${typeAt("order", 2)}`);
  ok(typeAt("c", 1) === "Customer", `局部变量类型：${typeAt("c", 1)}`);
  ok(typeAt("it", 1) === "Item", `增强 for 变量类型：${typeAt("it", 1)}`);
  ok(typeAt("conn", 1) === "Conn", `try 资源类型：${typeAt("conn", 1)}`);
  ok(typeAt("v", 1) === null, `var 不猜：${typeAt("v", 1)}`);
}

console.log(`Java 符号表：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
