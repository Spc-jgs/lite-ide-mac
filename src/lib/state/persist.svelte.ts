import { adoptRecent, claimEmptySession, probePath } from "../ipc/commands";
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
 * 升级前那份全局快照（`lite-ide.session`，不带后缀）。多窗口第 4 步起不再写它，
 * 只在升级后第一次启动时读、迁成它那个项目自己的一份（[`Persist.migrate`]）。
 * 读不出来就是 null，`session.parse` 保证不抛。调用时现读，不在模块初始化时存一份 ——
 * 那样状态测试在 import 之后放进去的旧快照就读不到了。
 */
function readLegacy(): session.Session | null {
  try {
    return session.parse(localStorage.getItem(session.KEY));
  } catch {
    return null;
  }
}

/*
 * 布局在任何 effect 跑之前就灌好：要用它做 `$state` 的初值，晚一拍读就会看见侧边栏
 * 从 240 跳到上次的宽度，那一下闪比不恢复还难受。
 *
 * 这时还不知道这个窗口是哪个项目（要等 `initial_paths`），所以先用「最近一次任何窗口
 * 用的布局」铺首屏，恢复到这个窗口自己那份快照时再换（session.LAYOUT_KEY 的注释）。
 * 「最近打开」不在这儿了：名单在 Rust，App 启动时取一次。
 */
layout.restore(
  (() => {
    try {
      return session.parseLayout(localStorage.getItem(session.LAYOUT_KEY));
    } catch {
      return null;
    }
  })() ??
    readLegacy()?.layout ??
    session.DEFAULT_LAYOUT,
);

class Persist {
  /**
   * 恢复期间不写。
   *
   * 保存的 effect 在挂载时就会跑一次，而那时恢复还没开始
   * （它要等 `initialPaths()` 这个 IPC 回来）—— 400ms 的防抖一到，
   * 就会拿一个「什么都没打开」的空状态**盖掉上次的快照**。
   * 恢复途中退出的话，上次的现场就真没了。清理（`prune`）同理：
   * 迁移、恢复都还没做完时，名单和快照都不全。
   */
  restoring = $state(true);

  #saveTimer: ReturnType<typeof setTimeout> | null = null;
  /** 上一次真正写进去的那串。草稿让写变频了，一模一样就别再写一遍 */
  #lastWritten = "";
  /** 已经为「草稿太大存不下」提醒过的文件，一个文件只说一次 */
  #warnedBig = new Set<string>();

  /**
   * 「没有项目的那份快照」（`lite-ide.session:`）这会儿归不归这个窗口。
   *
   * 那份快照只有一个键。两个没有项目的窗口（上次退出时开着两个、⌘N 开的空窗口、在一个窗口里
   * 关闭项目）都去恢复它，同一批标签和草稿就同时开在两个窗口里，之后两边轮流写、谁后写谁算数 ——
   * 代码审查查出来的。所以先问 Rust 要（`claimEmptySession`），要到了才恢复、才写；
   * 要不到的那个窗口这一份不记，它开着的草稿文件本身都在盘上。
   * 有了项目就不再归它（`afterRootChange` 清掉），再变回没有项目时重新要。
   */
  #ownsEmpty = false;
  #claiming: Promise<boolean> | null = null;

  #claimEmpty(): Promise<boolean> {
    if (this.#ownsEmpty) return Promise.resolve(true);
    this.#claiming ??= claimEmptySession()
      .catch(() => false)
      .then((ok) => {
        this.#claiming = null;
        // 问的这一会儿里有了项目：那份不归它了
        this.#ownsEmpty = ok && project.root === null;
        return this.#ownsEmpty;
      });
    return this.#claiming;
  }

  /**
   * 升级后第一次启动：把旧的全局快照迁成它那个项目自己的一份，「最近打开」交给 Rust，
   * 然后删掉旧键。返回这个窗口该开的路径 —— 没人指名时就是升级前开着的那个项目，
   * 不然升级完第一次打开，上次的现场就没了。
   *
   * 旧快照比同名项目自己那份更全（草稿标签原来只存在全局那份里，项目那份会滤掉），
   * 所以覆盖过去。写不下（配额）就退一步不带草稿；连那也写不下，**旧键不删**，下次再试。
   */
  async migrate(paths: string[]): Promise<string[]> {
    const old = readLegacy();
    if (!old) return paths;
    const key = session.keyFor(old.root);
    try {
      try {
        localStorage.setItem(key, session.serialize(old));
      } catch {
        localStorage.setItem(key, session.serialize(old, false));
      }
      localStorage.removeItem(session.KEY);
    } catch {
      /* 写不下就留着旧键，下次启动再迁 */
    }
    await adoptRecent(old.recent).catch(() => null);
    if (paths.length > 0 || !old.root) return paths;
    // 升级前开着的项目已经不在了（删了、挪了、是个临时目录）：不去开它，不然升级完第一次打开
    // 就是一句「打不开」。它那份快照照样迁过去了，目录回来了还能开回来
    const alive = await probePath(old.root).then((i) => i.kind === "dir").catch(() => false);
    return alive ? [old.root] : paths;
  }

  /**
   * 没有项目的窗口：恢复它自己那份（`lite-ide.session:`，草稿、随手开的文件）。
   * 有项目的窗口不走这里 —— 它的项目根一设上，`afterRootChange` 就把那个项目的快照摆回来。
   *
   * 全程「能恢复多少算多少」：文件没了就跳过，一个都没恢复出来就是一个干净的空界面 ——
   * 都不该报错。启动流程里任何一句 throw 都等于应用打不开。
   */
  async restoreEmpty() {
    if (!(await this.#claimEmpty())) return;
    await this.#restoreSnapshot(this.#read(null));
  }

  #read(root: string | null): session.Session | null {
    try {
      const s = session.parse(localStorage.getItem(session.keyFor(root)));
      // 只信 root 对得上的那份：手改过、或者别的版本写坏的，当没有
      return s && s.root === root ? s : null;
    } catch {
      return null;
    }
  }

  /** 一个窗口的快照摆回来：布局、⌘E 的最近文件、标签 */
  async #restoreSnapshot(s: session.Session | null) {
    if (!s) return;
    layout.restore(s.layout);
    files.recent = s.recentFiles ?? [];
    await this.#restoreTabs(s);
  }

