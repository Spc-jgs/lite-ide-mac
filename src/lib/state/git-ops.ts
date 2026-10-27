import { gitStage, readText, writeText, type GitEntry } from "../ipc/commands";
import { gitDiff, gitCommitDiff, gitCommitVsWorktree, gitDiscard, gitCommit, gitStashPush, gitStashPop, gitApplyCached, gitApplyWorktree, gitCherryPick, gitFileAt, gitOp } from "../ipc/git";
import { notify } from "./notify.svelte";
import { errText } from "./err-text";
import { tabs } from "./tabs.svelte";
import { tabflow } from "./tabflow.svelte";
import { worktree } from "./worktree.svelte";
import { git } from "./git.svelte";
import type { TabState } from "./tab";

/**
 * Git 的**动作**：写操作的统一出口（占锁 / 进度 / 收口）、丢弃、stash、提交、
 * 差异与合并标签。状态在 `git.svelte.ts`，这里只有函数。
 *
 * # 为什么和状态分开放（issue #32）
 *
 * 状态要在首屏之前就有 —— 文件树的 git 字母、标题栏的分支挂件、状态栏那格
 * 都读它。动作一个都不用：它们全在人点了什么之后才跑。而入口包卡在 150 KiB
 * 的红线上，`git` / `remote` / `branches` 三个 store 加起来 11 KB，动作占了
 * 八成。所以状态留在入口，动作走 `import()` —— `git.svelte.ts` 里每个动作
 * 是一行转发（`(await ops()).discard(...)`），调用方一个字不用改。
 *
 * 循环引用是**有意的**：这里静态 import `git`，那边动态 import 这里。
 * 动态的那条边不参与模块初始化顺序，所以不是环。
 *
 * 逻辑从 `git.svelte.ts` 搬过来，一个字不改；`this.` 换成 `git.`。
 */

/** git 的原话能不能接在「做什么：」后面放进胶囊：一行、合起来 90 字以内（胶囊最多三行，再长就是说明，该走卡片） */
export function fitsToast(what: string, raw: string): boolean {
  return !raw.includes("\n") && [...what, ...raw].length <= 90;
}

/** 上一次失败时 git 的原话（`run` 记，`conflictOrRaw` 读） */
let lastRaw = "";

/**
 * git 写操作的统一出口：做完刷新状态，失败统一收口（issue #15）。
 *
 * `doing` 是**正在做的那件事的名字**，不是可选的装饰 —— 这几条命令全都
 * 被有意挪到了阻塞池上（`git_commit` 因为 pre-commit 钩子跑什么是仓库
 * 说了算，跑一遍 eslint 三十秒；`git_switch` 检出几千个文件是秒级，
 * 见 rules/rust.md 那张表）。原来这段时间界面**什么都不显示**，
 * 和「点了没反应」长得一模一样 —— 而那正是让人反复点的形状。
 *
 * 顺带把「反复点」真的挡住了：一次只允许一个写操作。挪到阻塞池之后
 * 命令之间不再由主线程串行，两条 git 撞上 `index.lock` 是真会发生的
 * （issue #11 里专门记着这个回归点）。这里挡住，就不用等 git 报错再翻译。
 */
