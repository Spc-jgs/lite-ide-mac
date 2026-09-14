import { crumbsOf, projectName } from "../src/lib/state/crumbs.ts";

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

// ── 项目名 ──
ok(projectName("/Users/a/proj") === "proj", "取最后一段");
ok(projectName("/") === "/", "根目录本身没有最后一段，退回原串");
ok(projectName(null) === "lite-ide", "没打开项目时是应用名");

// ── 面包屑 ──
eq(
  crumbsOf("/p", "/p/src/a/b.ts", "b.ts"),
  [
    { name: "p", path: "/p", dir: true },
    { name: "src", path: "/p/src", dir: true },
    { name: "a", path: "/p/src/a", dir: true },
    { name: "b.ts", path: "/p/src/a/b.ts", dir: false },
  ],
  "项目内的文件：根 › 每层目录 › 文件，只有最后一段不可点",
);
eq(crumbsOf("/p", "/p/x.md", "x.md"), [{ name: "p", path: "/p", dir: true }, { name: "x.md", path: "/p/x.md", dir: false }], "根下直接的文件");
eq(crumbsOf("/p", "/q/x.md", "x.md"), [{ name: "x.md", path: "/q/x.md", dir: false }], "项目外的文件只给一段");
eq(crumbsOf(null, "/q/x.md", "x.md"), [{ name: "x.md", path: "/q/x.md", dir: false }], "没打开项目也只给一段");
eq(crumbsOf("/p", "git-diff:3", "差异 x"), [{ name: "差异 x", path: "git-diff:3", dir: false }], "合成 key 的标签只给它的名字");
// `/p` 和 `/pp/…` 不能混：前缀比较要带斜杠
eq(crumbsOf("/p", "/pp/x.md", "x.md"), [{ name: "x.md", path: "/pp/x.md", dir: false }], "同前缀不同目录不算项目内");

console.log(`${fail === 0 ? "✅" : "❌"} 面包屑：${pass} 通过，${fail} 失败`);
if (fail > 0) process.exit(1);
