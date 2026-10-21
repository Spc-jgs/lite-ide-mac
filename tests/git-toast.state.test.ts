/**
 * git 写操作没做成时走胶囊还是卡片（`git-ops.fitsToast`，2026-10-21）：一行说得清的原话进胶囊，多行的（列出涉及的文件）留卡片。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { fitsToast } = await import("../src/lib/state/git-ops");

let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

ok(fitsToast("新建分支", "fatal: a branch named 'x' already exists"), "一行的原话：胶囊");
ok(!fitsToast("切换分支", "error: 会被覆盖:\n\ta.txt"), "多行（列了文件）：卡片 —— 哪怕总共没几个字（样例故意短，别让长度那条替它挡）");
ok(!fitsToast("推送", "x".repeat(90)), "一行但太长（胶囊三行放不下）：卡片");
ok(fitsToast("提交", "中".repeat(80)), "按字数不按字节：80 个汉字 + 「提交」还是一句");

console.log(`${fail === 0 ? "✅" : "❌"} git 失败放哪：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