export async function run(what: string, fn: () => Promise<unknown>, doing: string): Promise<boolean> {
  if (!git.repo) return false;
  if (!git.claim(doing)) return false;
  /*
   * **慢的才说话。**
   *
   * 暂存一个文件通常不到 100ms，那种一闪而过的字比不显示更让人分心 ——
   * 眼角瞥见状态栏动了一下，回头看又没了。300ms 是「人开始觉得卡」的
   * 那条线：比它快的操作当作瞬时，比它慢的才需要一句「我在做」。
   */
  const tip = setTimeout(() => (notify.doing = doing), 300);
  try {
    await fn();
    await git.refresh();
    return true;
  } catch (e) {
    // git 的原话一行说得清（`fatal: invalid reference: x`）就走胶囊，不挡编辑区；多行的（涉及哪些文件）照旧卡片 ——
    // ui.md 第十五条的判据本来就是「一句话说得清 → 胶囊」。原来一律卡片（2026-10-21 用户：提示是不是都可以换成胶囊）。
    // 原话另记一份：冲突时 conflictOrRaw 要把它接在冲突说明后面，不能指望它在卡片上
    lastRaw = errText(e);
    if (fitsToast(what, lastRaw)) notify.fail(`${what}：${lastRaw}`);
    else notify.block(what, e);
    return false;
  } finally {
    // **三件事都必须在 finally 里。** 失败路径上漏掉定时器，300ms 后
    // 会亮起一句永远不灭的「正在提交…」；漏掉 writing，后面所有写操作
    // 都会被上面那道守卫挡下来
    clearTimeout(tip);
    notify.doing = "";
    git.release();
  }
}

/** 做到一半的操作叫什么（横幅、胶囊上的字） */
export const OP_NAME = { rebase: "变基", merge: "合并", "cherry-pick": "cherry-pick", revert: "回退" } as const;

/**
 * 做到一半的操作：继续 / 跳过这一步 / 放弃（docs/TAKEOVER.md G25）。做完说结果：继续之后变基可能又停在下一步的冲突上 ——
 * 读刷新后的状态说「又停住了」，不说「做完了」（ui.md 第十三条：说结果，不说请求）
 */
export async function opAction(action: "continue" | "skip" | "abort") {
  git.pendingOpAbort = false;
  const repo = git.repo;
  const op = git.status?.op;
  if (!repo || !op) return;
  const name = OP_NAME[op.kind];
  const label = action === "continue" ? `继续${name}` : action === "skip" ? "跳过此步" : `中止${name}`;
  const ok = await run(`${label}失败`, () => gitOp(repo, action), label);
  await worktree.changed();
  if (!ok) return;
  const now = git.status?.op;
  if (action === "abort") notify.ok(`已中止${name}，仓库已恢复到开始前的状态`);
  else if (now) notify.hint(`遇到冲突${now.step && now.total ? `（第 ${now.step} / ${now.total} 步）` : ""}：请解决并暂存后继续`);
  else notify.ok(`${name}已完成`);
}

export async function discard(entries: GitEntry[]) {
  git.pendingDiscard = null;
  const repo = git.repo;
  if (!repo) return;
  // 跟踪的走 git restore（不可撤销），未跟踪的进废纸篓（git 里没有它们的历史，直接删就彻底没了）—— gitsvc 里分了两条路
  const tracked = entries.filter((e) => !e.untracked).map((e) => e.path);
  const untracked = entries.filter((e) => e.untracked).map((e) => e.path);
  await run(
    "放弃更改失败",
    async () => {
      await gitDiscard(repo, tracked, untracked);
      // **只有这一条补了成功回执，暂存/取消暂存没补。**
      // 判据是「结果看不看得见」：暂存之后文件当场移到已暂存区，
      // 界面自己说清楚了，再弹一句是噪声；而丢弃是不可逆的那一档，
      // 文件直接从改动列表里消失，不说一句就分不清「丢掉了」和「没点中」。
      notify.ok(`已放弃 ${entries.length} 个文件的更改`, 3000);
    },
    "放弃更改",
  );
  await worktree.changed();
}

/**
 * 收进 stash / 放回来（issue #33 ⑪）。两条都走 `run`：写守卫、慢了才说话、
 * 完了刷新。放回来撞上冲突时 git 报错、stash 留着，刷新后改动列表里
 * 出现「冲突中」—— 那正是该看到的，不另外翻译。
 * 盘上的文件变了，两条都要 `worktree.changed()`。
 */
export async function stashPush(): Promise<boolean> {
  const ok = await run("储藏失败", async () => {
    await gitStashPush(git.repo!);
    notify.ok("已储藏更改，工作区已还原为 HEAD", 3000);
  }, "储藏更改");
  await worktree.changed();
  return ok;
}

