import { RECENT_MAX } from "./session";

/**
 * 项目根、最近打开过的项目、草稿目录。
 *
 * `root` 是这个应用里被读得最多的一个值（文件树、git、搜索、面包屑、快照都要），
 * 从 App.svelte 搬出来（issue #9 第 4b 步的前置）让打开文件那条流程能搬 ——
 * 它对目录的处理就是 `root = path`。写法同 `layout` / `tabs`。
 */
class Project {
  root = $state<string | null>(null);

  /**
   * 最近打开过的项目根，最新的排最前。
   *
   * 记的是**项目根不是文件**：会话恢复本来就以 root 为单位，
   * 开回一个项目上次的标签会跟着回来，比记住散落的文件有用得多。
   */
  recent = $state<string[]>([]);

  /** 把一个目录顶到最近列表最前面。已经在里面就是往前挪，不是加一条 */
  remember(dir: string) {
    this.recent = [dir, ...this.recent.filter((r) => r !== dir)].slice(0, RECENT_MAX);
  }

  /**
   * 草稿目录的绝对路径（`~/Library/Application Support/com.liteide.app/scratches`）。
   *
   * 启动时拿一次就不再变。**不 await 在启动路径上** —— 它只服务两件事
   * （判断一个标签是不是草稿、菜单里打开草稿目录），两件都发生在人动手之后，
   * 而这一次 IPC 是毫秒级的，早就回来了。为它把首屏往后推一拍不值。
   */
  scratchRoot = $state<string | null>(null);

  /**
   * 这个路径是不是一份草稿。
   *
   * `scratchRoot` 还没到位时一律算「不是」：它唯一的用处是决定
   * 「关掉时要不要把这个空文件丢掉」，而**猜错的方向必须是留下**——
   * 少丢一个空文件只是噪音，多丢一个就是删了不该删的东西。
   */
  isScratch(path: string): boolean {
    return this.scratchRoot !== null && path.startsWith(`${this.scratchRoot}/`);
  }
}

export const project = new Project();
