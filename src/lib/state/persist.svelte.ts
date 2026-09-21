import { probePath } from "../ipc/commands";
import * as session from "./session";
import { stashed } from "./doc";
import { notify } from "./notify.svelte";
import { layout } from "./layout.svelte";
import { project } from "./project.svelte";
import { files } from "./files.svelte";
import { tabs } from "./tabs.svelte";
import { docs, type ViewPos } from "./docs.svelte";
import { tabflow } from "./tabflow.svelte";

/**
 * 会话快照的读写：启动时把上次的现场摆回来，之后有变化就防抖落盘。
 *
 * 格式（parse / serialize / VERSION）在 `session.ts`，纯函数有测试；这里是**时机**：
 * 什么时候读、什么时候写、写不下怎么退。从 App.svelte 搬出来（issue #9 第 6b 步），
 * 逻辑一个字不改。四条 effect（定期落盘、响应式落盘、退出前补写、兑现恢复位置）
 * 还在 App —— `$effect` 进不了 store，它们各自只剩几行调这里的方法。
 */

/**
 * 上次退出时的现场。**模块初始化时同步读一次**，不放进 effect ——
 * 布局要用它做 `$state` 的初值，晚一拍读就会看见侧边栏从 240 跳到
 * 上次的宽度，那一下闪比不恢复还难受。
 *
 * 读不出来（第一次跑、隐私模式、数据被清、存的是坏数据）就是 null，
 * 一切照默认走。`session.parse` 保证不抛。
 */
export const saved: session.Session | null = (() => {
  try {
    return session.parse(localStorage.getItem(session.KEY));
  } catch {
    return null;
  }
})();

// 布局和最近列表在任何 effect 跑之前就灌好 —— 理由见上面
layout.restore(saved?.layout ?? session.DEFAULT_LAYOUT);
project.recent = saved?.recent ?? [];
files.recent = saved?.recentFiles ?? [];

class Persist {
  /**
   * 恢复期间不写。
   *
   * 保存的 effect 在挂载时就会跑一次，而那时 `restore()` 还没开始
   * （它要等 `initialPath()` 这个 IPC 回来）—— 400ms 的防抖一到，
   * 就会拿一个「什么都没打开」的空状态**盖掉上次的快照**。
   * 本次运行看不出问题（`saved` 早在初始化时就读进内存了），
   * 但恢复途中退出的话，上次的现场就真没了。
   */
  restoring = $state(true);

  #saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** 上一次真正写进去的那串。草稿让写变频了，一模一样就别再写一遍 */
  #lastWritten = "";
  /** 已经为「草稿太大存不下」提醒过的文件，一个文件只说一次 */
  #warnedBig = new Set<string>();

  /**
   * 把上次的现场摆回来。
   *
   * 全程「能恢复多少算多少」：项目根没了就不设，文件没了就跳过，
   * 一个都没恢复出来就是一个干净的空界面 —— 都不该报错。
   * 启动流程里任何一句 throw 都等于应用打不开。
   */
  async restore() {
    if (!saved) return;
    if (saved.root) {
      const ok = await probePath(saved.root)
        .then((i) => i.kind === "dir")
        .catch(() => false);
      if (ok) project.root = saved.root;
    }
    await this.#restoreTabs(saved);
  }

