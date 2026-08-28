/**
 * 跑 tests/ 下所有 *.test.ts。
 *
 * 不引测试框架是有意的：这里测的全是**纯函数**（diff 解析、双栏对照、
 * 泳道布局、冲突解析、改动行标记），输入输出都是普通数据结构。
 * 为它们装一套 vitest 加一堆 transform 配置，维护成本比被测代码还高。
 *
 * Node 22+ 能直接跑 .ts（原生剥类型），一个子进程跑一个文件 ——
 * 谁失败就把谁的完整输出打出来，其余的只留一行汇总。
 */
import { spawnSync } from "node:child_process";
import { readdirSync } from "node:fs";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";

const here = dirname(fileURLToPath(import.meta.url));
const files = readdirSync(here)
  .filter((f) => f.endsWith(".test.ts"))
  .sort();

if (files.length === 0) {
  console.error("tests/ 下一个 *.test.ts 都没有 —— 是不是路径写错了？");
  process.exit(1);
}

let failed = 0;
for (const f of files) {
  const r = spawnSync(process.execPath, [join(here, f)], { encoding: "utf8" });
  const out = (r.stdout || "").trim();
  if (r.status === 0) {
    console.log(out.split("\n").filter(Boolean).pop() ?? `✅ ${f}`);
  } else {
    failed++;
    console.error(`\n❌ ${f}`);
    if (out) console.error(out);
    if (r.stderr) console.error(r.stderr.trim());
  }
}

if (failed > 0) {
  console.error(`\n${failed} / ${files.length} 个测试文件失败`);
  process.exit(1);
}
console.log(`\n全部通过（${files.length} 个文件）`);
