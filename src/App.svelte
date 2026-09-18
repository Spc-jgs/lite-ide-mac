<script lang="ts">
  import { untrack, tick } from "svelte";
  import Rail from "./lib/shell/Rail.svelte";
  import Sidebar from "./lib/shell/Sidebar.svelte";
  import StatusBar from "./lib/shell/StatusBar.svelte";
  import Content from "./lib/shell/Content.svelte";
  import Overlays from "./lib/shell/Overlays.svelte";
  import TitleBar from "./lib/shell/TitleBar.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import { lazy, lazyGroup } from "./lib/lazy/lazy.svelte";
  import { notify } from "./lib/state/notify.svelte";
  import { layout } from "./lib/state/layout.svelte";
  import { tabs } from "./lib/state/tabs.svelte";
  import { tabflow } from "./lib/state/tabflow.svelte";
  import { project } from "./lib/state/project.svelte";
  import { worktree } from "./lib/state/worktree.svelte";
  import { files } from "./lib/state/files.svelte";
  import { git } from "./lib/state/git.svelte";
  import { remote } from "./lib/state/remote.svelte";
  import { branches } from "./lib/state/branches.svelte";
  import { nav } from "./lib/state/nav.svelte";
  import { persist, saved } from "./lib/state/persist.svelte";
  import { overlay } from "./lib/state/overlay.svelte";
  import { lang } from "./lib/state/lang.svelte";
  import { readPref, writePref, readNumPref, writeNumPref } from "./lib/state/prefs";
  import { terms } from "./lib/state/terms.svelte";
  import { docs } from "./lib/state/docs.svelte";
  import { wrapsByDefault } from "./lib/state/tab";
  import { scratches } from "./lib/state/scratches.svelte";
  import {
    probePath,
    ignoredDirs,
    appLogPath,
    clearAppLog,
    setRecent,
    syncMenuState,
    openExternal,
    diag,
    reportBudget,
    initialPaths,
    OPEN_PATHS_EVENT,
    watchRoot,
    gitStage,
    gitUnstage,
    scratchDir,
    revealInFinder,
    installCli,
  } from "./lib/ipc/commands";


  // 文档生命周期往外的两个钩子：保存完刷 git，光标动了安排存快照（见 docs.svelte.ts 文件头）
  docs.hooks.afterSave = () => void git.refresh();
  docs.hooks.afterPos = () => persist.schedule();
  // 草稿落盘之后侧边栏那行摘要要跟着变（issue #40）
  docs.hooks.afterAutosave = () => void scratches.refresh();
  // 远程操作的确认条长在 Git 那组懒加载的组件里，操作前先把它们拉起来
  remote.hooks.warmUi = () => gitUi.load();
  // 切项目：旧项目的现场存到它自己那份，关干净标签，摆新项目的标签（#33 ㉓）
  tabflow.hooks.beforeRootChange = (old) => persist.beforeRootChange(old);
  tabflow.hooks.afterRootChange = (next) => persist.afterRootChange(next);




  /** 缩略图开关。纯偏好，存 localStorage（理由见 state/prefs.ts） */
  let showMinimap = $state(readPref("minimap", true));
  $effect(() => {
    writePref("minimap", showMinimap);
  });

  /**
   * 编辑器字号（⌘= / ⌘- / ⌘0）。写在 :root 的 CSS 变量上，CM6 主题读它 ——
   * 不重建编辑器，光标和撤销栈都不动。夹在 9–28：小于 9 看不清，大于 28 一行放不下几个字。
   */
  const FONT_DEFAULT = 13;
  let editorFont = $state(readNumPref("editorFont", FONT_DEFAULT));
  $effect(() => {
    document.documentElement.style.setProperty("--editor-font-size", `${editorFont}px`);
    writeNumPref("editorFont", editorFont);
  });

  /**
   * git 说这个项目里哪些目录被忽略了。**按项目问一次，不是按目录问。**
   *
   * `null` 有两种含义，而它们要的行为一样，所以合成一个：还没问到（启动那一瞬）、
   * 问不到（不是 git 仓库、git 不在）。两种都退回「按名字判」。
   *
   * 为什么不在 `list_dir` 里顺手问：同一个项目的答案是同一份，而一次树刷新
   * 会重列每一个展开着的目录 —— 那就是十来次子进程换同一个答案
   * （本仓库实测一次 11.5ms / 19 条）。issue #13。
   */
  let ignored = $state<Set<string> | null>(null);

  /*
   * 文件索引（⌘P / ⌘Click / ⌘E 共用的那一份）跟着项目根和 `worktree.treeTick` 刷：
   * 切分支、终端里新建的文件都在 `treeTick` 上，刷一次 `rg --files` 0.02s。
   * 序号守卫在 store 里，切项目时上一趟晚到不会盖掉新的。
   */
  $effect(() => {
    const r = project.root;
    worktree.treeTick;
    void files.refresh(r);
  });

  /*
   * 跟着项目根和 `worktree.treeTick` 走。
   *
   * 带上 `worktree.treeTick` 是因为 `.gitignore` 本身是可以改的 —— 改完走一次
   * `worktree.changed()`（切分支、丢弃改动、从终端切回来都会），
   * 这份答案就跟着更新。不带的话，改完 `.gitignore` 得重开项目才生效。
   */
  $effect(() => {
    const r = project.root;
    worktree.treeTick;
    if (!r) {
      ignored = null;
      return;
    }
    let dead = false;
    void ignoredDirs(r)
      .then((list) => {
        // await 回来时这条 effect 可能早被清理了 —— 见 rules/frontend.md
        if (!dead) ignored = list === null ? null : new Set(list);
      })
      .catch(() => {
        if (!dead) ignored = null;
      });
    return () => {
      dead = true;
    };
  });


  // ─────────────────────────── Git ───────────────────────────


  /*
   * Git 那一套按需加载，和 CM6、xterm 同一条纪律（ARCHITECTURE.md 红线：
   * 入口包只放两种模式都要的东西）。静态引入时入口包从 120KB 涨到 140KB，
   * 而这些东西只看日志的人一次都不会用到。
   *
   * 五个一起拉：进了 Git 就基本都会用到，分五次只是多四次往返。
   *
   * 文件树上的 git 染色不在这里面：那只是 FileTree 里的一个 $derived，
   * 没有额外模块，打开就该看见。
   */
  /**
   * 文件树也按需加载（issue #32）。它 12 KB，是入口里最大的一块，而它的行
   * **本来就要等 `list_dir` 回来才有东西画** —— 首屏先出的是侧边栏的壳，
   * 不是树。所以一挂载就拉（不像搜索浮层那样等 300ms），和第一次 IPC 并行，
   * 树到的时候它多半已经在了。没到时侧边栏那块空着，不放「载入中」：
   * 那几十毫秒里放一句字反而闪一下。
   */
  const tree = lazy(() => import("./lib/shell/FileTree.svelte"), "文件树");
  tree.load();
  /**
   * 确认横幅那一整块也按需（issue #32，实测入口包少 3 KB 出头）。判据照旧：
   * 每一条横幅都要先发生点什么（关脏标签、外部改动、丢弃改动…）才出现，
   * 窗口出现之前一条都不会有。**但它不能等到真要用时才拉**：关脏标签的确认框
   * 晚一次 chunk 往返出来，人会以为 ⌘W 没反应 —— 所以首屏画完 300ms 后预拉
   * （同搜索浮层那套），任何一个 `pending*` 一亮再兜底拉一次。
   */
  const confirms = lazy(() => import("./lib/shell/Confirms.svelte"), "确认横幅");
  $effect(() => {
    const id = setTimeout(() => confirms.load(), 300);
    return () => clearTimeout(id);
  });
  $effect(() => {
    const need =
      !!tabflow.pendingClose || !!tabflow.pendingSwitch || !!tabs.active?.conflict ||
      !!notify.banner || !!git.pendingDiscard || git.trustOpen || !!branches.pendingWtRemove ||
      !!branches.pendingCheckout || !!remote.pendingDiverge || !!remote.pendingPush || !!remote.err;
    if (need) confirms.load();
  });
  /**
   * 底部工具窗（issue #32 瘦身）。它自己只在「面板开着或有终端」时才渲染，
   * 首屏之前一个像素都不画 —— 判据和文件树一样，所以模块也不该在入口包里。
   * 恢复出来的会话面板开着的话立刻拉；否则首屏后 300ms 预拉，第一次 ⌘J 不用等往返。
   */
  const panelUi = lazy(() => import("./lib/shell/Panel.svelte"), "底部工具窗");
  $effect(() => {
    if (layout.panel || terms.list.length > 0) panelUi.load();
  });
  $effect(() => {
    const id = setTimeout(() => panelUi.load(), 300);
    return () => clearTimeout(id);
  });
  /** 草稿列表（issue #40）。只在侧边栏切到它时才拉，多数会话一次都不切 */
  const scratchUi = lazy(() => import("./lib/shell/ScratchList.svelte"), "草稿列表");
  $effect(() => {
    if (layout.sidebar && layout.sideView === "scratch") scratchUi.load();
  });

  const gitUi = lazyGroup(
    {
      pane: () => import("./lib/git/GitPane.svelte"),
      diff: () => import("./lib/git/DiffView.svelte"),
      log: () => import("./lib/git/GitLog.svelte"),
      branch: () => import("./lib/git/BranchPicker.svelte"),
      merge: () => import("./lib/git/MergeView.svelte"),
      bars: () => import("./lib/git/RemoteBars.svelte"),
    },
    "Git 面板",
  );

  $effect(() => {
    const need =
      (layout.sideView === "git" && !!git.repo) ||
      tabs.list.some((t) => t.mode === "diff" || t.mode === "merge") ||
      (layout.panel && panelTool === "git" && layout.gitTab === "log") ||
      overlay.branchOpen;
    if (need) gitUi.load();
  });














  /** 换项目根就重新找仓库；确定不是仓库时 Git 视图切回文件树（它会是空的）。草稿视图不动 */
  $effect(() => {
    void git.locate(project.root).then((found) => {
      if (found === null && layout.sideView === "git") layout.sideView = "files";
    });
  });









  /**
   * 文件树里改完盘之后的收尾。
   *
   * 三件事一起做：重读打开的文件 + 重列目录（`workingTreeChanged`），
   * 再刷一次 git 状态 —— 新建出来的文件是未跟踪的，删掉的要显示成 D，
   * 少这一下文件树上的染色就停在改动之前。
   */
  async function afterFsChange(openThis: string | null) {
    if (openThis) await tabflow.openPath(openThis);
    await worktree.changed();
    void git.refresh();
  }












  /**
   * 实际在渲染的那个工具窗（开合 / 偏好本身在 `layout` 里）。
   *
   * `layout.panelView` 是**存下来的偏好**，它可以是 `git` 而当下并没有仓库 ——
   * 上次在一个 git 仓库里看着提交历史退出，这次打开的是个普通文件夹。
   * 那时面板头写着「Git」，底下却是一片空白（历史那块的渲染条件
   * 带着 `&& git.repo`），而头上已经没有「切回终端」的按钮了（切换搬去了导轨）。
   *
   * 所以渲染一律看这个，写状态才写 `layout.panelView` —— 偏好留着，
   * 下次真打开仓库时提交历史还在。
   */
  let panelTool = $derived<"term" | "git">(layout.panelView === "git" && git.repo ? "git" : "term");

  let hovering = $state(false);
  let logStatus = $state("");


























  /** 传给文件树的「定位到这里」请求。自增 tick 触发，理由见 FileTree 的 props 注释 */
  let revealPath = $state("");
  let revealTick = $state(0);

  /**
   * 在文件树里定位一个路径。目前只有面包屑用。
   *
   * 面包屑的目录段以前直接调 `openPath`，而 `openPath` 遇到目录会把
   * **项目根整个换掉** —— 文件树重列、Git 仓库重探、⌘P 索引重建，
   * 而人只是想看一眼那个目录在哪；换完还回不去（除非重新拖一次文件夹）。
   * 面包屑是「我现在在哪」的指示器，点它的合理预期是导航过去，不是改项目。
   *
   * 换根仍然是显式动作：拖文件夹进来、命令行参数、打开工作树。
   * 这里只改面包屑这一条调用点，`openPath` 本身不动。
   */
  function revealInTree(path: string) {
    layout.showSide("files");
    revealPath = path;
    revealTick++;
  }

  /*
   * 按需加载失败要说出来。以前每个 import 各自 catch 到 error 里，
   * 抽成 lazy() 之后错误存在各自的 store 上，这里统一汇到状态栏。
   *
   * **这张名单漏一个就是一处静默失败** —— `keysPanel` 就漏在这儿过：
   * ⌘/ 按下去什么都不出来，而状态栏一声不吭。2026-09-07 补上它和
   * 新加的 `overlays`。加新的 lazy() 时记得回来加一行。
   */
  $effect(() => {
    const e = gitUi.error || tree.error || panelUi.error || confirms.error || scratchUi.error;
    if (e) notify.fail(e);
  });

  // 语言识别表只在有标签打开时才拉（理由在 lang.svelte.ts）
  $effect(() => {
    if (tabs.active) lang.load();
  });















  let branchBtn = $state<HTMLElement | null>(null);
  /** 两条路（挂件、菜单）都从这儿走，锚点由同一个元素定（Git 栏的分支行 M9 删了，它和挂件是同一个东西印两遍） */
  function openBranchPicker() {
    overlay.openBranches(branchBtn?.getBoundingClientRect());
  }




  /** 项目主页。交给系统默认浏览器 —— 这个应用自己不开网页 */
  /**
   * 打开应用自己的运行日志。
   *
   * 走的是普通的 `openPath` —— 那个文件多半会被判成日志模式（体积/行数），
   * 于是级别过滤、tail、跳到下一处错误全都现成。**这就是这个功能的全部实现**：
   * 一个日志查看器不需要另外做一个「日志窗口」。
   *
   * 正常情况下这个文件**一定在** —— 启动时 `applog::install` 就把它建出来了，
   * 并且写了一行「启动 vX.Y.Z」。所以探不到它意味着日志根本没装上
   * （目录建不了、权限不对），那条消息要这么说，不能只说「打不开」。
   */
  /**
   * 清空应用日志。
   *
   * 清完必须走一次 `worktree.changed()` —— 否则开着那份日志的标签上
   * 还摊着刚被清掉的几百行，人会以为没生效，然后再点一次。
   * 这正是那条老规矩的又一例（**盘上的东西被外部改了，两件事要一起做**），
   * 只不过这次「外部」是我们自己。
   */
  async function clearLog() {
    try {
      await clearAppLog();
      await worktree.changed();
      notify.ok("应用日志已清空");
    } catch (e) {
      notify.fail(`清不掉应用日志：${e}`);
    }
  }

  async function openAppLog() {
    let path: string;
    try {
      path = await appLogPath();
      await probePath(path);
    } catch (e) {
      notify.fail(`应用日志没装上：${e}`);
      return;
    }
    await tabflow.openPath(path);
  }

  async function openRepoPage() {
    await openExternal("https://github.com/Spc-jgs/lite-ide-mac").catch(() => {
      notify.fail("打不开项目主页", 2600);
    });
  }












  // 有未保存改动时定期落一次盘 —— 为什么要有这条，见 persist.svelte.ts 的 tickDrafts。
  // 草稿的自动保存也搭这班车：它兜的是「恢复出来就是脏的、之后一个字没敲」那种，
  // 停止输入那条 500ms 的路到不了它（见 docs.autosaveSweep）
  $effect(() => {
    const id = setInterval(() => {
      persist.tickDrafts();
      void docs.autosaveSweep();
    }, 4000);
    return () => clearInterval(id);
  });

  /*
   * 窗口失焦 = 人切走了。草稿在这一刻落盘，不等空闲期（issue #40）——
   * 「记两笔、⌘Tab 切到别处」是草稿最典型的用法，等 4 秒 tick 太晚：
   * 那边可能正在用 Spotlight 找刚记的东西。
   */
  $effect(() => {
    const onBlur = () => void docs.autosaveSweep(true);
    window.addEventListener("blur", onBlur);
    return () => window.removeEventListener("blur", onBlur);
  });

  // 响应式那一半：布局、标签、项目根变了就存
  $effect(() => {
    // 显式读一遍，让 effect 订阅上它们
    void [project.root, tabs.list.length, tabs.activeId, layout.snapshot(), files.recent];
    persist.schedule();
  });

  /*
   * 退出前补一次。
   *
   * 防抖有 400ms 的窗口，而「移完光标马上 ⌘Q」正好落在里面 ——
   * 那次移动就丢了。pagehide 比 beforeunload 可靠（Safari/WKWebView 上
   * beforeunload 不一定触发），两个都挂上，写两次也无所谓。
   */
  $effect(() => {
    const flush = () => {
      persist.flush();
      // 退出时草稿多半写不完（IPC 回不来进程就没了），但发出去不亏：
      // 写不完的那份已经在快照里 stash 住，下次启动 4 秒内补上
      void docs.autosaveSweep(true);
    };
    window.addEventListener("pagehide", flush);
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      window.removeEventListener("beforeunload", flush);
    };
  });




  $effect(() => {
    const onFocus = () => {
      // 用户可能刚切出去，在终端里 commit / checkout / mv 完再切回来
      void worktree.changed();
      void git.refresh();
    };
    window.addEventListener("focus", onFocus);
    /*
     * 兜底轮询只查已打开文件的指纹，**不**重列目录 ——
     * 重列要按展开的目录数发一串 IPC，每 10 秒跑一次纯属白烧。
     * 目录结构的变化靠焦点事件捕捉就够了。
     */
    const id = setInterval(() => void docs.checkExternalChanges(), 10_000);
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(id);
    };
  });











  /** 双击 Shift 的上一次时间戳；按下任何其他键即作废 */
  let lastShiftUp = 0;

  function onWindowKeyUp(e: KeyboardEvent) {
    if (e.key !== "Shift") {
      lastShiftUp = 0;
      return;
    }
    const now = Date.now();
    // 连按两次 Shift —— IDEA 的「随处搜索」手势。
    // 阈值取 500ms，与系统默认双击间隔相当；太短会让手慢的人按不出来
    if (now - lastShiftUp < 500) {
      lastShiftUp = 0;
      overlay.openQuick("all");
    } else {
      lastShiftUp = now;
    }
  }

  function onWindowKey(e: KeyboardEvent) {
    /*
     * 按住 Shift 时 e.key 给的是**大写字母**（规范如此：key 是修饰后的字符值），
     * 所以 `e.key === "g" && e.shiftKey` 永远不成立 —— ⌘⇧G / ⌘⇧O / ⌘⇧F
     * 全都因为这个悄悄失效过。统一小写化之后两种情况都对。
     */
    const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
    if (e.key === "Escape" && overlay.quickOpen) {
      overlay.quickOpen = false;
      return;
    }
    if (!e.metaKey) return;
    /*
     * ── 这里只剩两条 ──
     *
     * 2026-09-05 加菜单栏时，**归菜单的键位全部从这儿摘掉了**
     * （⌘S ⌘W ⌘1 ⌘J ⇧⌘F ⇧⌘O ⇧⌘G ⌃⇧` ⌘O ⌘/）。
     *
     * 不是为了少写几行：菜单项一旦带上 accelerator，AppKit 会**先把键吃掉**，
     * webview 根本收不到 —— 两边都留着的话，要么双触发（⌘W 一下关两个标签），
     * 要么某一天 accelerator 没生效而 keydown 悄悄兜住了，
     * 于是「菜单坏了」这件事永远暴露不出来。
     *
     * 谁归谁的判据在 `src/lib/state/keymap.ts` 的 `owner` 列，
     * `src-tauri/tests/menu_sync.rs` 卡着两边不许漂。
     */

    /*
     * ⌘P 故意留在这儿（keymap.ts 里 owner = "key"）。
     *
     * 进菜单的话，焦点在终端里按 ⌘P 会被菜单抢走 —— 而终端里的 ⌘P
     * 更可能是想给 shell 的（zsh 的历史上一条）。菜单项只写标签、不挂键位。
     */
    if (k === "p") {
      e.preventDefault();
      overlay.openQuick("file");
      return;
    }
    // ⌘E 最近文件：和 ⌘P 是同一个面板，空着就列最近打开的
    if (k === "e") {
      e.preventDefault();
      overlay.openQuick("file");
      return;
    }
    /*
     * **这儿原来有一条 `⌘B → 切侧边栏`**（⌘1 的 VSCode 手感别名），
     * 2026-09-09 把 ⌘B 让给了「跳到声明」之后删掉了。
     *
     * 光把 `keymap.ts` 里的 `alias` 字段拿掉是不够的 —— 那张表是**说明书**，
     * 真正吃键的是这一段。
     *
     * **它不会挡住跳转**（实测：带着这段代码跑 smoke ⑮，三条照样全绿 ——
     * CM6 的 ⌘B 在编辑器里先跑，`preventDefault` 之后事件照样冒泡到 window）。
     * 留着它的后果是**多一个副作用**：在编辑器里按 ⌘B 跳转的同时，
     * 侧边栏自己开合一下。一个键干两件事，而速查表只说了一件。
     */
  }

  /**
   * 「安装命令行工具…」（issue #40）。装上了说一句；软链没装上（/usr/local/bin 要 sudo）
   * 就把那一句命令摆在横幅里 —— 它不会自动消失，人要把它抄进终端。
   */
  async function installCliTool() {
    notify.clear();
    try {
      const r = await installCli();
      if (r.linked) {
        notify.ok(`${r.replaced ? "已重新安装" : "已安装"} lite 命令 —— 终端里 lite <路径> 就能开`, 5000);
      } else {
        notify.block(
          "脚本已写好，但 /usr/local/bin 写不进去 —— 在终端里跑这一句补上软链：",
          r.linkCmd,
        );
      }
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /**
   * 菜单项按下去做什么。
   *
   * id 与 `keymap.ts`、`menu.rs` 三处同一套 —— 那两处由
   * `tests/menu_sync.rs` 卡着，这里是第三处，漏一个 case 的表现是
   * 「点了没反应」，所以末尾留了一条 diag。
   */
  async function runMenu(id: string) {
    if (id.startsWith("recent:")) {
      await tabflow.openRecent(id.slice("recent:".length));
      return;
    }
    switch (id) {
      case "open-folder": return void tabflow.openFolder();
      case "close-project": return tabflow.closeProject();
      case "new-scratch": return void tabflow.newScratch();
      case "open-scratch-dir": return void tabflow.openScratchDir();
      case "install-cli": return void installCliTool();
      case "recent-clear": project.recent = []; return;
      case "save": return docs.saveActive();
      case "save-as": return void worktree.saveAs();
      case "encoding":
        if (tabs.active) overlay.encOpen = true;
        return;
      case "close-tab":
        if (tabs.active) tabflow.requestClose(tabs.active.id);
        return;
      case "close-all-tabs": return tabflow.closeMany(tabs.list.map((t) => t.id));
      case "toggle-mode":
        if (tabs.active) tabflow.requestSwitchMode(tabs.active);
        return;
      case "quick-all": overlay.openQuick("all"); return;
      case "quick-file": overlay.openQuick("file"); return;
      case "recent-files": overlay.openQuick("file"); return;
      case "quick-content": overlay.openQuick("content"); return;
      case "find-word": return overlay.findWordAtCursor();
      case "goto-line":
        // 只对编辑器有意义：日志视图有自己的行号语义，差异 / 合并没有「行」
        if (tabs.active?.mode === "edit") overlay.gotoOpen = true;
        return;
      case "nav-back": return void nav.go("back");
      case "nav-fwd": return void nav.go("fwd");
      case "outline": return overlay.openOutline();
      case "toggle-sidebar": layout.toggleSidebar(project.root !== null); return;
      case "toggle-panel": layout.panel = !layout.panel; return;
      case "toggle-scratch":
        // 已经在草稿视图上再点一次就收起侧边栏，和导轨上那个按钮同一个手势
        if (layout.sidebar && layout.sideView === "scratch") layout.sidebar = false;
        else layout.showSide("scratch");
        return;
      case "toggle-minimap": showMinimap = !showMinimap; return;
      case "zoom-in": editorFont = Math.min(28, editorFont + 1); return;
      case "zoom-out": editorFont = Math.max(9, editorFont - 1); return;
      case "zoom-reset": editorFont = FONT_DEFAULT; return;
      case "toggle-wrap": {
        const t = tabs.active;
        if (t?.mode === "edit") t.wrap = !(t.wrap ?? wrapsByDefault(t.path));
        return;
      }
      case "new-terminal": terms.open(project.root ?? "~"); return;
      case "close-terminal":
        if (terms.activeId !== null) terms.close(terms.activeId);
        return;
      case "git-changes":
        // 已经在 Git 视图上再点一次就切回去，和 ⇧⌘G 是同一个手势
        layout.toggleGitChanges();
        return;
      case "git-file-diff": {
        const en = git.activeEntry;
        if (en) void git.openDiff(en, false);
        else notify.fail("当前文件没有未提交的改动", 2600);
        return;
      }
      case "git-blame":
        git.blameOn = !git.blameOn;
        if (git.blameOn && tabs.active?.mode !== "edit") notify.ok("注解已打开，打开一个仓库里的文件就能看到", 2600);
        return;
      case "git-log": layout.openGitTab("log"); return;
      case "git-console": layout.openGitTab("console"); return;
      case "git-branches": openBranchPicker(); return;
      case "git-refresh": return void git.refresh();
      case "git-pull": return void remote.pull();
      case "git-push": return void remote.askPush();
      case "git-fetch": return void remote.fetch("fetch");
      case "help-keys": overlay.keysOpen = true; return;
      case "help-repo": return void openRepoPage();
      case "help-log": return void openAppLog();
      case "help-log-clear": return void clearLog();
      default:
        diag(`菜单项 ${id} 没有对应的处理`);
    }
  }

  /**
   * 应用已在运行时系统又送来的路径（Finder 双击、拖 Dock、`open -a`，issue #40）。
   * 目录先开（它会切项目根、关掉干净标签），文件再一个个叠上去 —— 反过来的话
   * 刚开的文件会被切项目那一步关掉。串行 `await`：`openPath` 对目录的处理有钩子，
   * 并发进去顺序就乱了。
   */
  async function openIncoming(paths: string[]) {
    const infos = await Promise.all(
      paths.map((p) => probePath(p).then((i) => [p, i.kind] as const).catch(() => [p, null] as const)),
    );
    for (const [p, kind] of infos) if (kind === "dir") await tabflow.openPath(p);
    for (const [p, kind] of infos) if (kind !== "dir") await tabflow.openPath(p);
  }

  $effect(() => {
    // 见 `project.scratchRoot` 的注释：故意不挂在启动那条 await 链上
    void scratchDir()
      .then((d) => (project.scratchRoot = d))
      .catch(() => {});
    /*
     * **先挂监听，再取启动路径。** `initial_paths` 那一次调用把 Rust 侧标成
     * 「前端就绪」，之后系统送来的路径改为直接发 `open-paths` 事件 ——
     * 监听挂在它后面的话，中间那一拍到的事件就发给了空气。
     * 动态 import 的理由同拖放那条：静态引会把 event 那串拽进入口包。
     */
    let unlisten: (() => void) | null = null;
    let dead = false;
    const ready = import("@tauri-apps/api/event")
      .then((m) =>
        m.listen<string[]>(OPEN_PATHS_EVENT, (e) => {
          // 负载不是数组就不动：桩的 `__mockMenu` 会把菜单事件广播给所有监听器
          if (Array.isArray(e.payload)) void openIncoming(e.payload);
        }),
      )
      .then((f) => {
        if (dead) f();
        else unlisten = f;
      })
      .catch(() => null);
    ready
      .then(() => initialPaths())
      .then(async (paths) => {
        if (tabs.list.length > 0 || project.root !== null) return;
        if (paths.length === 0) {
          await persist.restore();
          return;
        }
        /*
         * 有人指名了路径 —— 命令行、Finder 双击、拖到 Dock 图标、`open -a` 都走这
         * （后三种是 `RunEvent::Opened`，Rust 侧攒下来一并给的）。分两种情况：
         *
         * - 全是**文件**：先把上次的现场恢复出来，再把它们开在上面。
         *   `lite-ide a.rs` 的意思是「顺手看一眼这个文件」，不是
         *   「把我的工作区清空」—— VS Code 的 `code a.js` 就是这个行为。
         * - 有**另一个目录**：那是在切项目，旧项目的标签铺过来只会碍事。
         *   同一个目录则照常恢复。
         */
        const infos = await Promise.all(paths.map((p) => probePath(p).catch(() => null)));
        const switchingProject = infos.some((i) => i?.kind === "dir" && i.path !== saved?.root);
        if (!switchingProject) await persist.restore();
        await openIncoming(paths);
      })
      .catch(() => {})
      // 恢复完还是一个标签都没有：落在草稿里（`launchScratch`），不停在空态卡片上
      .then(() => (tabs.list.length === 0 ? tabflow.launchScratch() : undefined))
      .catch(() => {})
      .finally(() => {
        persist.restoring = false;
        persist.schedule();
        void writeBudgetLine();
      });
    return () => {
      dead = true;
      unlisten?.();
    };
  });

  /**
   * 启动完成，把预算数写进 `app.log`（issue #28）。
   *
   * **等界面真的画完再量**：`tick()` 让 Svelte 把这一轮改动刷进 DOM，
   * 再等一帧。少了这两步，`nodes` 数出来的是恢复之前那个空界面 ——
   * 一个永远不变的数，画进趋势里只会让人以为什么都没涨。
   *
   * `boot` 是 Rust 侧在收到这条命令时现算的（从进程真正的起点），
   * 所以这两帧是**算进启动耗时里的** —— 那是对的：这一行要回答的
   * 正是「双击到界面可用花了多久」，而界面没画完就不算可用。
   *
   * 量不出来就算了，一条预算数不值得在启动路径上抛任何东西。
   */
  async function writeBudgetLine() {
    try {
      await tick();
      await new Promise<void>((r) => requestAnimationFrame(() => r()));
      await reportBudget(
        tabs.list.length,
        terms.list.length,
        document.querySelectorAll(".cm-editor").length,
        document.getElementsByTagName("*").length,
      );
    } catch {
      /* 预算行写不出去不是故障 */
    }
  }

  /*
   * 拖放监听。**`@tauri-apps/api/webview` 是动态 import 的，不是顶上那一排。**
   *
   * 静态引它一个 `getCurrentWebview`，会把 webview.js + window.js + dpi.js
   * 一整串拽进入口包 —— sourcemap 归因量到 15,572 字节，而入口包是首屏之前
   * 必须解析执行完的那一段。为一个"把文件拖进来"的监听付这个价不值。
   *
   * 换成动态之后它落到自己的 chunk 里，在首屏渲染完之后才加载；
   * 拖放本来就不可能在窗口出现之前发生。
   */
  $effect(() => {
    /*
     * 立刻挂上 catch，而不是只在清理函数里挂。
     * 浏览器里跑（没有 Tauri）时这个 promise 会直接 reject，
     * 而清理函数要等 effect 销毁才跑 —— 中间这段时间就是一条
     * "Uncaught (in promise)"，把控制台的真错误淹掉。
     */
    const reg = import("@tauri-apps/api/webview")
      .then((m) =>
        m.getCurrentWebview().onDragDropEvent((e) => {
          if (e.payload.type === "over") hovering = true;
          else if (e.payload.type === "drop") {
            hovering = false;
            for (const p of e.payload.paths) void tabflow.openPath(p);
          } else hovering = false;
        }),
      )
      .catch(() => null);
    // 注销这一半也要兜住：拿到的 unlisten 函数**自己**也可能抛
    // （窗口正在拆、或者桩不完整），而它抛出来同样是一条 uncaught
    return () => void reg.then((f) => f?.()).catch(() => {});
  });

  /**
   * 菜单事件。
   *
   * `@tauri-apps/api/event` 同样走动态 import，理由和拖放那条一样 ——
   * 静态引会把它连着 core 的一串拽进入口包，而菜单在窗口出现之前
   * 一次都点不到。
   */
  $effect(() => {
    const reg = import("@tauri-apps/api/event")
      .then((m) => m.listen<string>("menu", (e) => void runMenu(e.payload)))
      .catch(() => null);
    return () => void reg.then((f) => f?.()).catch(() => {});
  });

  /**
   * 文件系统监听（issue #33 ⑳）。换项目根就重新挂；事件来了按种类刷：
   * 只有 `.git/` 变了（提交、切分支、暂存）刷 git 状态就够；工作区文件变了
   * 要重读打开的文件 + 重列目录，git 状态也跟着（新建的文件是未跟踪的）。
   *
   * 原来只有「窗口获得焦点」和 10 秒轮询两条路 —— 终端就在应用里，在终端里
   * `git checkout` 完要切出去再切回来文件树才刷。焦点那条路留着：监听起不来
   * （网络卷、权限）时它是退路。
   */
  $effect(() => {
    void watchRoot(project.root ?? "");
  });
  $effect(() => {
    const reg = import("@tauri-apps/api/event")
      .then((m) =>
        m.listen<string>("fs-changed", (e) => {
          if (e.payload === "git") {
            // 先看 config 是不是变了（issue #24），再刷状态；受限时 refresh 本来就是空转
            void git.recheckTrust().then(() => git.refresh());
          } else {
            void worktree.changed();
            void git.refresh();
          }
        }),
      )
      .catch(() => null);
    return () => void reg.then((f) => f?.()).catch(() => {});
  });

  /**
   * 最近列表变了就推给 Rust 重建子菜单。
   *
   * 只在 Tauri 里有意义（浏览器里 invoke 走的是桩），失败一律吞掉 ——
   * 菜单没刷新是件不影响干活的事，不值得弹一条错误。
   */
  $effect(() => {
    void setRecent([...project.recent]).catch(() => {});
  });

  /**
   * 让用不上的菜单项变灰。
   *
   * 这是加菜单栏白捡的：今天所有键位都是 window 级监听，**不管当下
   * 有没有意义都会触发** —— 没有标签时按 ⌘S、不是 Git 仓库时按 ⇧⌘G，
   * 都是走一遍然后什么也没发生。灰掉的菜单项本身就是一句解释。
   */
  $effect(() => {
    void syncMenuState(tabs.active !== null, git.repo !== null, terms.activeId !== null, project.root !== null).catch(() => {});
  });

  /**
   * 项目根换了就记一笔。
   *
   * 放 effect 里而不是在 `openPath` 里调，是因为 project.root 有四条来路
   * （拖放、命令行、面包屑、菜单）—— 挂在赋值点上要写四遍，
   * 而**写四遍就等于早晚漏一遍**。
   */
  $effect(() => {
    const r = project.root;
    if (!r) return;
    /*
     * **必须 untrack。**
     *
     * `remember` 里读了 `recent`（要去重、要截断），而它写的也是 `recent` ——
     * 不套 untrack 的话这条 effect 就是「读一个状态又写同一个状态」，
     * Svelte 直接抛 `effect_update_depth_exceeded`，整个内容区变成崩溃屏。
     * （第一次跑就撞上了，不是什么边角情况。）
     *
     * 这条 effect 该依赖的只有 `root` —— 项目根换了才记一笔。
     */
    untrack(() => project.remember(r));
  });

</script>

<svelte:window onkeydown={onWindowKey} onkeyup={onWindowKeyUp} />

<Overlays Branch={gitUi.comps.branch} onAction={(id) => void runMenu(id)} />

<main class:hovering>
  <TitleBar
    root={project.root}
    gitSt={git.status}
    recent={project.recent}
    branchOpen={overlay.branchOpen}
    bind:branchBtn
    onOpenRecent={(r) => void tabflow.openRecent(r)}
    onOpenFolder={() => void tabflow.openFolder()}
    onClearRecent={() => (project.recent = [])}
    onOpenBranches={openBranchPicker}
    restricted={!!git.restricted}
    onOpenTrust={() => (git.trustOpen = true)}
  />

  <div
    class="workspace"
    class:no-side={!layout.sideShown(project.root !== null)}
    class:resizing={layout.resizing}
    style:--side-w="{layout.sidebarWidth}px"
  >
    <Rail
      root={project.root}
      repo={git.repo}
      changes={git.status?.entries.length ?? 0}
      {panelTool}
      onSearch={() => {
        overlay.openQuick("content");
      }}
      onTogglePanel={(v) => layout.togglePanel(v, panelTool)}
    />

    {#if layout.sideShown(project.root !== null)}
      <!--
        侧边栏外壳在 Sidebar.svelte 里；两块内容的数据和回调还接在 App 上
        （标签表、git 动作没搬出去），所以以 snippet 传进去。
      -->
      <Sidebar repo={git.repo} gitReady={!!gitUi.comps.pane}>
        {#snippet gitPane()}
          <gitUi.comps.pane
            status={git.status}
            busy={git.busy}
            onOpenDiff={(e, staged) => void (e.conflicted ? git.openMerge(e) : git.openDiff(e, staged))}
            onStage={(paths) => void git.run("暂存失败", () => gitStage(git.repo!, paths), "暂存")}
            onUnstage={(paths) =>
              void git.run("取消暂存失败", () => gitUnstage(git.repo!, paths), "取消暂存")}
            onDiscard={(es) => (git.pendingDiscard = es)}
            onCommit={(m, amend, push) =>
              void git.commit(m, amend).then((ok) => {
                // 提交成功才推；推之前照常走推送确认条（列出要推的提交）
                if (ok && push) void remote.askPush();
              })}
            onRefresh={() => void git.refresh()}
            stashCount={git.stashes.length}
            onStash={() => void git.stashPush()}
            onUnstash={() => void git.stashPop()}
            onOpenLog={() => layout.openGitTab("log")}
            ahead={git.status?.ahead ?? 0}
            behind={git.status?.behind ?? 0}
          />
        {/snippet}
        {#snippet scratchList()}
          {#if scratchUi.comp}
            <scratchUi.comp
              activePath={tabs.active?.path ?? ""}
              onOpen={(p, keep) => void tabflow.openPath(p, { preview: !keep })}
              onNew={() => void tabflow.newScratch()}
              onTrash={(p) => void tabflow.trashScratch(p)}
              onRename={(p, stem) => worktree.renameScratch(p, stem)}
              onSaveAs={(p) => void tabflow.openPath(p).then(() => worktree.saveAs())}
              onReveal={(p) => void revealInFinder(p).catch((e) => notify.fail(String(e)))}
            />
          {/if}
        {/snippet}
        {#snippet fileTree()}
          <!-- `root!`：没项目时侧边栏整个不渲染（`layout.sideShown`），走到这儿 root 一定非空 -->
          {#if tree.comp}
          <tree.comp
            root={project.root!}
            activePath={tabs.active?.path ?? ""}
            gitStatus={git.status}
            {ignored}
            reloadTick={worktree.treeTick}
            {revealPath}
            {revealTick}
            onOpen={(p, _dir, keep) => void tabflow.openPath(p, { preview: !keep })}
            dirtyUnder={(p) => tabs.dirtyUnder(p)}
            onCreated={(p, isDir) => void afterFsChange(isDir ? null : p)}
            onRenamed={(from, to, isDir) =>
              void worktree.renameOpenTabs(from, to, isDir, git.repo).then(() => afterFsChange(null))}
            onTrashed={(p, isDir) => {
              worktree.closeTabsUnder(p, isDir);
              void afterFsChange(null);
            }}
          />
          {/if}
        {/snippet}
      </Sidebar>
    {/if}

    <section class="main">
      <!-- 编辑器岛：标签栏是它的头，在岛里 —— 和两座工具窗岛同一个结构（头 + 身） -->
      <div class="editor-island">
      {#if tabs.list.length > 0}
        <Tabs
          tabs={tabs.list}
          activeId={tabs.activeId}
          root={project.root ?? ""}
          isScratch={(p) => project.isScratch(p)}
          onSelect={(id) => {
            tabs.activeId = id;
            tabs.audit("切标签");
          }}
          onClose={(...a) => tabflow.requestClose(...a)}
          onCloseMany={(...a) => tabflow.closeMany(...a)}
          onRevealInTree={revealInTree}
          onNewScratch={(...a) => tabflow.newScratch(...a)}
          onKeep={(id) => tabs.keep(id)}
          onPin={(id, on) => tabs.setPinned(id, on)}
        />
      {/if}

      <!-- 内容区顶上的那几条确认横幅，全在 Confirms.svelte 里读各自的 store -->
      {#if confirms.comp}<confirms.comp Bars={gitUi.comps.bars} />{/if}


      <Content
        Merge={gitUi.comps.merge}
        Diff={gitUi.comps.diff}
        {showMinimap}
        outlineTick={overlay.outlineTick}
        onLogStatus={(t) => (logStatus = t)}
        onOutline={(syms) => (overlay.symbols = syms)}
      />
      </div>

      <!--
        底部工具窗在 Panel.svelte 里。提交历史那块要这边的 git lazyGroup 和活动标签，
        以 snippet 传进去（同侧边栏的两块内容）。
      -->
      {#if panelUi.comp}
        <panelUi.comp root={project.root} repo={git.repo} {panelTool} gitLogReady={!!gitUi.comps.log}>
          {#snippet gitLog()}
            <gitUi.comps.log
              repo={git.repo!}
              filePath={tabs.active?.mode === "edit" ? tabs.active.path : ""}
              onOpenCommitDiff={(sha, short, p) => void git.openCommitDiff(sha, short, p)}
              onCheckout={(sha) => branches.switchTo(sha)}
            />
          {/snippet}
        </panelUi.comp>
      {/if}
    </section>
  </div>

  <StatusBar
    active={tabs.active}
    activeScratch={tabs.active ? project.isScratch(tabs.active.path) : false}
    activeEntry={git.activeEntry}
    root={project.root}
    {logStatus}
    onReveal={revealInTree}
    onSwitchMode={() => tabflow.requestSwitchMode(tabs.active!)}
    onOpenEncoding={() => (overlay.encOpen = true)}
    onOpenDiff={() => void git.openDiff(git.activeEntry!, false)}
  />
</main>

<style>
  main {
    height: 100%;
    display: grid;
    /*
     * `minmax(0, 1fr)` 而不是 `1fr`（2026-09-17 量出来的）。`1fr` 是 `minmax(auto, 1fr)`，
     * 轨道的下限是内容的 min-content —— 于是网格每次布局都要先问一遍「主区内容有多高」，
     * 那一问会把整棵子树（工作区 → 主区 → 内容岛 → 编辑器的每一行）**重新排版一遍**。
     * 编辑器里每敲一个键都让 `.cm-line` 变脏，这一问就跟着来一次：`sample` 到的 WebContent
     * 栈里 75% 的按键时间在 `GridTrackSizingAlgorithm → logicalHeightForGridItem → 布局`，
     * 粘了 2000 行日志的草稿里每键 9–18ms 全是它。下限写成 0，轨道高度只由窗口决定，
     * 内容变了只重排它自己那一小块。`.workspace` 的列同理。
     */
    grid-template-rows: 38px minmax(0, 1fr) 24px;
    /*
     * **不要在这儿画底。** 窗口的底是 Rust 侧挂的那块 NSVisualEffectView，
     * 这里填任何不透明色都会把它整块盖住 —— 表现是「vibrancy 没生效」，
     * 而 Rust 侧一切正常，从那头查不出来。
     * 该挡光的是内容层（编辑器 / 日志 / 终端），它们各自画自己的。
     */
    background: transparent;
  }
  main.hovering { outline: 2px solid var(--accent); outline-offset: -2px; }



  /*
   * # 浮岛（M8，2026-09-16）
   *
   * 外壳之间**不画线**。原来三条竖线四条横线两种亮度混用，侧边栏头 30px 和
   * 标签栏 38px 的横线在竖线两侧差 8px 接不上，而内容层本来就比外壳深一档，
   * 边上再压一条亮线等于同一条边说两遍。玻璃材质上用线分区本身就在和材质打架。
   *
   * 现在边界由「面」的形状表达：内容层（编辑器 / 底部工具窗）收成内缩 6px 的
   * 圆角岛（`--island-*`），岛与岛之间那条 6px 的缝就是分隔，也正好是拖拽热区。
   * 设计稿见 design/m8-chrome。
   */
  .workspace {
    display: grid;
    /* 四列：常驻竖条 · 侧边栏 · 拖拽条（也是岛的左缝）· 主区 */
    grid-template-columns: 34px var(--side-w, 240px) var(--island-gap) minmax(0, 1fr);
    overflow: hidden;
    transition: grid-template-columns 0.13s ease;
  }
  /* 拖拽时不要过渡，否则是一路追不上手的橡皮筋 */
  .workspace.resizing { transition: none; }
  @media (prefers-reduced-motion: reduce) { .workspace { transition: none; } }
  /* 收起侧边栏只去掉中间两列，竖条留着 —— 按钮的位置不能动 */
  .workspace.no-side { grid-template-columns: 34px minmax(0, 1fr); }
  /* 侧边栏收起后那条拖拽列也没了，岛的左缝由 main 自己补 */
  .workspace.no-side .main { padding-left: var(--island-gap); }


  /*
   * 用 flex 列而不是 grid：这一列里的元素是**条件渲染**的（标签栏、三种确认条、
   * 拖拽条、终端面板都可能不在），固定行数的 grid 会让后面的元素往前占位 ——
   * 曾经导致终端面板抢到 1fr 跑到内容区上面去。
   * flex 天然按实际存在的元素排布，content 吃掉剩余空间就行。
   */
  .main {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    /* 上缝、右缝、下缝：岛不贴标题栏、不贴窗口边、不贴状态栏。上缝让标签栏和侧边栏岛的头（也是 38px）落在同一条线上 */
    padding: var(--island-gap) var(--island-gap) var(--island-gap) 0;
  }
  /*
   * 编辑器岛 = 标签栏 + 内容（2026-09-18）。原来标签栏在岛外、属于外壳，而底部工具窗
   * 的头在岛里 —— 同一个窗口里两种「头在哪」。现在三座岛一个结构：头（38px）+ 身。
   * 岛的描边 / 圆角 / 底色在这一层，Content 只管裁自己。
   */
  .editor-island {
    position: relative; /* 确认卡片浮在它里面（Confirms.svelte） */
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--content-bg);
    border: var(--island-border);
    border-radius: var(--island-radius);
  }




</style>
