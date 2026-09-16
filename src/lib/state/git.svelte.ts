import { gitRoot, gitStatus, gitStashList, type GitEntry, type GitStatus, type GitStash } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import type { TabState } from "./tab";

/** 动作在 `git-ops.ts`，按需加载 —— 理由见那边的文件头 */
const ops = () => import("./git-ops");

/**
 * Git：仓库状态、找仓库、刷新、写锁。**动作**（提交、丢弃、stash、差异与合并标签）
 * 在 `git-ops.ts`，按需加载 —— 状态首屏就要，动作等人点了才要（issue #32）。
 *
 * 从 App.svelte 搬出来（issue #9 第 5a 步），逻辑一个字不改。
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
  /** stash 列表（issue #33 ⑪）。和 status 一起刷 —— 收进去、放出来都会动它 */
  stashes = $state<GitStash[]>([]);
  /** 正在刷新状态（`writing` 是另一件事：正在写） */
  busy = $state(false);
  /** 待确认丢弃的条目 —— 丢弃不可撤销，必须过用户这一关 */
  pendingDiscard = $state<GitEntry[] | null>(null);
  /**
   * 编辑器里显示注解（blame，issue #33 ⑭）。一个开关管所有标签 —— IDEA 是按文件开的，
   * 但「看谁改的」这个模式一旦进入，换文件多半还想看。只在内存里，重启就关。
   */
  blameOn = $state(false);

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
      // 两条子进程并行；stash 列表拿不到不算错（空仓库、老 git），当空表
      const [st, stashes] = await Promise.all([gitStatus(r), gitStashList(r).catch(() => [])]);
      // await 回来时仓库可能已经换了或关了（关闭项目 / 切项目正好撞上一次刷新）——
      // 那份状态是别人的，写进去标题栏就会挂着一个已经不存在的分支
      if (this.repo !== r) return;
      this.status = st;
      this.stashes = stashes;
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

  // ── 动作：全部转发到 git-ops.ts（按需加载）。签名和文档见那边 ──
  async run(what: string, fn: () => Promise<unknown>, doing: string): Promise<boolean> {
    return (await ops()).run(what, fn, doing);
  }
  async discard(entries: GitEntry[]) {
    return (await ops()).discard(entries);
  }
  async stashPush(): Promise<boolean> {
    return (await ops()).stashPush();
  }
  async stashPop(): Promise<boolean> {
    return (await ops()).stashPop();
  }
  async applyHunk(patch: string, unstage: boolean): Promise<boolean> {
    return (await ops()).applyHunk(patch, unstage);
  }
  async commit(message: string, amend: boolean): Promise<boolean> {
    return (await ops()).commit(message, amend);
  }
  async reloadDiff(id: number) {
    return (await ops()).reloadDiff(id);
  }
  async openDiff(e: GitEntry, staged: boolean) {
    return (await ops()).openDiff(e, staged);
  }
  async toggleDiffSide(id: number) {
    return (await ops()).toggleDiffSide(id);
  }
  async openCommitDiff(sha: string, short: string, rel: string) {
    return (await ops()).openCommitDiff(sha, short, rel);
  }
  async openMerge(e: GitEntry) {
    return (await ops()).openMerge(e);
  }
  async resolveMerge(tab: TabState, content: string, resolved: boolean) {
    return (await ops()).resolveMerge(tab, content, resolved);
  }
}

export const git = new Git();
