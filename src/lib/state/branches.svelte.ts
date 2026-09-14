import {
  gitSwitch,
  gitWorktreeAdd,
  gitWorktreeRemove,
  type SwitchErr,
  type GitWorktree,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { git } from "./git.svelte";
import { tabflow } from "./tabflow.svelte";
import { worktree } from "./worktree.svelte";

/**
 * 分支与工作树的动作：切分支（含被本地改动挡住那一问）、开 / 建 / 移除工作树。
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

  switchTo(name: string, create = false) {
    notify.closeBanner();
    this.pendingCheckout = null;
    // base 要在切之前抓 —— 切完 git.status.branch 就是新的那个了
    const base = git.status?.branch ?? "";
    void git.run(create ? "新建分支失败" : "切分支失败", async () => {
      try {
        await gitSwitch(git.repo!, name, create);
      } catch (e) {
        const err = e as SwitchErr;
        if (err?.kind === "local-changes" && err.files?.length) {
          this.pendingCheckout = { name, create, files: err.files };
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
      notify.ok(create ? `已从 ${base} 新建并切到 ${now}` : `已切到 ${now}${up}`, 2800);
      await worktree.changed();
    }, create ? "新建分支" : "切分支");
  }

  /** 丢掉挡路的那几个改动，然后把刚才那次切换重放一遍 */
  async discardThenCheckout() {
    const p = this.pendingCheckout;
    if (!p || !git.repo) return;
    this.pendingCheckout = null;
    const es = (git.status?.entries ?? []).filter((e) => p.files.includes(e.path));
    await git.discard(es);
    this.switchTo(p.name, p.create);
  }

  /**
   * 打开一个工作树 = **把项目根换过去**。
   *
   * `openPath` 对目录只做 `root = path`，**打开的标签一个都不动** ——
   * 于是文件树和 Git 栏切到了新工作树，而标签还指着旧的那份。
   * 这不是 bug（开着别处的文件是合法的），但一声不吭就变了半个界面，
   * 人会以为「怎么点了没反应」。做完说一句。
   */
  async openWorktree(path: string) {
    await tabflow.openPath(path);
    const name = path.slice(path.lastIndexOf("/") + 1) || path;
    notify.ok(`项目根已切到 ${name}（打开的标签没有动）`, 3200);
  }

  newWorktree(dir: string, branch: string) {
    void git.run("新建工作树失败", async () => {
      // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
      const path = await gitWorktreeAdd(git.repo!, dir, branch);
      notify.ok(`工作树已建在 ${path}`, 3600);
      await tabflow.openPath(path);
    }, "新建工作树");
  }

  removeWorktree(w: GitWorktree, force: boolean) {
    this.pendingWtRemove = null;
    void git.run("移除工作树失败", async () => {
      await gitWorktreeRemove(git.repo!, w.path, force);
      notify.ok(`已移除工作树 ${w.path}`);
      await worktree.changed();
    }, "移除工作树");
  }
}

export const branches = new Branches();
