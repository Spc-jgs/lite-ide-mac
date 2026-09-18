import {
  probePath,
  readText,
  fileStamp,
  detectEncoding,
  openLog,
  closeLog,
  discardEmptyScratch,
  createScratch,
  scratchDir,
  pickFolder,
  trashEntry,
  revealInFinder,
} from "../ipc/commands";
import { notify } from "./notify.svelte";
import { tabs } from "./tabs.svelte";
import { docs } from "./docs.svelte";
import { project } from "./project.svelte";
import { scratches } from "./scratches.svelte";
import { git } from "./git.svelte";
import { type TabState } from "./tab";
import { scratchTitle, splitFrontmatter } from "./frontmatter";

/**
 * 一个文件在标签表里的进出：打开（文件 / 文件夹 / 最近 / 草稿）、关闭（含「未保存怎么办」
 * 那一问）、切模式。
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
      const exist = tabs.byPath(info.path);
      if (exist) {
        if (!opts.preview) tabs.keep(exist.id);
        if (!this.restoringTabs) tabs.activeId = exist.id;
        // 显式打开一个已经开着的文件 = 「我要用它」，光标要进去；预览不抢焦点
        if (!opts.preview && !this.restoringTabs && exist.mode === "edit") docs.focusEditor();
        return;
      }

      const tab: Omit<TabState, "id"> = {
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
      const prev = opts.preview ? tabs.preview : null;
      if (opts.preview) tab.preview = true;
      const id = tabs.add(tab, prev ? tabs.list.indexOf(prev) : undefined);
      if (prev) this.doClose(prev);
      // 恢复期不抢：见 `restoringTabs` 上面那段
      if (!this.restoringTabs) tabs.activeId = id;
      /*
       * **开一个文件不再把它的父目录顶成项目根**（issue #40 第三层，2026-09-16）。
       *
       * 原来是「没有项目根时拿父目录顶上，文件树才有东西显示」。代价在两处都踩到了：
       * 双击 `~/Downloads/x.log` 看一眼，`~/Downloads` 就成了项目 —— 文件树列它、
       * git 去找它、watcher 挂上它，还进了「最近打开」；没开项目就 ⌘N 记东西，
       * 草稿目录成了项目，同样进最近列表。「看一眼 / 记两笔」本来就是**无项目**的动作，
       * 套一个项目上去就是那种「杀鸡用牛刀」的重量感。
       *
       * 没有根时：侧边栏是「打开文件夹…」那块空态（草稿视图照常），面包屑显示全路径，
       * ⇧⌘F 没有范围（菜单灰着）。要项目就 ⌘O 或者拖文件夹进来 —— 那是显式的。
       */
      // 恢复期不核：那时 activeId 故意停在 null 而标签一个个往里填，
      // 「有标签但没有活动标签」在这段窗口里是对的。恢复完了再一次核完
      if (!this.restoringTabs) tabs.audit("开标签");
    } catch (e) {
      if (!quiet) notify.fail(String(e));
    } finally {
      this.#opening.delete(path);
    }
  }

  /**
   * 关闭项目（issue #40 第三层）：回到「只有标签、没有项目」的轻窗口。
   *
   * 和切项目是同一套：旧项目的现场存到它自己那份、关掉它的**干净**标签
   * （脏的留着，自己会在关的时候问）、草稿不动。VS Code 的 Close Folder 也是
   * 这个语义。没有项目时什么都不做。
   */
  closeProject() {
    const old = project.root;
    if (!old) return;
    notify.clear();
    this.hooks.beforeRootChange?.(old, "");
    project.root = null;
    for (const t of [...tabs.list]) if (!t.dirty && !project.isScratch(t.path)) this.doClose(t);
    tabs.audit("关闭项目");
  }

  /**
   * 开原生的选择文件夹面板。取消了什么也不做。
   *
   * 选中之后走的是 `openPath` —— 它对目录的处理就是把 `project.root` 设过去，
   * 和拖一个文件夹进来、命令行传目录**是同一条路**。
   * 另起一套的话，「切项目要不要清掉旧标签」这类判断就会有两份。
   */
  async openFolder() {
    const dir = await pickFolder().catch(() => null);
    if (!dir) return;
    await this.openPath(dir);
  }

  /**
   * 从菜单里选一个最近项目。
   *
   * **不预先探测存在性。** 每次开菜单去 stat 一遍 8 个路径，
   * 碰上没挂载的网络卷会把菜单卡住 —— 改成点了才发现：
   * 打不开就报一句并把它从列表里摘掉，那时用户已经知道自己在等什么了。
   */
  async openRecent(dir: string) {
    const info = await probePath(dir).catch(() => null);
    if (info?.kind !== "dir") {
      notify.fail(`打不开 ${dir} —— 已从最近记录里移除`, 3200);
      project.recent = project.recent.filter((r) => r !== dir);
      return;
    }
    await this.openPath(dir);
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

  /**
   * 在 Finder 里显示草稿目录。
   *
   * 这条原来是「把草稿目录当项目根打开」—— 零新代码就有文件树、⌘P、⇧⌘F，
   * 但代价是**切走当前项目**：翻一条笔记要换工作区，多数人选不翻（issue #40）。
   * 翻草稿现在是侧边栏的「草稿」视图；这条只剩「去 Finder 里整理」这一个用途，
   * 而草稿目录在 Finder 里默认看不见（「资源库」是隐藏的），所以它还值得留着。
   *
   * 目录不存在**不是错误**，是「你还一条都没记过」—— 报一句红字会让人以为坏了。
   */
  async openScratchDir() {
    notify.clear();
    try {
      const dir = await scratchDir();
      if (!(await probePath(dir).catch(() => null))) {
        notify.ok("还没有草稿 —— ⌘N 记第一条", 2600);
        return;
      }
      await revealInFinder(dir);
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /**
   * 从草稿列表里把一份移到废纸篓（issue #40）。走的是文件树那条现成的废纸篓路径 ——
   * 应用里没有第二条删除路径。开着的标签一起关掉：留一个指向废纸篓里文件的标签，
   * 下次 ⌘S 会把它原地复活。
   */
  async trashScratch(path: string) {
    notify.clear();
    try {
      /*
       * 开着且脏的先落盘再移：移走的是盘上那份，编辑器里 500ms 内还没写下去的字
       * 要跟着一起进废纸篓 —— 「放回原处」放回来的才是完整的。写不成就停下问，
       * 别一边报错一边把文件挪走。
       */
      for (const t of tabs.under(path, false)) {
        if (t.dirty && !(await docs.autosaveBeforeClose(t))) {
          tabs.activeId = t.id;
          this.pendingClose = t;
          notify.fail("这份草稿有没写进盘的改动，先处理它再移到废纸篓", 3200);
          return;
        }
      }
      await trashEntry(path);
      for (const t of tabs.under(path, false)) this.doClose(t);
      void scratches.refresh();
    } catch (e) {
      notify.fail(String(e));
    }
  }

  requestSwitchMode(tab: TabState) {
    if (tab.dirty) {
      notify.fail("有未保存的改动，请先保存（⌘S）再切换模式", 2600);
      return;
    }
    const to = tab.mode === "edit" ? "log" : "edit";
    // 切到日志模式没有风险（mmap，内存与大小无关）；反方向要看体积
    if (to === "edit" && tab.size > CONFIRM_EDIT_BYTES) {
      this.pendingSwitch = tab;
      return;
    }
    void this.doSwitch(tab, to);
  }

  async doSwitch(tab: TabState, to: "edit" | "log") {
    this.pendingSwitch = null;
    notify.clear();
    try {
      if (tab.mode === "log" && tab.handle !== undefined) {
        await closeLog(tab.handle);
        tab.handle = undefined;
      }
      if (to === "log") {
        tab.handle = (await openLog(tab.path)).handle;
        tab.content = undefined;
      } else {
        const t = await readText(tab.path, tab.forced ? tab.encoding : undefined);
        tab.content = t.content;
        tab.encoding = t.encoding;
        tab.bom = t.bom;
        tab.eol = t.eol;
        tab.lossy = t.lossy;
      }
      tab.mode = to;
      tab.forced = to;
      tabs.audit("切模式");
    } catch (e) {
      notify.fail(String(e));
      // 切换失败要退回原状态，否则标签会停在一个既没句柄也没内容的空壳上
      if (tab.mode === "log" && tab.handle === undefined) {
        try {
          tab.handle = (await openLog(tab.path)).handle;
        } catch {
          /* 连回退都失败，只能让用户重开 */
        }
      }
    }
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
        tabs.activeId = again.id;
        this.pendingClose = again;
      });
      return;
    }
    this.doClose(tab);
  }

  /**
   * 关掉一批标签。干净的直接关，有改动的排队逐个问。
   *
   * **不能直接全关**：标签栏的「关闭其他 / 关闭右侧 / 关闭全部」一按下去，
   * 可能带走好几个正在改的文件，而它们的改动没有任何地方找得回来
   * （不像删文件还进废纸篓）。
   */
  closeMany(ids: number[]) {
    const dirty: number[] = [];
    for (const id of ids) {
      const t = tabs.byId(id);
      // 批量关闭一律跳过钉住的：「关闭其他 / 右侧 / 全部」正是钉住要防的那几下
      if (!t || t.pinned) continue;
      if (t.dirty) dirty.push(id);
      else this.doClose(t);
    }
    this.closeQueue = dirty;
    this.#askNextClose();
  }

  /** 从队列里取下一个来问；队列空了就把横幅收掉 */
  #askNextClose() {
    while (this.closeQueue.length) {
      const id = this.closeQueue[0];
      this.closeQueue = this.closeQueue.slice(1);
      const t = tabs.byId(id);
      if (!t) continue; // 中途被别处关掉了
      /*
       * 草稿先自己写一次（issue #40）：写成了就直接关、接着问下一个，
       * 和单个 ⌘W 走的 `requestClose` 是同一条规矩 —— 「关闭全部」里夹着一份
       * 草稿不该突然弹出来问。写不成才问。
       */
      if (project.isScratch(t.path)) {
        void docs.autosaveBeforeClose(t).then((saved) => {
          const again = tabs.byId(id);
          if (again && saved && !again.dirty) {
            this.doClose(again);
            this.#askNextClose();
            return;
          }
          if (again) {
            tabs.activeId = again.id;
            this.pendingClose = again;
          } else {
            this.#askNextClose();
          }
        });
        return;
      }
      tabs.activeId = t.id; // 让人看见要丢的到底是什么
      this.pendingClose = t;
      return;
    }
    this.pendingClose = null;
  }

  /**
   * 「保存并关闭 / 丢弃改动 / 取消」三个按钮的落点。
   *
   * 取消**把整批都停掉**，不是只跳过这一个：连着弹五次确认框、每次都得
   * 再点一次取消，比没有批量关闭还烦人。
   */
  async resolveClose(kind: "save" | "discard" | "cancel") {
    const t = this.pendingClose;
    if (!t) return;
    if (kind === "cancel") {
      this.closeQueue = [];
      this.pendingClose = null;
      return;
    }
    if (kind === "save") {
      tabs.activeId = t.id;
      // 写失败就停在这儿，别往下关 —— 关了改动就真没了
      if (!(await docs.save(docs.liveText(t)))) {
        this.closeQueue = [];
        return;
      }
    }
    this.doClose(t);
    this.#askNextClose();
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
