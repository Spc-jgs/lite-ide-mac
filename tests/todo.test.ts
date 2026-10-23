/**
 * TODO 工具窗的纯逻辑（src/lib/run/todo.ts，docs/TAKEOVER.md G18）：哪些算、说的是什么、怎么分组。
 */
import { TODO_PATTERN, groupTodos, todoOf } from "../src/lib/run/todo.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

// 正则（JS 和 rg 的 Rust 正则在这几样上写法一样）：注释里的算，变量名、Markdown 小节不算
const re = new RegExp(TODO_PATTERN);
ok(re.test("    // TODO: 改成批量查"), "// 注释");
ok(re.test("  /* FIXME 并发不安全 */"), "/* */");
ok(re.test("   * TODO 补文档"), "Javadoc 里的一行");
ok(re.test("# TODO 换成 venv"), "# 注释（Python / shell / YAML）");
ok(re.test("<!-- TODO 加截图 -->"), "HTML / XML 注释");
ok(!re.test("const todoList = []"), "变量名不算");
ok(!re.test("## TODO 列表"), "Markdown 的二级标题不算（# 后面跟着 #）");
ok(re.test("x = 1  # TODO 去掉魔数"), "行尾的 # 注释");

eq(todoOf("    // TODO: 改成批量查", 12), { line: 12, tag: "TODO", text: "改成批量查" }, "冒号去掉");
eq(todoOf("  /* FIXME 并发不安全 */", 3), { line: 3, tag: "FIXME", text: "并发不安全" }, "块注释的结尾去掉");
eq(todoOf("// TODO", 1), { line: 1, tag: "TODO", text: "// TODO" }, "后面没话：给整行");
eq(todoOf("// todo 小写的", 1), null, "小写不算（只认大写）");

eq(
  groupTodos([
    { path: "b/B.java", line: 9, text: "// TODO b9" },
    { path: "a/A.java", line: 20, text: "// FIXME a20" },
    { path: "a/A.java", line: 3, text: "// TODO a3" },
    { path: "a/A.java", line: 5, text: "// todoList" },
    { path: "README.md", line: 1, text: "# TODO" },
  ]),
  [
    { path: "a/A.java", items: [{ line: 3, tag: "TODO", text: "a3" }, { line: 20, tag: "FIXME", text: "a20" }] },
    { path: "b/B.java", items: [{ line: 9, tag: "TODO", text: "b9" }] },
  ],
  "按文件分组、文件按路径排、行号排；不是大写 TODO 的、Markdown 的标题丢掉",
);

console.log(fail === 0 ? `✅ TODO：${pass} 通过，0 失败` : `❌ TODO：${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
