import type { GitWorktree } from "../ipc/commands";

/** 动作在 `branches-ops.ts`，按需加载 —— 理由见 `git-ops.ts` 的文件头 */
const ops = () => import("./branches-ops");

/**
 * 分支与工作树的**状态**：被挡住的那次切换、待确认移除的工作树。动作在
 * `branches-ops.ts`，按需加载（issue #32）。
 *
 * 从 App.svelte 搬出来（issue #9 第 5b 步），逻辑一个字不改。
 * 分支浮层本身（开合、锚点）还在 App —— 锚点是标题栏那个挂件的 DOM 元素。
 */
class Branches {
  /**
   * 被本地改动挡住的那次切换。**不是错误，是「你得先决定怎么办」。**
   *
   * git 的原话是 "Please commit your changes or stash them before you switch
   * branches" —— 而提交和丢弃这两条路界面上都有，把英文原话贴给用户等于
   * 让他自己去开终端。`gitsvc::Error::LocalChanges` 把挡路的文件切出来了，
   * 这里据此给按钮。
   */
  pendingCheckout = $state<{ name: string; create: boolean; files: string[] } | null>(null);

  /** 待确认移除的工作树 —— 会删目录，必须过用户这一关 */
  pendingWtRemove = $state<GitWorktree | null>(null);

  /**
   * 待确认删除的分支（M9）。两步：先问一次（不可逆），`-d` 被「还有没合并的提交」
   * 拦下来再问第二次（`notMerged`，给「仍然删除」= `-D`）。
   */
  pendingBranchDelete = $state<{ name: string; notMerged: boolean } | null>(null);

  // ── 动作：全部转发到 branches-ops.ts（按需加载）。签名和文档见那边 ──
  switchTo(name: string, create = false, from = "") {
    void ops().then((m) => m.switchTo(name, create, from));
  }
  mergeInto(ref: string) {
    void ops().then((m) => m.mergeInto(ref));
  }
  renameBranch(old: string, next: string) {
    void ops().then((m) => m.renameBranch(old, next));
  }
  deleteBranch(name: string, force = false) {
    void ops().then((m) => m.deleteBranch(name, force));
  }
  async stashThenCheckout() {
    return (await ops()).stashThenCheckout();
  }
  async discardThenCheckout() {
    return (await ops()).discardThenCheckout();
  }
  async openWorktree(path: string) {
    return (await ops()).openWorktree(path);
  }
  newWorktree(dir: string, branch: string) {
    void ops().then((m) => m.newWorktree(dir, branch));
  }
  removeWorktree(w: GitWorktree, force: boolean) {
    void ops().then((m) => m.removeWorktree(w, force));
  }
}

export const branches = new Branches();
