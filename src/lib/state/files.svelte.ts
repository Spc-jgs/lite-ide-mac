import { listProjectFiles } from "../ipc/commands";
import { RECENT_FILES_MAX } from "./session";

/**
 * 项目的文件索引（⌘P / ⌘Click 跳转共用的那一份）+ 最近打开的文件（⌘E）。
 *
 * # 为什么是一个 store 而不是各拉各的
 *
 * 原来是**两份**：`QuickSearch` 自己拉一份、只在换项目时重建；`Content` 为 ⌘Click 再拉一份、
 * 跟着 `worktree.treeTick` 刷。于是终端里 `touch` 出来的新文件 ⌘Click 跳得过去、⌘P 却找不到，
 * 要换一次项目才行 —— 同一个问题两个答案。ARCHITECTURE 里「文件索引复用 ⌘P 那一份」
 * 那句写的是愿望不是现状。现在只有这一份，App 里一条 effect 跟着 `root` 和 `treeTick` 刷。
 *
 * # `truncated`
 *
 * Rust 侧到 5 万个文件就停。原来这件事界面上看不见：超过 5 万文件的仓库里，⌘Click
 * 没下划线看着像「不是项目里的文件」，其实是「索引没看到那儿」。现在两处都说出来。
 *
 * # 刷新的序号
 *
 * 切项目时上一个项目那趟还没回来，回来了不能盖掉新项目的表 —— 同 `git.refresh` 的 `seq`。
 */
class ProjectFiles {
  /** 相对项目根的路径，Rust 侧排过序 */
  list = $state<string[]>([]);
  truncated = $state(false);
  /** 最近打开的文件（绝对路径），最新的在前。进会话快照，重启还在 */
  recent = $state<string[]>([]);
  #seq = 0;

  async refresh(root: string | null) {
    const seq = ++this.#seq;
    if (!root) {
      this.list = [];
      this.truncated = false;
      return;
    }
    try {
      const r = await listProjectFiles(root);
      if (seq !== this.#seq) return;
      this.list = r.files;
      this.truncated = r.truncated;
    } catch {
      // 索引拉不到不该打扰人：⌘P 退成只搜草稿和内容，跳转的第二层歇菜，第一层照常
      if (seq !== this.#seq) return;
      this.list = [];
      this.truncated = false;
    }
  }

  /** 打开了一个文件。会话恢复期不叫 —— 那不是「刚打开」，是把上次的顺序原样摆回来 */
  touch(path: string) {
    this.recent = [path, ...this.recent.filter((p) => p !== path)].slice(0, RECENT_FILES_MAX);
  }

  /** 改名 / 搬家：原位换成新路径，不改顺序 —— 改名不是「刚打开」 */
  rename(from: string, to: string) {
    if (this.recent.includes(from)) this.recent = this.recent.map((p) => (p === from ? to : p));
  }

  /** 文件没了（废纸篓）：从最近里摘掉，不然 ⌘E 里点一下报一句「不在盘上了」 */
  forget(path: string) {
    if (this.recent.includes(path)) this.recent = this.recent.filter((p) => p !== path);
  }
}

export const files = new ProjectFiles();
