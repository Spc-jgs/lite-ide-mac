import {
  gitSwitch,
  gitStashPush,
  gitStashPop,
  gitWorktreeAdd,
  gitWorktreeRemove,
  gitMergeUpstream,
  gitBranchDelete,
  gitBranchRename,
  type SwitchErr,
  type BranchErr,
  type GitWorktree,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { git } from "./git.svelte";
import { tabflow } from "./tabflow.svelte";
import { worktree } from "./worktree.svelte";
import { branches } from "./branches.svelte";
import { readListPref, writeListPref } from "./prefs";

/**
 * 「最近」那组（照 IDEA 的 Recent）：最近切到过的分支，按仓库存在 localStorage，
 * 最多 5 条。存名字不存 sha：分支改名了这条就失效，list 那边对不上名字会自动不显示。
 */
const RECENT_MAX = 5;
export function recentKey(repo: string) {
  return `git-recent:${repo}`;
}
export function noteRecent(repo: string, name: string) {
  if (!name) return;
  const list = readListPref(recentKey(repo)).filter((n) => n !== name);
  list.unshift(name);
  writeListPref(recentKey(repo), list.slice(0, RECENT_MAX));
}

/**
 * 分支与工作树的**动作**：切分支（含被本地改动挡住那一问的三条出路）、
 * 开 / 建 / 移除工作树。状态在 `branches.svelte.ts`，那边一行转发到这里，
 * 按需加载 —— 理由见 `git-ops.ts` 的文件头（issue #32）。
 *
 * 逻辑从 `branches.svelte.ts` 搬过来，一个字不改；`this.` 换成 `branches.`。
 */

/**
 * `create` 为真时 `from` 是起点：`git switch -c name from`。空的话从当前 HEAD 分。
 * 起点在这里而不是先切过去再 `-c`：那是两次检出，中间那次会把工作区翻一遍。
 */
export function switchTo(name: string, create = false, from = "") {
  notify.closeBanner();
  branches.pendingCheckout = null;
  // base 要在切之前抓 —— 切完 git.status.branch 就是新的那个了
  const base = from || (git.status?.branch ?? "");
  void git.run(create ? "新建分支失败" : "切分支失败", async () => {
    try {
      await gitSwitch(git.repo!, name, create, from);
    } catch (e) {
      const err = e as SwitchErr;
      if (err?.kind === "local-changes" && err.files?.length) {
        branches.pendingCheckout = { name, create, files: err.files };
        return;
      }
      throw e;
    }
    /*
     * 先刷新再说话。**说的是「实际切到了哪儿」，不是「我请求切到哪儿」** ——
     * 请求 `origin/foo` 时 gitsvc 可能建了本地 `foo`，也可能切到了一个早就
     * 存在的同名本地分支（那时它跟踪的可能是别的远程）。前端去复现那套 DWIM
     * 规则就是把同一份判断写两处；读一遍刷新后的状态，说的话永远是真的。
     */
    await git.refresh();
    const now = git.status?.branch || name;
    const up = git.status?.upstream ? `（跟踪 ${git.status.upstream}）` : "";
    if (!git.status?.detached) noteRecent(git.repo!, now);
    notify.ok(create ? `已从 ${base} 新建并切到 ${now}` : `已切到 ${now}${up}`, 2800);
    await worktree.changed();
  }, create ? "新建分支" : "切分支");
}

/**
 * IDEA 的 Smart Checkout：改动收进 stash → 切分支 → 再放回来（issue #33 ⑪）。
 * 三步在一次 `git.run` 里，中间不刷新 —— 刷了也只是让改动列表闪一下空。
 *
 * 放回来撞上冲突时 `stash pop` 报错、stash 留着：分支已经切过去了，
 * 改动列表里出现「冲突中」，错误条上是 git 的原话。不回滚 —— 回滚要
 * 再切一次分支，而用户要的本来就是切过去。
 * 切分支这一步失败（极少：stash 之后还有别的东西挡着）就把 stash 放回来再报。
 */
export async function stashThenCheckout() {
  const p = branches.pendingCheckout;
  if (!p || !git.repo) return;
  branches.pendingCheckout = null;
  const repo = git.repo;
  const base = git.status?.branch ?? "";
  const ok = await git.run("切分支失败", async () => {
    await gitStashPush(repo);
    try {
      await gitSwitch(repo, p.name, p.create);
    } catch (e) {
      await gitStashPop(repo).catch(() => {});
      throw e;
    }
    await gitStashPop(repo);
  }, "切分支");
  if (ok) {
    if (!git.status?.detached) noteRecent(repo, git.status?.branch || p.name);
    notify.ok(`已切到 ${git.status?.branch || p.name}，改动已从 stash 取回`, 2800);
  }
  else {
    // `run` 失败不刷新，而这时分支多半已经切过去、盘上带着冲突标记
    await git.refresh();
  }
  if (!ok && git.status?.branch && git.status.branch !== base) {
    // 切过去了但取回时撞上冲突：错误条已经在了，这里补一句分支的事实
    notify.ok(`已切到 ${git.status.branch}`, 2800);
  }
  await worktree.changed();
}

/** 丢掉挡路的那几个改动，然后把刚才那次切换重放一遍 */
export async function discardThenCheckout() {
  const p = branches.pendingCheckout;
  if (!p || !git.repo) return;
  branches.pendingCheckout = null;
  const es = (git.status?.entries ?? []).filter((e) => p.files.includes(e.path));
  await git.discard(es);
  switchTo(p.name, p.create);
}

/**
 * 打开一个工作树 = **把项目根换过去**。
 *
 * `openPath` 对目录只做 `root = path`，**打开的标签一个都不动** ——
 * 于是文件树和 Git 栏切到了新工作树，而标签还指着旧的那份。
 * 这不是 bug（开着别处的文件是合法的），但一声不吭就变了半个界面，
 * 人会以为「怎么点了没反应」。做完说一句。
 */
export async function openWorktree(path: string) {
  await tabflow.openPath(path);
  const name = path.slice(path.lastIndexOf("/") + 1) || path;
  notify.ok(`项目根已切到 ${name}（打开的标签没有动）`, 3200);
}

export function newWorktree(dir: string, branch: string) {
  void git.run("新建工作树失败", async () => {
    // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
    const path = await gitWorktreeAdd(git.repo!, dir, branch);
    notify.ok(`工作树已建在 ${path}`, 3600);
    await tabflow.openPath(path);
  }, "新建工作树");
}

export function removeWorktree(w: GitWorktree, force: boolean) {
  branches.pendingWtRemove = null;
  void git.run("移除工作树失败", async () => {
    await gitWorktreeRemove(git.repo!, w.path, force);
    notify.ok(`已移除工作树 ${w.path}`);
    await worktree.changed();
  }, "移除工作树");
}

/**
 * 把 `ref` 合进当前分支（M9，分支菜单里的「合并到当前分支」）。
 *
 * 复用 `git_merge_upstream`：它本来就是 `git merge --no-edit <ref>`，只是名字里
 * 带着 upstream。用 `merge` 模式不用 `ff-only`：这条路是人主动选「合并」，
 * 快进不了就该真的合，不该停下来再问一次。冲突时 gitsvc 分成 `conflict`，
 * 工作区里的冲突标记会落进改动列表的「冲突中」，那里有 MergeView 接着。
 */
export async function mergeInto(ref: string) {
  const cur = git.status?.branch ?? "";
  const ok = await git.run("合并失败", async () => {
    await gitMergeUpstream(git.repo!, ref, "merge");
  }, "合并");
  await worktree.changed();
  if (ok) notify.ok(`已把 ${ref} 合进 ${cur || "当前分支"}`, 2800);
  else await git.refresh(); // 冲突时 run 不刷新，而这时盘上已经带着冲突标记
}

export function renameBranch(old: string, next: string) {
  void git.run("重命名分支失败", async () => {
    await gitBranchRename(git.repo!, old, next);
    notify.ok(`已把 ${old} 改名为 ${next}`, 2800);
  }, "重命名分支");
}

/**
 * 删本地分支。第一次走 `-d`：分支上有没合进别处的提交时 git 会拒，那一档
 * （`kind === "not-merged"`）不是错误，是要人决定 —— 摆成确认卡片，给「仍然删除」
 * （`-D`）一条路。**其余错误照旧上抛**：当前分支删不掉那句 git 说得比我们准。
 */
export function deleteBranch(name: string, force = false) {
  branches.pendingBranchDelete = null;
  void git.run("删除分支失败", async () => {
    try {
      await gitBranchDelete(git.repo!, name, force);
    } catch (e) {
      const err = e as BranchErr;
      if (err?.kind === "not-merged" && !force) {
        branches.pendingBranchDelete = { name, notMerged: true };
        return;
      }
      throw e;
    }
    notify.ok(`已删除分支 ${name}`, 2800);
  }, "删除分支");
}
