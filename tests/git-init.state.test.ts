/**
 * 还不是仓库的项目里 `git init`：`.git` 一冒出来文件监听报 `fs-changed: "git"`，`git.onGitDirChanged` 要重新找仓库（2026-10-13）。
 * 原来这一路只「复查信任 + 刷状态」，而这两样在没有仓库时都是空转 —— 要关掉项目再打开才认得出来。
 */
import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { git } = await import("../src/lib/state/git.svelte");

let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };

const root = "/work/fresh";
await git.locate(root);
ok(git.repo === null, "前提：不是仓库");
await git.onGitDirChanged(root);
ok(git.repo === null, "还没 init 时来一下 .git 事件：照样不是仓库（不能凭空认一个）");
(globalThis as { __mockGitInit: (r: string) => void }).__mockGitInit(root);
await git.onGitDirChanged(root);
ok(git.repo === root, `git init 之后那一下事件：认出来了，实际 ${git.repo}`);

// 已经是仓库时照旧：复查信任 + 刷状态，不重新找（repo 不变）
await git.onGitDirChanged(root);
ok(git.repo === root, "已经是仓库：照旧刷，repo 不变");

console.log(`${fail === 0 ? "✅" : "❌"} git init 之后自动认出仓库（状态层）：${pass} 通过，${fail} 失败`);
process.exit(fail === 0 ? 0 : 1);