export async function stashPop(): Promise<boolean> {
  const ok = await run("恢复储藏失败", async () => {
    await gitStashPop(git.repo!);
    notify.ok("已恢复储藏的更改", 3000);
  }, "恢复储藏");
  // `run` 失败不刷新，而 pop 撞上冲突时盘上**已经**变了 —— 冲突得让人看见
  if (!ok) await conflictOrRaw("恢复储藏");
  await worktree.changed();
  return ok;
}

/**
 * 写操作失败、刷新之后发现是**撞上冲突**：把横幅从「git 的原话」换成第三档的说法
 * （ui.md 十三：把挡路的摆出来，给现成的出路）。git 那段 "hint: After resolving the
 * conflicts, mark them with git add/rm…" 是写给命令行用户的；界面用户要的是
 * 「哪几个文件、去哪儿解决」。不是冲突（比如 sha 不存在）就保留 `run` 已经弹出的原话。
 */
async function conflictOrRaw(what: string) {
  await git.refresh();
  const files = git.status?.entries.filter((e) => e.conflicted).map((e) => e.path) ?? [];
  if (files.length === 0) return;
  const raw = lastRaw;
  // run 那里可能已经弹了一句一行的胶囊：冲突的说明更完整，收掉它
  notify.dismiss();
  notify.block(
    `${what}时发生冲突`,
    `${files.length} 个文件存在冲突：${files.join("、")}\n` +
      "请在「更改」中逐个打开并解决，保存后即标记为已解决；全部解决后提交即可。" +
      (raw ? `\n\nGit 输出：${raw}` : ""),
  );
}

/**
 * 按块暂存 / 取消暂存（issue #33 ⑫）：patch 由差异视图从它手上那份 diff 拆出来。
 * 做完 `run` 会刷新 status，`refresh` 再把开着的工作区差异标签重拉 —— 那一块
 * 就从这一侧消失、出现在另一侧。
 */
export function applyHunk(patch: string, unstage: boolean): Promise<boolean> {
  return run(unstage ? "取消暂存失败" : "暂存失败", async () => {
    await gitApplyCached(git.repo!, patch, unstage);
  }, unstage ? "取消暂存" : "暂存");
}

/**
 * 撤销一块（issue #38）：`git apply -R` 到工作区。确认条已经过了（`pendingRevertHunk`），
 * 这里只管做。盘上的文件变了 —— 同 `discard`，做完 `worktree.changed()` 让开着的
 * 编辑器重读；差异标签由 `run` 里的 `refresh` 重拉，那一块就从视图里消失。
 * 成功回执同 `discard` 那条的理由：不可逆的那档，不说一句分不清「撤了」和「没点中」。
 */
export async function revertHunk(patch: string): Promise<boolean> {
  git.pendingRevertHunk = null;
  const ok = await run("回滚更改失败", async () => {
    await gitApplyWorktree(git.repo!, patch, true);
    notify.ok("已回滚此处更改", 3000);
  }, "回滚更改");
  await worktree.changed();
  return ok;
}

/**
 * 差异标签**新侧**的全文（2026-10-13，差异视图「展开 N 行未改动」用）。没改动的行两边一样，从新侧取就行；新侧在哪按差异的种类：
 * - 某次提交的差异：那次提交（`git show <sha>:path`）
 * - 暂存区的差异（暂存区 ↔ HEAD）：暂存区（`git show :path`）
 * - 工作区的差异、「和本地比较」：盘上那份
 * `truncated` = 文件超过上限、只拿到前半截 —— 调用方据此说「太大」，而不是误报成「文件改过了」
 */
export async function diffNewText(id: number): Promise<{ text: string; truncated: boolean } | null> {
  const tab = tabs.byId(id);
  const root = git.repo;
  if (!tab || !root || !tab.rel) return null;
  if (tab.diffSha && !tab.diffToLocal) return gitFileAt(root, tab.diffSha, tab.rel);
  if (tab.diffStaged && !tab.diffSha) return gitFileAt(root, "", tab.rel);
  const f = await readText(`${root}/${tab.rel}`);
  return { text: f.content, truncated: false };
}

