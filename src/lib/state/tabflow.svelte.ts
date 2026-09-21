import {
  probePath,
  readText,
  fileStamp,
  detectEncoding,
  openLog,
  closeLog,
  discardEmptyScratch,
  createScratch,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { project } from "./project.svelte";
import { scratches } from "./scratches.svelte";
import { files } from "./files.svelte";
import { git } from "./git.svelte";
import { type TabState } from "./tab";
import { scratchTitle, splitFrontmatter } from "./frontmatter";

/** 动作在 `tabflow-ops.ts`，按需加载 —— 理由见那边的文件头 */
const ops = () => import("./tabflow-ops");

/**
 * 一个文件在标签表里的进出：打开（文件 / 文件夹 / 最近 / 草稿）、关闭（含「未保存怎么办」
 * 那一问）、切模式。
 *
 * **首屏之前要跑的留在这儿**（`openPath` / `launchScratch` / `newScratch` / `requestClose` / `doClose`
 * 和三个确认横幅读的状态）；其余动作（开文件夹、最近、关项目、切模式、批量关、废纸篓）
 * 在 `tabflow-ops.ts`，按需加载 —— 同 `git-ops.ts` 那套（入口包瘦身 2026-09-21）。
 *
 * `tabs` 是表，这里是**流程**：读盘、开日志引擎、问用户、写回表。
 * 从 App.svelte 搬出来（issue #9 第 4b 步），逻辑一个字不改。
 * 三条确认横幅（关脏标签 / 大文件切编辑 / 外部改动冲突）读这里的
 * `pendingClose` / `pendingSwitch` / `closeQueue` 来渲染。
 */

/**
 * 手动切到编辑模式时，超过这个大小要先确认。
 * 编辑模式会把全文读进内存并交给 CodeMirror，大文件是真的会卡。
 */
export const CONFIRM_EDIT_BYTES = 8 << 20;

class TabFlow {
  /** 待确认关闭的脏标签 —— 直接丢弃改动太粗暴，也不该静默保存 */
  pendingClose = $state<TabState | null>(null);
  /** 待确认的模式切换（大文件切到编辑模式时用） */
  pendingSwitch = $state<TabState | null>(null);
  /**
   * 批量关闭时还没问过的标签 —— **只装有未保存改动的那些**。
   *
   * 干净的标签在 `closeMany` 里当场就关了，不进队列：为一堆没改动的文件
   * 逐个弹确认框，没有任何信息量。
   */
  closeQueue = $state<number[]>([]);

  /** 正在打开的路径，防止双击或事件重放时重复探测 */
  #opening = new Set<string>();

  /**
   * 正在把上次的标签摆回来。**唯一的作用是拦住 `openPath` 去动 `activeId`。**
   *
   * 内容区是 `{#key tabs.active.id}` 包着的 —— `activeId` 一变就销毁重建。
   * 而恢复是一个一个 `await openPath()` 的，每开一个就把 `activeId` 顶成它，
   * 于是恢复五个标签 = 建五次编辑器再销毁四次，肉眼可见地一个一个闪过去。
   *
   * 改成由 `restoreSession` 在**恰好走到该激活的那个标签时**设一次 `activeId`，
   * 编辑器只建一次。标签仍按存下来的顺序逐个进列表 —— 那只是标签条在长，
   * 不重建任何东西。
   *
   * **和 App 里的 `restoring` 是两回事，不能合并。** 那个管的是「恢复期不写快照」，
   * 它要一直盖到启动路径的最后 —— 包括命令行传进来的那个文件
   * （`lite-ide a.rs`）。而那个文件**恰恰应该**被激活，合并了就等于
   * `lite-ide a.rs` 打开却不切过去。
   */
  restoringTabs = false;

  /**
   * 换项目根前后的钩子（issue #33 ㉓）。App 装的是 `persist.beforeRootChange` /
   * `afterRootChange`：旧项目的现场存到它自己那份、关干净标签、摆新项目的标签。
   * `persist` 已经 import 这里，反过来 import 就是环，所以走钩子。
   */
  hooks: {
    beforeRootChange?: (old: string | null, next: string) => void;
    afterRootChange?: (next: string) => Promise<void> | void;
  } = {};

  /**
   * `quiet` 给会话恢复用：上次开着的文件这次可能已经不在了
   * （删了、改名了、切到了没有它的分支）。那是完全正常的事，
   * 逐个弹「读不到 xxx」只会在启动时糊一屏红字。
   *
   * `preview` = 开成预览标签（issue #33 ⑯，语义见 `TabState.preview`）。
   * 谁传 true：单击文件树、搜索结果、⌘B 跳转、⌥⌘←/→。谁不传：⌘P、拖进来、
   * 命令行、最近项目、双击 —— 那些是「我要这个文件」，不是「看一眼」。
   * **已经开着的文件被显式打开一次就保留下来**（双击树里那一行正是走这条），
   * 而被预览地打开一次不改变它的状态。
   */
  async openPath(path: string, opts: { quiet?: boolean; preview?: boolean } = {}) {
    const quiet = opts.quiet ?? false;
    if (this.#opening.has(path)) return;
    this.#opening.add(path);
    if (!quiet) notify.clear();
    try {
      const info = await probePath(path);
      if (info.kind === "dir") {
        const old = project.root;
        if (old === info.path) return;
        this.hooks.beforeRootChange?.(old, info.path);
        project.root = info.path;
        await this.hooks.afterRootChange?.(info.path);
        return;
      }
      if (!this.restoringTabs) files.touch(info.path);
      const exist = tabs.byPath(info.path);
      if (exist) {
        if (!opts.preview) tabs.keep(exist.id);
        if (!this.restoringTabs) tabs.show(exist.id);
        // 显式打开一个已经开着的文件 = 「我要用它」，光标要进去；预览不抢焦点
        if (!opts.preview && !this.restoringTabs && exist.mode === "edit") docs.focusEditor();
        return;
      }

      const tab: Omit<TabState, "id" | "group"> = {
        path: info.path,
        name: info.name,
        mode: info.mode,
        dirty: false,
        reason: info.reason,
        size: info.size,
      };
      if (info.mode === "log") {
        tab.handle = (await openLog(info.path)).handle;
        // 日志模式在前端用 TextDecoder 解码，只需要标签
        tab.encoding = await detectEncoding(info.path).catch(() => "UTF-8");
      } else {
        const t = await readText(info.path);
        tab.content = t.content;
        tab.encoding = t.encoding;
        tab.bom = t.bom;
        tab.eol = t.eol;
        tab.lossy = t.lossy;
        tab.stamp = await fileStamp(info.path);
        // 草稿的标签名是它的第一行（见 TabState.title）
        if (project.isScratch(info.path)) tab.title = scratchTitle(t.content);
      }
      /*
       * 预览标签顶掉预览标签：新的落在旧的那一格，旧的关掉。
       * 先加后关 —— 反过来的话 `remove` 会先把 activeId 挪到邻居上，
       * 内容区白白重建一次；而且旧的一关，「它在第几格」就没了。
       * 走 `doClose` 而不是 `tabs.remove`：日志模式的引擎句柄要还。
       */
      // 预览标签按组算（issue #35）：顶掉的是焦点组那一个
      const prev = opts.preview ? tabs.previewIn(tabs.activeGroup) : null;
      if (opts.preview) tab.preview = true;
      const id = tabs.add(tab, prev ? tabs.list.indexOf(prev) : undefined);
      if (prev) this.doClose(prev);
      // 恢复期不抢：见 `restoringTabs` 上面那段
      if (!this.restoringTabs) tabs.show(id);
      /*
       * **开一个文件不再把它的父目录顶成项目根**（issue #40 第三层，2026-09-16）。
       *
       * 原来是「没有项目根时拿父目录顶上，文件树才有东西显示」。代价在两处都踩到了：
       * 双击 `~/Downloads/x.log` 看一眼，`~/Downloads` 就成了项目 —— 文件树列它、
       * git 去找它、watcher 挂上它，还进了「最近打开」；没开项目就 ⌘N 记东西，
       * 草稿目录成了项目，同样进最近列表。「看一眼 / 记两笔」本来就是**无项目**的动作，
       * 套一个项目上去就是那种「杀鸡用牛刀」的重量感。
       *
       * 没有根时：侧边栏不出现（草稿视图照常，见 `layout.sideShown`），面包屑显示全路径，
       * ⇧⌘F 没有范围（菜单灰着）。要项目就 ⌘O 或者拖文件夹进来 —— 那是显式的。
       */
      // 恢复期不核：那时 activeId 故意停在 null 而标签一个个往里填，
      // 「有标签但没有活动标签」在这段窗口里是对的。恢复完了再一次核完
      if (!this.restoringTabs) tabs.audit("开标签");
    } catch (e) {
      // 打不开的（删了、卷没挂）从「最近」里摘掉，下次 ⌘E 不再列它
      files.forget(path);
      if (!quiet) notify.fail(String(e));
    } finally {
      this.#opening.delete(path);
    }
  }

  /**
   * 新建一份草稿并打开。
   *
   * 名字按**本地时间**取（`2026-09-09 1030.md`）：草稿是「看日志时顺手记两笔」
   * 的临时纸，翻回来时唯一记得的线索就是「大概什么时候记的」。
   * 不弹输入框问名字 —— 中间隔一次打字，「想记就记」就没了。
   *
   * 时间戳在这边算而不是 Rust 侧：std 里没有本地时区，为一个文件名
   * 拽一个日期库进去不值，而 `new Date()` 天然就是本地的。
   */
  /**
   * ⌘N。**静默**记下锚点（M10）：项目 / 分支 / HEAD / 当前文件:行，写进文件头。
   * 静默是硬要求 —— 记一笔的动作仍然是「⌘N 然后打字」，多一个问句人就回备忘录了。
   * 四样都可以为空：没开项目就是一份普通草稿。
   */
  async newScratch() {
    notify.clear();
    try {
      const d = new Date();
      const pad = (n: number) => String(n).padStart(2, "0");
      const stem =
        `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
        `${pad(d.getHours())}${pad(d.getMinutes())}`;
      await this.openPath(await createScratch(stem, this.anchorNow()));
      void scratches.refresh();
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /**
   * 启动时一个标签都没有 → 直接落在一份草稿里，光标在闪（Sublime 那样）。
   * 空态卡片自己写着「记点东西」，却要人先点一下 —— 定位和首屏不一致。
   *
   * **不每次都新建。** `newScratch` 是立刻落盘一个带头的文件，每天开一次就多一份
   * 空草稿，一个月后列表全是垃圾。最新那份草稿如果正文还是空的（`firstLine` 空串），
   * 就开它 —— 盘上最多只有一份空草稿，也不需要退出时反向清理。
   * 只在启动时用：关掉最后一个标签回到卡片，那时人是主动清空的。
   */
  async launchScratch() {
    await scratches.refresh().catch(() => {});
    const empty = scratches.list
      .filter((e) => e.firstLine === "")
      .sort((a, b) => b.mtimeMs - a.mtimeMs)[0];
    if (empty) {
      await this.openPath(empty.path);
      return;
    }
    await this.newScratch();
  }

  /**
   * 此刻的锚点。`at` 只记项目里、编辑模式的标签（日志 / 差异 / 草稿本身不算）；
   * 行号取编辑器最后报上来的光标行（`docs.posByPath`），没报过就不记行。
   */
  anchorNow() {
    const root = project.root;
    const t = tabs.active;
    let at = "";
    if (root && t && t.mode === "edit" && t.path.startsWith(`${root}/`) && !project.isScratch(t.path)) {
      const line = docs.lineOf(t.path);
      at = t.path.slice(root.length + 1) + (line ? `:${line}` : "");
    }
    const st = git.status;
    return {
      project: root ?? "",
      branch: st?.branch ?? "",
      head: st?.head ?? "",
      at,
    };
  }

  requestClose(id: number, force = false) {
    const tab = tabs.byId(id);
    if (!tab) return;
    // 钉住的不跟着 ⌘W / ✕ 走（issue #33 ⑰）。右键「关闭」带 force
    if (tab.pinned && !force) {
      notify.ok(`${tab.name} 已钉住 —— 点标签上的图钉取消钉住，或右键「关闭」`, 2800);
      return;
    }
    if (tab.dirty) {
      /*
       * 草稿先自己写一次（issue #40）：写成了就直接关，不问 ——
       * 关一张便签和关 Sublime 的标签要一样便宜。写不成才问，
       * 那时是真的有东西会丢。非草稿一律问，`autosaveBeforeClose` 对它们恒为 false。
       */
      void docs.autosaveBeforeClose(tab).then((saved) => {
        const again = tabs.byId(id);
        if (!again) return;
        if (saved && !again.dirty) {
          this.doClose(again);
          return;
        }
        tabs.show(again.id);
        this.pendingClose = again;
      });
      return;
    }
    this.doClose(tab);
  }

  // ── 动作：全部转发到 tabflow-ops.ts（按需加载，入口包瘦身 2026-09-21）。签名和文档见那边 ──
  async closeProject() {
    return (await ops()).closeProject();
  }
  async openFolder() {
    return (await ops()).openFolder();
  }
  async openRecent(dir: string) {
    return (await ops()).openRecent(dir);
  }
  async openScratchDir() {
    return (await ops()).openScratchDir();
  }
  async trashScratch(path: string) {
    return (await ops()).trashScratch(path);
  }
  async requestSwitchMode(tab: TabState) {
    return (await ops()).requestSwitchMode(tab);
  }
  async doSwitch(tab: TabState, to: "edit" | "log") {
    return (await ops()).doSwitch(tab, to);
  }
  async closeMany(ids: number[]) {
    return (await ops()).closeMany(ids);
  }
  async resolveClose(kind: "save" | "discard" | "cancel") {
    return (await ops()).resolveClose(kind);
  }

  doClose(tab: TabState) {
    if (tab.mode === "log" && tab.handle !== undefined) void closeLog(tab.handle);
    /*
     * 点了加号又一个字没写，关掉就把那个 0 字节的文件丢掉 ——
     * 留着是纯噪音，而它从生到死没有过内容，没有任何东西可以丢失。
     *
     * **写过又删光再关**的那种走不到这儿：那时 `dirty` 是真的，
     * 界面会先弹「保存并关闭 / 丢弃改动」。
     *
     * 失败一律吞掉：删不动（没权限、已经被别处删了）不该在关标签时糊一句红字，
     * 而且什么都没损失。真正的判据在 Rust 侧，这边只负责「像不像」。
     */
    if (
      project.isScratch(tab.path) &&
      tab.mode === "edit" &&
      !tab.dirty &&
      // 「空」= 正文空：带锚点头的草稿一建出来就有几十字节（M10），头不算字
      (tab.content ?? "").slice(splitFrontmatter(tab.content ?? "")[1]).trim() === ""
    ) {
      void discardEmptyScratch(tab.path)
        .catch(() => {})
        .finally(() => void scratches.refresh());
    }
    tabs.remove(tab.id);
    this.pendingClose = null;
    tabs.audit("关标签");
  }
}

export const tabflow = new TabFlow();
