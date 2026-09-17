/**
 * Git 的命令包装 —— **只有懒加载的模块用的那些**（issue #32 瘦身，2026-09-17）。
 *
 * 首屏之前就要的三条（`gitRoot` / `gitStatus` / `gitStashList`：文件树的 git 字母、
 * 分支挂件、状态栏那格）留在 `commands.ts`。这里的提交 / 差异 / 分支 / 工作树 /
 * 拉取推送，调用方全在 `*-ops.ts` 和 Git 面板那些 lazy 的模块里 —— 包装函数跟着
 * 调用方走，入口包就不用带它们。DTO 一律还在 `commands.ts`（`dto_sync.rs` 只看那一个文件）。
 */
import { invoke, type Channel } from "@tauri-apps/api/core";
import type { GitCmd, GitEntry, DiffText, GitLogEntry, GitBranch, GitWorktree, RemoteProgress } from "./commands";

/**
 * 跑过的 git，最新的在前。
 *
 * 只在内存里，关掉应用就没 —— 它回答的是「刚才那条为什么失败」，不是考古。
 * 上限、截断和凭据打码全在 Rust 侧（`gitsvc::console`），前端只负责显示。
 */
export const gitConsole = () => invoke<GitCmd[]>("git_console");

/** 清空 Git 控制台。只碰内存里那个环，盘上本来就没有东西 */
export const clearGitConsole = () => invoke<void>("clear_git_console");

export const gitDiff = (root: string, path: string, staged: boolean, untracked: boolean) =>
  invoke<DiffText>("git_diff", { root, path, staged, untracked });

/** 按块暂存（issue #33 ⑫）：一段 patch 应用到暂存区；`reverse` = 撤掉 */
export const gitApplyCached = (root: string, patch: string, reverse: boolean) =>
  invoke<void>("git_apply_cached", { root, patch, reverse });

/** 已跟踪文件的改动收进 stash，工作区回到 HEAD；未跟踪的留在原地。没改动时报错 */
export const gitStashPush = (root: string) => invoke<void>("git_stash_push", { root });

/** 最新的 stash 放回工作区并删掉。撞上冲突时报错，stash 留着，改动列表里出现冲突 */
export const gitStashPop = (root: string) => invoke<void>("git_stash_pop", { root });

/** 不可撤销 —— 调用前必须让用户确认过 */
export const gitDiscard = (root: string, paths: string[], untracked: string[]) =>
  invoke<void>("git_discard", { root, paths, untracked });

export const gitCommit = (root: string, message: string, amend = false) =>
  invoke<string>("git_commit", { root, message, amend });

export const gitLogEntries = (root: string, limit = 200, all = false, path = "") =>
  invoke<GitLogEntry[]>("git_log_entries", { root, limit, all, path });

export const gitCommitFiles = (root: string, sha: string) =>
  invoke<GitEntry[]>("git_commit_files", { root, sha });

export const gitCommitDiff = (root: string, sha: string, path = "") =>
  invoke<DiffText>("git_commit_diff", { root, sha, path });

export const gitBranches = (root: string) => invoke<GitBranch[]>("git_branches", { root });

/**
 * 切分支。**失败时 reject 的是 `SwitchErr` 对象，不是字符串** ——
 * 调用方要 `catch` 之后判 `kind`，不能直接 `String(e)` 往界面上贴。
 */
export const gitSwitch = (root: string, name: string, create = false, from = "") =>
  invoke<string>("git_switch", { root, name, create, from });

/**
 * 删本地分支。**失败时 reject 的是 `BranchErr` 对象**，调用方判 `kind`。
 * `force` 走 `-D`，只在用户看过「还有没合并的提交」之后才传。
 */
export const gitBranchDelete = (root: string, name: string, force = false) =>
  invoke<void>("git_branch_delete", { root, name, force });

/** 重命名本地分支。目标名已存在时报错，不覆盖 */
export const gitBranchRename = (root: string, old: string, new_: string) =>
  invoke<void>("git_branch_rename", { root, old, new: new_ });

export const gitWorktrees = (root: string) => invoke<GitWorktree[]>("git_worktrees", { root });

/**
 * 新建工作树，返回新目录绝对路径 —— 可以直接当项目根打开。
 * 分支存不存在由 Rust 侧判断并决定加不加 `-b`。
 */
export const gitWorktreeAdd = (root: string, path: string, branch: string) =>
  invoke<string>("git_worktree_add", { root, path, branch });

/** 会删掉那个目录，调用前必须确认 */
export const gitWorktreeRemove = (root: string, path: string, force = false) =>
  invoke<void>("git_worktree_remove", { root, path, force });

/**
 * 抓远程。只读，不动工作区 —— 失败了没有任何后果。
 *
 * **`opId` 由调用方给，不是 Rust 返回的。**
 * 反过来写过一版（Rust 生成、跟着返回值给出去），而那样取消按钮
 * **永远点不动**：返回值要等操作跑完才到前端。
 *
 * 进度走 `Channel`，和终端那条是同一套机制。
 */
export const gitFetch = (
  root: string,
  remote: string,
  opId: number,
  onProgress: Channel<RemoteProgress>,
) => invoke<void>("git_fetch", { root, remote, opId, onProgress });

/** 推送当前分支。`setUpstream` 只在这个分支还没有上游时传真 */
export const gitPush = (
  root: string,
  remote: string,
  branch: string,
  setUpstream: boolean,
  opId: number,
  onProgress: Channel<RemoteProgress>,
) => invoke<void>("git_push", { root, remote, branch, setUpstream, opId, onProgress });

/**
 * 把已经抓下来的上游合进当前分支。不走网络。
 *
 * 拉取 = `gitFetch` + 这个，不是 `git pull` —— 复合命令失败时分不清
 * 是网络断了还是合并冲突了。
 */
export const gitMergeUpstream = (root: string, upstream: string, mode: "ff-only" | "merge" | "rebase") =>
  invoke<void>("git_merge_upstream", { root, upstream, mode });

/** 取消一个正在跑的远程操作。只对 fetch 开放 —— push 中途取消状态不确定 */
export const gitCancel = (id: number) => invoke<boolean>("git_cancel", { id });

/** 推上去会送出哪些提交。照 IDEA：列出提交，不是只给计数 */
export const gitOutgoing = (root: string, upstream: string, branch: string) =>
  invoke<string[]>("git_outgoing", { root, upstream, branch });
