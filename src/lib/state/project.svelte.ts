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
   *
   * **这只是一份副本**（多窗口第 4 步）：名单在 Rust（`windows.rs`），窗口开了一个项目时
   * Rust 自己记，变了广播 `recent-changed`，App 收到就整份换掉。前端不再自己往里加 ——
   * 原来每个窗口各记各的、各写各的，后写的盖掉先写的。
   */
  recent = $state<string[]>([]);

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

  /**
   * 数据库控制台（docs/DATABASE.md DB-2）是哪个连接的：应用数据目录（草稿目录的上一层）里 `db/console/<连接 id>/x.sql`，不是就 null。
   * 只认应用数据目录底下的 —— 项目里碰巧同样层级的 .sql 不是控制台
   */
  dbConsole(path: string): string | null {
    const r = this.scratchRoot;
    const base = r && `${r.slice(0, r.lastIndexOf("/"))}/db/console/`;
    return (base && path.startsWith(base) && /^(c[0-9a-f]+)\/[^/]+\.sql$/.exec(path.slice(base.length))?.[1]) || null;
  }

  /** 自己存、不用 ⌘S 的：草稿，和数据库控制台（同 IDEA 的控制台。它不是草稿：标签名是连接名、不收进草稿本、空了也不删） */
  autosaves(path: string): boolean {
    return this.isScratch(path) || this.dbConsole(path) !== null;
  }
}

export const project = new Project();
