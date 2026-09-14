import {
  gitRoot,
  gitStatus,
  gitDiff,
  gitCommitDiff,
  gitStage,
  gitDiscard,
  gitCommit,
  readText,
  writeText,
  type GitEntry,
  type GitStatus,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { tabflow } from "./tabflow.svelte";
import { worktree } from "./worktree.svelte";
import type { TabState } from "./tab";

/**
 * Git：仓库状态、写操作的统一出口（占锁 / 进度 / 收口）、差异与合并标签。
 *
 * 从 App.svelte 搬出来（issue #9 第 5a 步），逻辑一个字不改。三块放一个文件
 * 是因为它们互相调：`refresh` 要重拉打开着的差异标签，`resolveMerge` 要占锁，
 * 拆成两个文件就得互相 import 或者再来一对钩子。
 *
 * **不在这里的**：分支 / 工作树（5b）、拉取推送（5c）、`editorMarks` 那条 effect
 * （它是给编辑器看的，留在 App 直到编辑器那块也出去）、「换项目根就重新找仓库」
 * 那条 effect（`$effect` 只能在组件或 `$effect.root` 里，App 那条 effect 调这里的
 * `locate`）。
 */
class Git {
  /** 项目所属仓库的根；不是仓库就是 null，整块 Git 功能随之隐身 */
  repo = $state<string | null>(null);
  status = $state<GitStatus | null>(null);
  /** 正在刷新状态（`writing` 是另一件事：正在写） */
  busy = $state(false);
  /** 待确认丢弃的条目 —— 丢弃不可撤销，必须过用户这一关 */
  pendingDiscard = $state<GitEntry[] | null>(null);

  /** 当前编辑的文件在 git 状态里对应的那条，没有就是干净的 */
  activeEntry = $derived.by(() => {
    const st = this.status;
    const t = tabs.active;
    if (!st || !t || t.mode === "diff") return null;
    const prefix = `${st.root}/`;
    if (!t.path.startsWith(prefix)) return null;
    const rel = t.path.slice(prefix.length);
    return st.entries.find((e) => e.path === rel) ?? null;
  });

  /**
   * 正在跑的那个**写**操作叫什么（`busy` 是另一件事：它指「正在刷新状态」）。
   *
   * **守卫看它，不看 `notify.doing`** —— 后者要等 300ms 才亮（见 `run`），
   * 那段空窗期里守卫会形同虚设。
   */
  writing = $state<string | null>(null);

  /**
   * 换项目根就重新找仓库。找不到时把 Git 的一切都清干净。
   *
   * App 的 effect 调（`$effect` 只能在组件里）。返回找到的仓库根；**确定**不是仓库
   * 返回 `null`；没法下判断（还没有项目根、探测本身出错）返回 `undefined` ——
   * 调用方只在「确定不是仓库」时把侧边栏切回文件树。
   *
   * 「还没有项目根」必须是 `undefined` 不是 `null`：启动时这条 effect 先于会话恢复
   * 跑一次（`root` 还是 null），要是当成「确定不是仓库」，会把快照里刚恢复的
   * `sideView: "git"` 改回文件树，而后面真找到仓库也不会改回去 —— 拆分时踩过
   * （grok 第二轮 review 抓的，JOURNAL 2026-09-14）。
   */
  async locate(root: string | null): Promise<string | null | undefined> {
    if (!root) {
      this.repo = null;
      this.status = null;
      return undefined;
    }
    try {
      const found = await gitRoot(root);
      this.repo = found;
      if (!found) this.status = null;
      else void this.refresh();
      return found;
    } catch {
      this.repo = null;
      this.status = null;
      return undefined;
    }
  }

  /**
   * 刷新一次 git 状态。
   *
   * 触发点是「窗口获得焦点」「保存之后」「做完任一 git 动作」，不是定时轮询 ——
   * 每次都要起一个 git 子进程（约 5–15ms），常年轮询是白烧电。
   * 用户在终端里 commit 完切回来，焦点事件正好把状态带新。
   */
  async refresh() {
    const r = this.repo;
    if (!r) return;
    this.busy = true;
    try {
      this.status = await gitStatus(r);
      // 打开着的工作区差异跟着更新，否则暂存完还停在旧内容上。
      // 历史提交的差异是不变的，重拉纯属浪费一次子进程
      await Promise.all(
        tabs.list.filter((t) => t.mode === "diff" && !t.diffSha).map((t) => this.reloadDiff(t.id)),
      );
    } catch (e) {
      notify.fail(String(e), 4000);
    } finally {
      this.busy = false;
    }
  }

  /**
   * 占住「这个仓库正在被写」这件事。占得到返回 true。
   *
   * # 为什么要从 `run` 里抽出来（issue #23）
   *
   * 这道守卫原来长在 `gitDo` 里面，于是它只盖住走 `gitDo` 的那些路径。
   * 拉取、推送、解决冲突这三条**在外面** —— 前两条有自己的进度条和取消
   * （`syncing`），第三条直接调 `gitStage`。结果是：
   *
   * > pre-commit 钩子跑三十秒的时候，拉取按钮仍然可以点。
   *
   * 而那正是守卫要挡的场景 —— 命令挪到阻塞池之后它们之间不再由主线程串行，
   * 两条 git 撞上 `index.lock` 是真会发生的（issue #11 记着这个回归点）。
   * 撞上之后用户看到的是 git 的英文报错，而 issue #15 刚把这类东西翻译掉。
   *
   * 抽出来之后这个信号的语义也更准了：它说的是**「这个仓库现在有人在写」**，
   * 不是「`gitDo` 在跑」。
   *
   * **`git fetch` 不占**：它不碰 index（写的是 refs 和 FETCH_HEAD），
   * 和 commit 用的不是同一把锁。为了对称而把它也挡住，只会让
   * 「钩子跑着的时候连拉一下都不行」——挡住的是一件本来不会出事的事。
   * 但 `doPull` **整体**要占，它末尾那次合并是真的动 index。
   */
  claim(doing: string): boolean {
    if (this.writing) {
      /*
       * **不能走 `notify.fail`。** 状态栏左槽是 `{#if doing}{:else if info}
       * {:else if error}`，而 doing 排在最前面 —— 慢操作正是 doing 亮着的
       * 时候，那句 fail 写进去也显示不出来，4 秒后还被自己的定时器清掉。
       * 于是用户看到的仍然是「点了没反应」，正是这道守卫要避免的东西。
       *
       * 直接改 doing 的文案：渲染是「正在${doing}…」，这里拼出来就是
       * 「正在提交，请等它做完…」。`writing` 存的是原始动作名，不会被
       * 这句话污染，所以点第三次、第四次文案也不会越接越长。
       * 操作结束时占用方的 `finally` 会清掉它，不用另设一个定时器。
       */
      notify.doing = `${this.writing}，请等它做完`;
      return false;
    }
    this.writing = doing;
    return true;
  }

  /**
   * 放开。**每个 `claim` 都必须有一个配对的、在 `finally` 里的这句。**
   *
   * 顺带把 `notify.doing` 清掉 —— 这一句是被守卫挡下来的那次调用写进去的
   * （「正在提交，请等它做完」），而**写它的那次调用已经 return 了，
   * 没有人会来清**。只有占着锁的那一方知道什么时候该收场。
   *
   * 漏了这句的表现：拉取被挡一次之后，状态栏左槽永远挂着
   * 「正在合并上游，请等它做完…」，连当前打开的是哪个文件都被它盖住 ——
   * 浏览器里实测到的，三秒后仍在。`gitDo` 一直是对的（它自己 finally 里
   * 清了），错的是新收进来的那三条。放进 `release` 就不会再漏一条。
   */
  release() {
    this.writing = null;
    notify.doing = "";
  }

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
  async run(what: string, fn: () => Promise<unknown>, doing: string): Promise<boolean> {
    if (!this.repo) return false;
    if (!this.claim(doing)) return false;
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
      await this.refresh();
      return true;
    } catch (e) {
      notify.block(what, e);
      return false;
    } finally {
      // **三件事都必须在 finally 里。** 失败路径上漏掉定时器，300ms 后
      // 会亮起一句永远不灭的「正在提交…」；漏掉 writing，后面所有写操作
      // 都会被上面那道守卫挡下来
      clearTimeout(tip);
      notify.doing = "";
      this.release();
    }
  }

  async discard(entries: GitEntry[]) {
    this.pendingDiscard = null;
    const repo = this.repo;
    if (!repo) return;
    // 跟踪的走 git restore，未跟踪的只能直接删 —— gitsvc 里分了两条路
    const tracked = entries.filter((e) => !e.untracked).map((e) => e.path);
    const untracked = entries.filter((e) => e.untracked).map((e) => e.path);
    await this.run(
      "丢弃失败",
      async () => {
        await gitDiscard(repo, tracked, untracked);
        // **只有这一条补了成功回执，暂存/取消暂存没补。**
        // 判据是「结果看不看得见」：暂存之后文件当场移到已暂存区，
        // 界面自己说清楚了，再弹一句是噪声；而丢弃是不可逆的那一档，
        // 文件直接从改动列表里消失，不说一句就分不清「丢掉了」和「没点中」。
        notify.ok(`已丢弃 ${entries.length} 个文件的改动`, 3000);
      },
      "丢弃改动",
    );
    await worktree.changed();
  }

  /** 返回提交成没成 —— 「提交并推送」要据此决定推不推 */
  commit(message: string, amend: boolean): Promise<boolean> {
    return this.run("提交失败", async () => {
      const out = await gitCommit(this.repo!, message, amend);
      notify.ok(out.split("\n")[0] || "已提交", 3000);
    }, "提交");
  }

  // ── 差异 / 合并标签 ──

  async reloadDiff(id: number) {
    const tab = tabs.byId(id);
    const repo = this.repo;
    if (!tab || !repo || tab.mode !== "diff" || !tab.rel) return;
    try {
      if (tab.diffSha) {
        const d = await gitCommitDiff(repo, tab.diffSha, tab.rel);
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
  async openDiff(e: GitEntry, staged: boolean) {
    if (!this.repo) return;
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
    tabs.activeId = id;
    await this.reloadDiff(id);
  }

  /** 差异标签上切换「已暂存 ↔ 未暂存」 */
  async toggleDiffSide(id: number) {
    const tab = tabs.byId(id);
    if (!tab) return;
    tab.diffStaged = !tab.diffStaged;
    // 未跟踪文件一旦进了暂存区，就该按普通 diff 读，不能再走 --no-index
    const e = this.status?.entries.find((x) => x.path === tab.rel);
    tab.diffUntracked = !!e?.untracked && !tab.diffStaged;
    await this.reloadDiff(id);
  }

  /** 从日志里打开某次提交中某个文件的差异 */
  async openCommitDiff(sha: string, short: string, rel: string) {
    if (!this.repo) return;
    const key = `git-commit:${sha}:${rel}`;
    let id = tabs.list.find((t) => t.path === key)?.id;
    if (id === undefined) {
      id = tabs.add({
        path: key,
        name: rel.slice(rel.lastIndexOf("/") + 1),
        mode: "diff",
        dirty: false,
        size: 0,
        rel,
        diffSha: sha,
        diffShort: short,
      });
    }
    tabs.activeId = id;
    await this.reloadDiff(id);
  }

  /**
   * 打开冲突合并标签。
   *
   * 读的是**工作区文件**而不是 `git show :2:` / `:3:` 那三个暂存位 ——
   * 工作区那份才是用户此刻真正会提交的东西，他可能已经手改过一部分，
   * 从暂存位重建会把那些手改悄悄抹掉。
   */
  async openMerge(e: GitEntry) {
    const repo = this.repo;
    if (!repo) return;
    const full = `${repo}/${e.path}`;
    const key = `git-merge:${e.path}`;
    try {
      const content = (await readText(full)).content;
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
        });
      } else {
        const t = tabs.byId(id);
        if (t) t.mergeText = content;
      }
      tabs.activeId = id;
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
  async resolveMerge(tab: TabState, content: string, resolved: boolean) {
    const repo = this.repo;
    if (!repo || !tab.rel) return;
    if (!this.claim(resolved ? "标记为解决" : "保存冲突进度")) return;
    try {
      await writeText(`${repo}/${tab.rel}`, content, tab.encoding);
      if (resolved) {
        await gitStage(repo, [tab.rel]);
        notify.ok(`${tab.name} 已标记为解决`);
        tabflow.doClose(tab);
      } else {
        tab.mergeText = content;
        notify.ok(`${tab.name} 进度已保存`);
      }
      await this.refresh();
    } catch (e) {
      notify.fail(String(e));
    } finally {
      this.release();
    }
  }
}

export const git = new Git();
