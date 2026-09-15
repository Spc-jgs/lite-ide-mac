import type { RemoteErr } from "../ipc/commands";

/** 动作在 `remote-ops.ts`，按需加载 —— 理由见 `git-ops.ts` 的文件头 */
const ops = () => import("./remote-ops");

/**
 * 拉取与推送的**状态**：进度、分岔决策、推送确认、失败提示。动作在 `remote-ops.ts`，
 * 按需加载（issue #32）。
 *
 * 从 App.svelte 搬出来（issue #9 第 5b 步），逻辑一个字不改。
 *
 * 一个往外的钩子 `warmUi`：分岔决策条 / 推送确认条 / 失败提示都长在 Git 那组
 * 懒加载的组件里（`RemoteBars`），从菜单直接拉时它可能还没到位 —— 不先拉一下，
 * 决策条不会出现，看着像「点了没反应」。那组 lazy 在 App 里，App 装这个钩子。
 */
class Remote {
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

  // ── 动作：全部转发到 remote-ops.ts（按需加载）。签名和文档见那边 ──
  async fetch(what: "pull" | "fetch"): Promise<boolean> {
    return (await ops()).fetch(what);
  }
  async pull(mode?: "merge" | "rebase") {
    return (await ops()).pull(mode);
  }
  async askPush() {
    return (await ops()).askPush();
  }
  async push() {
    return (await ops()).push();
  }
  /** 取消不用等模块：正在跑的操作必然已经把模块拉进来了 */
  cancel() {
    void ops().then((m) => m.cancel());
  }
}

export const remote = new Remote();