/** 返回提交成没成 —— 「提交并推送」要据此决定推不推 */
export function commit(message: string, amend: boolean): Promise<boolean> {
  return run("提交失败", async () => {
    const out = await gitCommit(git.repo!, message, amend);
    // 胶囊说 git 回的那一行（分支、短 sha、标题），按钮原地说「已提交」—— 两句不是同一个信息
    notify.ok(out.split("\n")[0] || "已提交");
    git.flashDone("提交");
  }, "提交");
}

// ── 差异 / 合并标签 ──

export async function reloadDiff(id: number) {
  const tab = tabs.byId(id);
  const repo = git.repo;
  if (!tab || !repo || tab.mode !== "diff" || !tab.rel) return;
  try {
    if (tab.diffSha) {
      // 「那次提交本身」和「那次提交到现在」是两个问题（issue #39），走两条命令
      const d = tab.diffToLocal
        ? await gitCommitVsWorktree(repo, tab.diffSha, tab.rel)
        : await gitCommitDiff(repo, tab.diffSha, tab.rel);
      tab.diffRaw = d.text;
      tab.diffCapped = d.truncated;
      return;
    }
    let d = await gitDiff(repo, tab.rel, !!tab.diffStaged, !!tab.diffUntracked);
    /*
     * 这一侧空了，就看看另一侧有没有东西。
     *
     * 典型情形：差异标签开着，用户在改动列表里把这个文件暂存了 ——
     * 改动跑去了暂存区，工作区侧变空，标签上就只剩一句「没有差异」，
     * 看着像坏了。其实内容还在，只是换了一边。自动跟过去。
     */
    if (!d.text.trim()) {
      const other = await gitDiff(repo, tab.rel, !tab.diffStaged, false);
      if (other.text.trim()) {
        tab.diffStaged = !tab.diffStaged;
        tab.diffUntracked = false;
        d = other;
      }
    }
    tab.diffRaw = d.text;
    tab.diffCapped = d.truncated;
  } catch (e) {
    tab.diffRaw = "";
    tab.diffCapped = false;
    notify.fail(String(e));
  }
}

/** 打开（或复用）一个差异标签 */
export async function openDiff(e: GitEntry, staged: boolean) {
  if (!git.repo) return;
  const key = `git-diff:${e.path}`;
  let id = tabs.list.find((t) => t.mode === "diff" && t.path === key)?.id;
  if (id === undefined) {
    id = tabs.add({
      path: key,
      name: e.path.slice(e.path.lastIndexOf("/") + 1),
      mode: "diff",
      dirty: false,
      size: 0,
      rel: e.path,
    });
  }
  // 从数组里重新取一次，拿到的才是响应式的那份
  const tab = tabs.byId(id);
  if (!tab) return;
  tab.diffStaged = staged;
  tab.diffUntracked = e.untracked && !staged;
  tabs.show(id);
  await reloadDiff(id);
}

/** 差异标签上切换「已暂存 ↔ 未暂存」 */
export async function toggleDiffSide(id: number) {
  const tab = tabs.byId(id);
  if (!tab) return;
  tab.diffStaged = !tab.diffStaged;
  // 未跟踪文件一旦进了暂存区，就该按普通 diff 读，不能再走 --no-index
  const e = git.status?.entries.find((x) => x.path === tab.rel);
  tab.diffUntracked = !!e?.untracked && !tab.diffStaged;
  await reloadDiff(id);
}

/** 从日志里打开某次提交中某个文件的差异 */
/**
 * 开一个历史差异标签。`toLocal`（issue #39「和本地比较」）= 那次提交 → 现在的工作区，
 * 和「那次提交本身」是两个标签、两个 key —— 同一个文件两种比法可以同时开着，
 * 所以标签名要能分开：本地那种的显示名带「↔ 本地」。
 */
