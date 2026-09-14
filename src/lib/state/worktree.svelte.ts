import { openLog, closeLog } from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { tabflow } from "./tabflow.svelte";

/**
 * 盘上的东西被外部改了，开着的标签和文件树要跟上。
 *
 * 从 App.svelte 搬出来（issue #9 第 4d 步），逻辑一个字不改。
 * `afterFsChange`（文件树里改完盘之后的收尾）还在 App —— 它要刷 git 状态，
 * 那是第 5 步的事。
 */
class Worktree {
  /** 文件树刷新计数，由 `changed()` 推进。文件树、`.gitignore` 缓存、跳转索引都盯着它 */
  treeTick = $state(0);

  /**
   * 盘上的东西被外部改了，两件事要一起做。
   *
   * 切分支、丢弃改动、移除工作树、以及用户切出去在终端里敲完命令切回来，
   * 都属于这一类：**内容和目录结构都可能变了**。两件事必须一起做，
   * 少做哪一件都会留下一个说谎的界面：
   *
   * - 只重读文件内容 → 树上还挂着已经不存在的文件
   * - 只重列目录 → 打开的标签还显示着旧分支的内容
   *
   * 早先这两行是在四个地方各写一遍的，其中一处只写了后半句。
   * 给它一个名字，就不会再漏。
   */
  async changed() {
    await docs.checkExternalChanges();
    this.treeTick++;
  }

  /**
   * 在文件树里改完名，打开着的标签要跟着走。
   *
   * 少了这一步的表现是：标签还挂着旧名字，按 ⌘S 报「文件不在盘上了」——
   * 而名字是人刚刚亲手改的，最不会去怀疑的就是这件事。
   *
   * `repo` 是仓库根：差异 / 冲突标签的 `rel` 相对它算。等 git 那套有了自己的
   * store（第 5 步）就不用传了。
   */
  async renameOpenTabs(from: string, to: string, isDir: boolean, repo: string | null) {
    const moved: number[] = [];
    for (const t of tabs.under(from, isDir)) {
      const np = to + t.path.slice(from.length);
      // 位置记忆的 key 也是路径，一起搬 —— 不搬的话切回这个文件会跳回第一行
      const pos = docs.posByPath.get(t.path);
      if (pos !== undefined) {
        docs.posByPath.delete(t.path);
        docs.posByPath.set(np, pos);
      }
      t.path = np;
      t.name = np.slice(np.lastIndexOf("/") + 1);
      // 差异/冲突标签的 rel 是相对仓库根的，跟着重算 ——
      // 不算的话下一次刷新会拿一条已经不存在的路径去问 git
      if (t.rel && repo && np.startsWith(`${repo}/`)) t.rel = np.slice(repo.length + 1);
      moved.push(t.id);
    }

    /*
     * 日志模式还要把引擎句柄换掉。
     *
     * 引擎记着的是**打开时那条路径**（`LogFile { path, .. }`），而
     * `refresh()` 走 `std::fs::metadata(&self.path)` —— 改完名那条路径没了。
     * 症状很隐蔽：已经映射好的内容照样翻得动（mmap 还在），只有 tail
     * 和「文件长了」的检测一直报刷新失败，而标签看上去一切正常。
     *
     * 重开一个句柄再把旧的关掉，顺序不能反：先关的话中间那一下
     * 标签会短暂地没有句柄，而渲染随时可能发生。
     */
    for (const id of moved) {
      const before = tabs.byId(id);
      if (!before || before.mode !== "log" || before.handle === undefined) continue;
      const stale = before.handle;
      try {
        const h = (await openLog(before.path)).handle;
        // await 回来必须按 id 重新取一次 —— 手上那个引用可能已经不是
        // 响应式的那一份了（AGENTS.md 里那条 $state 数组的坑）
        const now = tabs.byId(id);
        if (now) now.handle = h;
        void closeLog(stale);
      } catch (e) {
        notify.fail(`${before.name} 改名后重开日志失败，tail 会停：${String(e)}`);
      }
    }
  }

  /** 进废纸篓的东西，开着的标签一并关掉（确认框已经说过会关几个未保存的） */
  closeTabsUnder(p: string, isDir: boolean) {
    for (const t of tabs.under(p, isDir)) tabflow.doClose(t);
  }
}

export const worktree = new Worktree();
