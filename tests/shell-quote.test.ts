// 拖文件进终端时插入的路径（src/lib/terminal/shell-quote.ts）。
import { shellQuote, dropText } from "../src/lib/terminal/shell-quote.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

ok(shellQuote("/Users/me/proj/pom.xml") === "/Users/me/proj/pom.xml", "普通路径原样");
ok(shellQuote("/Users/me/My File (1).txt") === "/Users/me/My\\ File\\ \\(1\\).txt", `空格和括号加反斜杠：${shellQuote("/Users/me/My File (1).txt")}`);
ok(shellQuote("a'b\"c$d`e") === "a\\'b\\\"c\\$d\\`e", "引号、$、反引号");
ok(shellQuote("中文 目录/报告.log") === "中文\\ 目录/报告.log", "中文不动，只转义空格");
ok(dropText(["/a b", "/c"]) === "/a\\ b /c ", "一批：空格隔开、末尾留一个空格");

console.log(`shell 路径转义：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
