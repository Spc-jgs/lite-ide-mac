import { Channel } from "@tauri-apps/api/core";
import {
  gitFetch,
  gitPush,
  gitMergeUpstream,
  gitCancel,
  gitOutgoing,
  type RemoteProgress,
  type RemoteErr,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { git } from "./git.svelte";
import { project } from "./project.svelte";
import { worktree } from "./worktree.svelte";

/**
 * 拉取与推送：进度、取消、分岔决策、推送确认、失败提示。
 *
 * 从 App.svelte 搬出来（issue #9 第 5b 步），逻辑一个字不改。
 *
 * 一个往外的钩子 `warmUi`：分岔决策条 / 推送确认条 / 失败提示都长在 Git 那组
 * 懒加载的组件里（`RemoteBars`），从菜单直接拉时它可能还没到位 —— 不先拉一下，
 * 决策条不会出现，看着像「点了没反应」。那组 lazy 在 App 里，App 装这个钩子。
 */
class Remote {
  /**
   * 远程操作的 id。**前端发号，不是等 Rust 返回。**
   *
   * 等返回值的话取消按钮永远点不动 —— 返回值要等操作跑完才到。
   * 这是实测点了一次取消、发现 `git_cancel` 压根没被调到才发现的。
   */
  #nextOpId = 0;

  /** 正在跑的远程操作。null = 没有 */
  syncing = $state<{
    what: "pull" | "push" | "fetch";
    id: number;
    phase: string;
    percent: number | null;
  } | null>(null);

  /** 分岔了要先决定合并还是变基。null = 没在问 */
  pendingDiverge = $state<{ upstream: string } | null>(null);

  /**
   * 上次选的合并方式。
   *
   * IDEA 的「记住这次选择」——分岔是常态，每次都问同一个问题很烦。
   * **但只记在内存里**：跨重启还记着的话，下次分岔时会用一个人早就忘了的
   * 策略默默合并，那比多问一次糟。
   */
  lastMergeMode = $state<"merge" | "rebase" | null>(null);

  /** 推送前的确认。列出要推的提交（照 IDEA），而不是只给一个计数 */
  pendingPush = $state<{ branch: string; setUpstream: boolean; commits: string[] } | null>(null);

  /** 远程操作失败时展开的那块。`raw` 是 git 的原话 */
  err = $state<(RemoteErr & { hint: string }) | null>(null);

  hooks: {
    /** 把 Git 那组懒加载的组件先拉起来（确认条在里面） */
    warmUi?: () => void;
  } = {};

  /**
   * 把 RemoteErr 变成一句能照着做的话。
   *
   * git 的原话不能直接给用户看 ——「terminal prompts disabled」会让人以为是
   * 我们的开关设错了，而真正该做的事是去认证一次。
   * **但原话要留着能展开**：转译错了的时候人得有办法绕过我们。
   */
  async #toHint(e: RemoteErr): Promise<string> {
    if (e.kind === "auth-https") {
      return `在终端里跑一次，输一遍账号密码，之后就一直有效：\n  git -C ${project.root} fetch`;
    }
    if (e.kind === "auth-ssh") {
      return "把私钥加进 ssh-agent：\n  ssh-add --apple-use-keychain ~/.ssh/id_ed25519";
    }
    if (e.kind === "rejected") return "远程上有你本地还没有的提交。先拉下来，再推。";
    return "";
  }

  async #showErr(e: RemoteErr) {
    if (e.kind === "cancelled") return; // 用户自己取消的，不是错误
    this.err = { ...e, hint: await this.#toHint(e) };
  }

  /** 进度通道。每次操作新建一个 —— Channel 是一次性的 */
  #progressChannel(what: "pull" | "push" | "fetch") {
    const ch = new Channel<RemoteProgress>();
    ch.onmessage = (p) => {
      if (!this.syncing) return;
      this.syncing = { ...this.syncing, what, phase: p.phase, percent: p.percent };
    };
    return ch;
  }

  /**
   * 抓远程。只读，不动工作区 —— 失败了没有任何后果。
   * 拉取的第一步也是它。
   */
  async fetch(what: "pull" | "fetch"): Promise<boolean> {
    if (!git.repo || this.syncing) return false;
    this.hooks.warmUi?.();
    this.err = null;
    const opId = ++this.#nextOpId;
    this.syncing = { what, id: opId, phase: "正在连接…", percent: null };
    try {
      await gitFetch(git.repo, "origin", opId, this.#progressChannel(what));
      await git.refresh();
      return true;
    } catch (e) {
      await this.#showErr(e as RemoteErr);
      return false;
    } finally {
      this.syncing = null;
    }
  }

  /**
   * 拉取 = fetch + 本地合并两步，**不是 `git pull`**。
   *
   * 复合命令失败时分不清是网络断了还是合并冲突了（退出码都非零）。
   * 拆开之后：第一步失败就是纯网络/凭据，第二步失败就是冲突，
   * 而冲突有 MergeView 接着。
   *
   * 默认只允许快进 —— 永远不会「拉一下，凭空多出一个合并提交」。
   * 快进不了就停下来问（或者用上次记住的选择）。
   *
   * 拉一次。**返回值是「要用这个模式再拉一次」**，null = 不用再拉。
   *
   * # 为什么重试要走返回值，不能在 catch 里直接递归
   *
   * 原来那句是 `void doPull(lastMergeMode); return;` —— 加上 issue #23 的
   * 守卫之后它会**把自己挡下来**：`doPull` 里 `claim` 之前没有 `await`，
   * 递归那次同步就跑到守卫上，而这时外层的 `finally` 还没执行、锁还在自己手里。
   * 表现会是「分岔之后自动重试静默失灵，只弹一句『正在合并上游，请等它做完』」。
   *
   * 也不能改成「先 `git.release()` 再递归」：那样外层的 `finally` 会**再放一次**，
   * 而那时锁已经属于内层了 —— 等于凭空把锁开了。
   *
   * 把重试挪到 `finally` 之后就没有这两个问题。重试只可能发生一次
   * （第二次带着 `mode`，走不进那个分支）。
   */
  async #pullOnce(
    upstream: string,
    mode?: "merge" | "rebase",
  ): Promise<"merge" | "rebase" | null> {
    // **整个 pull 都占着锁，包括前面那次 fetch。** fetch 自己不动 index，
    // 但它后面紧跟着的合并动。只圈住合并的话，fetch 期间开始的一次提交
    // 会让合并被挡下来 —— 那时 pull 已经拉下来一半，停在一个说不清的状态上
    if (!git.claim("合并上游")) return null;
    try {
      if (!mode && !(await this.fetch("pull"))) return null;
      await gitMergeUpstream(git.repo!, upstream, mode ?? "ff-only");
      await worktree.changed();
      await git.refresh();
      notify.ok(mode === "rebase" ? "已变基到上游" : "已合并上游");
      return null;
    } catch (e) {
      const err = e as RemoteErr;
      // 快进不了 = 分岔了，要先做决定。这不是错误，是个岔路口
      if (err.kind === "conflict" && !mode) {
        // 记过一次就直接用，不再问（IDEA 的「记住这次选择」）
        if (this.lastMergeMode) return this.lastMergeMode;
        this.pendingDiverge = { upstream };
        return null;
      }
      await this.#showErr(err);
      // 合并冲突之后工作区变了，得把界面对上
      await worktree.changed();
      await git.refresh();
      return null;
    } finally {
      git.release();
    }
  }

  async pull(mode?: "merge" | "rebase") {
    if (!git.repo) return;
    // 确认条在 Git 那一组里（懒的）。从菜单直接拉时它可能还没到位 ——
    // 不先拉一下的话，分岔决策条不会出现，看着像「点了没反应」
    this.hooks.warmUi?.();
    const upstream = git.status?.upstream;
    if (!upstream) {
      notify.fail("这个分支没有上游，先推送一次", 3000);
      return;
    }
    const retry = await this.#pullOnce(upstream, mode);
    if (retry) await this.#pullOnce(upstream, retry);
  }

  /** 推送。先把要推的提交列出来让人看清 —— 照 IDEA 的推送对话框 */
  async askPush() {
    if (!git.repo || !git.status) return;
    this.hooks.warmUi?.();
    const branch = git.status.branch;
    if (!branch) {
      notify.fail("游离状态下不能推送", 2600);
      return;
    }
    const setUpstream = !git.status.upstream;
    let commits: string[] = [];
    try {
      commits = await gitOutgoing(git.repo, git.status.upstream ?? "", branch);
    } catch {
      commits = []; // 列不出来不该挡住推送，只是少了一份确认信息
    }
    this.pendingPush = { branch, setUpstream, commits };
  }

  async push() {
    const req = this.pendingPush;
    this.pendingPush = null;
    if (!git.repo || !req || this.syncing) return;
    /*
     * **push 也占锁**（issue #23），虽然它自己不动 index。
     *
     * 理由不是锁冲突，是**因果**：正在跑的那次提交会改变要推的内容。
     * 钩子跑到一半时点推送，推上去的是钩子跑完之前的 HEAD ——
     * 命令都成功，结果却不是人想要的那个，而且事后完全看不出来。
     * 这种「都没报错但答案是错的」比一句 `index.lock` 报错难查得多。
     */
    if (!git.claim("推送")) return;
    this.err = null;
    const opId = ++this.#nextOpId;
    this.syncing = { what: "push", id: opId, phase: "正在连接…", percent: null };
    try {
      await gitPush(git.repo, "origin", req.branch, req.setUpstream, opId, this.#progressChannel("push"));
      await git.refresh();
      notify.ok("已推送");
    } catch (e) {
      await this.#showErr(e as RemoteErr);
    } finally {
      this.syncing = null;
      git.release();
    }
  }

  /**
   * 取消。**只对 fetch 开放。**
   *
   * push 中途 kill 掉的是本地这一端，而远程可能已经收完了 ——
   * 一个点了之后状态不确定的取消按钮，比没有按钮更糟。
   */
  cancel() {
    if (this.syncing) void gitCancel(this.syncing.id);
  }
}

export const remote = new Remote();
