// ⌘P 的 Goto Anything 输入解析（src/lib/search/goto.ts，issue #43）
import { parseGoto } from "../src/lib/search/goto.ts";

let pass = 0,
  fail = 0;
const eq = (a: unknown, b: unknown, m: string) => {
  if (JSON.stringify(a) === JSON.stringify(b)) pass++;
  else {
    fail++;
    console.error(`  ✗ ${m}：${JSON.stringify(a)} ≠ ${JSON.stringify(b)}`);
  }
};

// ── 符号 ──
eq(parseGoto("order@listMember"), { kind: "symbol", file: "order", sym: "listMember" }, "文件@符号");
eq(parseGoto("@ping"), { kind: "symbol", file: "", sym: "ping" }, "单独 @ = 当前文件");
eq(parseGoto("admin@"), { kind: "symbol", file: "admin", sym: "" }, "@ 刚打出来：列那个文件的全部符号");
eq(parseGoto("node_modules/@types/x@foo"), { kind: "symbol", file: "node_modules/@types/x", sym: "foo" }, "认最后一个 @：前面那个是路径的一部分");
eq(parseGoto("icon@2x.png"), { kind: "plain" }, "@ 后面带 . 不是符号：文件名里本来就有的 @");
eq(parseGoto("@types/node"), { kind: "plain" }, "@ 后面带 / 不是符号");
eq(parseGoto("get$Value@a$b"), { kind: "symbol", file: "get$Value", sym: "a$b" }, "$ 算标识符（JS）");

// ── 行列 ──
eq(parseGoto("main.py:10"), { kind: "line", file: "main.py", line: 10, col: null }, "文件:行");
eq(parseGoto("main.py:10:4"), { kind: "line", file: "main.py", line: 10, col: 4 }, "文件:行:列");
eq(parseGoto(":42"), { kind: "line", file: "", line: 42, col: null }, "单独 : = 当前文件");
eq(parseGoto("main.py:"), { kind: "line", file: "main.py", line: null, col: null }, "冒号刚打出来：数字还没到");
eq(parseGoto("main.py:10:"), { kind: "line", file: "main.py", line: 10, col: null }, "第二个冒号刚打出来");
eq(parseGoto("a:b"), { kind: "plain" }, "冒号后面不是数字：更像是在搜正文");
eq(parseGoto("a:b:12"), { kind: "plain" }, "前半截里还有冒号：不猜");

// ── 普通搜索 ──
eq(parseGoto("OrderClient"), { kind: "plain" }, "什么都没带");
eq(parseGoto(""), { kind: "plain" }, "空");
eq(parseGoto(" order @x"), { kind: "symbol", file: "order", sym: "x" }, "文件部分的首尾空格去掉");

console.log(`${fail === 0 ? "✅" : "❌"} Goto Anything 解析：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