  /**
   * 每个项目一份快照（issue #33 ㉓，照 IDEA 每个项目一份 `workspace.xml`）。
   *
   * 键是 `lite-ide.session:<root>`；不带后缀的那份仍是「上次退出时」的现场，
   * 启动时读它。每次落盘两份都写（内容一样，多一次 setItem）；不在最近列表里的
   * 项目那份顺手清掉，localStorage 里最多躺 8 份。
   */
  #keyFor(root: string) {
    return `${session.KEY}:${root}`;
  }

  /**
   * 切项目：旧项目的现场存到它自己那份，关掉它的**干净**标签，再把新项目上次的
   * 标签摆回来。脏标签留着 —— 那是没保存的活，跟着人走到哪儿都不能丢；
   * 它们自己会在关的时候问。
   *
   * VS Code 打开另一个文件夹就是换工作区（标签跟着换）；IDEA 是另开一个窗口。
   * 这里没有多窗口，取 VS Code 那条。
   *
   * 由 `tabflow` 在改 `project.root` 前后各叫一次（钩子），这里不碰 root。
   */
  beforeRootChange(old: string | null) {
    if (!old || this.restoring) return;
    // 旧项目的现场立刻落盘 —— 防抖那 400ms 里 root 就换了，再写就是新项目的了
    this.flush();
  }

  async afterRootChange(next: string) {
    // 草稿不跟项目走（issue #40）：它是贴在桌角的便签，换个项目它还在
    for (const t of [...tabs.list]) if (!t.dirty && !project.isScratch(t.path)) tabflow.doClose(t);
    let mine: session.Session | null = null;
    try {
      mine = session.parse(localStorage.getItem(this.#keyFor(next)));
    } catch {
      mine = null;
    }
    // 别的项目存的快照 root 是别的：只信 root 对得上的那份。
    // 读回来再滤一遍草稿：写的时候已经滤过，但老快照里可能还躺着 —— 理由见 withoutTabs
    if (mine && mine.root === next) {
      await this.#restoreTabs(session.withoutTabs(mine, (p) => project.isScratch(p)));
    }
  }

  async #restoreTabs(saved: session.Session) {
    /*
     * **先记位置，再开文件。** 编辑器和日志视图都在挂载时从 `posByPath` 取
     * 上次的位置（`initialView` / `initialTop`），后写就赶不上第一次挂载。
     * 反过来写过一版，位置恢复整个不生效。
     */
    for (const t of saved.tabs) {
      if (t.line === undefined) continue;
      const pos: ViewPos = { line: t.line };
      if (t.col !== undefined) pos.col = t.col;
      if (t.top !== undefined) pos.top = t.top;
      if (t.toff !== undefined) pos.toff = t.toff;
      docs.posByPath.set(t.path, pos);
    }
    /*
     * 串行开，不并行。
     *
     * 并行看着快，但每个文件都要 probe + 读全文（或 mmap + 探编码），
     * 二十个文件一起冲进 IPC 会把启动的头一秒占满，首屏反而更晚出来。
     * 而且 `openPath` 里 `if (!project.root) project.root = 父目录` 这句依赖顺序。
     *
     * **但串行不等于要一个一个地闪。** `tabflow.restoringTabs` 期间 `openPath`
     * 不碰 `activeId`（见它上面那段），所以内容区一次都不重建；
     * 走到该激活的那个标签时点一次，编辑器**只建一次**，
     * 剩下的标签在它后面继续往标签条里填。
     *
     * 顺序仍是存下来的顺序 —— 把该激活的那个提到最前面能让它更早出来，
     * 但标签条的顺序就跟上次不一样了，那是个更难受的毛病。
     */
    const wantPath = saved.tabs[saved.active]?.path;
    tabflow.restoringTabs = true;
    try {
      for (const t of saved.tabs) {
        await tabflow.openPath(t.path, { quiet: true, preview: t.preview });
        // 钉住的：快照里的顺序本来就是钉住的在前，这里只补标记，不再挪位
        if (t.pinned) {
          const hit = tabs.byPath(t.path);
          if (hit) hit.pinned = true;
        }
        if (t.path === wantPath) {
          const hit = tabs.byPath(t.path);
          // 这一下是整个恢复过程里唯一一次内容区渲染
          if (hit) tabs.show(hit.id);
        }
      }
    } finally {
      // 这里必须 finally：漏掉的话 activeId 就永久失灵，
      // 而 openPath 是会抛的（文件没了、读不动、编码探测失败）
      tabflow.restoringTabs = false;
    }
    /*
     * 兑现草稿。**必须在文件都读进来之后**：判据是「草稿和盘上现在那份一不一样」，
     * 盘上那份要先有。
     *
     * 三种情况，都不需要我们替谁做主（见 session.ts 的长注释）：
     * - 盘上没变 → 原样恢复，dirty 由 `stashed` 按内容算出来
     * - 盘上变了而草稿还不一样 → 就是应用运行中早就有的那个冲突，
     *   摆出「用磁盘上的 / 保留我的」让用户选
     * - 草稿恰好和盘上现在一样 → `stashed` 自己会把它丢掉，也就不脏
     */
    for (const snapTab of saved.tabs) {
      if (snapTab.draft === undefined) continue;
      const tab = tabs.list.find((t) => t.path === snapTab.path && t.mode === "edit");
      if (!tab) continue;
      Object.assign(tab, stashed(tab, snapTab.draft));
      if (!tab.dirty) continue;
      tabs.keep(tab.id);
      const 盘上变了 =
        !snapTab.stamp ||
        !tab.stamp ||
        snapTab.stamp.mtimeMs !== tab.stamp.mtimeMs ||
        snapTab.stamp.size !== tab.stamp.size;
      if (盘上变了) tab.conflict = true;
    }
    /*
     * 兜底。正常路径上 activeId 在上面那个循环里就点过了 ——
     * 这里只服务两种情况：上次激活的那个文件这次不在了（循环里没命中），
     * 或者 `saved.active` 越界。那时退到第一个恢复成功的标签，
     * 总比停在一个空内容区上好。
     */
    if (tabs.activeId === null && tabs.list.length > 0) tabs.show(tabs.list[0].id);
    this.#regroup(saved);
    // 上次开着、这次已经不在的文件：位置记忆也删掉，不然它们
    // 会一直躺在快照里，每次启动都白试一遍
    for (const t of saved.tabs) {
      if (!tabs.list.some((x) => x.path === t.path)) docs.posByPath.delete(t.path);
    }
    tabs.audit("会话恢复");
  }

  /**
   * 把分屏摆回去（issue #35）。**在标签都开完之后做**：`openPath` 把每个标签落在焦点组，
   * 恢复期焦点组一直是 0，所以到这儿全在左组；这里按快照把右组的挑出来、两组各自显示谁点上。
   * 判据和 `session.normalizeGroups` 一样是「能恢复多少算多少」：右组的文件一个都不在了
   * 就是单栏；某组快照里显示的那个不在了，就显示这组第一个恢复出来的。
   */
  #regroup(saved: session.Session) {
    if (!saved.tabs.some((t) => t.group === 1)) return;
    for (const t of saved.tabs) {
      if (t.group !== 1) continue;
      const hit = tabs.byPath(t.path);
      if (hit) hit.group = 1;
    }
    if (tabs.inGroup(1).length === 0 || tabs.inGroup(0).length === 0) {
      for (const t of tabs.list) t.group = 0;
      return;
    }
    const shownIn = (g: 0 | 1) => {
      const want = saved.tabs.find((t) => (t.group ?? 0) === g && t.shown);
      const hit = want ? tabs.byPath(want.path) : null;
      return hit?.group === g ? hit.id : tabs.inGroup(g)[0].id;
    };
    tabs.shown = [shownIn(0), shownIn(1)];
    // 活动标签是它所在组显示的那个 —— 它的组是刚改的，`shown` 要以它为准
    const a = tabs.activeId ?? tabs.shown[0]!;
    tabs.show(a);
  }

  /** 按当前状态拍一张快照 */
  snapshot(): session.Session {
    return {
      root: project.root,
      tabs: tabs.list.map((t) => {
        const pos = docs.viewOf(t);
        const snap: session.TabSnap = { path: t.path };
        if (pos !== undefined) {
          snap.line = pos.line;
          if (pos.col !== undefined) snap.col = pos.col;
          if (pos.top !== undefined) snap.top = pos.top;
          if (pos.toff !== undefined) snap.toff = pos.toff;
        }
        if (t.preview) snap.preview = true;
        if (t.pinned) snap.pinned = true;
        // 分屏（issue #35）：右组打 1，每组正在显示的打 shown。单栏时 shown 就是活动的那个，照写不碍事
        if (t.group === 1) snap.group = 1;
        if (tabs.shown[t.group] === t.id) snap.shown = true;
        /*
         * 有未保存改动就把草稿一起存下来 —— 「没手动保存就退出，改动直接没」
         * 是这个应用最容易咬人的一条，而会话恢复对外说的是「回到上次的现场」。
         *
         * `liveText` 而不是 `t.draft`：当前标签的编辑器还活着，草稿字段
         * 可能停在几步之前（见 doc.ts）。
         * `stamp` 必须一起存，恢复时要靠它判断盘上那份有没有被人动过。
         * 超限的草稿由 `session.serialize` 丢掉，这里不预先筛。
         */
        if (t.mode === "edit" && t.dirty) {
          snap.draft = docs.liveText(t);
          if (t.stamp) snap.stamp = { mtimeMs: t.stamp.mtimeMs, size: t.stamp.size };
        }
        return snap;
      }),
      active: Math.max(0, tabs.list.findIndex((t) => t.id === tabs.activeId)),
      layout: layout.snapshot(),
      recent: [...project.recent],
      recentFiles: [...files.recent],
    };
  }

  write() {
    this.#saveTimer = null;
    if (this.restoring) return;
    const snap = this.snapshot();
    let text: string;
    try {
      text = session.serialize(snap);
    } catch {
      return; // 序列化都失败就彻底放弃，不能让它冒到启动路径上
    }
    if (text === this.#lastWritten) return;
    /*
     * 两份：不带后缀的是「上次退出时」，启动读它，**带草稿**；带 root 的是
     * 这个项目自己的，**不带草稿** —— 草稿跨项目常驻，记进某个项目的快照
     * 会在切回来时把已经关掉的草稿复活（issue #40，见 session.withoutTabs）。
     */
    const projectSnap = session.withoutTabs(snap, (p) => project.isScratch(p));
    const write = (withDrafts: boolean) => {
      localStorage.setItem(session.KEY, withDrafts ? text : session.serialize(snap, false));
      if (snap.root) localStorage.setItem(this.#keyFor(snap.root), session.serialize(projectSnap, withDrafts));
    };
    try {
      write(true);
      this.#lastWritten = text;
    } catch {
      /*
       * 写不下多半是草稿把配额撑爆了。**退一步再存一次**：宁可丢草稿，
       * 也不能连「上次开了哪些文件、光标在哪」一起赔进去 ——
       * 后者是草稿进来之前就有的保证，不该被新功能连累。
       */
      try {
        write(false);
        this.#lastWritten = session.serialize(snap, false);
      } catch {
        /* 隐私模式之类，连基本的都写不下就算了 */
      }
    }
    this.#prune(snap);
  }

  /** 不在最近列表里的项目那份快照清掉 —— 最近列表封顶 8，快照也就最多 8 份 */
  #prune(snap: session.Session) {
    try {
      const keep = new Set(snap.recent.map((r) => this.#keyFor(r)));
      if (snap.root) keep.add(this.#keyFor(snap.root));
      const prefix = `${session.KEY}:`;
      const stale: string[] = [];
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k && k.startsWith(prefix) && !keep.has(k)) stale.push(k);
      }
      for (const k of stale) localStorage.removeItem(k);
    } catch {
      /* 清不掉就留着，下次再说 */
    }
  }

  /**
   * 防抖 400ms 后存。
   *
   * 拖侧边栏、移光标、滚日志都会走这里，每次都写 localStorage 是**同步 IO**，
   * 不防抖的话拖动时能明显感觉到滞手。
   */
  schedule() {
    if (this.#saveTimer) clearTimeout(this.#saveTimer);
    this.#saveTimer = setTimeout(() => this.write(), 400);
  }

  /** 退出前补一次：把防抖里那次直接写掉 */
  flush() {
    if (this.#saveTimer) clearTimeout(this.#saveTimer);
    this.write();
  }

  /**
   * 有未保存改动时定期落一次盘（App 里 4 秒一次的 interval 调它）。
   *
   * 响应式那条 effect 订阅的是布局、标签、项目根 —— **打字不动其中任何一个**，
   * 所以光靠它，「改了半天一直没切标签也没退出」这个最该被记住的状态一次都不会存。
   * 退出前的 pagehide 补写能兜住正常退出，但兜不住崩溃（Rust 侧是 panic = abort，
   * 一个 panic 就是进程当场死，没有 pagehide）。
   *
   * 只在真有脏标签时才动；`write` 里还有一道「和上次一模一样就不写」。
   */
  tickDrafts() {
    const dirty = tabs.list.filter((t) => t.mode === "edit" && t.dirty);
    if (dirty.length === 0) return;
    // 存不下的那种要当面说 —— 不说的话用户以为自己被记住了
    for (const t of dirty) {
      if (this.#warnedBig.has(t.path)) continue;
      if (docs.liveText(t).length <= session.MAX_DRAFT_CHARS) continue;
      this.#warnedBig.add(t.path);
      notify.fail(`${t.name} 太大，未保存的改动不会被记住 —— 请 ⌘S 保存`, 6000);
    }
    this.schedule();
  }
}

export const persist = new Persist();
