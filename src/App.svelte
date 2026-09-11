<script lang="ts">
  import { untrack, tick } from "svelte";
  import { Channel } from "@tauri-apps/api/core";
  import FileTree from "./lib/shell/FileTree.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import type { Action } from "./lib/search/QuickSearch.svelte";
  import type { JumpHit } from "./lib/editor/jump";
  import { lazy, lazyGroup } from "./lib/lazy/lazy.svelte";
  import { notify } from "./lib/state/notify.svelte";
  import * as session from "./lib/state/session";
  import { textToSave, settled, stashed } from "./lib/state/doc";
  import { audit } from "./lib/state/invariant";
  import { isLogName } from "./lib/logview/is-log-name";
  import Crash from "./lib/shell/Crash.svelte";
  import Icon from "./lib/shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "./lib/shell/ContextMenu.svelte";
  import { KEYS, byId as keyById } from "./lib/state/keymap";
  import type { Sym } from "./lib/editor/outline";
  import type { ChangeKind } from "./lib/git/diff";
  import {
    probePath,
    ignoredDirs,
    appLogPath,
    clearAppLog,
    readText,
    pickFolder,
    setRecent,
    syncMenuState,
    openExternal,
    gitFetch,
    gitPush,
    gitMergeUpstream,
    gitCancel,
    gitOutgoing,
    type RemoteProgress,
    type RemoteErr,
    type SwitchErr,
    diag,
    writeText,
    fileStamp,
    type Stamp,
    openLog,
    closeLog,
    reportBudget,
    devtoolsBuild,
    initialPath,
    gitRoot,
    gitStatus,
    gitDiff,
    gitStage,
    gitUnstage,
    gitDiscard,
    gitCommit,
    gitCommitDiff,
    gitSwitch,
    gitWorktreeAdd,
    gitWorktreeRemove,
    detectEncoding,
    listProjectFiles,
    scratchDir,
    createScratch,
    discardEmptyScratch,
    type GitEntry,
    type GitStatus,
    type GitWorktree,
  } from "./lib/ipc/commands";

  interface TabState {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log" | "diff" | "merge";
    dirty: boolean;
    /** log 模式的引擎句柄 */
    handle?: number;
    /** edit 模式打开时的磁盘内容 —— **dirty 的基线**，不是编辑器里的实时文本 */
    content?: string;
    /**
     * 未保存的实时文本。只有改过才有。
     *
     * 为什么要单独存一份：编辑器是 `{#key active.id}` 包着的，切标签就销毁重建，
     * 而重建时拿的是这里的字段。以前只有 `content` 一个字段，编辑器里的改动
     * 从来没回写过 —— 切走再切回来，改动和「有未保存改动」的标记**一起**消失，
     * 人完全察觉不到自己丢了东西。
     *
     * 存两份而不是一份，是因为 dirty 要靠「实时文本 ≠ 磁盘那份」算出来；
     * 只留一个字段的话基线会被草稿顶掉，标记就再也亮不起来了。
     */
    draft?: string;
    /** 被判为 log 模式的原因 */
    reason?: string;
    /** 文件字节数，用于判断切到编辑模式是否有风险 */
    size: number;
    /** 用户手动指定过模式；自动判定只是默认值，不该是死判决 */
    forced?: "edit" | "log";
    /** 打开或保存时的文件指纹，用来发现外部改动 */
    stamp?: Stamp;
    /** 外部改动了，但本地也有未保存改动 —— 需要用户裁决 */
    conflict?: boolean;
    /** 差异标签：相对仓库根的路径 */
    rel?: string;
    /** 看的是暂存区还是工作区 */
    diffStaged?: boolean;
    diffUntracked?: boolean;
    diffRaw?: string;
    /** 差异被 Rust 侧的 1MB 上限掐断了，界面要说出来 */
    diffCapped?: boolean;
    /** 非空表示这是「某次提交里的差异」，只读历史，不是工作区 */
    diffSha?: string;
    diffShort?: string;
    /** 冲突标签：带冲突标记的工作区原文 */
    mergeText?: string;
    /**
     * 文件编码标签（WHATWG，如 `UTF-8` / `GBK`）。
     * 读进来是什么就用什么存回去 —— 保存不该顺手改变文件的编码。
     */
    encoding?: string;
    bom?: boolean;
    /** 解码时有解不出的字节；带着它保存会把那些字节永久换成 U+FFFD */
    lossy?: boolean;
  }

  /**
   * 手动切到编辑模式时，超过这个大小要先确认。
   * 编辑模式会把全文读进内存并交给 CodeMirror，大文件是真的会卡。
   */
  const CONFIRM_EDIT_BYTES = 8 << 20;

  /**
   * 上次退出时的现场。**同步读一次**，不放进 effect ——
   * 布局要用它做 `$state` 的初值，晚一拍读就会看见侧边栏从 240 跳到
   * 上次的宽度，那一下闪比不恢复还难受。
   *
   * 读不出来（第一次跑、隐私模式、数据被清、存的是坏数据）就是 null，
   * 一切照默认走。`session.parse` 保证不抛。
   */
  const saved = (() => {
    try {
      return session.parse(localStorage.getItem(session.KEY));
    } catch {
      return null;
    }
  })();
  const savedLayout = saved?.layout ?? session.DEFAULT_LAYOUT;

  /**
   * 每个文件上次停在第几行。
   *
   * **刻意不做成 `$state`**：它在编辑时每换一行就写一次，做成响应式等于
   * 每换行都惊动一次渲染，而界面上没有任何地方要显示它 —— 它只在存快照
   * 和恢复时被读。普通 Map 就够。
   */
  const posByPath = new Map<string, number>();
  /**
   * 还没兑现的恢复位置。标签被恢复出来时不能立刻跳 ——
   * 那时组件还没挂上。等它第一次成为活动标签再跳，跳完就从这里删掉，
   * 否则之后每次切回这个标签都会被拽回那一行。
   */
  const pendingPos = new Map<string, number>();

  let root = $state<string | null>(null);
  let tabs = $state<TabState[]>([]);
  let activeId = $state<number | null>(null);
  let nextId = 1;

  /**
   * 在一个状态转换点上核一遍标签的不变量（issue #27，判据全在
   * `state/invariant.ts` 里，这里只负责「在哪些点上核」）。
   *
   * **显式调用，不做成 `$effect`。** effect 会跟着 `tabs` 里任何一个字段动 ——
   * 包括每敲一个键就翻一次的 `dirty`，那就成了热路径上的自检，
   * 正是 issue 里点名不能做的事。转换点一共就这五个，写清楚比自动触发好查。
   *
   * 第三个参数是「此刻哪个标签挂着活编辑器」：编辑器的 onChange 只改 `dirty`，
   * `draft` 要等 onStash 才回写，所以正在被编辑的那个标签本来就会
   * 短暂地 dirty 而无 draft —— 不告诉自检器这件事，它会在每次敲键盘时报假警。
   */
  function auditTabs(where: string) {
    audit(tabs, activeId, activeId, where);
  }

  /**
   * 缩略图开关。存 localStorage —— 这是个纯偏好，没必要为它建一套配置文件；
   * 读失败（隐私模式、站点数据被清）就用默认值，不能让它把启动流程炸掉。
   */
  let showMinimap = $state(readPref("minimap", true));
  $effect(() => {
    try {
      localStorage.setItem("lite-ide.minimap", showMinimap ? "1" : "0");
    } catch {
      /* 存不下就算了，下次开还是默认值 */
    }
  });

  function readPref(key: string, dflt: boolean): boolean {
    try {
      const v = localStorage.getItem(`lite-ide.${key}`);
      return v === null ? dflt : v === "1";
    } catch {
      return dflt;
    }
  }

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
   * 跟着项目根和 `treeTick` 走。
   *
   * 带上 `treeTick` 是因为 `.gitignore` 本身是可以改的 —— 改完走一次
   * `workingTreeChanged()`（切分支、丢弃改动、从终端切回来都会），
   * 这份答案就跟着更新。不带的话，改完 `.gitignore` 得重开项目才生效。
   */
  $effect(() => {
    const r = root;
    treeTick;
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

  /** 文件树刷新计数，由 workingTreeChanged() 推进 */
  let treeTick = $state(0);

  let sidebar = $state(savedLayout.sidebar);
  let sidebarWidth = $state(savedLayout.sidebarWidth);
  /** 侧边栏当前显示哪个视图。不在仓库里时强制回文件树 */
  let sideView = $state<"files" | "git">(savedLayout.sideView);

  // ─────────────────────────── Git ───────────────────────────

  /** 项目所属仓库的根；不是仓库就是 null，整块 Git 功能随之隐身 */
  let repo = $state<string | null>(null);
  let gitSt = $state<GitStatus | null>(null);
  let gitBusy = $state(false);
  /** 待确认丢弃的条目 —— 丢弃不可撤销，必须过用户这一关 */
  let pendingDiscard = $state<GitEntry[] | null>(null);

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
  const git = lazyGroup(
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
      (sideView === "git" && !!repo) ||
      tabs.some((t) => t.mode === "diff" || t.mode === "merge") ||
      (panel && panelTool === "log") ||
      branchOpen;
    if (need) git.load();
  });

  /** 分支 / 工作树选择器 */
  let branchOpen = $state(false);

  // ─────────────────────────── 编码 ───────────────────────────

  let encOpen = $state(false);
  const encPicker = lazy(() => import("./lib/encoding/EncodingPicker.svelte"), "编码选择器");
  /*
   * 速查表是「忘了才看」的东西，一次都不点开也很正常 ——
   * 让它在首屏之前被解析执行不划算。判据同 ARCHITECTURE 那条：
   * 问一句「这东西在窗口出现之前有用吗」。
   */
  const keysPanel = lazy(() => import("./lib/search/Keys.svelte"), "快捷键速查");
  $effect(() => {
    if (encOpen) encPicker.load();
  });

  /** 按新编码重新解码当前文件 */
  async function reopenWith(label: string) {
    const tab = active;
    if (!tab) return;
    try {
      if (tab.mode === "log") {
        // 日志模式只是换个 TextDecoder 标签，不用重开句柄
        tab.encoding = label;
        return;
      }
      if (tab.dirty) {
        notify.fail("有未保存的改动，请先保存（⌘S）再换编码重新打开", 3000);
        return;
      }
      const t = await readText(tab.path, label);
      tab.content = t.content;
      tab.encoding = t.encoding;
      tab.bom = t.bom;
      tab.lossy = t.lossy;
      savedTick++;
      notify.ok(`已按 ${t.encoding} 重新打开${t.lossy ? "（仍有解不出的字节）" : ""}`, 3000);
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /** 只改「将来存成什么编码」，不动当前内容 */
  function saveAsEncoding(label: string, bom: boolean) {
    const tab = active;
    if (!tab || tab.mode !== "edit") return;
    tab.encoding = label;
    tab.bom = bom;
    // 内容没变但目标编码变了，得让用户知道要按 ⌘S 才会真的落盘
    tab.dirty = true;
    notify.ok(`下次保存将写成 ${label}${bom ? " + BOM" : ""}，按 ⌘S 生效`, 3600);
  }

  /**
   * 被本地改动挡住的那次切换。**不是错误，是「你得先决定怎么办」。**
   *
   * git 的原话是 "Please commit your changes or stash them before you switch
   * branches" —— 而提交和丢弃这两条路界面上都有，把英文原话贴给用户等于
   * 让他自己去开终端。`gitsvc::Error::LocalChanges` 把挡路的文件切出来了，
   * 这里据此给按钮。
   */
  let pendingCheckout = $state<{
    name: string;
    create: boolean;
    files: string[];
  } | null>(null);

  function switchBranch(name: string, create = false) {
    notify.closeBanner();
    pendingCheckout = null;
    // base 要在切之前抓 —— 切完 gitSt.branch 就是新的那个了
    const base = gitSt?.branch ?? "";
    void gitDo(create ? "新建分支失败" : "切分支失败", async () => {
      try {
        await gitSwitch(repo!, name, create);
      } catch (e) {
        const err = e as SwitchErr;
        if (err?.kind === "local-changes" && err.files?.length) {
          pendingCheckout = { name, create, files: err.files };
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
      await refreshGit();
      const now = gitSt?.branch || name;
      const up = gitSt?.upstream ? `（跟踪 ${gitSt.upstream}）` : "";
      notify.ok(create ? `已从 ${base} 新建并切到 ${now}` : `已切到 ${now}${up}`, 2800);
      await workingTreeChanged();
    }, create ? "新建分支" : "切分支");
  }

  /** 丢掉挡路的那几个改动，然后把刚才那次切换重放一遍 */
  async function discardThenCheckout() {
    const p = pendingCheckout;
    if (!p || !repo) return;
    pendingCheckout = null;
    const es = (gitSt?.entries ?? []).filter((e) => p.files.includes(e.path));
    await doDiscard(es);
    switchBranch(p.name, p.create);
  }

  /**
   * 打开一个工作树 = **把项目根换过去**。
   *
   * `openPath` 对目录只做 `root = path`，**打开的标签一个都不动** ——
   * 于是文件树和 Git 栏切到了新工作树，而标签还指着旧的那份。
   * 这不是 bug（开着别处的文件是合法的），但一声不吭就变了半个界面，
   * 人会以为「怎么点了没反应」。做完说一句。
   */
  async function openWorktree(path: string) {
    await openPath(path);
    const name = path.slice(path.lastIndexOf("/") + 1) || path;
    notify.ok(`项目根已切到 ${name}（打开的标签没有动）`, 3200);
  }

  function newWorktree(dir: string, branch: string) {
    void gitDo("新建工作树失败", async () => {
      // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
      const path = await gitWorktreeAdd(repo!, dir, branch);
      notify.ok(`工作树已建在 ${path}`, 3600);
      await openPath(path);
    }, "新建工作树");
  }

  /** 待确认移除的工作树 —— 会删目录，必须过用户这一关 */
  let pendingWtRemove = $state<GitWorktree | null>(null);

  function doRemoveWorktree(w: GitWorktree, force: boolean) {
    pendingWtRemove = null;
    void gitDo("移除工作树失败", async () => {
      await gitWorktreeRemove(repo!, w.path, force);
      notify.ok(`已移除工作树 ${w.path}`);
      await workingTreeChanged();
    }, "移除工作树");
  }

  /**
   * 当前编辑文件相对 HEAD 的改动行，喂给编辑器缩略图。
   *
   * 数据源是 `git diff` 而不是自己在前端算：算法现成的，而且和差异视图
   * 用的是同一份输出，两处显示不会打架。
   *
   * 已知的不足：标记反映的是**磁盘上那份**。编辑器里改了还没存时，标记不会跟着动 ——
   * 要做到 IDEA 那种实时跟随，得拿 HEAD 版本在前端跑一遍 diff，那是另一件事。
   * 保存之后 refreshGit 会把它带新。
   */
  let editorMarks = $state<Map<number, ChangeKind> | null>(null);

  $effect(() => {
    const tab = active;
    const st = gitSt;
    const r = repo;
    if (!tab || tab.mode !== "edit" || !r || !st) {
      editorMarks = null;
      return;
    }
    const prefix = `${st.root}/`;
    if (!tab.path.startsWith(prefix)) {
      editorMarks = null;
      return;
    }
    const rel = tab.path.slice(prefix.length);
    const e = st.entries.find((x) => x.path === rel);
    // 干净的文件不用跑 diff；未跟踪的文件整份都是新的，标满一屏没有信息量
    if (!e || e.untracked) {
      editorMarks = null;
      return;
    }
    // 动态引入：静态引会把整个 diff 解析模块（约 7KB）拽进入口包，
    // 而它只在「打开了一个仓库里被改过的文件」时才用得上。
    // 动态引之后它和 Git 那几个组件共用同一个按需块，一次都不会白加载。
    void Promise.all([gitDiff(r, rel, false, false), import("./lib/git/diff")])
      .then(([d, m]) => (editorMarks = m.changedLines(d.text)))
      .catch(() => (editorMarks = null));
  });

  /** 从日志里打开某次提交中某个文件的差异 */
  async function openCommitDiff(sha: string, short: string, rel: string) {
    if (!repo) return;
    const key = `git-commit:${sha}:${rel}`;
    let id = tabs.find((t) => t.path === key)?.id;
    if (id === undefined) {
      id = nextId++;
      tabs = [
        ...tabs,
        {
          id,
          path: key,
          name: rel.slice(rel.lastIndexOf("/") + 1),
          mode: "diff",
          dirty: false,
          size: 0,
          rel,
          diffSha: sha,
          diffShort: short,
        },
      ];
    }
    activeId = id;
    await reloadDiff(id);
  }

  /** 换项目根就重新找仓库。找不到时把 Git 的一切都清干净 */
  $effect(() => {
    const r = root;
    if (!r) {
      repo = null;
      gitSt = null;
      return;
    }
    gitRoot(r)
      .then((found) => {
        repo = found;
        if (!found) {
          gitSt = null;
          sideView = "files";
        } else {
          void refreshGit();
        }
      })
      .catch(() => {
        repo = null;
        gitSt = null;
      });
  });

  /**
   * 刷新一次 git 状态。
   *
   * 触发点是「窗口获得焦点」「保存之后」「做完任一 git 动作」，不是定时轮询 ——
   * 每次都要起一个 git 子进程（约 5–15ms），常年轮询是白烧电。
   * 用户在终端里 commit 完切回来，焦点事件正好把状态带新。
   */
  async function refreshGit() {
    const r = repo;
    if (!r) return;
    gitBusy = true;
    try {
      gitSt = await gitStatus(r);
      // 打开着的工作区差异跟着更新，否则暂存完还停在旧内容上。
      // 历史提交的差异是不变的，重拉纯属浪费一次子进程
      await Promise.all(
        tabs.filter((t) => t.mode === "diff" && !t.diffSha).map((t) => reloadDiff(t.id)),
      );
    } catch (e) {
      notify.fail(String(e), 4000);
    } finally {
      gitBusy = false;
    }
  }

  /**
   * 按 id 取标签，**必须从 `tabs` 里拿**。
   *
   * `tabs` 是 `$state`，数组里的元素在读取时被包成代理。往创建时那个
   * 原始对象上写（`const tab = {...}; tabs = [...tabs, tab]; tab.x = 1`）
   * 确实改得动底层数据，但**不会产生任何信号**，界面不会重渲染 ——
   * 差异面板因此一直停在「没有差异」，直到别的操作碰巧引起一次重绘。
   * 异步流程尤其容易踩：await 回来时手上那个引用早已不是响应式的那一份。
   */
  /**
   * 盘上的文件被改了 —— 不是被我们改的。
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
  async function workingTreeChanged() {
    await checkExternalChanges();
    treeTick++;
  }

  /**
   * 一个标签是不是「在 p 底下」。目录要连子树一起算 ——
   * 改名或删掉一个目录，里面开着的每个文件都受影响。
   */
  function underPath(tabPath: string, p: string, isDir: boolean) {
    return tabPath === p || (isDir && tabPath.startsWith(`${p}/`));
  }

  /** 传给文件树：这条路径底下有几个未保存的标签（删除确认框要说清楚） */
  function dirtyUnder(p: string): number {
    return tabs.filter((t) => t.dirty && underPath(t.path, p, true)).length;
  }

  /**
   * 在文件树里改完名，打开着的标签要跟着走。
   *
   * 少了这一步的表现是：标签还挂着旧名字，按 ⌘S 报「文件不在盘上了」——
   * 而名字是人刚刚亲手改的，最不会去怀疑的就是这件事。
   */
  async function renameOpenTabs(from: string, to: string, isDir: boolean) {
    const moved: number[] = [];
    for (const t of tabs) {
      if (!underPath(t.path, from, isDir)) continue;
      const np = to + t.path.slice(from.length);
      // 位置记忆的 key 也是路径，一起搬 —— 不搬的话切回这个文件会跳回第一行
      const pos = posByPath.get(t.path);
      if (pos !== undefined) {
        posByPath.delete(t.path);
        posByPath.set(np, pos);
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
      const before = tabById(id);
      if (!before || before.mode !== "log" || before.handle === undefined) continue;
      const stale = before.handle;
      try {
        const h = (await openLog(before.path)).handle;
        // await 回来必须按 id 重新取一次 —— 手上那个引用可能已经不是
        // 响应式的那一份了（AGENTS.md 里那条 $state 数组的坑）
        const now = tabById(id);
        if (now) now.handle = h;
        void closeLog(stale);
      } catch (e) {
        notify.fail(`${before.name} 改名后重开日志失败，tail 会停：${String(e)}`);
      }
    }
  }

  /** 进废纸篓的东西，开着的标签一并关掉（确认框已经说过会关几个未保存的） */
  function closeTabsUnder(p: string, isDir: boolean) {
    for (const t of tabs.filter((t) => underPath(t.path, p, isDir))) doClose(t);
  }

  /**
   * 文件树里改完盘之后的收尾。
   *
   * 三件事一起做：重读打开的文件 + 重列目录（`workingTreeChanged`），
   * 再刷一次 git 状态 —— 新建出来的文件是未跟踪的，删掉的要显示成 D，
   * 少这一下文件树上的染色就停在改动之前。
   */
  async function afterFsChange(openThis: string | null) {
    if (openThis) await openPath(openThis);
    await workingTreeChanged();
    void refreshGit();
  }

  function tabById(id: number): TabState | null {
    return tabs.find((t) => t.id === id) ?? null;
  }

  async function reloadDiff(id: number) {
    const tab = tabById(id);
    if (!tab || !repo || tab.mode !== "diff" || !tab.rel) return;
    try {
      if (tab.diffSha) {
        const d = await gitCommitDiff(repo, tab.diffSha, tab.rel);
        tab.diffRaw = d.text;
        tab.diffCapped = d.truncated;
        return;
      }
      let d = await gitDiff(repo, tab.rel, !!tab.diffStaged, !!tab.diffUntracked);
      /*
       * 这一侧空了，就看看另一侧有没有东西。
       *
       * 典型情形：差异标签开着，用户在改动列表里把这个文件暂存了 ——
       * 改动跑去了暂存区，工作区侧变空，标签上就只剩一句「没有差异」，
       * 看着像坏了。其实内容还在，只是换了一边。自动跟过去。
       */
      if (!d.text.trim()) {
        const other = await gitDiff(repo, tab.rel, !tab.diffStaged, false);
        if (other.text.trim()) {
          tab.diffStaged = !tab.diffStaged;
          tab.diffUntracked = false;
          d = other;
        }
      }
      tab.diffRaw = d.text;
      tab.diffCapped = d.truncated;
    } catch (e) {
      tab.diffRaw = "";
      tab.diffCapped = false;
      notify.fail(String(e));
    }
  }

  /**
   * 打开冲突合并标签。
   *
   * 读的是**工作区文件**而不是 `git show :2:` / `:3:` 那三个暂存位 ——
   * 工作区那份才是用户此刻真正会提交的东西，他可能已经手改过一部分，
   * 从暂存位重建会把那些手改悄悄抹掉。
   */
  async function openMerge(e: GitEntry) {
    if (!repo) return;
    const full = `${repo}/${e.path}`;
    const key = `git-merge:${e.path}`;
    try {
      const content = (await readText(full)).content;
      let id = tabs.find((t) => t.path === key)?.id;
      if (id === undefined) {
        id = nextId++;
        tabs = [
          ...tabs,
          {
            id,
            path: key,
            name: e.path.slice(e.path.lastIndexOf("/") + 1),
            mode: "merge",
            dirty: false,
            size: 0,
            rel: e.path,
            mergeText: content,
          },
        ];
      } else {
        const t = tabById(id);
        if (t) t.mergeText = content;
      }
      activeId = id;
    } catch (err) {
      notify.fail(String(err));
    }
  }

  /**
   * 冲突解决完写回文件；全部决定完的才 git add 标记已解决。
   *
   * 占锁（issue #23）：`gitStage` 动 index，而这条路原来在守卫外面 ——
   * 解决冲突的场合**尤其**容易撞上，那时人往往同时开着终端在跑 `git status`
   * 或另一次 `git add`。
   *
   * 写文件那一步也圈在里面：它和紧跟着的 `gitStage` 必须是一件事，
   * 中间插进一次别的写操作，暂存的就是半份内容。
   */
  async function resolveMerge(tab: TabState, content: string, resolved: boolean) {
    if (!repo || !tab.rel) return;
    if (!claimGit(resolved ? "标记为解决" : "保存冲突进度")) return;
    try {
      await writeText(`${repo}/${tab.rel}`, content, tab.encoding);
      if (resolved) {
        await gitStage(repo, [tab.rel]);
        notify.ok(`${tab.name} 已标记为解决`);
        doClose(tab);
      } else {
        tab.mergeText = content;
        notify.ok(`${tab.name} 进度已保存`);
      }
      await refreshGit();
    } catch (e) {
      notify.fail(String(e));
    } finally {
      releaseGit();
    }
  }

  /** 打开（或复用）一个差异标签 */
  async function openDiff(e: GitEntry, staged: boolean) {
    if (!repo) return;
    const key = `git-diff:${e.path}`;
    let id = tabs.find((t) => t.mode === "diff" && t.path === key)?.id;
    if (id === undefined) {
      id = nextId++;
      tabs = [
        ...tabs,
        {
          id,
          path: key,
          name: e.path.slice(e.path.lastIndexOf("/") + 1),
          mode: "diff",
          dirty: false,
          size: 0,
          rel: e.path,
        },
      ];
    }
    // 从数组里重新取一次，拿到的才是响应式的那份
    const tab = tabById(id);
    if (!tab) return;
    tab.diffStaged = staged;
    tab.diffUntracked = e.untracked && !staged;
    activeId = id;
    await reloadDiff(id);
  }

  /** 差异标签上切换「已暂存 ↔ 未暂存」 */
  async function toggleDiffSide(id: number) {
    const tab = tabById(id);
    if (!tab) return;
    tab.diffStaged = !tab.diffStaged;
    // 未跟踪文件一旦进了暂存区，就该按普通 diff 读，不能再走 --no-index
    const e = gitSt?.entries.find((x) => x.path === tab.rel);
    tab.diffUntracked = !!e?.untracked && !tab.diffStaged;
    await reloadDiff(id);
  }

  /**
   * 正在跑的那个**写**操作叫什么（`gitBusy` 是另一件事：它指「正在刷新状态」）。
   *
   * **守卫看它，不看 `notify.doing`** —— 后者要等 300ms 才亮（见下面），
   * 那段空窗期里守卫会形同虚设。
   */
  let gitWriting = $state<string | null>(null);

  /**
   * 占住「这个仓库正在被写」这件事。占得到返回 true。
   *
   * # 为什么要从 `gitDo` 里抽出来（issue #23）
   *
   * 这道守卫原来长在 `gitDo` 里面，于是它只盖住走 `gitDo` 的那些路径。
   * 拉取、推送、解决冲突这三条**在外面** —— 前两条有自己的进度条和取消
   * （`syncing`），第三条直接调 `gitStage`。结果是：
   *
   * > pre-commit 钩子跑三十秒的时候，拉取按钮仍然可以点。
   *
   * 而那正是守卫要挡的场景 —— 命令挪到阻塞池之后它们之间不再由主线程串行，
   * 两条 git 撞上 `index.lock` 是真会发生的（issue #11 记着这个回归点）。
   * 撞上之后用户看到的是 git 的英文报错，而 issue #15 刚把这类东西翻译掉。
   *
   * 抽出来之后这个信号的语义也更准了：它说的是**「这个仓库现在有人在写」**，
   * 不是「`gitDo` 在跑」。
   *
   * **`git fetch` 不占**：它不碰 index（写的是 refs 和 FETCH_HEAD），
   * 和 commit 用的不是同一把锁。为了对称而把它也挡住，只会让
   * 「钩子跑着的时候连拉一下都不行」——挡住的是一件本来不会出事的事。
   * 但 `doPull` **整体**要占，它末尾那次合并是真的动 index。
   */
  function claimGit(doing: string): boolean {
    if (gitWriting) {
      /*
       * **不能走 `notify.fail`。** 状态栏左槽是 `{#if doing}{:else if info}
       * {:else if error}`，而 doing 排在最前面 —— 慢操作正是 doing 亮着的
       * 时候，那句 fail 写进去也显示不出来，4 秒后还被自己的定时器清掉。
       * 于是用户看到的仍然是「点了没反应」，正是这道守卫要避免的东西。
       *
       * 直接改 doing 的文案：渲染是「正在${doing}…」，这里拼出来就是
       * 「正在提交，请等它做完…」。`gitWriting` 存的是原始动作名，不会被
       * 这句话污染，所以点第三次、第四次文案也不会越接越长。
       * 操作结束时占用方的 `finally` 会清掉它，不用另设一个定时器。
       */
      notify.doing = `${gitWriting}，请等它做完`;
      return false;
    }
    gitWriting = doing;
    return true;
  }

  /**
   * 放开。**每个 `claimGit` 都必须有一个配对的、在 `finally` 里的这句。**
   *
   * 顺带把 `notify.doing` 清掉 —— 这一句是被守卫挡下来的那次调用写进去的
   * （「正在提交，请等它做完」），而**写它的那次调用已经 return 了，
   * 没有人会来清**。只有占着锁的那一方知道什么时候该收场。
   *
   * 漏了这句的表现：拉取被挡一次之后，状态栏左槽永远挂着
   * 「正在合并上游，请等它做完…」，连当前打开的是哪个文件都被它盖住 ——
   * 浏览器里实测到的，三秒后仍在。`gitDo` 一直是对的（它自己 finally 里
   * 清了），错的是新收进来的那三条。放进 `releaseGit` 就不会再漏一条。
   */
  function releaseGit() {
    gitWriting = null;
    notify.doing = "";
  }

  /**
   * git 写操作的统一出口：做完刷新状态，失败统一收口（issue #15）。
   *
   * `doing` 是**正在做的那件事的名字**，不是可选的装饰 —— 这几条命令全都
   * 被有意挪到了阻塞池上（`git_commit` 因为 pre-commit 钩子跑什么是仓库
   * 说了算，跑一遍 eslint 三十秒；`git_switch` 检出几千个文件是秒级，
   * 见 rules/rust.md 那张表）。原来这段时间界面**什么都不显示**，
   * 和「点了没反应」长得一模一样 —— 而那正是让人反复点的形状。
   *
   * 顺带把「反复点」真的挡住了：一次只允许一个写操作。挪到阻塞池之后
   * 命令之间不再由主线程串行，两条 git 撞上 `index.lock` 是真会发生的
   * （issue #11 里专门记着这个回归点）。这里挡住，就不用等 git 报错再翻译。
   */
  async function gitDo(what: string, fn: () => Promise<unknown>, doing: string) {
    if (!repo) return;
    if (!claimGit(doing)) return;
    /*
     * **慢的才说话。**
     *
     * 暂存一个文件通常不到 100ms，那种一闪而过的字比不显示更让人分心 ——
     * 眼角瞥见状态栏动了一下，回头看又没了。300ms 是「人开始觉得卡」的
     * 那条线：比它快的操作当作瞬时，比它慢的才需要一句「我在做」。
     */
    const tip = setTimeout(() => (notify.doing = doing), 300);
    try {
      await fn();
      await refreshGit();
    } catch (e) {
      notify.block(what, e);
    } finally {
      // **三件事都必须在 finally 里。** 失败路径上漏掉定时器，300ms 后
      // 会亮起一句永远不灭的「正在提交…」；漏掉 gitWriting，后面所有写操作
      // 都会被上面那道守卫挡下来
      clearTimeout(tip);
      notify.doing = "";
      releaseGit();
    }
  }

  async function doDiscard(entries: GitEntry[]) {
    pendingDiscard = null;
    if (!repo) return;
    // 跟踪的走 git restore，未跟踪的只能直接删 —— gitsvc 里分了两条路
    const tracked = entries.filter((e) => !e.untracked).map((e) => e.path);
    const untracked = entries.filter((e) => e.untracked).map((e) => e.path);
    await gitDo(
      "丢弃失败",
      async () => {
        await gitDiscard(repo!, tracked, untracked);
        // **只有这一条补了成功回执，暂存/取消暂存没补。**
        // 判据是「结果看不看得见」：暂存之后文件当场移到已暂存区，
        // 界面自己说清楚了，再弹一句是噪声；而丢弃是不可逆的那一档，
        // 文件直接从改动列表里消失，不说一句就分不清「丢掉了」和「没点中」。
        notify.ok(`已丢弃 ${entries.length} 个文件的改动`, 3000);
      },
      "丢弃改动",
    );
    await workingTreeChanged();
  }

  function doGitCommit(message: string, amend: boolean) {
    void gitDo("提交失败", async () => {
      const out = await gitCommit(repo!, message, amend);
      notify.ok(out.split("\n")[0] || "已提交", 3000);
    }, "提交");
  }
  let panel = $state(savedLayout.panel);
  let panelHeight = $state(savedLayout.panelHeight);
  /** 底部面板当前是哪个工具窗。终端实例永不卸载，只是藏起来 */
  let panelView = $state<"term" | "log" | "git">(savedLayout.panelView);
  /**
   * 实际在渲染的那个工具窗。
   *
   * `panelView` 是**存下来的偏好**，它可以是 `log` 而当下并没有仓库 ——
   * 上次在一个 git 仓库里看着提交历史退出，这次打开的是个普通文件夹。
   * 那时面板头写着「提交历史」，底下却是一片空白（历史那块的渲染条件
   * 带着 `&& repo`），而头上已经没有「切回终端」的按钮了（切换搬去了导轨）。
   *
   * 所以渲染一律看这个，写状态才写 `panelView` —— 偏好留着，
   * 下次真打开仓库时提交历史还在。
   */
  let panelTool = $derived<"term" | "log" | "git">(
    panelView === "log" && repo ? "log" : panelView === "git" && repo ? "git" : "term",
  );

  /**
   * Git 控制台那一页（issue #29）。**按需加载**，和终端、CM6 同一条纪律 ——
   * 一个诊断页面不该让每个人的启动多付钱。
   */
  const gitcon = lazy(() => import("./lib/git/GitConsole.svelte"), "Git 控制台");
  /** xterm.js 约 250KB，不开终端就不该付这个钱 —— 与 CM6 同样按需加载 */
  const terminal = lazy(() => import("./lib/terminal/Terminal.svelte"), "终端");
  /**
   * 多个终端并存。切换标签时**不能卸载**未激活的那些 ——
   * 组件一销毁 Session 就 drop，shell 直接被 kill，正在跑的命令全没了。
   * 所以用 CSS 隐藏，实例一直活着。
   */
  interface TermTab {
    id: number;
    /** 工作目录在创建时快照一次，之后不跟着 root 走 */
    cwd: string;
    title: string;
  }
  let terms = $state<TermTab[]>([]);
  let activeTermId = $state<number | null>(null);
  let nextTermId = 1;

  function newTerm(cwd?: string) {
    const dir = cwd ?? root ?? "~";
    const base = dir === "~" ? "~" : dir.slice(dir.lastIndexOf("/") + 1) || dir;
    /*
     * 重名要带序号。终端的标题取自工作目录名，而绝大多数时候几个终端开的
     * 是**同一个**目录（项目根）—— 于是三个标签页全写着 `proj`，
     * 标签栏和「全部终端」下拉都变成「随便点一个」。
     */
    let title = base;
    for (let n = 2; terms.some((t) => t.title === title); n++) title = `${base} (${n})`;
    const t: TermTab = { id: nextTermId++, cwd: dir, title };
    terms = [...terms, t];
    activeTermId = t.id;
    panel = true;
  }

  function closeTerm(id: number) {
    const idx = terms.findIndex((t) => t.id === id);
    terms = terms.filter((t) => t.id !== id);
    if (activeTermId === id) {
      activeTermId = terms[Math.min(idx, terms.length - 1)]?.id ?? null;
    }
    // 最后一个终端关掉就把面板一起收起，省得留个空壳
    if (terms.length === 0) panel = false;
  }

  /**
   * 导轨上的工具窗开关：点别的就切过去，点当前这个就收起。
   *
   * 和最上面 sidebar 那个开关同一个手势 —— 一个按钮既是「去那儿」
   * 也是「不看了」，不用再去找第二个地方收起。
   */
  function togglePanelView(v: "term" | "log" | "git") {
    // 判据是**正在显示的那个**，不是存下来的偏好 —— 偏好是 log 而没有仓库时
    // 亮着的是终端那个按钮，再点它就该收起，而不是「切到终端」（已经在了）
    if (panel && panelTool === v) {
      panel = false;
      return;
    }
    panelView = v;
    panel = true;
  }

  /** 面板头右边那两个下拉：`list` 是全部终端，`more` 是更多操作 */
  let panelMenu = $state<{ x: number; y: number; kind: "list" | "more" } | null>(null);

  function openPanelMenu(e: MouseEvent, kind: "list" | "more") {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    panelMenu = { x: r.left, y: r.bottom + 2, kind };
  }

  let panelMenuItems = $derived.by<MenuItem[]>(() => {
    if (!panelMenu) return [];
    if (panelMenu.kind === "list") {
      // 前面那个格子标出当前项。全角空格占位，切换时标题不会左右跳
      return terms.map((t) => ({
        label: `${t.id === activeTermId ? "●" : "\u3000"} ${t.title}`,
        run: () => (activeTermId = t.id),
      }));
    }
    const items: MenuItem[] = [{ label: "新建终端", run: () => newTerm() }];
    if (activeTermId !== null) {
      const id = activeTermId;
      items.push({ label: "关闭当前终端", run: () => closeTerm(id) });
    }
    if (terms.length > 1) {
      const keep = activeTermId;
      items.push({
        label: "关闭其他终端",
        run: () => {
          for (const t of [...terms]) if (t.id !== keep) closeTerm(t.id);
        },
      });
    }
    /*
     * 「全部关闭」带 danger，和 Git 栏的「全部丢弃」同一条判据：
     * 关掉一个终端等于 kill 掉里面正在跑的东西，撤不回来。一个一个关，
     * 每一下都还在看着标题；一下关掉全部，跑着的 gradle build 就没了。
     */
    if (terms.length > 1) {
      items.push({
        label: "全部关闭",
        sep: true,
        danger: true,
        run: () => {
          for (const t of [...terms]) closeTerm(t.id);
        },
      });
    }
    return items;
  });

  // 打开面板时若一个终端都没有，自动起一个。
  // 只在终端页上做 —— 冲着 Git 日志来的人不该莫名多出一个 shell
  $effect(() => {
    if (panel && panelTool === "term" && terms.length === 0 && root !== null) newTerm(root);
  });
  let hovering = $state(false);
  let logStatus = $state("");
  /** 待确认关闭的脏标签 —— 直接丢弃改动太粗暴，也不该静默保存 */
  let pendingClose = $state<TabState | null>(null);

  /**
   * CodeMirror 6 核心约 340KB，日志模式一点也用不上 —— 静态引入会把入口包
   * 从 71KB 顶到 412KB，与"秒开"的立身之本冲突。改成打开第一个可编辑文件时
   * 才 import，本地加载只有几毫秒。
   */
  const editor = lazy(() => import("./lib/editor/Editor.svelte"), "编辑器");

  /*
   * 日志视图同样按需加载 —— 和 Editor 对称。
   * 早先它是静态引入的，等于只写代码的人一直在为整套日志视图
   * （虚拟滚动 + 过滤条 + 8 种格式的解析着色）付钱。
   */
  const logPane = lazy(() => import("./lib/logview/LogPane.svelte"), "日志视图");

  $effect(() => {
    if (active?.mode === "log") logPane.load();
  });
  /** 每次保存成功自增，Editor 据此重置 dirty 基线 */
  let savedTick = $state(0);

  /**
   * 两个搜索浮层（⌘P 随处搜索、⌘⇧O 文件结构）。
   *
   * 一起拉是因为**它们共用 `fuzzy.ts`** —— 分两次的话那份排序算法要么进公共块、
   * 要么各带一份，而两个浮层本来就是同一类东西（键盘唤出、盖在界面上）。
   *
   * 挪出入口包**实测省 10,183 字节**（挪走前后各量一次，不是按 sourcemap 归因
   * 的估值 —— 归因会高估数据密集的模块，见 .claude/rules/frontend.md）。
   * 判据同 `keysPanel`：问一句「这东西在窗口出现之前有用吗」——
   * 没有，它们都得等一次按键。
   *
   * 但和速查表不同的是，**这两个是天天按的**，不能让第一次 ⌘P 等一次 chunk 往返。
   * 所以首屏画完之后就预拉（见下面那个 setTimeout）：既不占首屏之前那段，
   * 又保证人真按下去的时候它已经在了。
   */
  const overlays = lazyGroup(
    {
      quick: () => import("./lib/search/QuickSearch.svelte"),
      outline: () => import("./lib/search/Outline.svelte"),
    },
    "搜索浮层",
  );

  $effect(() => {
    // 兜底：预拉万一没跑到（或者失败过），真按下去时补一次。
    // `load()` 是幂等的，重复调用会被它自己的状态挡掉
    if (quickOpen || outlineOpen) overlays.load();
  });

  $effect(() => {
    /*
     * 首屏之后再拉。
     *
     * 300ms 不是随便取的：它要**明确落在首屏绘制之后**（否则等于没挪出去），
     * 又要远早于人按下第一个 ⌘P。用 `setTimeout` 而不是
     * `requestIdleCallback` —— 后者 Safari 16.4 才有，而构建目标是 safari15。
     */
    const id = setTimeout(() => overlays.load(), 300);
    return () => clearTimeout(id);
  });

  let quickOpen = $state(false);
  let quickScope = $state<"all" | "file" | "content" | "action">("all");
  /**
   * 打开随处搜索时预填的词。只有「在项目里找这个名字」会设它，
   * **每条打开浮层的路都要把它清掉** —— 不清的话，上次找过的名字
   * 会莫名其妙地出现在下一次 ⌘P 里。
   */
  let quickSeed = $state("");
  /** 待跳转的行号；带 nonce，连点同一条结果也能重新定位 */
  let gotoLine = $state<{ line: number; nonce: number } | null>(null);
  let gotoNonce = 0;

  // 文件结构大纲
  let outlineOpen = $state(false);
  let outlineTick = $state(0);
  /**
   * 空态卡片上列的那几条。
   *
   * 不是全表 —— 全表在 ⌘/ 的速查浮层里。这里只留「不知道就上不了手」的，
   * 顺序即显示顺序（两列铺开）。**从 keymap.ts 取，不手抄**：
   * 原来手抄的那份把 ⌘⇧F / ⌘⇧O / ⌘⇧G 三处修饰键次序全写反了。
   */
  const HINT_IDS = [
    "quick-all", "save", "quick-file", "close-tab", "quick-content",
    "toggle-sidebar", "outline", "toggle-panel", "git-changes", "log-next-hit",
  ];
  const keyHints = HINT_IDS.map((id) => keyById(id)).filter((k) => k !== undefined);

  /** 快捷键速查浮层。⌘/ 归菜单（帮助 › 快捷键速查），这里只存开合 */
  let keysOpen = $state(false);

  let symbols = $state<Sym[]>([]);
  function openOutline() {
    if (active?.mode !== "edit") return;
    symbols = [];
    outlineTick++;
    outlineOpen = true;
  }

  /**
   * 随处搜索里的「操作」。**从键位表生成，不再手写第二份。**
   *
   * 加菜单栏之前这里是一张 69 行的手写表，标签和 `hint` 各写一遍 ——
   * 而同样的信息在菜单栏、速查表、空态卡片里还各有一份。四处手抄的结果
   * 是可预见的：改一个键位漏掉三处。
   *
   * # 它同时是浏览器里唯一的入口
   *
   * `pnpm dev` 跑在浏览器里，**那儿没有菜单栏** —— 归菜单的动作
   * （⌘S ⌘O ⇧⌘G ⌘/ …）在那里一个都够不着，改 UI 的主循环就废了一半。
   * 让这张表覆盖全部动作之后，两边都通：Tauri 里走菜单，浏览器里走这儿。
   *
   * `cm6` 那一档不进来 —— 它们是编辑器内部的键位（⌘F 查找面板），
   * 不是这个应用能"执行"的动作。
   */
  const actions: Action[] = KEYS.filter((k) => k.owner !== "cm6").map((k) => ({
    id: k.id,
    // Git 那一摊加前缀：单看「刷新状态」「提交历史」不知道是谁的
    label: k.group === "Git" ? `Git：${k.label}` : k.label,
    hint: k.accel ?? k.gesture,
    run: () => void runMenu(k.id),
  }));

  /** 当前编辑的文件在 git 状态里对应的那条，没有就是干净的 */
  let activeEntry = $derived.by(() => {
    if (!gitSt || !active || active.mode === "diff") return null;
    const prefix = `${gitSt.root}/`;
    if (!active.path.startsWith(prefix)) return null;
    const rel = active.path.slice(prefix.length);
    return gitSt.entries.find((e) => e.path === rel) ?? null;
  });

  /**
   * 编辑器交回来的实时文本 —— 换文件或销毁前调一次。
   *
   * **按 path 找标签，不能用 `active`**：这个回调发生在切标签之后，
   * 那时 `active` 已经是新的那个了，写回去就是把 A 的内容盖到 B 头上。
   */
  function stashDraft(path: string, text: string) {
    const t = tabs.find((x) => x.path === path && x.mode === "edit");
    if (!t) return; // 标签已经被关掉了，草稿跟着作废
    Object.assign(t, stashed(t, text));
  }

  /**
   * 当前挂载着的那个编辑器，以及从它里面读实时文本的口子。
   *
   * 只可能有一个 —— 编辑器是 `{#key active.id}` 包着的，同一时刻只挂一个。
   * 记路径是为了**认领**：切标签时新实例可能先挂、旧实例后卸，
   * 旧实例交回的那个 null 不能把新实例的口子抹掉。
   */
  let live: { path: string; get: () => string } | null = null;

  function onEditorLive(path: string, get: (() => string) | null) {
    if (get) live = { path, get };
    else if (live?.path === path) live = null;
  }

  /** 「读出光标底下那个词」的口子。认领规则同 `live`，见它上面那段 */
  let wordProbe: { path: string; get: () => string | null } | null = null;

  function onEditorWordProbe(path: string, get: (() => string | null) | null) {
    if (get) wordProbe = { path, get };
    else if (wordProbe?.path === path) wordProbe = null;
  }

  /**
   * 这个标签当前该保存的文本。
   *
   * 编辑器还活着就以它为准 —— `draft` 只在换文件/销毁时回写一次，
   * `content` 是磁盘那份，两个都可能停在几步之前。
   * 判据和取值都在 `state/doc.ts` 里，那边有测试。
   */
  const liveText = (t: TabState) =>
    textToSave(t, live?.path === t.path && t.mode === "edit" ? live.get() : null);

  /** ⌘S 之外的保存入口（命令面板）。编辑器里的 ⌘S 走 CM6 自己的 keymap */
  function saveActive() {
    if (active?.mode === "edit") void save(liveText(active));
  }

  /**
   * ⌘Click 跳转要的文件索引（⌘P 那一份，相对项目根的路径）。
   *
   * **提前拉，不等人按键。** 它是「这个类在不在项目里」的唯一依据，
   * 而那个问题在 ⌘hover 的每一次鼠标移动上都要答一遍 —— 现拉就是
   * 每次 hover 隔一个 IPC 往返，下划线跟不上鼠标。`rg --files` 实测 0.02s，
   * 换项目时拉一次完全付得起。
   */
  let projectFiles = $state<string[]>([]);

  $effect(() => {
    const r = root;
    // treeTick 一变就重拉：切分支之后新增的文件也得跳得过去
    treeTick;
    if (!r) {
      projectFiles = [];
      return;
    }
    let dead = false;
    void listProjectFiles(r)
      .then((f) => {
        if (!dead) projectFiles = f;
      })
      // 索引拉不到不该打扰人：跳转的第二层歇菜，第一层照常работа
      .catch(() => {});
    return () => {
      dead = true;
    };
  });

  /**
   * 跳转历史。⌥⌘← 回去、⌥⌘→ 再回来（IDEA 的键位）。
   *
   * **跳出去回不来比不能跳更难受**，所以这两条和跳转本身是同一批东西，
   * 不是后续增强。
   *
   * 存的是「路径 + 行号」而不是标签 id：跳到的文件可能在中途被关掉，
   * 而按下 ⌥⌘← 的意思是「回到我刚才看的那个地方」，标签在不在无所谓。
   */
  interface NavSpot {
    path: string;
    line: number;
  }
  let navBack = $state<NavSpot[]>([]);
  let navFwd = $state<NavSpot[]>([]);
  /** 上限。留着几百条既没人用，也让 localStorage 那份快照白胖一圈 */
  const NAV_MAX = 50;

  /** 此刻在哪儿。`posByPath` 里存的是编辑器最后报上来的光标行 */
  function hereNow(): NavSpot | null {
    if (!active) return null;
    return { path: active.path, line: posByPath.get(active.path) ?? 1 };
  }

  /**
   * 跳转落点。**先把当前位置压栈再走** —— 顺序反了的话，
   * 压进去的就是目的地，⌥⌘← 会把你留在原地。
   */
  async function jumpTo(hit: JumpHit) {
    const from = hereNow();
    if (from) {
      navBack = [...navBack.slice(-(NAV_MAX - 1)), from];
      // 新的跳转让「前进」失效 —— 和浏览器一样，历史分叉时旧的那一支作废
      navFwd = [];
    }
    const target = hit.target;
    if (target.rel === "") {
      // 本文件：不重新打开，直接跳行
      if (target.line !== undefined) gotoLine = { line: target.line, nonce: ++gotoNonce };
      return;
    }
    await openAt(target.rel, target.line);
  }

  /** ⌥⌘← / ⌥⌘→。两条对称，合成一个函数免得两边漏改 */
  async function navGo(dir: "back" | "fwd") {
    const from = dir === "back" ? navBack : navFwd;
    if (from.length === 0) {
      notify.ok(dir === "back" ? "没有可回退的位置" : "没有可前进的位置", 1600);
      return;
    }
    const spot = from[from.length - 1];
    const here = hereNow();
    if (dir === "back") {
      navBack = navBack.slice(0, -1);
      if (here) navFwd = [...navFwd.slice(-(NAV_MAX - 1)), here];
    } else {
      navFwd = navFwd.slice(0, -1);
      if (here) navBack = [...navBack.slice(-(NAV_MAX - 1)), here];
    }
    await openAt(spot.path, spot.line);
  }

  /**
   * 「在项目里找这个名字」—— 跳转够不着时的退路。
   *
   * 它**不伪装成跳转**：拿光标处的词跑一次现成的全局搜索，结果照常列在
   * 搜索面板里让人自己挑。省掉的只是「选中、复制、⇧⌘F、粘贴」这四下，
   * 而不是给一个精度可疑的下划线。
   */
  function findWordAtCursor() {
    const w = active && wordProbe?.path === active.path ? wordProbe.get() : null;
    if (!w) {
      notify.ok("把光标放到一个名字上再按", 2000);
      return;
    }
    quickScope = "content";
    quickSeed = w;
    quickOpen = true;
  }

  /** 搜索结果点击：打开文件，带行号则跳过去 */
  async function openAt(path: string, line?: number) {
    const full = path.startsWith("/") ? path : `${root ?? ""}/${path}`;
    await openPath(full);
    if (line !== undefined) gotoLine = { line, nonce: ++gotoNonce };
  }

  $effect(() => {
    // Git 控制台只在真的切到那一页时才拉那个 chunk
    if (panel && panelTool === "git") gitcon.load();
  });

  $effect(() => {
    if (panel) terminal.load();
  });

  $effect(() => {
    if (active?.mode === "edit") editor.load();
  });

  let active = $derived(tabs.find((t) => t.id === activeId) ?? null);

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
    sideView = "files";
    sidebar = true;
    revealPath = path;
    revealTick++;
  }

  /**
   * 标题栏面包屑：项目名 › 中间目录 › 文件名。
   *
   * 只对真实文件算 —— 差异/合并标签的 path 是 `git-diff:xxx` 这类合成 key，
   * 拿它切路径会得到一堆垃圾段。那种情况退回显示标签名。
   */
  let crumbs = $derived.by(() => {
    const t = active;
    if (!t) return [] as { name: string; path: string; dir: boolean }[];
    const root0 = root;
    if (!root0 || !t.path.startsWith(`${root0}/`)) {
      return [{ name: t.name, path: t.path, dir: false }];
    }
    const rootName = root0.slice(root0.lastIndexOf("/") + 1) || root0;
    const rel = t.path.slice(root0.length + 1).split("/");
    const out = [{ name: rootName, path: root0, dir: true }];
    let acc = root0;
    rel.forEach((seg, i) => {
      acc += `/${seg}`;
      out.push({ name: seg, path: acc, dir: i < rel.length - 1 });
    });
    return out;
  });

  /*
   * 按需加载失败要说出来。以前每个 import 各自 catch 到 error 里，
   * 抽成 lazy() 之后错误存在各自的 store 上，这里统一汇到状态栏。
   *
   * **这张名单漏一个就是一处静默失败** —— `keysPanel` 就漏在这儿过：
   * ⌘/ 按下去什么都不出来，而状态栏一声不吭。2026-09-07 补上它和
   * 新加的 `overlays`。加新的 lazy() 时记得回来加一行。
   */
  $effect(() => {
    const e =
      editor.error ||
      logPane.error ||
      terminal.error ||
      git.error ||
      encPicker.error ||
      keysPanel.error ||
      overlays.error;
    if (e) notify.fail(e);
  });

  /** 走 legacy stream parser 的语言没有语法树，界面要明说 */
  const LEZER_LANGS = new Set([
    "java", "javascript", "typescript", "python", "markdown", "json", "rust",
    "yaml", "html", "css", "sass", "less", "xml", "sql", "cpp", "php", "vue", "liquid",
  ]);
  /**
   * 语言识别表（文件名 → 语言 id → 显示名）。**只在有标签打开时才拉。**
   *
   * 入口包是**首屏之前必须解析执行完**的那一段，而这张表回答的两个问题
   * （状态栏显示什么语言、⌘⇧O 支不支持这个文件）都要先有一个打开的文件
   * 才成立 —— 一个都没打开时它纯属压秤。**实测省 3,419 字节。**
   *
   * （sourcemap 归因说它有 11.0 KB，差了三倍：这张表几乎全是数据，
   * 语句少、mapping 稀，归因会把后面邻居的字节一起算到它头上。
   * 收益一律以「挪走前后各量一次」为准。）
   *
   * 表还没到手时：语言那格空着，`outlineSupported` 是假。两者都只持续到
   * 那个几 KB 的 chunk 回来为止，而它和编辑器（370 KB）是同时开始拉的。
   *
   * 编辑器那边照旧直接 `import` 它 —— 那个 chunk 本来就是懒的，
   * 两处引到的是同一份模块。
   */
  let langs = $state<typeof import("./lib/editor/langs") | null>(null);
  $effect(() => {
    if (active && !langs) void import("./lib/editor/langs").then((m) => (langs = m));
  });

  let outlineSupported = $derived(
    active?.mode === "edit" && !!langs && LEZER_LANGS.has(langs.langOf(active.path) ?? ""),
  );


  /** 正在打开的路径，防止双击或事件重放时重复探测 */
  const opening = new Set<string>();

  /**
   * 正在把上次的标签摆回来。**唯一的作用是拦住 `openPath` 去动 `activeId`。**
   *
   * 内容区是 `{#key active.id}` 包着的 —— activeId 一变就销毁重建。
   * 而恢复是一个一个 `await openPath()` 的，每开一个就把 activeId 顶成它，
   * 于是恢复 8 个标签 = **把编辑器建了 8 次**，界面一个文件一个文件地闪过去。
   * （CM6 还是懒加载的，第一次要等 chunk 到位，闪得更明显。）
   *
   * 改成由 `restoreSession` 在**恰好走到该激活的那个标签时**设一次 activeId，
   * 编辑器只建一次。标签仍按存下来的顺序逐个进列表 —— 那只是标签条在长，
   * 不重建任何东西。
   *
   * **和下面那个 `restoring` 是两回事，不能合并。** 那个管的是「恢复期不写快照」，
   * 它要一直盖到启动路径的最后 —— 包括命令行传进来的那个文件
   * （`lite-ide a.rs`）。而那个文件**恰恰应该**被激活，合并了就等于
   * `lite-ide a.rs` 打开却不切过去。
   */
  let restoringTabs = false;

  /**
   * 草稿目录的绝对路径（`~/Library/Application Support/com.liteide.app/scratches`）。
   *
   * 启动时拿一次就不再变。**不 await 在启动路径上** —— 它只服务两件事
   * （判断一个标签是不是草稿、菜单里打开草稿目录），两件都发生在人动手之后，
   * 而这一次 IPC 是毫秒级的，早就回来了。为它把首屏往后推一拍不值。
   */
  let scratchRoot = $state<string | null>(null);

  /**
   * 这个路径是不是一份草稿。
   *
   * `scratchRoot` 还没到位时一律算「不是」：它唯一的用处是决定
   * 「关掉时要不要把这个空文件丢掉」，而**猜错的方向必须是留下**——
   * 少丢一个空文件只是噪音，多丢一个就是删了不该删的东西。
   */
  function isScratch(path: string): boolean {
    return scratchRoot !== null && path.startsWith(`${scratchRoot}/`);
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
  async function newScratch() {
    notify.clear();
    try {
      const d = new Date();
      const pad = (n: number) => String(n).padStart(2, "0");
      const stem =
        `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())} ` +
        `${pad(d.getHours())}${pad(d.getMinutes())}`;
      await openPath(await createScratch(stem));
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /**
   * 把草稿目录当项目根打开 —— 文件树、⌘P、⇧⌘F 立刻全都有，零新代码。
   *
   * 草稿目录在 Finder 里默认看不见（「资源库」是隐藏的），这是翻旧草稿唯一的入口。
   * 代价说在前面：Git 面板会空，那个目录不是仓库。
   *
   * 目录不存在**不是错误**，是「你还一条都没记过」——
   * 报一句红字会让人以为坏了。
   */
  async function openScratchDir() {
    notify.clear();
    try {
      const dir = await scratchDir();
      if (!(await probePath(dir).catch(() => null))) {
        notify.ok("还没有草稿 —— ⌘N 记第一条", 2600);
        return;
      }
      await openPath(dir);
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /**
   * `quiet` 给会话恢复用：上次开着的文件这次可能已经不在了
   * （删了、改名了、切到了没有它的分支）。那是完全正常的事，
   * 逐个弹「读不到 xxx」只会在启动时糊一屏红字。
   */
  async function openPath(path: string, quiet = false) {
    if (opening.has(path)) return;
    opening.add(path);
    if (!quiet) notify.clear();
    try {
      const info = await probePath(path);
      if (info.kind === "dir") {
        root = info.path;
        return;
      }
      const exist = tabs.find((t) => t.path === info.path);
      if (exist) {
        if (!restoringTabs) activeId = exist.id;
        return;
      }

      const tab: TabState = {
        id: nextId++,
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
        tab.lossy = t.lossy;
        tab.stamp = await fileStamp(info.path);
      }
      tabs = [...tabs, tab];
      // 恢复期不抢：见 `restoringTabs` 上面那段
      if (!restoringTabs) activeId = tab.id;
      /*
       * 没有项目根时，拿这个文件的父目录顶上，文件树才有东西显示。
       *
       * **草稿不需要在这儿特判**，虽然一眼看上去像要：正开着项目时 `root`
       * 已经有值，这句根本不执行，文件树不会被草稿顶走；而没开项目就记东西时，
       * 树里显示的正好是你的草稿目录 —— 那时你手上也没有别的东西可看。
       * （加一道 `!isScratch(...)` 的守卫是我第一版写的，它永远不会为假。）
       */
      if (!root) root = info.path.slice(0, info.path.lastIndexOf("/")) || "/";
      // 恢复期不核：那时 activeId 故意停在 null 而标签一个个往里填，
      // 「有标签但没有活动标签」在这段窗口里是对的。恢复完了再一次核完
      if (!restoringTabs) auditTabs("开标签");
    } catch (e) {
      if (!quiet) notify.fail(String(e));
    } finally {
      opening.delete(path);
    }
  }

  /**
   * 最近打开过的项目根，最新的排最前。
   *
   * 记的是**项目根不是文件**：会话恢复本来就以 root 为单位，
   * 开回一个项目上次的标签会跟着回来，比记住散落的文件有用得多。
   */
  let recent = $state<string[]>(saved?.recent ?? []);

  /** 把一个目录顶到最近列表最前面。已经在里面就是往前挪，不是加一条 */
  function remember(dir: string) {
    recent = [dir, ...recent.filter((r) => r !== dir)].slice(0, session.RECENT_MAX);
  }

  /**
   * 开原生的选择文件夹面板。取消了什么也不做。
   *
   * 选中之后走的是 `openPath` —— 它对目录的处理就是把 root 设过去，
   * 和拖一个文件夹进来、命令行传目录**是同一条路**。
   * 另起一套的话，「切项目要不要清掉旧标签」这类判断就会有两份。
   */
  async function openFolder() {
    const dir = await pickFolder().catch(() => null);
    if (!dir) return;
    await openPath(dir);
  }

  /**
   * 从菜单里选一个最近项目。
   *
   * **不预先探测存在性。** 每次开菜单去 stat 一遍 8 个路径，
   * 碰上没挂载的网络卷会把菜单卡住 —— 改成点了才发现：
   * 打不开就报一句并把它从列表里摘掉，那时用户已经知道自己在等什么了。
   */
  async function openRecent(dir: string) {
    const info = await probePath(dir).catch(() => null);
    if (info?.kind !== "dir") {
      notify.fail(`打不开 ${dir} —— 已从最近记录里移除`, 3200);
      recent = recent.filter((r) => r !== dir);
      return;
    }
    await openPath(dir);
  }

  /**
   * 标题栏项目挂件的下拉。
   *
   * 照 IDEA 的 project widget：显示当前项目名，点开是最近项目 + 打开 + 清除。
   * 这三件事的逻辑**一条都不是新写的** —— `recent` / `openRecent` / `openFolder`
   * 早就在了，以前只有 macOS 菜单栏的「最近打开」子菜单用得着它们，
   * 而 `pnpm dev` 跑在浏览器里，那儿一个菜单项都没有。
   */
  let projMenu = $state<{ x: number; y: number } | null>(null);

  function openProjMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    projMenu = { x: r.left, y: r.bottom + 2 };
  }

  /**
   * 分支浮层挂在挂件底下，所以要把挂件的位置一起交出去。
   *
   * **位置在打开之前就得定下来**，和 `git_fetch` 的 op_id 是同一条判据：
   * 凡是「先开始、后返回句柄」的东西，句柄必须早于用它的人。这里更直接 ——
   * 浮层渲染的那一帧就要知道往哪儿掉。
   */
  let branchAnchor = $state<{ x: number; y: number } | null>(null);
  let branchBtn = $state<HTMLElement | null>(null);

  /**
   * 一律从挂件底下掉下来，**不管是谁开的**：点挂件、Git 栏里的分支行、
   * 菜单里的「分支与工作树」，三条路都读同一个元素的位置。
   * 三处各写各的话，从菜单开出来的那次就会掉在别的地方。
   */
  function openBranchPicker() {
    const r = branchBtn?.getBoundingClientRect();
    branchAnchor = r ? { x: r.left, y: r.bottom + 4 } : null;
    branchOpen = true;
  }

  let projName = $derived(root ? root.slice(root.lastIndexOf("/") + 1) || root : "lite-ide");
  /** 方块里那个字。IDEA 用项目名首字母，中文项目名就直接用第一个字 */
  let projInitial = $derived((projName[0] ?? "?").toUpperCase());

  let projMenuItems = $derived.by<MenuItem[]>(() => {
    if (!projMenu) return [];
    // 前面那个格子标出当前项目。全角空格占位，切换时名字不会左右跳
    const items: MenuItem[] = recent.map((r) => ({
      label: `${r === root ? "●" : "\u3000"} ${r.slice(r.lastIndexOf("/") + 1) || r}`,
      run: () => void openRecent(r),
    }));
    items.push({ label: "打开文件夹…", sep: items.length > 0, run: () => void openFolder() });
    if (recent.length > 0) items.push({ label: "清除最近记录", run: () => (recent = []) });
    return items;
  });

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
   * 清完必须走一次 `workingTreeChanged()` —— 否则开着那份日志的标签上
   * 还摊着刚被清掉的几百行，人会以为没生效，然后再点一次。
   * 这正是那条老规矩的又一例（**盘上的东西被外部改了，两件事要一起做**），
   * 只不过这次「外部」是我们自己。
   */
  async function clearLog() {
    try {
      await clearAppLog();
      await workingTreeChanged();
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
    await openPath(path);
  }

  async function openRepoPage() {
    await openExternal("https://github.com/Spc-jgs/lite-ide-mac").catch(() => {
      notify.fail("打不开项目主页", 2600);
    });
  }

  // ── 拉取与推送 ────────────────────────────────────────────────

  /**
   * 远程操作的 id。**前端发号，不是等 Rust 返回。**
   *
   * 等返回值的话取消按钮永远点不动 —— 返回值要等操作跑完才到。
   * 这是实测点了一次取消、发现 `git_cancel` 压根没被调到才发现的。
   */
  let nextOpId = 0;

  /** 正在跑的远程操作。null = 没有 */
  let syncing = $state<{
    what: "pull" | "push" | "fetch";
    id: number;
    phase: string;
    percent: number | null;
  } | null>(null);

  /** 分岔了要先决定合并还是变基。null = 没在问 */
  let pendingDiverge = $state<{ upstream: string } | null>(null);

  /**
   * 上次选的合并方式。
   *
   * IDEA 的「记住这次选择」——分岔是常态，每次都问同一个问题很烦。
   * **但只记在内存里**：跨重启还记着的话，下次分岔时会用一个人早就忘了的
   * 策略默默合并，那比多问一次糟。
   */
  let lastMergeMode = $state<"merge" | "rebase" | null>(null);

  /** 推送前的确认。列出要推的提交（照 IDEA），而不是只给一个计数 */
  let pendingPush = $state<{ branch: string; setUpstream: boolean; commits: string[] } | null>(null);

  /** 远程操作失败时展开的那块。`raw` 是 git 的原话 */
  let remoteErr = $state<(RemoteErr & { hint: string }) | null>(null);

  /**
   * 把 RemoteErr 变成一句能照着做的话。
   *
   * git 的原话不能直接给用户看 ——「terminal prompts disabled」会让人以为是
   * 我们的开关设错了，而真正该做的事是去认证一次。
   * **但原话要留着能展开**：转译错了的时候人得有办法绕过我们。
   */
  async function toHint(e: RemoteErr): Promise<string> {
    if (e.kind === "auth-https") {
      return `在终端里跑一次，输一遍账号密码，之后就一直有效：\n  git -C ${root} fetch`;
    }
    if (e.kind === "auth-ssh") {
      return "把私钥加进 ssh-agent：\n  ssh-add --apple-use-keychain ~/.ssh/id_ed25519";
    }
    if (e.kind === "rejected") return "远程上有你本地还没有的提交。先拉下来，再推。";
    return "";
  }

  async function showRemoteErr(e: RemoteErr) {
    if (e.kind === "cancelled") return; // 用户自己取消的，不是错误
    remoteErr = { ...e, hint: await toHint(e) };
  }

  /** 进度通道。每次操作新建一个 —— Channel 是一次性的 */
  function progressChannel(what: "pull" | "push" | "fetch") {
    const ch = new Channel<RemoteProgress>();
    ch.onmessage = (p) => {
      if (!syncing) return;
      syncing = { ...syncing, what, phase: p.phase, percent: p.percent };
    };
    return ch;
  }

  /**
   * 抓远程。只读，不动工作区 —— 失败了没有任何后果。
   * 拉取的第一步也是它。
   */
  async function doFetch(what: "pull" | "fetch"): Promise<boolean> {
    if (!repo || syncing) return false;
    git.load();
    remoteErr = null;
    const opId = ++nextOpId;
    syncing = { what, id: opId, phase: "正在连接…", percent: null };
    try {
      await gitFetch(repo, "origin", opId, progressChannel(what));
      await refreshGit();
      return true;
    } catch (e) {
      await showRemoteErr(e as RemoteErr);
      return false;
    } finally {
      syncing = null;
    }
  }

  /**
   * 拉取 = fetch + 本地合并两步，**不是 `git pull`**。
   *
   * 复合命令失败时分不清是网络断了还是合并冲突了（退出码都非零）。
   * 拆开之后：第一步失败就是纯网络/凭据，第二步失败就是冲突，
   * 而冲突有 MergeView 接着。
   *
   * 默认只允许快进 —— 永远不会「拉一下，凭空多出一个合并提交」。
   * 快进不了就停下来问（或者用上次记住的选择）。
   */
  /**
   * 拉一次。**返回值是「要用这个模式再拉一次」**，null = 不用再拉。
   *
   * # 为什么重试要走返回值，不能在 catch 里直接递归
   *
   * 原来那句是 `void doPull(lastMergeMode); return;` —— 加上 issue #23 的
   * 守卫之后它会**把自己挡下来**：`doPull` 里 `claimGit` 之前没有 `await`，
   * 递归那次同步就跑到守卫上，而这时外层的 `finally` 还没执行、锁还在自己手里。
   * 表现会是「分岔之后自动重试静默失灵，只弹一句『正在合并上游，请等它做完』」。
   *
   * 也不能改成「先 `releaseGit()` 再递归」：那样外层的 `finally` 会**再放一次**，
   * 而那时锁已经属于内层了 —— 等于凭空把锁开了。
   *
   * 把重试挪到 `finally` 之后就没有这两个问题。重试只可能发生一次
   * （第二次带着 `mode`，走不进那个分支）。
   */
  async function pullOnce(
    upstream: string,
    mode?: "merge" | "rebase",
  ): Promise<"merge" | "rebase" | null> {
    // **整个 pull 都占着锁，包括前面那次 fetch。** fetch 自己不动 index，
    // 但它后面紧跟着的合并动。只圈住合并的话，fetch 期间开始的一次提交
    // 会让合并被挡下来 —— 那时 pull 已经拉下来一半，停在一个说不清的状态上
    if (!claimGit("合并上游")) return null;
    try {
      if (!mode && !(await doFetch("pull"))) return null;
      await gitMergeUpstream(repo!, upstream, mode ?? "ff-only");
      await workingTreeChanged();
      await refreshGit();
      notify.ok(mode === "rebase" ? "已变基到上游" : "已合并上游");
      return null;
    } catch (e) {
      const err = e as RemoteErr;
      // 快进不了 = 分岔了，要先做决定。这不是错误，是个岔路口
      if (err.kind === "conflict" && !mode) {
        // 记过一次就直接用，不再问（IDEA 的「记住这次选择」）
        if (lastMergeMode) return lastMergeMode;
        pendingDiverge = { upstream };
        return null;
      }
      await showRemoteErr(err);
      // 合并冲突之后工作区变了，得把界面对上
      await workingTreeChanged();
      await refreshGit();
      return null;
    } finally {
      releaseGit();
    }
  }

  async function doPull(mode?: "merge" | "rebase") {
    if (!repo) return;
    // 确认条在 Git 那一组里（懒的）。从菜单直接拉时它可能还没到位 ——
    // 不先拉一下的话，分岔决策条不会出现，看着像「点了没反应」
    git.load();
    const upstream = gitSt?.upstream;
    if (!upstream) {
      notify.fail("这个分支没有上游，先推送一次", 3000);
      return;
    }
    const retry = await pullOnce(upstream, mode);
    if (retry) await pullOnce(upstream, retry);
  }

  /** 推送。先把要推的提交列出来让人看清 —— 照 IDEA 的推送对话框 */
  async function askPush() {
    if (!repo || !gitSt) return;
    git.load();
    const branch = gitSt.branch;
    if (!branch) {
      notify.fail("游离状态下不能推送", 2600);
      return;
    }
    const setUpstream = !gitSt.upstream;
    let commits: string[] = [];
    try {
      commits = await gitOutgoing(repo, gitSt.upstream ?? "", branch);
    } catch {
      commits = []; // 列不出来不该挡住推送，只是少了一份确认信息
    }
    pendingPush = { branch, setUpstream, commits };
  }

  async function doPush() {
    const req = pendingPush;
    pendingPush = null;
    if (!repo || !req || syncing) return;
    /*
     * **push 也占锁**（issue #23），虽然它自己不动 index。
     *
     * 理由不是锁冲突，是**因果**：正在跑的那次提交会改变要推的内容。
     * 钩子跑到一半时点推送，推上去的是钩子跑完之前的 HEAD ——
     * 命令都成功，结果却不是人想要的那个，而且事后完全看不出来。
     * 这种「都没报错但答案是错的」比一句 `index.lock` 报错难查得多。
     */
    if (!claimGit("推送")) return;
    remoteErr = null;
    const opId = ++nextOpId;
    syncing = { what: "push", id: opId, phase: "正在连接…", percent: null };
    try {
      await gitPush(repo, "origin", req.branch, req.setUpstream, opId, progressChannel("push"));
      await refreshGit();
      notify.ok("已推送");
    } catch (e) {
      await showRemoteErr(e as RemoteErr);
    } finally {
      syncing = null;
      releaseGit();
    }
  }

  /**
   * 取消。**只对 fetch 开放。**
   *
   * push 中途 kill 掉的是本地这一端，而远程可能已经收完了 ——
   * 一个点了之后状态不确定的取消按钮，比没有按钮更糟。
   */
  function cancelSync() {
    if (syncing) void gitCancel(syncing.id);
  }

  /**
   * 把上次的现场摆回来。
   *
   * 全程「能恢复多少算多少」：项目根没了就不设，文件没了就跳过，
   * 一个都没恢复出来就是一个干净的空界面 —— 都不该报错。
   * 启动流程里任何一句 throw 都等于应用打不开。
   */
  async function restoreSession() {
    if (!saved) return;
    if (saved.root) {
      const ok = await probePath(saved.root)
        .then((i) => i.kind === "dir")
        .catch(() => false);
      if (ok) root = saved.root;
    }
    /*
     * **先记位置，再开文件。** 反过来写过一版，位置恢复整个不生效：
     * `openPath` 一把标签加进去，activeId 就变了，兑现位置的那个 effect
     * 当场就跑 —— 而那时 `pendingPos` 里还什么都没有。等 effect 跑完再写进去，
     * activeId 已经不会再变，effect 也就不会再跑第二次了。
     */
    for (const t of saved.tabs) {
      if (t.line !== undefined) pendingPos.set(t.path, t.line);
    }
    /*
     * 串行开，不并行。
     *
     * 并行看着快，但每个文件都要 probe + 读全文（或 mmap + 探编码），
     * 二十个文件一起冲进 IPC 会把启动的头一秒占满，首屏反而更晚出来。
     * 而且 `openPath` 里 `if (!root) root = 父目录` 这句依赖顺序。
     *
     * **但串行不等于要一个一个地闪。** `restoringTabs` 期间 `openPath`
     * 不碰 `activeId`（见它上面那段），所以内容区一次都不重建；
     * 走到该激活的那个标签时点一次，编辑器**只建一次**，
     * 剩下的标签在它后面继续往标签条里填。
     *
     * 顺序仍是存下来的顺序 —— 把该激活的那个提到最前面能让它更早出来，
     * 但标签条的顺序就跟上次不一样了，那是个更难受的毛病。
     */
    const wantPath = saved.tabs[saved.active]?.path;
    restoringTabs = true;
    try {
      for (const t of saved.tabs) {
        await openPath(t.path, true);
        if (t.path === wantPath) {
          const hit = tabs.find((x) => x.path === t.path);
          // 这一下是整个恢复过程里唯一一次内容区渲染
          if (hit) activeId = hit.id;
        }
      }
    } finally {
      // 这里必须 finally：漏掉的话 activeId 就永久失灵，
      // 而 openPath 是会抛的（文件没了、读不动、编码探测失败）
      restoringTabs = false;
    }
    /*
     * 兑现草稿。**必须在文件都读进来之后**：判据是「草稿和盘上现在那份一不一样」，
     * 盘上那份要先有。
     *
     * 三种情况，都不需要我们替谁做主（见 state/session.ts 的长注释）：
     * - 盘上没变 → 原样恢复，dirty 由 `stashed` 按内容算出来
     * - 盘上变了而草稿还不一样 → 就是应用运行中早就有的那个冲突，
     *   摆出「用磁盘上的 / 保留我的」让用户选
     * - 草稿恰好和盘上现在一样 → `stashed` 自己会把它丢掉，也就不脏
     */
    for (const snapTab of saved.tabs) {
      if (snapTab.draft === undefined) continue;
      const tab = tabs.find((t) => t.path === snapTab.path && t.mode === "edit");
      if (!tab) continue;
      Object.assign(tab, stashed(tab, snapTab.draft));
      if (!tab.dirty) continue;
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
    if (activeId === null && tabs.length > 0) activeId = tabs[0].id;
    // 上次开着、这次已经不在的文件：从记忆里也删掉，不然它们
    // 会一直躺在快照里，每次启动都白试一遍
    for (const t of saved.tabs) {
      if (!tabs.some((x) => x.path === t.path)) pendingPos.delete(t.path);
    }
    auditTabs("会话恢复");
  }

  /**
   * 活动标签换了：如果它带着一个待兑现的恢复位置，跳过去并**销号**。
   *
   * 销号是关键 —— 不删的话，以后每次切回这个标签都会被拽回那一行，
   * 用户在别处读到一半切走再切回来就莫名其妙跳走了。
   */
  $effect(() => {
    const t = active;
    if (!t) return;
    const line = pendingPos.get(t.path);
    if (line === undefined) return;
    pendingPos.delete(t.path);
    gotoLine = { line, nonce: ++gotoNonce };
  });

  /** 按当前状态拍一张快照 */
  function snapshot(): session.Session {
    return {
      root,
      tabs: tabs.map((t) => {
        const line = posByPath.get(t.path);
        const snap: session.TabSnap = { path: t.path };
        if (line !== undefined) snap.line = line;
        /*
         * 有未保存改动就把草稿一起存下来 —— 「没手动保存就退出，改动直接没」
         * 是这个应用最容易咬人的一条，而会话恢复对外说的是「回到上次的现场」。
         *
         * `liveText` 而不是 `t.draft`：当前标签的编辑器还活着，草稿字段
         * 可能停在几步之前（见 state/doc.ts）。
         * `stamp` 必须一起存，恢复时要靠它判断盘上那份有没有被人动过。
         * 超限的草稿由 `session.serialize` 丢掉，这里不预先筛。
         */
        if (t.mode === "edit" && t.dirty) {
          snap.draft = liveText(t);
          if (t.stamp) snap.stamp = { mtimeMs: t.stamp.mtimeMs, size: t.stamp.size };
        }
        return snap;
      }),
      active: Math.max(0, tabs.findIndex((t) => t.id === activeId)),
      layout: { sidebar, sidebarWidth, sideView, panel, panelHeight, panelView },
      recent: [...recent],
    };
  }

  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  /**
   * 恢复期间不写。
   *
   * 保存的 effect 在挂载时就会跑一次，而那时 `restoreSession()` 还没开始
   * （它要等 `initialPath()` 这个 IPC 回来）—— 400ms 的防抖一到，
   * 就会拿一个「什么都没打开」的空状态**盖掉上次的快照**。
   * 本次运行看不出问题（`saved` 早在初始化时就读进内存了），
   * 但恢复途中退出的话，上次的现场就真没了。
   */
  let restoring = $state(true);

  /** 上一次真正写进去的那串。草稿让写变频了，一模一样就别再写一遍 */
  let lastWritten = "";

  function writeSession() {
    saveTimer = null;
    if (restoring) return;
    const snap = snapshot();
    let text: string;
    try {
      text = session.serialize(snap);
    } catch {
      return; // 序列化都失败就彻底放弃，不能让它冒到启动路径上
    }
    if (text === lastWritten) return;
    try {
      localStorage.setItem(session.KEY, text);
      lastWritten = text;
    } catch {
      /*
       * 写不下多半是草稿把配额撑爆了。**退一步再存一次**：宁可丢草稿，
       * 也不能连「上次开了哪些文件、光标在哪」一起赔进去 ——
       * 后者是草稿进来之前就有的保证，不该被新功能连累。
       */
      try {
        const plain = session.serialize(snap, false);
        localStorage.setItem(session.KEY, plain);
        lastWritten = plain;
      } catch {
        /* 隐私模式之类，连基本的都写不下就算了 */
      }
    }
  }

  /**
   * 防抖 400ms 后存。
   *
   * 拖侧边栏、移光标、滚日志都会走这里，每次都写 localStorage 是**同步 IO**，
   * 不防抖的话拖动时能明显感觉到滞手。
   */
  function scheduleSave() {
    if (saveTimer) clearTimeout(saveTimer);
    saveTimer = setTimeout(writeSession, 400);
  }

  /**
   * 记下某个文件当前停在哪一行。
   *
   * **必须自己调 `scheduleSave()`**，不能指望下面那个 effect ——
   * `posByPath` 是普通 Map（故意的，见它的声明），改它不产生任何信号。
   * 少了这一句，「开文件 → 滚到第 5000 行 → 退出」这条最典型的路径
   * 就什么都没存下来，而快照看着还挺正常，最难查。
   */
  function markPos(path: string, line: number) {
    if (line < 1) return;
    posByPath.set(path, line);
    scheduleSave();
  }

  /** 已经为「草稿太大存不下」提醒过的文件，一个文件只说一次 */
  const warnedBig = new Set<string>();

  /*
   * 有未保存改动时定期落一次盘。
   *
   * 下面那条响应式 effect 订阅的是布局、标签、项目根 —— **打字不动其中任何一个**，
   * 所以光靠它，「改了半天一直没切标签也没退出」这个最该被记住的状态一次都不会存。
   * 退出前的 pagehide 补写能兜住正常退出，但兜不住崩溃（Rust 侧是 panic = abort，
   * 一个 panic 就是进程当场死，没有 pagehide）。
   *
   * 4 秒一次，而且只在真有脏标签时才动；`writeSession` 里还有一道
   * 「和上次一模一样就不写」。
   */
  $effect(() => {
    const id = setInterval(() => {
      const dirty = tabs.filter((t) => t.mode === "edit" && t.dirty);
      if (dirty.length === 0) return;
      // 存不下的那种要当面说 —— 不说的话用户以为自己被记住了
      for (const t of dirty) {
        if (warnedBig.has(t.path)) continue;
        if (liveText(t).length <= session.MAX_DRAFT_CHARS) continue;
        warnedBig.add(t.path);
        notify.fail(`${t.name} 太大，未保存的改动不会被记住 —— 请 ⌘S 保存`, 6000);
      }
      scheduleSave();
    }, 4000);
    return () => clearInterval(id);
  });

  // 响应式那一半：布局、标签、项目根变了就存
  $effect(() => {
    // 显式读一遍，让 effect 订阅上它们
    void [root, tabs.length, activeId, sidebar, sidebarWidth, sideView, panel, panelHeight, panelView];
    scheduleSave();
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
      if (saveTimer) clearTimeout(saveTimer);
      writeSession();
    };
    window.addEventListener("pagehide", flush);
    window.addEventListener("beforeunload", flush);
    return () => {
      window.removeEventListener("pagehide", flush);
      window.removeEventListener("beforeunload", flush);
    };
  });

  /**
   * 保存当前编辑标签。**返回是否真的写成了。**
   *
   * 以前是 `Promise<void>` 而错误在这里就被 notify 吃掉了，于是
   * 「保存并关闭」写成 `save(...).then(() => doClose(t))` —— 磁盘写失败
   * （满了、没权限、文件被外部删了）时它照样把标签关掉，改动当场就没。
   * 批量关闭把这条路走得多得多，所以先把成败传出去。
   */
  async function save(content: string): Promise<boolean> {
    const tab = active;
    if (!tab || tab.mode !== "edit") return false;
    try {
      // 保存返回新指纹，必须记下来，否则下次检查会把自己的保存当成外部修改
      tab.stamp = await writeText(tab.path, content, tab.encoding, tab.bom);
      // 磁盘那份成了准。草稿一起清掉 —— 三处「读回磁盘」共用 settled 这一个出口，
      // 原来各写一遍，其中一处漏了清草稿（见 state/doc.ts 的注释）
      Object.assign(tab, settled(content));
      tab.conflict = false;
      savedTick++;
      notify.ok(`已保存 ${tab.name}`, 1800);
      // 保存八成改变了 git 状态，顺手刷一下，文件树的标记才跟得上
      void refreshGit();
      return true;
    } catch (e) {
      notify.fail(String(e));
      return false;
    }
  }

  /**
   * 检查打开的编辑标签是否被外部改动。
   *
   * 时机选在窗口获得焦点时 —— 用户从别处切回来才是他关心这件事的时刻，
   * 也不必为了这个常年跑一个轮询。另配一个 10 秒的兜底轮询，
   * 应付「一直没离开窗口但文件被后台进程改了」的情况。
   */
  async function checkExternalChanges() {
    for (const tab of tabs) {
      if (tab.mode !== "edit") continue;
      let now: Stamp;
      try {
        now = await fileStamp(tab.path);
      } catch {
        // 文件没了或读不到：不打扰，用户保存时自然会报错
        continue;
      }
      const before = tab.stamp;
      if (!before || (before.mtimeMs === now.mtimeMs && before.size === now.size)) continue;

      if (tab.dirty) {
        // 两边都改了，只能让用户裁决
        tab.conflict = true;
        tab.stamp = now;
      } else {
        // 本地没动过，直接跟上外部的版本 —— 这是最常见也最无害的情况
        try {
          // 沿用已知编码重读，不重新探测 —— 文件只是内容变了，编码没道理换
          const t = await readText(tab.path, tab.encoding);
          Object.assign(tab, settled(t.content));
          tab.lossy = t.lossy;
          tab.stamp = now;
          savedTick++;
          notify.ok(`${tab.name} 已被外部修改，已重新加载`, 2600);
        } catch (e) {
          notify.fail(String(e));
        }
      }
    }
  }

  async function resolveConflict(tab: TabState, take: "disk" | "mine") {
    tab.conflict = false;
    if (take === "disk") {
      try {
        Object.assign(tab, settled((await readText(tab.path, tab.encoding)).content));
        tab.stamp = await fileStamp(tab.path);
        savedTick++;
      } catch (e) {
        notify.fail(String(e));
      }
    }
    // take === "mine"：什么都不做，保留编辑器里的内容，
    // 下次 ⌘S 会覆盖磁盘 —— 指纹已经更新过，不会再重复告警
  }

  $effect(() => {
    const onFocus = () => {
      // 用户可能刚切出去，在终端里 commit / checkout / mv 完再切回来
      void workingTreeChanged();
      void refreshGit();
    };
    window.addEventListener("focus", onFocus);
    /*
     * 兜底轮询只查已打开文件的指纹，**不**重列目录 ——
     * 重列要按展开的目录数发一串 IPC，每 10 秒跑一次纯属白烧。
     * 目录结构的变化靠焦点事件捕捉就够了。
     */
    const id = setInterval(() => void checkExternalChanges(), 10_000);
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(id);
    };
  });

  /**
   * 这份构建带不带 Web Inspector（issue #20）。
   *
   * 调试版和正式版**装在同一个路径上**（`pnpm app:bundle:devtools` 覆盖
   * `pnpm app:bundle` 的产物），而「盘上只留一份 .app」是这个仓库的硬纪律 ——
   * 两条加起来的结果是：忘了打回去的话，你双击的那份一直开着 inspector，
   * 而界面上**没有任何迹象**。
   *
   * 挂在项目挂件的 tooltip 上，和构建时间并排：排查「你跑的是哪个构建」时
   * 本来就要看那一眼，不多一个新习惯。
   */
  let devtools = $state(false);
  /**
   * 项目挂件的 tooltip。
   *
   * **换行必须写在表达式里，不能在模板里写 `&#10;`。** 原来就是后者，
   * 而实测它出来的是一个**空格**（`charCodeAt` 是 32 不是 10）——
   * 也就是说「tooltip 第二行是构建时间」这句话从来没成立过，三行全挤在一行里。
   * 一条挂在界面上、用来确认「你跑的是哪个构建」的信息，自己却在说谎。
   */
  let projTip = $derived(
    [
      root ?? "还没打开文件夹",
      `lite-ide · 构建于 ${__BUILD_TIME__}`,
      ...(devtools ? ["⚠︎ 调试版：带 Web Inspector，别拿它当正式版用"] : []),
    ].join("\n"),
  );
  $effect(() => {
    let dead = false;
    void devtoolsBuild().then((v) => {
      if (!dead) devtools = v;
    });
    return () => {
      dead = true;
    };
  });

  /** 待确认的模式切换（大文件切到编辑模式时用） */
  let pendingSwitch = $state<TabState | null>(null);

  function requestSwitchMode(tab: TabState) {
    if (tab.dirty) {
      notify.fail("有未保存的改动，请先保存（⌘S）再切换模式", 2600);
      return;
    }
    const to = tab.mode === "edit" ? "log" : "edit";
    // 切到日志模式没有风险（mmap，内存与大小无关）；反方向要看体积
    if (to === "edit" && tab.size > CONFIRM_EDIT_BYTES) {
      pendingSwitch = tab;
      return;
    }
    void doSwitch(tab, to);
  }

  async function doSwitch(tab: TabState, to: "edit" | "log") {
    pendingSwitch = null;
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
        tab.lossy = t.lossy;
      }
      tab.mode = to;
      tab.forced = to;
      auditTabs("切模式");
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

  function requestClose(id: number) {
    const tab = tabs.find((t) => t.id === id);
    if (!tab) return;
    if (tab.dirty) {
      activeId = tab.id;
      pendingClose = tab;
      return;
    }
    doClose(tab);
  }

  /**
   * 批量关闭时还没问过的标签 —— **只装有未保存改动的那些**。
   *
   * 干净的标签在 `closeMany` 里当场就关了，不进队列：为一堆没改动的文件
   * 逐个弹确认框，没有任何信息量。
   */
  let closeQueue = $state<number[]>([]);

  /**
   * 关掉一批标签。干净的直接关，有改动的排队逐个问。
   *
   * **不能直接全关**：标签栏的「关闭其他 / 关闭右侧 / 关闭全部」一按下去，
   * 可能带走好几个正在改的文件，而它们的改动没有任何地方找得回来
   * （不像删文件还进废纸篓）。
   */
  function closeMany(ids: number[]) {
    const dirty: number[] = [];
    for (const id of ids) {
      const t = tabById(id);
      if (!t) continue;
      if (t.dirty) dirty.push(id);
      else doClose(t);
    }
    closeQueue = dirty;
    askNextClose();
  }

  /** 从队列里取下一个来问；队列空了就把横幅收掉 */
  function askNextClose() {
    while (closeQueue.length) {
      const id = closeQueue[0];
      closeQueue = closeQueue.slice(1);
      const t = tabById(id);
      if (!t) continue; // 中途被别处关掉了
      activeId = t.id; // 让人看见要丢的到底是什么
      pendingClose = t;
      return;
    }
    pendingClose = null;
  }

  /**
   * 「保存并关闭 / 丢弃改动 / 取消」三个按钮的落点。
   *
   * 取消**把整批都停掉**，不是只跳过这一个：连着弹五次确认框、每次都得
   * 再点一次取消，比没有批量关闭还烦人。
   */
  async function resolveClose(kind: "save" | "discard" | "cancel") {
    const t = pendingClose;
    if (!t) return;
    if (kind === "cancel") {
      closeQueue = [];
      pendingClose = null;
      return;
    }
    if (kind === "save") {
      activeId = t.id;
      // 写失败就停在这儿，别往下关 —— 关了改动就真没了
      if (!(await save(liveText(t)))) {
        closeQueue = [];
        return;
      }
    }
    doClose(t);
    askNextClose();
  }

  function doClose(tab: TabState) {
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
    if (isScratch(tab.path) && tab.mode === "edit" && !tab.dirty && (tab.content ?? "") === "") {
      void discardEmptyScratch(tab.path).catch(() => {});
    }
    const idx = tabs.findIndex((t) => t.id === tab.id);
    tabs = tabs.filter((t) => t.id !== tab.id);
    if (activeId === tab.id) {
      activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
    }
    pendingClose = null;
    auditTabs("关标签");
  }

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
      quickScope = "all";
      quickSeed = "";
      quickOpen = true;
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
    if (e.key === "Escape" && quickOpen) {
      quickOpen = false;
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
      quickScope = "file";
      quickSeed = "";
      quickOpen = true;
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
   * 菜单项按下去做什么。
   *
   * id 与 `keymap.ts`、`menu.rs` 三处同一套 —— 那两处由
   * `tests/menu_sync.rs` 卡着，这里是第三处，漏一个 case 的表现是
   * 「点了没反应」，所以末尾留了一条 diag。
   */
  async function runMenu(id: string) {
    if (id.startsWith("recent:")) {
      await openRecent(id.slice("recent:".length));
      return;
    }
    switch (id) {
      case "open-folder": return void openFolder();
      case "new-scratch": return void newScratch();
      case "open-scratch-dir": return void openScratchDir();
      case "recent-clear": recent = []; return;
      case "save": return saveActive();
      case "encoding":
        if (active) encOpen = true;
        return;
      case "close-tab":
        if (active) requestClose(active.id);
        return;
      case "close-all-tabs": return closeMany(tabs.map((t) => t.id));
      case "toggle-mode":
        if (active) requestSwitchMode(active);
        return;
      case "quick-all": quickScope = "all"; quickSeed = ""; quickOpen = true; return;
      case "quick-file": quickScope = "file"; quickSeed = ""; quickOpen = true; return;
      case "quick-content": quickScope = "content"; quickSeed = ""; quickOpen = true; return;
      case "find-word": return findWordAtCursor();
      case "nav-back": return void navGo("back");
      case "nav-fwd": return void navGo("fwd");
      case "outline": return openOutline();
      case "toggle-sidebar": sidebar = !sidebar; return;
      case "toggle-panel": panel = !panel; return;
      case "toggle-minimap": showMinimap = !showMinimap; return;
      case "new-terminal": return newTerm();
      case "close-terminal":
        if (activeTermId !== null) closeTerm(activeTermId);
        return;
      case "git-changes":
        // 已经在 Git 视图上再点一次就切回去，和 ⇧⌘G 是同一个手势
        sideView = sidebar && sideView === "git" ? "files" : "git";
        sidebar = true;
        return;
      case "git-file-diff": {
        const en = activeEntry;
        if (en) void openDiff(en, false);
        else notify.fail("当前文件没有未提交的改动", 2600);
        return;
      }
      case "git-log": panel = true; panelView = "log"; return;
      case "git-console": panel = true; panelView = "git"; return;
      case "git-branches": openBranchPicker(); return;
      case "git-refresh": return void refreshGit();
      case "git-pull": return void doPull();
      case "git-push": return void askPush();
      case "git-fetch": return void doFetch("fetch");
      case "help-keys":
        keysPanel.load();
        keysOpen = true;
        return;
      case "help-repo": return void openRepoPage();
      case "help-log": return void openAppLog();
      case "help-log-clear": return void clearLog();
      default:
        diag(`菜单项 ${id} 没有对应的处理`);
    }
  }

  /**
   * 拖拽期间关掉列宽过渡。
   * 收起/展开侧边栏时有个 130ms 的过渡看着舒服，但拖拽时每一帧都在改宽度，
   * 带着过渡就是一路追不上手的橡皮筋感。
   */
  let resizing = $state(false);

  /** 侧边栏横向拖拽。上限留出编辑区的活路，不让它被挤没 */
  function startSideResize(e: PointerEvent) {
    e.preventDefault();
    resizing = true;
    const startX = e.clientX;
    const startW = sidebarWidth;
    const move = (ev: PointerEvent) => {
      sidebarWidth = Math.max(140, Math.min(window.innerWidth - 360, startW + (ev.clientX - startX)));
    };
    const up = () => {
      resizing = false;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startY = e.clientY;
    const startH = panelHeight;
    const move = (ev: PointerEvent) => {
      // 往上拖变高：面板贴在底部，位移要反号
      panelHeight = Math.max(90, Math.min(window.innerHeight - 200, startH - (ev.clientY - startY)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }

  $effect(() => {
    // 见 `scratchRoot` 的注释：故意不挂在启动那条 await 链上
    void scratchDir()
      .then((d) => (scratchRoot = d))
      .catch(() => {});
    initialPath()
      .then(async (p) => {
        if (tabs.length > 0 || root !== null) return;
        if (!p) {
          await restoreSession();
          return;
        }
        /*
         * 命令行（或拖到图标上）指名了路径。分两种情况：
         *
         * - 指的是**文件**：先把上次的现场恢复出来，再把这个文件开在上面。
         *   `lite-ide a.rs` 的意思是「顺手看一眼这个文件」，不是
         *   「把我的工作区清空」—— VS Code 的 `code a.js` 就是这个行为。
         * - 指的是**另一个目录**：那是在切项目，旧项目的标签铺过来只会碍事。
         *   同一个目录则照常恢复。
         */
        const info = await probePath(p).catch(() => null);
        const switchingProject = info?.kind === "dir" && info.path !== saved?.root;
        if (!switchingProject) await restoreSession();
        await openPath(p);
      })
      .catch(() => {})
      .finally(() => {
        restoring = false;
        scheduleSave();
        void writeBudgetLine();
      });
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
        tabs.length,
        terms.length,
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
            for (const p of e.payload.paths) void openPath(p);
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
   * 最近列表变了就推给 Rust 重建子菜单。
   *
   * 只在 Tauri 里有意义（浏览器里 invoke 走的是桩），失败一律吞掉 ——
   * 菜单没刷新是件不影响干活的事，不值得弹一条错误。
   */
  $effect(() => {
    void setRecent([...recent]).catch(() => {});
  });

  /**
   * 让用不上的菜单项变灰。
   *
   * 这是加菜单栏白捡的：今天所有键位都是 window 级监听，**不管当下
   * 有没有意义都会触发** —— 没有标签时按 ⌘S、不是 Git 仓库时按 ⇧⌘G，
   * 都是走一遍然后什么也没发生。灰掉的菜单项本身就是一句解释。
   */
  $effect(() => {
    void syncMenuState(active !== null, repo !== null, activeTermId !== null).catch(() => {});
  });

  /**
   * 项目根换了就记一笔。
   *
   * 放 effect 里而不是在 `openPath` 里调，是因为 root 有四条来路
   * （拖放、命令行、面包屑、菜单）—— 挂在赋值点上要写四遍，
   * 而**写四遍就等于早晚漏一遍**。
   */
  $effect(() => {
    const r = root;
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
    untrack(() => remember(r));
  });

</script>

<svelte:window onkeydown={onWindowKey} onkeyup={onWindowKeyUp} />

{#if keysPanel.comp}
  <keysPanel.comp bind:open={keysOpen} />
{/if}

{#if overlays.comps.outline}
  <overlays.comps.outline
    bind:open={outlineOpen}
    {symbols}
    fileName={active?.name ?? ""}
    supported={outlineSupported}
    onPick={(line) => (gotoLine = { line, nonce: ++gotoNonce })}
  />
{/if}

{#if overlays.comps.quick}
  <overlays.comps.quick
    bind:open={quickOpen}
    bind:scope={quickScope}
    seed={quickSeed}
    {root}
    {actions}
    onOpenFile={openAt}
  />
{/if}

{#if encPicker.comp && active}
  <encPicker.comp
    bind:open={encOpen}
    current={active.encoding ?? "UTF-8"}
    bom={!!active.bom}
    lossy={!!active.lossy}
    readonly={active.mode !== "edit"}
    onReopen={(l) => void reopenWith(l)}
    onSaveAs={saveAsEncoding}
  />
{/if}

{#if git.comps.branch && repo}
  <git.comps.branch
    bind:open={branchOpen}
    anchor={branchAnchor}
    {repo}
    ahead={gitSt?.ahead ?? 0}
    behind={gitSt?.behind ?? 0}
    onSwitch={(n) => switchBranch(n)}
    onNewBranch={(n) => switchBranch(n, true)}
    onOpenWorktree={(p) => void openWorktree(p)}
    onNewWorktree={newWorktree}
    onRemoveWorktree={(w) => (pendingWtRemove = w)}
  />
{/if}

{#if projMenu}
  <ContextMenu
    x={projMenu.x}
    y={projMenu.y}
    label="项目"
    items={projMenuItems}
    onclose={() => (projMenu = null)}
  />
{/if}

{#if panelMenu}
  <ContextMenu
    x={panelMenu.x}
    y={panelMenu.y}
    label={panelMenu.kind === "list" ? "全部终端" : "终端的操作"}
    items={panelMenuItems}
    onclose={() => (panelMenu = null)}
  />
{/if}

<main class:hovering>
  <!--
    标题栏 = IDEA 的 main toolbar：**「哪个项目 / 哪个分支」，只有这两件事。**

    面包屑原来在这儿（那时的理由是「顺带占掉右边那块常年空着的地方」），
    2026-09-06 搬到状态栏左边去了 —— IDEA 的导航栏就在那儿，而且路径属于
    「我在哪个文件」，和底下那排文件状态是同一组信息。
    腾出来的右边不是浪费，是**窗口拖动区**。
  -->
  <header class="titlebar" data-tauri-drag-region>
    <!--
      项目挂件。没打开项目时显示应用名 —— **按钮位置在两个状态下完全一致**，
      和导轨上那条「控件的位置必须是肌肉记忆能记住的」是同一条。

      tooltip 第二行是构建时间，别删：报上来的 bug 复现不了时，
      第一件事就是确认对方跑的是哪个构建（为此白查过一次代码）。
      它原来挂在这儿那个 `lite-ide` 字样上，而那个字样现在只有空项目时才出现。
    -->
    <button class="twidget proj" onclick={(e) => openProjMenu(e)} title={projTip}>
    >
      <span class="sq" aria-hidden="true">{projInitial}</span>
      <span class="wlabel">{projName}</span>
      <Icon name="chevron-down" size={10} />
    </button>
    {#if gitSt}
      <button
        class="twidget"
        class:on={branchOpen}
        bind:this={branchBtn}
        onclick={openBranchPicker}
        title="切换分支 / 工作树"
      >
        <Icon name="git" size={12} />
        <span class="wlabel">{gitSt.branch || "游离"}</span>
        {#if gitSt.ahead}<span class="ab">↑{gitSt.ahead}</span>{/if}
        {#if gitSt.behind}<span class="ab">↓{gitSt.behind}</span>{/if}
        <Icon name="chevron-down" size={10} />
      </button>
    {/if}
    <span class="tgap" data-tauri-drag-region></span>
  </header>

  <div
    class="workspace"
    class:no-side={!sidebar}
    class:resizing
    style:--side-w="{sidebarWidth}px"
  >
    <!--
      常驻的工具竖条。所有侧边栏控件都住在这里，收起侧边栏时竖条留着 ——
      于是按钮在两个状态下位置完全一致。

      早先的做法是「展开时按钮在侧边栏头部右侧、收起时在标题栏左边」，
      结果同一个按钮在两个状态间横跳约 290 像素，每次都要重新找它在哪。
      控件的位置必须是肌肉记忆能记住的。
    -->
    <nav class="rail" aria-label="侧边栏工具">
      <button
        class="rbtn"
        class:on={sidebar}
        onclick={() => (sidebar = !sidebar)}
        title={sidebar ? "收起侧边栏 ⌘1" : "展开侧边栏 ⌘1"}
        aria-label={sidebar ? "收起侧边栏" : "展开侧边栏"}
        aria-expanded={sidebar}
      >
        <Icon name="sidebar" />
      </button>
      {#if root}
        <button
          class="rbtn"
          class:on={sidebar && sideView === "files"}
          onclick={() => {
            sideView = "files";
            sidebar = true;
          }}
          title="文件树"
          aria-label="文件树"
        >
          <Icon name="files" />
        </button>
        {#if repo}
          <button
            class="rbtn"
            class:on={sidebar && sideView === "git"}
            onclick={() => {
              sideView = "git";
              sidebar = true;
            }}
            title="Git 改动 ⌘⇧G"
            aria-label="Git 改动"
          >
            <Icon name="git" />
            <!--
              角标带数字。原来是个不带数字的 5px 圆点，而改动条数印在状态栏的
              「改动 9」按钮上 —— 那个按钮和这个图标是同一件事的两份入口，
              删掉按钮时计数不能跟着一起没。IDEA 的提交工具窗图标就是这么标的。
              99 是上限：三位数会把 26px 的按钮撑变形，而「到底是 128 还是 132」
              在这个位置上没人要看。
            -->
            {#if gitSt && gitSt.entries.length > 0}
              <span class="badge">{gitSt.entries.length > 99 ? "99+" : gitSt.entries.length}</span>
            {/if}
          </button>
        {/if}
        <button
          class="rbtn"
          onclick={() => {
            quickScope = "content";
            quickSeed = "";
            quickOpen = true;
          }}
          title="在项目中搜内容 ⌘⇧F"
          aria-label="搜索"
        >
          <Icon name="search" />
        </button>
      {/if}
      <span class="rgap"></span>
      <!--
        底部工具窗的开关住在导轨上，和上面的文件树 / Git 改动同一套。

        原来这里是一个笼统的「面板」开关，而**切哪个工具窗**摆在面板头上 ——
        于是「换一个工具窗」这件事在同一个应用里有两种长相：侧边栏在导轨上换，
        底部在面板头上换。IDEA 只有一处，就是导轨；面板头腾出来留给
        工具窗自己的名字和它的标签页。

        点当前这个就收起 —— 和最上面 sidebar 那个开关是同一个手势。
      -->
      <button
        class="rbtn"
        class:on={panel && panelTool === "term"}
        onclick={() => togglePanelView("term")}
        title="终端 ⌘J"
        aria-label="终端"
      >
        <Icon name="terminal" />
      </button>
      {#if repo}
        <button
          class="rbtn"
          class:on={panel && panelTool === "log"}
          onclick={() => togglePanelView("log")}
          title="提交历史"
          aria-label="提交历史"
        >
          <Icon name="history" />
        </button>
        <!--
          Git 控制台（issue #29）。和提交历史一样只在有仓库时出现 ——
          没有仓库时它永远是空的，一个永远空着的按钮只是噪音。
        -->
        <button
          class="rbtn"
          class:on={panel && panelTool === "git"}
          onclick={() => togglePanelView("git")}
          title="Git 控制台：跑过的每一条 git"
          aria-label="Git 控制台"
        >
          <Icon name="git" />
        </button>
      {/if}
    </nav>

    {#if sidebar}
      <aside>
        <svelte:boundary>
        {#if !root}
          <div class="no-root">把文件夹拖进来</div>
        {:else if sideView === "git" && repo && git.comps.pane}
          <git.comps.pane
            status={gitSt}
            busy={gitBusy}
            onOpenDiff={(e, staged) => void (e.conflicted ? openMerge(e) : openDiff(e, staged))}
            onStage={(paths) => void gitDo("暂存失败", () => gitStage(repo!, paths), "暂存")}
            onUnstage={(paths) =>
              void gitDo("取消暂存失败", () => gitUnstage(repo!, paths), "取消暂存")}
            onDiscard={(es) => (pendingDiscard = es)}
            onCommit={doGitCommit}
            onRefresh={() => void refreshGit()}
            onOpenBranches={openBranchPicker}
            onOpenLog={() => {
              panelView = "log";
              panel = true;
            }}
            ahead={gitSt?.ahead ?? 0}
            behind={gitSt?.behind ?? 0}
            onSync={(what) => void (what === "push" ? askPush() : doPull())}
            syncing={syncing ? { what: syncing.what, phase: syncing.phase, percent: syncing.percent } : null}
            onCancelSync={syncing && syncing.what !== "push" ? cancelSync : null}
          />
        {:else if sideView === "git" && repo}
          <div class="no-root">正在载入 Git 面板…</div>
        {:else}
          <FileTree
            {root}
            activePath={active?.path ?? ""}
            gitStatus={gitSt}
            {ignored}
            reloadTick={treeTick}
            {revealPath}
            {revealTick}
            onOpen={(p) => void openPath(p)}
            {dirtyUnder}
            onCreated={(p, isDir) => void afterFsChange(isDir ? null : p)}
            onRenamed={(from, to, isDir) =>
              void renameOpenTabs(from, to, isDir).then(() => afterFsChange(null))}
            onTrashed={(p, isDir) => {
              closeTabsUnder(p, isDir);
              void afterFsChange(null);
            }}
          />
        {/if}
        {#snippet failed(err, reset)}
          <Crash error={err} scope="侧边栏" onReset={reset} />
        {/snippet}
        </svelte:boundary>
      </aside>
      <div
        class="side-resizer"
        role="separator"
        aria-label="调整侧边栏宽度"
        aria-orientation="vertical"
        onpointerdown={startSideResize}
      ></div>
    {/if}

    <section class="main">
      {#if tabs.length > 0}
        <Tabs
          {tabs}
          {activeId}
          root={root ?? ""}
          onSelect={(id) => {
            activeId = id;
            auditTabs("切标签");
          }}
          onClose={requestClose}
          onCloseMany={closeMany}
          onRevealInTree={revealInTree}
          onNewScratch={newScratch}
        />
      {/if}

      {#if active?.conflict}
        <div class="confirm conflict">
          <span><b>{active.name}</b> 在编辑器外被改过，而你这边也有未保存的改动</span>
          <button class="primary" onclick={() => resolveConflict(active!, "mine")}>保留我的</button>
          <button onclick={() => resolveConflict(active!, "disk")}>用磁盘上的</button>
        </div>
      {/if}

      {#if pendingSwitch}
        <div class="confirm">
          <span>
            <b>{pendingSwitch.name}</b> 有 {(pendingSwitch.size / 1048576).toFixed(1)}MB，
            编辑模式会把全文读进内存，可能明显卡顿
          </span>
          <button class="primary" onclick={() => doSwitch(pendingSwitch!, "edit")}>仍然编辑</button>
          <button onclick={() => (pendingSwitch = null)}>取消</button>
        </div>
      {/if}

      {#if notify.banner}
        <div class="confirm err-banner">
          <span class="btext">
            <b>{notify.banner.title}</b>
            <span class="bbody">{notify.banner.body}</span>
          </span>
          <button onclick={() => notify.closeBanner()}>知道了</button>
        </div>
      {/if}

      {#if pendingWtRemove}
        <div class="confirm danger">
          <span>
            要移除工作树 <b>{pendingWtRemove.path}</b> 吗？
            <b>那个目录会被删掉</b>，里面未提交的改动会一起没
          </span>
          <button class="danger" onclick={() => doRemoveWorktree(pendingWtRemove!, false)}>移除</button>
          <button class="danger" onclick={() => doRemoveWorktree(pendingWtRemove!, true)}>强制移除</button>
          <button onclick={() => (pendingWtRemove = null)}>取消</button>
        </div>
      {/if}

      {#if pendingCheckout}
        <!--
          这不是错误横幅，是一个选择题 —— 所以它长得和「丢弃改动」「关闭脏标签」
          一样，不是 err-banner。git 拒绝切分支这件事本身没什么可报的，
          真正要说的是「这几个文件挡着，你打算怎么办」。
        -->
        <div class="confirm">
          <span>
            切到 <b>{pendingCheckout.name}</b> 会覆盖
            <b>{pendingCheckout.files.length} 个文件</b>的改动：
            <span class="rest">{pendingCheckout.files.slice(0, 3).join("、")}{pendingCheckout.files.length > 3 ? " …" : ""}</span>
          </span>
          <button
            class="primary"
            onclick={() => {
              pendingCheckout = null;
              sideView = "git";
              sidebar = true;
            }}
          >去提交</button>
          <button class="danger" onclick={() => void discardThenCheckout()}>丢弃这些改动并切换</button>
          <button onclick={() => (pendingCheckout = null)}>取消</button>
        </div>
      {/if}

      {#if pendingDiscard}
        <div class="confirm danger">
          <span>
            要丢弃
            {#if pendingDiscard.length === 1}
              <b>{pendingDiscard[0].path}</b>
            {:else}
              <b>{pendingDiscard.length} 个文件</b>
            {/if}
            的改动吗？未跟踪的文件会被直接删除，<b>这一步不可撤销</b>
          </span>
          <button class="danger" onclick={() => void doDiscard(pendingDiscard!)}>丢弃</button>
          <button onclick={() => (pendingDiscard = null)}>取消</button>
        </div>
      {/if}

      {#if git.comps.bars && (pendingDiverge || pendingPush || remoteErr)}
        <git.comps.bars
          diverge={pendingDiverge}
          push={pendingPush}
          err={remoteErr}
          upstream={gitSt?.upstream ?? ""}
          ahead={gitSt?.ahead ?? 0}
          onMerge={(mode, rem) => {
            if (rem) lastMergeMode = mode;
            pendingDiverge = null;
            void doPull(mode);
          }}
          onPush={() => void doPush()}
          onPull={() => {
            remoteErr = null;
            void doPull();
          }}
          onDismiss={(which) => {
            if (which === "diverge") pendingDiverge = null;
            else if (which === "push") pendingPush = null;
            else remoteErr = null;
          }}
        />
      {/if}

      {#if pendingClose}
        <div class="confirm">
          <span><b>{pendingClose.name}</b> 有未保存的改动</span>
          {#if closeQueue.length}
            <!-- 批量关闭时要说清后面还有几个，否则人不知道这个框还要弹几次 -->
            <span class="rest">（后面还有 {closeQueue.length} 个）</span>
          {/if}
          <button class="primary" onclick={() => void resolveClose("save")}>保存并关闭</button>
          <button onclick={() => void resolveClose("discard")}>丢弃改动</button>
          <button onclick={() => void resolveClose("cancel")}>取消</button>
        </div>
      {/if}

      <!--
        内容区单独设边界：编辑器 / 日志 / 差异里任何一处抛异常，
        都不该把整个外壳一起带走 —— 文件树、终端、状态栏还得能用。
        boundary 的 reset 会重建这棵子树，多数一次性的渲染错误重试一下就好了。
      -->
      <svelte:boundary onerror={(e) => notify.fail(`内容区出错：${e}`)}>
      <div class="content">
        {#if !active}
          <!--
            收进一张卡片。原本是四行居中文字铺在整个内容区里 —— 1440 宽的窗口上
            读起来是散的，眼睛没有落点。快捷键排成两列之后它才像个「起点」。
          -->
          <div class="empty">
            <div class="card">
              <div class="big">打开一个文件夹开始</div>
              <p>也可以直接把文件或文件夹拖进来 —— 代码走编辑模式，大文件与日志自动走只读的日志模式</p>
              <div class="go">
                <button class="primary" onclick={() => void openFolder()}>打开文件夹…</button>
                <kbd>⌘O</kbd>
                <span class="gap"></span>
                {#if recent.length > 0}
                  <span class="lastly">最近：</span>
                  <button class="link" onclick={() => void openRecent(recent[0])}>
                    {recent[0].slice(recent[0].lastIndexOf("/") + 1) || recent[0]}
                  </button>
                {/if}
              </div>
              <!--
                这份表原来是**手抄的第三份**，而且抄错了：⌘⇧F / ⌘⇧O / ⌘⇧G
                三处修饰键次序都反了（Apple 的次序是 ⌃⌥⇧⌘）。
                现在从 keymap.ts 渲染 —— 那张表由 tests/keymap.test.ts 卡着次序。
              -->
              <div class="keymap">
                {#each keyHints as k (k.id)}
                  <span><b>{k.gesture ?? k.accel}</b> {k.label}</span>
                {/each}
              </div>
              {#if notify.error}<p class="err">{notify.error}</p>{/if}
            </div>
          </div>
        {:else if active.mode === "merge" && git.comps.merge}
          {#key active.id}
            <git.comps.merge
              text={active.mergeText ?? ""}
              path={active.rel ?? active.name}
              onResolve={(c, r) => void resolveMerge(active!, c, r)}
            />
          {/key}
        {:else if active.mode === "merge"}
          <div class="empty"><p>正在载入合并视图…</p></div>
        {:else if active.mode === "diff" && git.comps.diff}
          {#key active.id}
            <git.comps.diff
              raw={active.diffRaw ?? ""}
              capped={!!active.diffCapped}
              path={active.rel ?? active.name}
              staged={!!active.diffStaged}
              commit={active.diffShort ?? ""}
              untracked={!!active.diffUntracked}
              onToggleStaged={() => void toggleDiffSide(active!.id)}
            />
          {/key}
        {:else if active.mode === "diff"}
          <div class="empty"><p>正在载入差异视图…</p></div>
        {:else if active.mode === "log" && active.handle !== undefined && logPane.comp}
          {#key active.id}
            <logPane.comp
              handle={active.handle}
              {gotoLine}
              encoding={active.encoding ?? "utf-8"}
              onStatus={(s) => (logStatus = s)}
              onTop={(l) => markPos(active!.path, l)}
            />
          {/key}
        {:else if active.mode === "log"}
          <div class="empty"><p>正在载入日志视图…</p></div>
        {:else if editor.comp}
          {#key active.id}
            <editor.comp
              path={active.path}
              initial={active.draft ?? active.content ?? ""}
              baseline={active.content ?? ""}
              {savedTick}
              {gotoLine}
              {outlineTick}
              marks={editorMarks}
              {showMinimap}
              onChange={(d) => (active!.dirty = d)}
              onSave={save}
              onStash={stashDraft}
              onLive={onEditorLive}
              onWordProbe={onEditorWordProbe}
              onOutline={(s) => (symbols = s)}
              onCursor={(l) => markPos(active!.path, l)}
              jumpFiles={projectFiles}
              jumpRel={root && active.path.startsWith(`${root}/`)
                ? active.path.slice(root.length + 1)
                : null}
              jumpLang={langs?.langOf(active.path) ?? ""}
              onJump={(hit) => void jumpTo(hit)}
            />
          {/key}
        {:else}
          <div class="empty"><p>正在载入编辑器…</p></div>
        {/if}
      </div>

      {#snippet failed(err, reset)}
        <div class="content">
          <Crash error={err} scope={active ? `${active.name} 的视图` : "内容区"} onReset={reset} />
        </div>
      {/snippet}
      </svelte:boundary>

      <!--
        条件是 `panel || terms.length > 0`，不是 `panel`。

        收起面板**不能卸载**这一块：组件一销毁 Session 就 drop，shell 被 kill。
        跑着 gradle build 的时候按 ⌘J 腾点地方，构建就没了 —— 而且没有任何提示。
        （下面切 Git 日志页那处早就想到了这一层，这里漏了一级。）

        `terms.length > 0` 那半边保证「从没开过终端」时不会白挂一块 DOM，
        也保证关掉最后一个终端后这块能真正消失（closeTerm 会清空 terms）。
      -->
      {#if panel || terms.length > 0}
        <div
          class="resizer"
          class:hidden={!panel}
          role="separator"
          aria-label="调整终端高度"
          onpointerdown={startResize}
        ></div>
        <div class="panel" class:hidden={!panel} style:height="{panelHeight}px">
          <!--
            工具窗的头：**名字在最左，标签页跟在后面，动作靠右**。

            工具窗之间的切换不在这里（在导轨上），所以这一行只讲一件事：
            「你现在看的是哪个工具窗、它有哪几个标签页」。名字比标签亮一档 ——
            面板收起再展开时，第一眼要能认出这是哪个工具窗。
          -->
          <div class="panel-head">
            <span class="tw-name">
              {panelTool === "term" ? "终端" : panelTool === "git" ? "Git 控制台" : "提交历史"}
            </span>
            {#if panelTool === "term"}
              <div class="ptabs">
                {#each terms as t (t.id)}
                  <div class="ptab" class:on={t.id === activeTermId}>
                    <button class="pt-label" onclick={() => (activeTermId = t.id)} title={t.cwd}>
                      {t.title}
                    </button>
                    <button
                      class="pt-x"
                      onclick={() => closeTerm(t.id)}
                      aria-label="关闭 {t.title}"
                      title="关闭 {t.title}"
                    >✕</button>
                  </div>
                {/each}
              </div>
              <button
                class="phbtn"
                onclick={() => newTerm()}
                title="新建终端 ⌃⇧`"
                aria-label="新建终端"
              >
                <Icon name="plus" />
              </button>
              <!--
                标签页多到溢出时，横向滚动条是看不见的（高度 0）——
                这个下拉是唯一能一眼看全、并且直接跳过去的路
              -->
              {#if terms.length > 1}
                <button
                  class="phbtn"
                  onclick={(e) => openPanelMenu(e, "list")}
                  title="全部终端"
                  aria-label="全部终端"
                >
                  <Icon name="chevron-down" />
                </button>
              {/if}
            {/if}
            <span class="gap"></span>
            <!--
              「更多」只在终端页出 —— 提交历史那边一条真动作都没有，
              摆一个点开是空的按钮，比没有这个按钮糟。
            -->
            {#if panelTool === "term" && terms.length > 0}
              <button
                class="phbtn"
                onclick={(e) => openPanelMenu(e, "more")}
                title="更多操作"
                aria-label="更多操作"
              >
                <Icon name="more-v" />
              </button>
            {/if}
            <button
              class="phbtn"
              onclick={() => (panel = false)}
              title="收起 ⌘J"
              aria-label="收起面板"
            >
              <Icon name="minus" />
            </button>
          </div>
          <div class="panel-body">
            <!--
              终端整块只藏不卸载：组件一销毁 Session 就 drop，shell 直接被 kill。
              切到 Git 日志页时正在跑的命令必须还在跑。
            -->
            <div class="tool-slot" class:hidden={panelTool !== "term"}>
              {#if terminal.comp}
                {#each terms as t (t.id)}
                  <div class="term-slot" class:hidden={t.id !== activeTermId}>
                    <terminal.comp cwd={t.cwd} onExit={() => closeTerm(t.id)} />
                  </div>
                {/each}
              {:else}
                <div class="loading">正在载入终端…</div>
              {/if}
            </div>
            <!-- 收起时别去拉 git log：那是一串没人看的子进程 -->
            <!-- 同上：切走就整个销毁，那条 1.5 秒的轮询跟着停 -->
            {#if panel && panelTool === "git" && repo}
              <div class="tool-slot">
                {#if gitcon.comp}
                  <gitcon.comp />
                {:else}
                  <div class="loading">正在载入 Git 控制台…</div>
                {/if}
              </div>
            {/if}
            {#if panel && panelTool === "log" && repo}
              <div class="tool-slot">
                {#if git.comps.log}
                  <git.comps.log
                    {repo}
                    filePath={active?.mode === "edit" ? active.path : ""}
                    onOpenCommitDiff={(sha, short, p) => void openCommitDiff(sha, short, p)}
                  />
                {:else}
                  <div class="loading">正在载入 Git 日志…</div>
                {/if}
              </div>
            {/if}
          </div>
        </div>
      {/if}
    </section>
  </div>

  <!--
    状态栏 = IDEA 的 status bar，**左右两半各管一件事**：

    - 左：我在哪个文件（导航栏 / 面包屑）。IDEA 里不用导航栏时这块显示最近的
      事件消息 —— 这里照抄：`notify` 一来就顶掉路径。以前提示消息挤在挂件中间，
      窗口一窄它先被挤掉，而它恰恰是最该让人看见的。
    - 右：这个文件什么状态，而且**点了都能改**（模式 / 编码 / 差异）。

    这里以前还挂着「搜索 ⇧⇧」「终端 ⌘J」「改动 N」「历史」四个 —— 全是**打开某个
    工具窗**，而那四件事导轨上一个不落地都有（搜索还有双击 ⇧）。同一件事在一屏里
    说两遍，正是上一轮「工具窗切换只能有一处」那条判据本身。
    「改动 N」的计数没丢，挪到导轨 Git 图标的角标上了。
  -->
  <footer class="statusbar">
    <!-- 左槽 -->
    <!--
      **「正在做」排在最前面。** 它是唯一一条「事情还没完」的消息，
      而另外两条说的都是已经完了。操作跑着的时候被一条旧的「已保存」
      顶掉，等于把界面上唯一能证明「它在动」的东西藏起来 ——
      那正是 issue #15 要修的形状。
    -->
    {#if notify.doing}
      <!--
        **整句放进一个表达式，不要写成 `正在{notify.doing}…`。**
        那样 Svelte 会生成三个文本节点，在 macOS 的辅助功能树里就是三段
        独立的 static text，读屏和自动化都拼不回一句话 ——
        scripts/smoke.sh 里按「正在提交」找了半天找不到，就是这么回事。
      -->
      <span class="cell doing navslot">{`正在${notify.doing}…`}</span>
    {:else if notify.info}
      <span class="cell ok navslot">{notify.info}</span>
    {:else if notify.error}
      <span class="cell err navslot">{notify.error}</span>
    {:else if crumbs.length > 0}
      <nav class="crumbs navslot" aria-label="当前文件路径">
        {#each crumbs as c, i (c.path)}
          {#if i > 0}<span class="sep" aria-hidden="true">›</span>{/if}
          {#if c.dir}
            <button class="crumb" onclick={() => revealInTree(c.path)} title="在文件树中显示 {c.path}">{c.name}</button>
          {:else}
            <span class="crumb here" title={c.path}>{c.name}</span>
          {/if}
        {/each}
      </nav>
    {:else}
      <span class="cell dim navslot">{root ? projName : "等待文件夹"}</span>
    {/if}
    <span class="spacer"></span>

    <!-- 右槽 -->
    {#if active?.mode === "merge"}
      <span class="cell warn">冲突合并</span>
    {:else if active?.mode === "diff"}
      <span class="cell dim">
        {active.diffSha ? `提交 ${active.diffShort}` : `差异 · ${active.diffStaged ? "已暂存" : "未暂存"}`}
      </span>
    {:else if active}
      <!--
        **这个按钮只在日志场景出现。**

        它原来对每一个打开的文件都在，而绝大多数文件根本不存在「切到日志模式」
        这个需求 —— 一个 `.ts` 切过去只会得到一份没高亮、不能编辑的文本。
        一个永远在、九成场合按下去只有坏处的按钮，等于白占了状态栏一格。

        两个条件：**已经在日志模式**（那必须留着回去的路，否则单向门），
        或者**文件名看着像日志**（判据在 `is-log-name.ts`，纯按名字，不看内容）。

        藏起来不等于做不了 —— 菜单里的「切换编辑 / 日志模式」对任何文件都还在。
      -->
      {#if active.mode === "log" || isLogName(active.path)}
        <button
          class="cell btn mode"
          onclick={() => requestSwitchMode(active!)}
          title={active.mode === "log" ? "切换到编辑模式" : "切换到日志模式（只读，带级别过滤与 tail）"}
        >
          {active.mode === "log" ? "日志模式" : "编辑模式"} ⇄
        </button>
        <!--
          **竖线跟着它后面那格一起退场。**

          窄窗口下 `drop-2` 会藏掉语言、只读原因、保存状态，而竖线原来是
          独立的、不带 drop 类的 —— 于是 640px 宽时状态栏上出现两条挨着的竖线，
          末尾还吊着一条后面什么都没有的。分隔线分的是「区」，区没了线也该没。

          它也在 `{#if}` 里面：按钮不在时这条线就成了开头那条，
          左边什么都没有 —— 同一个毛病，换了个位置。
        -->
        <span class="vsep drop-2" aria-hidden="true"></span>
      {/if}
      {#if active.mode === "log"}
        <!-- 「为什么是只读」原来在标题栏。它说的是当前文件的状态，该和别的状态挂件在一起 -->
        <span
          class="cell dim drop-2"
          title={active.forced ? "你手动切到了日志模式" : "自动判定的原因"}
        >只读 · {active.forced ? "手动切换" : active.reason || "自动判定"}</span>
      {:else}
        <span class="cell dim drop-2">{langs ? langs.langLabel(langs.langOf(active.path)) : ""}</span>
      {/if}
      <span class="vsep" aria-hidden="true"></span>
      <button
        class="cell btn enc"
        class:bad={active.lossy}
        onclick={() => (encOpen = true)}
        title={active.lossy
          ? "有解不出的字节，点这里换个编码重新打开"
          : "文件编码 —— 点击可换编码重新打开或另存"}
      >
        {active.encoding ?? "UTF-8"}{active.bom ? " ·BOM" : ""}{active.lossy ? " ⚠" : ""}
      </button>
      {#if active.mode === "log"}
        <span class="vsep" aria-hidden="true"></span>
        <span class="cell">{logStatus}</span>
      {:else}
        <span class="vsep drop-2" aria-hidden="true"></span>
        <span class="cell drop-2" class:accent={active.dirty}>
          {active.dirty ? "已修改" : "无改动"}
        </span>
      {/if}
      {#if activeEntry}
        <span class="vsep" aria-hidden="true"></span>
        <button
          class="cell btn git"
          onclick={() => void openDiff(activeEntry!, false)}
          title="查看这个文件的改动"
        >
          <!--
            这里说的是「相对 git 有没有未提交的改动」，跟左边那格的
            「无改动 / 已修改」（缓冲区有没有未保存的编辑）是两件事。
            原本写「有改动」，于是状态栏上会并排出现「无改动」和「有改动」，
            读起来自相矛盾。改成「未提交」，两格就能同时成立且不打架。
          -->
          {activeEntry.untracked ? "未跟踪" : "未提交"}
        </button>
      {/if}
    {/if}
  </footer>
</main>

<style>
  main {
    height: 100%;
    display: grid;
    grid-template-rows: 38px 1fr 24px;
    /*
     * **不要在这儿画底。** 窗口的底是 Rust 侧挂的那块 NSVisualEffectView，
     * 这里填任何不透明色都会把它整块盖住 —— 表现是「vibrancy 没生效」，
     * 而 Rust 侧一切正常，从那头查不出来。
     * 该挡光的是内容层（编辑器 / 日志 / 终端），它们各自画自己的。
     */
    background: transparent;
  }
  main.hovering { outline: 2px solid var(--accent); outline-offset: -2px; }

  .titlebar {
    display: flex;
    align-items: center;
    gap: 8px;
    /* 给 macOS 红绿灯让位 */
    padding: 0 12px 0 78px;
    /* 贴着窗口上边，窗口阴影在这条边上最弱 —— 浅色壁纸下不压一层，小字糊进桌面 */
    background: var(--chrome-scrim);
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
    user-select: none;
  }
  /* tgap 是**拖动区**，不是留白 —— 面包屑搬走之后这一大片正是拿窗口的地方 */
  .titlebar .tgap { flex: 1; min-width: 12px; }

  /*
   * 标题栏挂件（项目 / 分支）。两个长得一模一样，**中间不画竖线** ——
   * 线只分区不分项，而它们本来就是同一组「我现在在哪个项目的哪个分支上」。
   */
  .twidget {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    max-width: 240px;
    height: 24px;
    padding: 0 6px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .twidget:hover { background: var(--hover); color: var(--text-dim); }
  /* 浮层开着时挂件保持点亮 —— 否则那块浮层看着像凭空冒出来的 */
  .twidget.on { background: var(--selected); color: var(--text-dim); }
  .twidget:active { background: var(--pressed); }
  .twidget:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .twidget .wlabel {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .twidget.proj .wlabel { color: var(--text); }
  /*
   * 项目名首字的方块。IDEA 的项目挂件就是这个形状，它的用处不是装饰：
   * 同时开着两个窗口时，一眼认出「这个窗口是哪个项目」靠的是这个色块，
   * 不是去读那几个字。
   */
  .twidget .sq {
    flex: none;
    display: grid;
    place-content: center;
    width: 16px;
    height: 16px;
    border-radius: 5px;
    background: var(--selected);
    color: var(--text);
    font-size: 9.5px;
    font-weight: 600;
  }
  .twidget:hover .sq { background: var(--pressed); }

  /*
   * 面包屑。2026-09-06 从标题栏搬到状态栏左边 —— IDEA 的导航栏就在那儿，
   * 而且「我在哪个文件」和右边那排「这个文件什么状态」是同一组信息。
   */
  .statusbar .crumbs {
    display: flex;
    align-items: center;
    gap: 3px;
    /*
     * `.statusbar > * { flex: none }` 会让这一条**不收缩**，窄窗口下
     * 一条长路径能把右边的状态挂件整个顶出屏幕。必须在这里覆回来。
     */
    flex: 0 1 auto;
    min-width: 0;
    overflow: hidden;
    /* 状态栏整体是 code-font，而路径是可读文本不是标识符 */
    font-family: var(--ui-font);
  }
  .statusbar .crumbs .sep { flex: none; color: var(--text-faint); font-size: 10px; }
  .crumb {
    flex: none;
    max-width: 160px;
    height: 17px;
    padding: 0 4px;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11.5px;
    line-height: 17px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* 只有目录段可点（点了在文件树里定位），文件段是 span，不该有 hover 反馈 */
  button.crumb:hover { background: var(--hover); color: var(--text); }
  .crumb.here { color: var(--text-dim); flex: 0 1 auto; min-width: 40px; }
  /* 左槽整块：路径、项目名、以及顶掉它们的那条提示消息 */
  .statusbar .navslot { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  /* 分支名是标识符，用等宽；ahead/behind 用 accent，它是「该做点什么」的信号 */
  .twidget .ab { color: var(--accent); font-family: var(--code-font); flex: none; font-size: 11px; }

  .workspace {
    display: grid;
    /* 四列：常驻竖条 · 侧边栏 · 拖拽条 · 主区 */
    grid-template-columns: 34px var(--side-w, 240px) 4px 1fr;
    overflow: hidden;
    transition: grid-template-columns 0.13s ease;
  }
  /* 拖拽时不要过渡，否则是一路追不上手的橡皮筋 */
  .workspace.resizing { transition: none; }
  @media (prefers-reduced-motion: reduce) { .workspace { transition: none; } }
  /* 收起侧边栏只去掉中间两列，竖条留着 —— 按钮的位置不能动 */
  .workspace.no-side { grid-template-columns: 34px 1fr; }

  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 5px 0 6px;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .rail .rgap { flex: 1; }
  .rbtn {
    position: relative;
    flex: none;
    display: grid;
    place-content: center;
    width: 26px;
    height: 26px;
    background: transparent;
    border: none;
    border-radius: var(--r-md);
    color: var(--text-faint);
    cursor: default;
    transition: background 0.09s, color 0.09s;
  }
  .rbtn:hover { background: var(--hover); color: var(--text); }
  /*
   * 选中态用中性白，不用 accent —— accent 在这一列里已经有活儿干了：
   * 旁边那个「有未提交改动」的红点。两个都上色就分不出哪个是状态、
   * 哪个是"你现在在这儿"。
   */
  .rbtn.on { color: var(--text); background: var(--selected); }
  .rbtn:active { background: var(--pressed); }
  .rbtn:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* 有未提交改动时给 Git 图标一个角标，收起侧边栏也知道有几处 */
  .rbtn .badge {
    position: absolute;
    right: 0;
    top: 0;
    display: grid;
    place-content: center;
    min-width: 13px;
    height: 13px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--git-modified);
    /*
      深色字压在 --git-modified（#6ba1e8，浅蓝）上，深浅两套主题里这个底色是同一个，
      所以这里直接写死一个近黑而不是用 --text：--text 在浅色主题下是深的、
      深色主题下是白的，而白字压在浅蓝上读不清。

      **不描边。** 角标会盖住图标右上那个结点，直觉是用外壳色描一圈把它抠出来 ——
      但外壳层是 transparent（后面是 NSVisualEffectView），描一圈实色就是在玻璃上
      凿一个洞。角标本身不透明，压住一段描边足够说清「它在上面」。
    */
    color: #101014;
    font-family: var(--code-font);
    font-size: 9px;
    font-weight: 600;
  }
  @media (prefers-reduced-motion: reduce) { .rbtn { transition: none; } }
  /*
   * 拖拽条：**热区和画出来的线要分开。**
   *
   * 原来是 `background: var(--border)` —— 热区多宽，亮条就多宽，
   * 于是界面正中间横着一条 4px 的白条（876px 高，玻璃上更扎眼）。
   * 但 4px 是好按的下限，不能为了好看把热区缩掉。
   *
   * 所以底留空，只用一个居中的 1px 伪元素画线。悬停时线变 accent，
   * 按住时才把整条 4px 点亮 —— 那时人已经在拖了，反馈越实越好。
   */
  .side-resizer {
    position: relative;
    background: transparent;
    cursor: col-resize;
  }
  .side-resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 1.5px;
    width: 1px;
    background: var(--border);
    transition: background 0.1s;
  }
  .side-resizer:hover::after { background: var(--accent); }
  .side-resizer:active { background: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .side-resizer::after { transition: none; } }
  aside { overflow: hidden; }
  /* 不画右边线，理由同 FileTree 的 `.tree` —— 那条边界归 `.side-resizer` */
  .no-root {
    padding: 14px 12px;
    color: var(--text-faint);
    font-size: 12px;
    background: var(--panel-bg);
    height: 100%;
  }

  /*
   * 用 flex 列而不是 grid：这一列里的元素是**条件渲染**的（标签栏、三种确认条、
   * 拖拽条、终端面板都可能不在），固定行数的 grid 会让后面的元素往前占位 ——
   * 曾经导致终端面板抢到 1fr 跑到内容区上面去。
   * flex 天然按实际存在的元素排布，content 吃掉剩余空间就行。
   */
  .main { display: flex; flex-direction: column; overflow: hidden; }
  .content { flex: 1; min-height: 0; overflow: hidden; }

  /* 与 .side-resizer 同一条判据：热区 4px，画出来的只有居中 1px */
  .resizer {
    position: relative;
    flex: none;
    height: 4px;
    background: transparent;
    cursor: row-resize;
  }
  .resizer::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 1.5px;
    height: 1px;
    background: var(--border);
    transition: background 0.1s;
  }
  .resizer:hover::after { background: var(--accent); }
  .resizer:active { background: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .resizer::after { transition: none; } }
  /* 收起时整块不占位也不可见，但**仍然挂在 DOM 上** —— 见上面那段注释 */
  .resizer.hidden,
  .panel.hidden { display: none; }
  /*
   * **上边不画线。** 和侧边栏那条是同一个毛病：`.resizer` 已经用伪元素画了
   * 一条，这里再来一条，两条隔 1.5px。`.resizer` 和 `.panel` 共用同一个
   * `class:hidden={!panel}`，收起时一起走，线不会落单。
   */
  .panel {
    flex: none;
    display: grid;
    /* 26 → 32：22px 的圆角标签要有呼吸位，贴着上下边看着像被切掉一半 */
    grid-template-rows: 32px 1fr;
    overflow: hidden;
  }
  .panel-head {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 9px;
    background: var(--panel-bg);
    color: var(--text-dim);
    user-select: none;
  }
  /*
   * 工具窗的名字。**它不是按钮** —— 切工具窗在导轨上，这里只回答
   * 「你现在看的是哪个」。比标签亮一档，右边那点留白就是分隔，
   * 不画竖线：线只用来分区，不用来分项。
   */
  .tw-name {
    flex: none;
    font-size: 12px;
    color: var(--text);
    padding-right: 7px;
  }
  .panel-head .gap { flex: 1; }
  /*
   * 头上的动作按钮：＋ / ⌄ / ⋮ / —。都是 22px 的方格子，
   * 和标签一样高 —— 一行里两种高度会让人以为它们不是一类东西。
   */
  .phbtn {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    cursor: default;
  }
  .phbtn:hover { background: var(--hover); color: var(--text); }
  .phbtn:active { background: var(--pressed); }
  .phbtn:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }

  /*
   * 终端标签页。和上面的编辑器标签栏是**同一套**：内缩的圆角块 + `--selected`，
   * 没有竖线也没有下划线。两条标签栏在同一个窗口里，长相必须一致 ——
   * 否则人会以为它们是两种不同的东西。
   */
  .ptabs {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .ptabs::-webkit-scrollbar { height: 0; }
  .ptab {
    display: flex;
    align-items: center;
    flex: none;
    height: 22px;
    border-radius: var(--r-sm);
    background: transparent;
  }
  .ptab:hover { background: var(--hover); }
  .ptab.on { background: var(--selected); }
  .pt-label {
    height: 100%;
    max-width: 140px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-size: 11.5px;
    padding: 0 2px 0 9px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ptab.on .pt-label { color: var(--text); }
  /*
   * ✕ 的格子固定 16px，平时透明，hover / 当前项才显形 ——
   * 常驻的话每个标签一个 ✕，而任何一刻最多只关得掉一个；
   * 而格子固定，点击目标就不会跟着 hover 左右挪。
   */
  .pt-x {
    flex: none;
    display: grid;
    place-content: center;
    width: 16px;
    height: 16px;
    margin-right: 3px;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-faint);
    font-size: 9px;
    line-height: 1;
    cursor: default;
    opacity: 0;
  }
  .ptab:hover .pt-x, .ptab.on .pt-x { opacity: 1; }
  /* 当前标签的底已经是 --selected 了，hover 再用它等于没反馈 */
  .pt-x:hover { background: var(--pressed); color: var(--text); }
  .pt-x:focus-visible { opacity: 1; outline: 1px solid var(--accent); outline-offset: -1px; }

  /* 工具页整块叠在一起，只切可见性 —— 终端不能卸载 */
  .tool-slot { position: absolute; inset: 0; }
  .tool-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }
  .panel-body { overflow: hidden; position: relative; }
  .term-slot { position: absolute; inset: 0; }
  /* 用 visibility 而不是 display:none —— 后者会让 xterm 的尺寸计算拿到 0，
     切回来时排版是乱的 */
  .term-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }

  .loading {
    display: grid;
    place-content: center;
    height: 100%;
    color: var(--text-faint);
    font-size: 12px;
  }

  .empty {
    height: 100%;
    display: grid;
    place-content: center;
    text-align: center;
    color: var(--text-dim);
  }
  /* 确认条不参与伸缩，始终贴在标签栏下方 */
  .confirm { flex: none; }
  .empty .card {
    width: min(420px, 90%);
    padding: 18px 20px 16px;
    /* 空态卡片是浮层：外壳层是透的，卡片跟着透就成了一圈没有底的框 */
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    text-align: left;
  }
  .empty .big { font-size: 14.5px; color: var(--text); margin-bottom: 4px; }
  .empty p { margin: 0; font-size: 11.5px; line-height: 1.6; color: var(--text-faint); }
  /*
   * 主动作是按钮，拖拽退成第二说法 —— 拖拽是这几种开法里最不像 macOS 的一种，
   * 而它原来是卡片上唯一的说法。
   */
  .empty .go {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-top: 14px;
  }
  .empty .go .gap { flex: 1; }
  .empty .primary {
    padding: 4px 12px;
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    color: #fff;
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .empty .primary:hover { filter: brightness(1.08); }
  .empty .go kbd {
    font-family: var(--code-font);
    font-size: 10.5px;
    color: var(--text-faint);
    background: var(--hover);
    border-radius: var(--r-sm);
    padding: 1px 5px;
  }
  .empty .lastly { font-size: 11.5px; color: var(--text-faint); }
  /* 空态是最需要「最近」的时刻 —— 那时侧边栏还没有任何内容 */
  .empty .link {
    background: transparent;
    border: none;
    padding: 0;
    color: var(--accent);
    font-family: var(--ui-font);
    font-size: 11.5px;
    cursor: default;
    max-width: 160px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .empty .link:hover { text-decoration: underline; }
  .empty .primary:focus-visible,
  .empty .link:focus-visible { outline: 1px solid var(--accent); outline-offset: 2px; }

  .empty .keymap {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 5px 20px;
    margin-top: 14px;
    font-family: var(--code-font);
    font-size: 11px;
    color: var(--text-dim);
  }
  .empty .keymap b { color: var(--text-faint); font-weight: 400; margin-right: 4px; }
  .empty .err {
    margin-top: 14px;
    padding-top: 11px;
    border-top: 1px solid var(--border-soft);
    color: var(--lvl-error);
    font-family: var(--code-font);
    font-size: 11px;
  }

  .confirm {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 12px;
    background: var(--elevated);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .confirm b { color: var(--text); font-weight: 600; }
  .confirm .rest { color: var(--text-faint); font-size: 11.5px; }
  .confirm button {
    padding: 3px 10px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: 11.5px;
    cursor: default;
  }
  .confirm button:hover { background: var(--hover); color: var(--text); }

  .confirm button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .confirm.conflict { background: rgba(214, 174, 88, 0.12); border-bottom-color: var(--lvl-warn); }
  /* 不可撤销的操作用红色描边，别让它长得跟普通确认一样 */
  .confirm.danger { background: rgba(247, 84, 100, 0.10); border-bottom-color: var(--lvl-error); }
  .confirm.err-banner {
    align-items: flex-start;
    background: rgba(247, 84, 100, 0.10);
    border-bottom-color: var(--lvl-error);
  }
  .err-banner .btext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .err-banner b { color: var(--lvl-error); }
  /* git 的说明本来就是分行排版的，保住换行；太长时可以滚 */
  .err-banner .bbody {
    white-space: pre-wrap;
    font-family: var(--code-font);
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--text-dim);
    max-height: 7.5em;
    overflow-y: auto;
  }
  .confirm button.danger {
    background: var(--lvl-error);
    border-color: var(--lvl-error);
    color: #fff;
  }

  .statusbar {
    display: flex;
    align-items: center;
    flex-wrap: nowrap;
    /* 窗口窄的时候宁可把右边挤掉，也不能换行 —— 换行会把状态栏撑成两行，
       把编辑区顶掉一截 */
    overflow: hidden;
    gap: 16px;
    padding: 0 12px;
    /* 同标题栏：贴着窗口下边，需要一层 scrim 兜住 11.5px 的小字 */
    background: var(--chrome-scrim);
    border-top: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--text-dim);
    font-family: var(--code-font);
    user-select: none;
  }
  .statusbar .spacer { flex: 1; min-width: 0; }
  .statusbar > * { flex: none; white-space: nowrap; }
  /* 窄窗口下先让「知道了也不改变下一步」的那几格退场：语言、只读原因、保存状态 */
  @media (max-width: 740px) {
    .statusbar .drop-2 { display: none; }
  }
  .statusbar .dim { color: var(--text-faint); }
  .statusbar .ok { color: var(--accent); }
  .statusbar .err { color: var(--lvl-error); }
  /*
   * 「正在做」是中性的：不是成功也不是失败，用正文色，不抢 accent。
   * 加一点点透明当作「还没定下来」的暗示 —— 不用转圈动画，
   * 状态栏上一个一直转的东西比它想传达的信息更吵。
   */
  .statusbar .doing { color: var(--text); opacity: 0.75; }
  .statusbar .warn { color: var(--lvl-warn); }
  .statusbar .btn.enc { font-size: 11px; }
  /* 解码有损是必须让人看见的事，不能只做成一个安静的标签 */
  .statusbar .btn.enc.bad { color: var(--lvl-error); }
  .statusbar .btn {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 11.5px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .statusbar .btn:hover { background: var(--hover); color: var(--text); }
  .statusbar .btn.mode { color: var(--text-dim); }
  .statusbar .btn.mode:hover { color: var(--accent); }
  .statusbar .btn.git { color: var(--git-modified); }
  /*
   * 挂件之间的竖线。**这是分区不是分项** —— 模式、语言/只读原因、编码、
   * 保存状态、git 状态，五组各说一件事，同字号同颜色排在一起时得有个断点。
   *
   * （它原来的理由是「左边一组是文档事实、右边一组是动作」，而右边那组
   * 打开工具窗的按钮 2026-09-06 整组撤了 —— 那些事导轨上都有。）
   */
  .statusbar .vsep {
    flex: none;
    width: 1px;
    height: 11px;
    background: var(--border);
  }
  /* 「已修改」是唯一会改变你下一步动作的那一项，值得提到 accent */
  .statusbar .accent { color: var(--accent); }
</style>
