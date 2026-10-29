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
// 突发十次刷新：状态和 stash 只准一轮在途，完成后补一次而不是全部并发。
let release!: () => void;
const gate = new Promise<void>(r => release = r);
let inFlight = 0, peak = 0, queries = 0;
internal.invoke = async (cmd: string, args: unknown) => {
  if (cmd !== "git_status" && cmd !== "git_stash_list") return original(cmd, args);
  queries++; inFlight++; peak = Math.max(peak, inFlight);
  await gate;
  try { return cmd === "git_status" ? { ...base, head: "burst-final" } : []; }
  finally { inFlight--; }
};
const burst = Array.from({ length: 10 }, (_, i) => git.refresh(i === 3));
await new Promise(r => setTimeout(r, 0));
ok(queries === 2 && peak === 2, "首轮未完成前，十次触发只启动 status + stash 两个查询");
release();
await Promise.all(burst);
ok(queries === 4 && peak === 2 && git.status?.head === "burst-final" && !git.busy, "十次触发合并为一轮补刷，最终状态生效且 busy 收尾");
internal.invoke = original;
// status 提前失败也要等 stash 收尾，不能让下一轮与上一轮的查询重叠。
let releaseStash!: () => void;
const stashGate = new Promise<void>(r => releaseStash = r);
queries = 0;
let statusCalls = 0;
internal.invoke = async (cmd: string, args: unknown) => {
  if (cmd === "git_status") { queries++; if (++statusCalls === 1) throw new Error("status failed"); return base; }
  if (cmd === "git_stash_list") { queries++; await stashGate; return []; }
  return original(cmd, args);
};
const failedRound = git.refresh();
const queuedRound = git.refresh(true);
await new Promise(r => setTimeout(r, 0));
ok(queries === 2, "status 失败后仍等同轮 stash 结束，不提前叠下一轮");
releaseStash();
await Promise.all([failedRound, queuedRound]);
ok(queries === 4 && !git.busy, "失败后补刷成功且释放占位");

let releaseOld!: () => void;
const oldGate = new Promise<void>(r => releaseOld = r);
internal.invoke = async (cmd: string, args: any) => {
  if (cmd === "git_status") {
    if (args.root === "/proj") await oldGate;
    return { ...base, root: args.root, head: args.root };
  }
  return cmd === "git_stash_list" ? [] : original(cmd, args);
};
const oldRepo = git.refresh();
git.repo = "/other";
await git.refresh();
releaseOld();
await oldRepo;
ok(git.status?.root === "/other" && !git.busy, "旧仓库结束不覆盖新仓库或挂住 busy");
internal.invoke = original;
console.log(`${fail ? "❌" : "✅"} Git 刷新联动：${pass} 通过，${fail} 失败`);
process.exit(fail ? 1 : 0);
