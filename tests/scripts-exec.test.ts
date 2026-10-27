/**
 * 验收脚本都要有执行位：AGENTS.md 里写的是直接执行（`scripts/accept/http.sh`），没执行位就是 126 退出、一行输出都没有 ——
 * 一轮回归里它混在别的结果中间，像是「这一份没跑」而不是「这一份坏了」。http.sh、history.sh 都这样过（2026-10-28 各撞一次）
 */
import { readdirSync, statSync } from "node:fs";
import { join } from "node:path";

const root = join(import.meta.dirname, "..", "scripts");
const bad: string[] = [];
for (const dir of [root, join(root, "accept"), join(root, "lib")]) {
  for (const f of readdirSync(dir)) {
    // lib 下的 bridge.sh 是 source 进来的，不直接执行
    if (!f.endsWith(".sh") || dir.endsWith("lib")) continue;
    if ((statSync(join(dir, f)).mode & 0o111) === 0) bad.push(join(dir, f).slice(root.length + 1));
  }
}
if (bad.length) {
  console.error(`  ✗ 这些脚本没有执行位（chmod +x 再提交）：${bad.join("、")}`);
  console.log("❌ 脚本执行位：0 通过，1 失败");
  process.exit(1);
}
console.log("✅ 脚本执行位：1 通过，0 失败");