  /**
   * 切项目：旧项目的现场存到它自己那份，关掉它的**干净**标签，再把新项目上次的
   * 标签摆回来。脏标签留着 —— 那是没保存的活，跟着人走到哪儿都不能丢；
   * 它们自己会在关的时候问。
   *
   * 多窗口第 4 步起，**已经有项目的窗口不再就地换项目**（`tabflow.openPath` 交给 Rust 开新窗口，
   * 照 IDEA）。还会走到这儿的只有两种：没有项目的窗口第一次得到项目（启动、或者在空窗口里 ⌘O），
   * 和「关闭项目」（root → null，见 tabflow-ops）。前一种里「关掉干净标签」照样有意义：
   * 空窗口里随手开着的文件和这个项目无关 —— 设计稿原来说这半边成了死代码，那是错的。
   *
   * 由 `tabflow` 在改 `project.root` 前后各叫一次（钩子），这里不碰 root。
   */
  beforeRootChange(old: string | null) {
    if (!old || this.restoring) return;
    // 旧项目的现场立刻落盘 —— 防抖那 400ms 里 root 就换了，再写就是新项目的了
    this.flush();
  }

  async afterRootChange(next: string) {
    this.#ownsEmpty = false;
    // 草稿不跟项目走（issue #40）：它是贴在桌角的便签，换个项目它还在
    for (const t of [...tabs.list]) if (!t.dirty && !project.isScratch(t.path)) tabflow.doClose(t);
    /*
     * 草稿标签原来要从项目快照里滤掉（原来的 `session.withoutTabs`，第 4 步删了）：单窗口时草稿「跟着人走」，存在全局那份里，
     * 记进项目快照会在切回来时把已经关掉的草稿复活。多窗口之后每个窗口只写它自己那份，
     * 草稿标签就是这个窗口的一部分，原样存、原样回来。已经开着的同一份草稿 `openPath` 会认出来。
     */
    await this.#restoreSnapshot(this.#read(next));
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
      // 「最近打开」在 Rust（多窗口第 4 步），快照里不再存
      recent: [],
      // ⌘E 的最近文件跟着窗口走（也就是跟着项目，同 IDEA）
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
    if (snap.root === null && !this.#ownsEmpty) {
      /*
       * 没有项目、那份快照还没归它：先去要，要到了再写一次。要不到就不写 ——
       * 另一个没有项目的窗口占着它（`#ownsEmpty` 的注释）。记下这一串，内容没变就不再去问
       */
      this.#lastWritten = text;
      void this.#claimEmpty().then((ok) => {
        if (!ok) return;
        this.#lastWritten = "";
        this.schedule();
      });
      return;
    }
    /*
     * 写这个窗口自己那份（按项目根，没有项目是 `lite-ide.session:`），连草稿标签一起。
     * 原来写两份：不带后缀的全局「上次退出时」+ 项目那份 —— 全局那份在多窗口下是
     * 后写的盖掉先写的（多窗口第 4 步，见 session.KEY 的注释）。
     * 布局另外写一份给下一个窗口铺首屏用（session.LAYOUT_KEY）。
     */
    const key = session.keyFor(snap.root);
    const write = (withDrafts: boolean) => {
      localStorage.setItem(key, withDrafts ? text : session.serialize(snap, false));
      localStorage.setItem(session.LAYOUT_KEY, JSON.stringify(snap.layout));
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
  }

  /**
   * 清掉不再需要的项目快照（最近列表封顶 8 个，快照也就大致这么多份）。
   *
   * 原来每次落盘都清、按的是**这个窗口内存里**的最近列表 —— 多窗口时 A 的列表里没有 B 刚开的
   * 项目，A 一落盘就把 B 的快照当成「挤出去的」删了。现在名单在 Rust，「最近打开」变了的时候
   * （`recent-changed`）由 App 带着 Rust 给的 `keep`（最近打开 + 开着的窗口 + 点 Dock 还会开回来的）叫这里一次。
   * 删哪些的判据全在 `session.staleKeys`（带草稿的、keep 为空时一律不删）。
   */
  prune(keep: string[]) {
    // 名单是空的就一个都不删 —— 判这条要在加上自己的项目根**之前**：加完它永远不空，
    // 这道闸就成了摆设（第一版就是这么写的，状态测试当场红）
    if (this.restoring || keep.length === 0) return;
    try {
      const keys: string[] = [];
      for (let i = 0; i < localStorage.length; i++) {
        const k = localStorage.key(i);
        if (k) keys.push(k);
      }
      // 自己这个窗口的项目无论如何都留着（它可能刚打开，还没进 Rust 的名单）
      const mine = project.root ? [project.root] : [];
      for (const k of session.staleKeys(keys, [...keep, ...mine], (x) => localStorage.getItem(x))) localStorage.removeItem(k);
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
   * ⌘Q 前有 `flush` 事件补写（App.svelte，多窗口第 3 步），关单个窗口有 pagehide，
   * 但两条都兜不住崩溃（Rust 侧是 panic = abort，进程当场死）、也兜不住 Dock 右键「退出」
   * 和注销关机（那几条直接走 AppKit 的 `terminate:`）—— 这个定时落盘是它们的退路。
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
