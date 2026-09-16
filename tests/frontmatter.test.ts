import { splitFrontmatter, parseAt, scratchTitle } from "../src/lib/state/frontmatter.ts";

let pass = 0,
  fail = 0;
const ok = (c: boolean, m: string) => {
  if (c) pass++;
  else {
    fail++;
    console.error("  ✗ " + m);
  }
};

const head = "---\nproject: /Users/x/proj\nbranch: main\nhead: b6176f3\nat: src/a.ts:12\n---\n\n";
const [a, off] = splitFrontmatter(head + "# 标题\n正文");
ok(a?.project === "/Users/x/proj" && a.branch === "main" && a.head === "b6176f3" && a.at === "src/a.ts:12", "四个字段都读回来");
ok((head + "# 标题\n正文").slice(off) === "# 标题\n正文", "正文起点跳过头和那个空行");
ok(splitFrontmatter("正文\n---\nbranch: x\n---\n")[0] === null, "`---` 不在第一行不算头");
ok(splitFrontmatter("---\nbranch: x\n没有收尾")[0] === null, "没收尾不算头");
ok(splitFrontmatter("")[0] === null, "空串");
ok(splitFrontmatter("---\nfoo: bar\nbranch: z\n---\n")[0]?.branch === "z", "认不出的 key 跳过");
ok(splitFrontmatter("---\r\nbranch: z\r\n---\r\n")[0]?.branch === "z", "CRLF 也认");
// 头只有 branch 一项、正文紧跟没有空行
ok("---\nbranch: z\n---\n正文".slice(splitFrontmatter("---\nbranch: z\n---\n正文")[1]) === "正文", "没有空行时正文从收尾的下一行起");

// scratchTitle 跳过头
ok(scratchTitle(head + "# 周会\n") === "周会", "标签名跳过头");
ok(scratchTitle(head) === undefined, "只有头没有正文 = 没有标题");
ok(scratchTitle("---\n第一行\n") === "---", "没收尾的 `---` 当正文（和 Rust 一致）");

// parseAt
ok(parseAt("src/a.ts:12")?.line === 12 && parseAt("src/a.ts:12")?.path === "src/a.ts", "路径:行");
ok(parseAt("src/a.ts")?.line === undefined, "没有行");
ok(parseAt("") === null, "空");
ok(parseAt("C:/x/y.ts:3")?.path === "C:/x/y.ts", "只认最后一个冒号后的数字");

console.log(`${fail === 0 ? "✅" : "❌"} 草稿锚点：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