export async function openCommitDiff(sha: string, short: string, rel: string, toLocal = false) {
  if (!git.repo) return;
  const key = `${toLocal ? "git-local" : "git-commit"}:${sha}:${rel}`;
  let id = tabs.list.find((t) => t.path === key)?.id;
  if (id === undefined) {
    const name = rel.slice(rel.lastIndexOf("/") + 1);
    id = tabs.add({
      path: key,
      name,
      mode: "diff",
      dirty: false,
      size: 0,
      rel,
      diffSha: sha,
      diffShort: short,
      ...(toLocal ? { diffToLocal: true, title: `${name} ↔ 本地` } : {}),
    });
  }
  tabs.show(id);
  await reloadDiff(id);
}

/**
 * cherry-pick（issue #39）。撞冲突的路和 `stashPop` 一样：git 报错、盘上带冲突标记、
 * 改动列表出现「冲突中」，不回滚 —— `run` 失败不刷新，所以失败也要自己 `refresh` 一次，
 * 不然冲突文件在界面上看不见。盘上变了，`worktree.changed()` 让开着的编辑器重读。
 */
export async function cherryPick(sha: string, short: string): Promise<boolean> {
  const ok = await run("拣选失败", async () => {
    await gitCherryPick(git.repo!, sha);
    notify.ok(`已将 ${short} 拣选到当前分支`, 3000);
  }, "cherry-pick");
  if (!ok) await conflictOrRaw(`cherry-pick ${short}`);
  await worktree.changed();
  return ok;
}

/**
 * 打开冲突合并标签。
 *
 * 读的是**工作区文件**而不是 `git show :2:` / `:3:` 那三个暂存位 ——
 * 工作区那份才是用户此刻真正会提交的东西，他可能已经手改过一部分，
 * 从暂存位重建会把那些手改悄悄抹掉。
 */
export async function openMerge(e: GitEntry) {
  const repo = git.repo;
  if (!repo) return;
  const full = `${repo}/${e.path}`;
  const key = `git-merge:${e.path}`;
  try {
    const file = await readText(full);
    const content = file.content;
    let id = tabs.list.find((t) => t.path === key)?.id;
    if (id === undefined) {
      id = tabs.add({
        path: key,
        name: e.path.slice(e.path.lastIndexOf("/") + 1),
        mode: "merge",
        dirty: false,
        size: 0,
        rel: e.path,
        mergeText: content,
        encoding: file.encoding,
        bom: file.bom,
        eol: file.eol,
      });
    } else {
      const t = tabs.byId(id);
      if (t) {
        t.mergeText = content;
        t.encoding = file.encoding;
        t.bom = file.bom;
        t.eol = file.eol;
      }
    }
    tabs.show(id);
  } catch (err) {
    notify.fail(String(err));
  }
}

/**
 * 冲突解决完写回文件；全部决定完的才 git add 标记已解决。
 *
 * 占锁（issue #23）：`gitStage` 动 index，而这条路原来在守卫外面 ——
 * 解决冲突的场合**尤其**容易撞上，那时人往往同时开着终端在跑 `git status`
 * 或另一次 `git add`。
 *
 * 写文件那一步也圈在里面：它和紧跟着的 `gitStage` 必须是一件事，
 * 中间插进一次别的写操作，暂存的就是半份内容。
 */
export async function resolveMerge(tab: TabState, content: string, resolved: boolean) {
  const repo = git.repo;
  if (!repo || !tab.rel) return;
  if (!git.claim(resolved ? "标记为解决" : "保存冲突进度")) return;
  try {
    await writeText(`${repo}/${tab.rel}`, content, tab.encoding, tab.bom, tab.eol);
    if (resolved) {
      await gitStage(repo, [tab.rel]);
      notify.ok(`${tab.name} 已标记为解决`);
      tabflow.doClose(tab);
    } else {
      tab.mergeText = content;
      notify.ok(`已保存 ${tab.name} 的冲突解决进度`);
    }
    await git.refresh();
  } catch (e) {
    notify.fail(String(e));
  } finally {
    git.release();
  }
}
