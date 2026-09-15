import { groupByDir } from "../src/lib/git/group.ts";
let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const P = (...ps: string[]) => ps.map((path) => ({ path }));

{
  const g = groupByDir(P("src/b.ts", "README.md", "src/a.ts", "docs/x.md", "Cargo.toml"));
  ok(g.length === 3, `三组，实得 ${g.length}`);
  ok(g[0].dir === "" && g[0].items.map((x) => x.path).join() === "README.md,Cargo.toml", "根目录的在最前、不排序（保持传入顺序）");
  ok(g[1].dir === "docs" && g[2].dir === "src", "目录按名字排");
  ok(g[2].items.map((x) => x.path).join() === "src/b.ts,src/a.ts", "组内保持传入顺序 —— 传入的已经是 git 排过的");
}
ok(groupByDir([]).length === 0, "空表");
ok(groupByDir(P("a.txt")).length === 1 && groupByDir(P("a.txt"))[0].dir === "", "只有根文件时一组");
{
  const g = groupByDir(P("scratch/tmp/", "scratch/draft.md"));
  ok(g.length === 1 && g[0].dir === "scratch", `目录条目（以 / 结尾）按它的父目录归组，实得 ${JSON.stringify(g)}`);
}
ok(groupByDir(P("deep/a/b/c.ts"))[0].dir === "deep/a/b", "压扁的树：目录头就是完整相对路径，不嵌套");

console.log(`${fail === 0 ? "✅" : "❌"} 改动分组：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
