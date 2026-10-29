import { installMockIpc } from "../src/lib/dev/mock-ipc";
installMockIpc();
const { git } = await import("../src/lib/state/git.svelte");
const internal = (globalThis as any).__TAURI_INTERNALS__;
const original = internal.invoke;
const base = await original("git_status", { root: "/proj" });
let head = base.head;
internal.invoke = async (cmd: string, args: unknown) => cmd === "git_status" ? { ...base, head } : original(cmd, args);
let pass = 0, fail = 0;
const ok = (c: unknown, m: string) => { if (c) pass++; else { fail++; console.error("✗ " + m); } };
git.repo = "/proj";
git.status = base;
const start = git.historyRevision;
await git.refresh();
ok(git.historyRevision === start, "仅工作区刷新，不重复拉提交历史");
head = "newhead";
await git.refresh();
ok(git.historyRevision === start + 1, "新提交 / amend 后 HEAD 变化使历史失效");
await git.refresh(true);
ok(git.historyRevision === start + 2, "显式刷新在 HEAD 未变化时也使历史失效");
await git.onGitDirChanged("/proj");
ok(git.historyRevision === start + 3, "远程引用等 .git 变化也使历史失效");

// 最早发出的查询最后回来，不能把 HEAD 和历史信号倒退。
let call = 0;
internal.invoke = async (cmd: string, args: unknown) => {
  if (cmd !== "git_status") return original(cmd, args);
  const n = ++call;
  await new Promise(r => setTimeout(r, n === 1 ? 40 : 1));
  return { ...base, head: n === 1 ? "outdated" : "current" };
};
const beforeRace = git.historyRevision;
await Promise.all([git.refresh(), git.refresh()]);
ok(git.status?.head === "current" && git.historyRevision === beforeRace + 1, "乱序状态响应不覆盖 HEAD，也不重复通知历史");
call = 0;
internal.invoke = async (cmd: string, args: unknown) => {
  if (cmd !== "git_status") return original(cmd, args);
  const n = ++call;
  await new Promise(r => setTimeout(r, n === 1 ? 40 : 1));
  return { ...base, head: "current" };
};
const beforeForcedRace = git.historyRevision;
await Promise.all([git.refresh(true), git.refresh()]);
ok(git.historyRevision === beforeForcedRace + 1, "显式刷新被普通刷新超越时，历史更新请求仍然保留");
internal.invoke = original;
console.log(`${fail ? "❌" : "✅"} Git 刷新联动：${pass} 通过，${fail} 失败`);
process.exit(fail ? 1 : 0);
