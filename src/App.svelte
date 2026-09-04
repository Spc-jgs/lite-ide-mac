<script lang="ts">
  import FileTree from "./lib/shell/FileTree.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import QuickSearch, { type Action } from "./lib/search/QuickSearch.svelte";
  import { lazy, lazyGroup } from "./lib/lazy/lazy.svelte";
  import { notify } from "./lib/state/notify.svelte";
  import * as session from "./lib/state/session";
  import { textToSave, settled, stashed } from "./lib/state/doc";
  import Crash from "./lib/shell/Crash.svelte";
  import Icon from "./lib/shell/Icon.svelte";
  import Outline from "./lib/search/Outline.svelte";
  import type { Sym } from "./lib/editor/outline";
  import { langOf, langLabel } from "./lib/editor/langs";
  import type { ChangeKind } from "./lib/git/diff";
  import {
    probePath,
    readText,
    writeText,
    fileStamp,
    type Stamp,
    openLog,
    closeLog,
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
    },
    "Git 面板",
  );

  $effect(() => {
    const need =
      (sideView === "git" && !!repo) ||
      tabs.some((t) => t.mode === "diff" || t.mode === "merge") ||
      (panel && panelView === "log") ||
      branchOpen;
    if (need) git.load();
  });

  /** 分支 / 工作树选择器 */
  let branchOpen = $state(false);

  // ─────────────────────────── 编码 ───────────────────────────

  let encOpen = $state(false);
  const encPicker = lazy(() => import("./lib/encoding/EncodingPicker.svelte"), "编码选择器");
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

  function switchBranch(name: string, create = false) {
    notify.closeBanner();
    void gitDo(create ? "新建分支失败" : "切分支失败", async () => {
      await gitSwitch(repo!, name, create);
      notify.ok(create ? `已新建并切到 ${name}` : `已切到 ${name}`, 2600);
      await workingTreeChanged();
    });
  }

  function newWorktree(dir: string, branch: string) {
    void gitDo("新建工作树失败", async () => {
      // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
      const path = await gitWorktreeAdd(repo!, dir, branch);
      notify.ok(`工作树已建在 ${path}`, 3600);
      await openPath(path);
    });
  }

  /** 待确认移除的工作树 —— 会删目录，必须过用户这一关 */
  let pendingWtRemove = $state<GitWorktree | null>(null);

  function doRemoveWorktree(w: GitWorktree, force: boolean) {
    pendingWtRemove = null;
    void gitDo("移除工作树失败", async () => {
      await gitWorktreeRemove(repo!, w.path, force);
      notify.ok(`已移除工作树 ${w.path}`);
      await workingTreeChanged();
    });
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

  /** 冲突解决完写回文件；全部决定完的才 git add 标记已解决 */
  async function resolveMerge(tab: TabState, content: string, resolved: boolean) {
    if (!repo || !tab.rel) return;
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
    }
  }

  /** 打开（或复用）一个差异标签 */
  async function openDiff(e: GitEntry, staged: boolean) {
    if (!repo || e.isDir) return;
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

  /** 包一层：任何 git 写操作之后都要刷新状态，也统一收口错误 */
  async function gitDo(what: string, fn: () => Promise<unknown>) {
    if (!repo) return;
    try {
      await fn();
      await refreshGit();
    } catch (e) {
      notify.block(what, e);
    }
  }

  async function doDiscard(entries: GitEntry[]) {
    pendingDiscard = null;
    if (!repo) return;
    // 跟踪的走 git restore，未跟踪的只能直接删 —— gitsvc 里分了两条路
    const tracked = entries.filter((e) => !e.untracked).map((e) => e.path);
    const untracked = entries.filter((e) => e.untracked).map((e) => e.path);
    await gitDo("丢弃失败", () => gitDiscard(repo!, tracked, untracked));
    await workingTreeChanged();
  }

  function doGitCommit(message: string, amend: boolean) {
    void gitDo("提交失败", async () => {
      const out = await gitCommit(repo!, message, amend);
      notify.ok(out.split("\n")[0] || "已提交", 3000);
    });
  }
  let panel = $state(savedLayout.panel);
  let panelHeight = $state(savedLayout.panelHeight);
  /** 底部面板当前是哪个工具窗。终端实例永不卸载，只是藏起来 */
  let panelView = $state<"term" | "log">(savedLayout.panelView);
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
    const t: TermTab = {
      id: nextTermId++,
      cwd: dir,
      title: dir === "~" ? "~" : dir.slice(dir.lastIndexOf("/") + 1) || dir,
    };
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

  // 打开面板时若一个终端都没有，自动起一个。
  // 只在终端页上做 —— 冲着 Git 日志来的人不该莫名多出一个 shell
  $effect(() => {
    if (panel && panelView === "term" && terms.length === 0 && root !== null) newTerm(root);
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

  let quickOpen = $state(false);
  let quickScope = $state<"all" | "file" | "content" | "action">("all");
  /** 待跳转的行号；带 nonce，连点同一条结果也能重新定位 */
  let gotoLine = $state<{ line: number; nonce: number } | null>(null);
  let gotoNonce = 0;

  // 文件结构大纲
  let outlineOpen = $state(false);
  let outlineTick = $state(0);
  let symbols = $state<Sym[]>([]);
  function openOutline() {
    if (active?.mode !== "edit") return;
    symbols = [];
    outlineTick++;
    outlineOpen = true;
  }

  const actions: Action[] = [
    { id: "toggle-sidebar", label: "切换侧边栏", hint: "⌘1", run: () => (sidebar = !sidebar) },
    { id: "toggle-terminal", label: "切换终端", hint: "⌘J", run: () => (panel = !panel) },
    { id: "new-terminal", label: "新建终端", hint: "⌃⇧`", run: () => newTerm() },
    {
      id: "close-terminal",
      label: "关闭当前终端",
      run: () => {
        if (activeTermId !== null) closeTerm(activeTermId);
      },
    },
    {
      id: "switch-mode",
      label: "切换编辑 / 日志模式",
      run: () => {
        if (active) requestSwitchMode(active);
      },
    },
    { id: "save", label: "保存当前文件", hint: "⌘S", run: () => saveActive() },
    {
      id: "close-tab",
      label: "关闭当前标签",
      hint: "⌘W",
      run: () => {
        if (active) requestClose(active.id);
      },
    },
    { id: "close-all", label: "关闭所有标签", run: () => closeMany(tabs.map((t) => t.id)) },
    {
      id: "git-view",
      label: "Git：改动列表",
      hint: "⌘⇧G",
      run: () => {
        if (!repo) {
          notify.fail("当前项目不是 Git 仓库", 2600);
          return;
        }
        sideView = "git";
        sidebar = true;
      },
    },
    { id: "git-refresh", label: "Git：刷新状态", run: () => void refreshGit() },
    {
      id: "toggle-minimap",
      label: "切换代码缩略图",
      run: () => (showMinimap = !showMinimap),
    },
    {
      id: "encoding",
      label: "文件编码：查看 / 重新打开 / 换编码保存",
      run: () => {
        if (active) encOpen = true;
        else {
          notify.fail("先打开一个文件", 2200);
        }
      },
    },
    {
      id: "git-diff-current",
      label: "Git：查看当前文件的改动",
      run: () => {
        const e = activeEntry;
        if (e) void openDiff(e, false);
        else {
          notify.fail("当前文件没有未提交的改动", 2600);
        }
      },
    },
  ];

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

  /** 搜索结果点击：打开文件，带行号则跳过去 */
  async function openAt(path: string, line?: number) {
    const full = path.startsWith("/") ? path : `${root ?? ""}/${path}`;
    await openPath(full);
    if (line !== undefined) gotoLine = { line, nonce: ++gotoNonce };
  }

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

  // 按需加载失败要说出来。以前每个 import 各自 catch 到 error 里，
  // 抽成 lazy() 之后错误存在各自的 store 上，这里统一汇到状态栏。
  $effect(() => {
    const e =
      editor.error || logPane.error || terminal.error || git.error || encPicker.error;
    if (e) notify.fail(e);
  });

  /** 走 legacy stream parser 的语言没有语法树，界面要明说 */
  const LEZER_LANGS = new Set([
    "java", "javascript", "typescript", "python", "markdown", "json", "rust",
    "yaml", "html", "css", "sass", "less", "xml", "sql", "cpp", "php", "vue", "liquid",
  ]);
  let outlineSupported = $derived(
    active?.mode === "edit" && LEZER_LANGS.has(langOf(active.path) ?? ""),
  );


  /** 正在打开的路径，防止双击或事件重放时重复探测 */
  const opening = new Set<string>();

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
        activeId = exist.id;
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
      activeId = tab.id;
      // 没有项目根时，拿这个文件的父目录顶上，文件树才有东西显示
      if (!root) root = info.path.slice(0, info.path.lastIndexOf("/")) || "/";
    } catch (e) {
      if (!quiet) notify.fail(String(e));
    } finally {
      opening.delete(path);
    }
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
     */
    for (const t of saved.tabs) await openPath(t.path, true);
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
    const want = saved.tabs[saved.active]?.path;
    const hit = want ? tabs.find((t) => t.path === want) : null;
    if (hit) activeId = hit.id;
    // 上次开着、这次已经不在的文件：从记忆里也删掉，不然它们
    // 会一直躺在快照里，每次启动都白试一遍
    for (const t of saved.tabs) {
      if (!tabs.some((x) => x.path === t.path)) pendingPos.delete(t.path);
    }
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
    const idx = tabs.findIndex((t) => t.id === tab.id);
    tabs = tabs.filter((t) => t.id !== tab.id);
    if (activeId === tab.id) {
      activeId = tabs[Math.min(idx, tabs.length - 1)]?.id ?? null;
    }
    pendingClose = null;
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
    // ⌃⇧` 新建终端，与 VSCode 一致
    if (e.ctrlKey && e.shiftKey && (e.key === "~" || e.key === "`")) {
      e.preventDefault();
      newTerm();
      return;
    }
    if (!e.metaKey) return;
    /*
     * ⌘S 也要在编辑器**没有焦点**时管用。
     *
     * 原来它只挂在 CM6 的 keymap 上 —— 焦点在文件树、Git 面板或者终端上时
     * 按 ⌘S 什么也不发生，**而且没有任何提示**。而欢迎页一直把它和 ⌘P、⌘J
     * 并排列成全局快捷键。
     *
     * `defaultPrevented` 是防重的关键：焦点在编辑器里时 CM6 已经处理过并
     * preventDefault 了，事件照样会冒到 window —— 不判这一句就是存两次
     * （两次写盘、两条「已保存」）。
     *
     * 走 `saveActive()` 拿的是编辑器里的实时文本（见 liveText），
     * 不是几步之前的草稿。
     */
    if (k === "s") {
      if (e.defaultPrevented) return;
      e.preventDefault();
      saveActive();
      return;
    }
    if (k === "p") {
      e.preventDefault();
      quickScope = "file";
      quickOpen = true;
      return;
    }
    if (k === "o" && e.shiftKey) {
      e.preventDefault();
      openOutline();
      return;
    }
    if (k === "g" && e.shiftKey) {
      e.preventDefault();
      if (repo) {
        // 已经在 Git 视图上再按一次就切回去，来回都是同一个手势
        sideView = sidebar && sideView === "git" ? "files" : "git";
        sidebar = true;
      }
      return;
    }
    if (k === "f" && e.shiftKey) {
      e.preventDefault();
      quickScope = "content";
      quickOpen = true;
      return;
    }
    if (k === "1" || k === "b") {
      // ⌘B 是 VSCode 的侧边栏键位，很多人手指记的是它
      e.preventDefault();
      sidebar = !sidebar;
    } else if (k === "j") {
      e.preventDefault();
      panel = !panel;
    } else if (k === "w" && active) {
      e.preventDefault();
      requestClose(active.id);
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
      });
  });

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

</script>

<svelte:window onkeydown={onWindowKey} onkeyup={onWindowKeyUp} />

<Outline
  bind:open={outlineOpen}
  {symbols}
  fileName={active?.name ?? ""}
  supported={outlineSupported}
  onPick={(line) => (gotoLine = { line, nonce: ++gotoNonce })}
/>

<QuickSearch bind:open={quickOpen} bind:scope={quickScope} {root} {actions} onOpenFile={openAt} />

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
    {repo}
    onSwitch={(n) => switchBranch(n)}
    onNewBranch={(n) => switchBranch(n, true)}
    onOpenWorktree={(p) => void openPath(p)}
    onNewWorktree={newWorktree}
    onRemoveWorktree={(w) => (pendingWtRemove = w)}
  />
{/if}

<main class:hovering>
  <header class="titlebar" data-tauri-drag-region>
    {#if crumbs.length > 0}
      <!--
        面包屑。标签上只有文件名，目录深了之后「我在哪」得回头看文件树；
        顺带占掉右边那块常年空着的地方（1440 宽的窗口上原本有约 1200px 是空的）。
      -->
      <nav class="crumbs" aria-label="当前文件路径">
        {#each crumbs as c, i (c.path)}
          {#if i > 0}<span class="sep" aria-hidden="true">›</span>{/if}
          {#if c.dir}
            <button class="crumb" onclick={() => revealInTree(c.path)} title="在文件树中显示 {c.path}">{c.name}</button>
          {:else}
            <span class="crumb here" title={c.path}>{c.name}</span>
          {/if}
        {/each}
      </nav>
      {#if active?.mode === "log"}
        <span class="why" title={active.forced ? "你手动切到了日志模式" : "自动判定的原因"}>
          只读 · {active.forced ? "手动切换" : active.reason || "自动判定"}
        </span>
      {/if}
    {:else}
      <span class="app" title="lite-ide · 构建于 {__BUILD_TIME__}">lite-ide</span>
    {/if}
    <span class="tgap" data-tauri-drag-region></span>
    {#if gitSt}
      <button class="tbranch" onclick={() => (branchOpen = true)} title="切换分支 / 工作树">
        <Icon name="git" size={12} />
        <span class="bn">{gitSt.branch || "游离"}</span>
        {#if gitSt.ahead}<span class="ab">↑{gitSt.ahead}</span>{/if}
        {#if gitSt.behind}<span class="ab">↓{gitSt.behind}</span>{/if}
      </button>
    {/if}
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
            {#if gitSt && gitSt.entries.length > 0}
              <span class="dot" aria-hidden="true"></span>
            {/if}
          </button>
        {/if}
        <button
          class="rbtn"
          onclick={() => {
            quickScope = "content";
            quickOpen = true;
          }}
          title="在项目中搜内容 ⌘⇧F"
          aria-label="搜索"
        >
          <Icon name="search" />
        </button>
      {/if}
      <span class="rgap"></span>
      <button
        class="rbtn"
        class:on={panel}
        onclick={() => (panel = !panel)}
        title="终端 / Git 日志 ⌘J"
        aria-label="底部面板"
      >
        <Icon name="panel" />
      </button>
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
            onStage={(paths) => void gitDo("暂存失败", () => gitStage(repo!, paths))}
            onUnstage={(paths) => void gitDo("取消暂存失败", () => gitUnstage(repo!, paths))}
            onDiscard={(es) => (pendingDiscard = es)}
            onCommit={doGitCommit}
            onRefresh={() => void refreshGit()}
          />
        {:else if sideView === "git" && repo}
          <div class="no-root">正在载入 Git 面板…</div>
        {:else}
          <FileTree
            {root}
            activePath={active?.path ?? ""}
            gitStatus={gitSt}
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
          onSelect={(id) => (activeId = id)}
          onClose={requestClose}
          onCloseMany={closeMany}
          onRevealInTree={revealInTree}
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
              <div class="big">把文件或文件夹拖进来</div>
              <p>代码走编辑模式，大文件与日志自动走只读的日志模式</p>
              <div class="keymap">
                <span><b>⇧⇧</b> 随处搜索</span>
                <span><b>⌘S</b> 保存</span>
                <span><b>⌘P</b> 找文件</span>
                <span><b>⌘W</b> 关闭标签</span>
                <span><b>⌘⇧F</b> 搜内容</span>
                <span><b>⌘1</b> 侧边栏</span>
                <span><b>⌘⇧O</b> 文件结构</span>
                <span><b>⌘J</b> 终端</span>
                <span><b>⌘⇧G</b> 改动</span>
                <span><b>F3</b> 日志里跳命中</span>
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
              onOutline={(s) => (symbols = s)}
              onCursor={(l) => markPos(active!.path, l)}
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
          <div class="panel-head">
            <button class="tool" class:on={panelView === "term"} onclick={() => (panelView = "term")}>
              终端{terms.length > 1 ? ` (${terms.length})` : ""}
            </button>
            {#if repo}
              <button class="tool" class:on={panelView === "log"} onclick={() => (panelView = "log")}>
                Git 日志
              </button>
            {/if}
            <span class="vsep"></span>
            <div class="tterms" class:hidden={panelView !== "term"}>
              {#each terms as t (t.id)}
                <div class="tterm" class:on={t.id === activeTermId}>
                  <button class="tt-label" onclick={() => (activeTermId = t.id)} title={t.cwd}>
                    {t.title}
                  </button>
                  <button class="tt-x" onclick={() => closeTerm(t.id)} aria-label="关闭终端">✕</button>
                </div>
              {/each}
              <button class="tt-add" onclick={() => newTerm()} title="新建终端 ⌃⇧`">＋</button>
            </div>
            <span class="gap"></span>
            <button onclick={() => (panel = false)} title="收起 ⌘J">✕</button>
          </div>
          <div class="panel-body">
            <!--
              终端整块只藏不卸载：组件一销毁 Session 就 drop，shell 直接被 kill。
              切到 Git 日志页时正在跑的命令必须还在跑。
            -->
            <div class="tool-slot" class:hidden={panelView !== "term"}>
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
            {#if panel && panelView === "log" && repo}
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

  <footer class="statusbar">
    {#if active?.mode === "merge"}
      <span class="cell warn">冲突合并</span>
    {:else if active?.mode === "diff"}
      <span class="cell dim">
        {active.diffSha ? `提交 ${active.diffShort}` : `差异 · ${active.diffStaged ? "已暂存" : "未暂存"}`}
      </span>
    {:else if active}
      <button
        class="cell btn mode"
        onclick={() => requestSwitchMode(active!)}
        title={active.mode === "log" ? "切换到编辑模式" : "切换到日志模式（只读，带级别过滤与 tail）"}
      >
        {active.mode === "log" ? "日志模式" : "编辑模式"} ⇄
      </button>
      {#if active.mode === "edit"}
        <span class="vsep" aria-hidden="true"></span>
        <span class="cell dim drop-2">{langLabel(langOf(active.path))}</span>
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
      <span class="vsep" aria-hidden="true"></span>
      {#if active.mode === "log"}
        <span class="cell">{logStatus}</span>
      {:else}
        <span class="cell drop-2" class:accent={active.dirty}>
          {active.dirty ? "已修改" : "无改动"}
        </span>
      {/if}
      {#if activeEntry}
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
    {:else}
      <span class="cell dim">等待文件</span>
    {/if}
    {#if notify.info}<span class="cell ok">{notify.info}</span>{/if}
    {#if notify.error}<span class="cell err">{notify.error}</span>{/if}
    <span class="spacer"></span>
    <!-- 分支挪到标题栏了（离文件上下文更近），这里只留动作，不重复显示同一件事 -->
    {#if gitSt}
      <button
        class="cell btn"
        class:on={sidebar && sideView === "git"}
        onclick={() => {
          sideView = sidebar && sideView === "git" ? "files" : "git";
          sidebar = true;
        }}
        title="改动列表 ⌘⇧G"
      >改动{#if gitSt.entries.length}<span class="chg">{gitSt.entries.length}</span>{/if}</button>
      <button
        class="cell btn"
        class:on={panel && panelView === "log"}
        onclick={() => {
          panelView = "log";
          panel = true;
        }}
        title="提交历史"
      >历史</button>
      <span class="vsep" aria-hidden="true"></span>
    {/if}
    <button class="cell btn" onclick={() => { quickScope = "all"; quickOpen = true; }}>搜索 ⇧⇧</button>
    <button class="cell btn" class:on={panel} onclick={() => (panel = !panel)}>
      终端 ⌘J{terms.length > 1 ? ` (${terms.length})` : ""}
    </button>
    <span class="cell dim drop-1">{tabs.length} 个标签</span>
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
  .titlebar .app { color: var(--text); font-weight: 500; }
  .titlebar .tgap { flex: 1; min-width: 12px; }

  .crumbs {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    overflow: hidden;
  }
  .crumbs .sep { flex: none; color: var(--text-faint); font-size: 11px; }
  .crumb {
    flex: none;
    max-width: 180px;
    padding: 1px 3px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* 只有目录段可点（点了把它设成项目根），文件段是 span，不该有 hover 反馈 */
  button.crumb:hover { background: var(--hover); color: var(--text); }
  .crumb.here { color: var(--text); flex: 0 1 auto; min-width: 40px; }

  .tbranch {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    flex: none;
    max-width: 240px;
    padding: 2px 7px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-size: 11px;
    cursor: default;
  }
  .tbranch:hover { background: var(--hover); color: var(--text-dim); }
  .tbranch .bn {
    font-family: var(--code-font);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tbranch .ab { color: var(--accent); font-family: var(--code-font); flex: none; }
  .titlebar .why {
    margin-left: 4px;
    font-family: var(--code-font);
    font-size: 10.5px;
    color: var(--text-faint);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    padding: 1px 5px;
  }

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
  /* 有未提交改动时给 Git 图标一个小红点，收起侧边栏也知道有东西 */
  .rbtn .dot {
    position: absolute;
    right: 4px;
    top: 4px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--git-modified);
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
  .no-root {
    padding: 14px 12px;
    color: var(--text-faint);
    font-size: 12px;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
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
  .panel {
    flex: none;
    display: grid;
    grid-template-rows: 26px 1fr;
    overflow: hidden;
    border-top: 1px solid var(--border);
  }
  .panel-head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px;
    background: var(--panel-bg);
    font-size: 11px;
    color: var(--text-dim);
    user-select: none;
  }
  .panel-head .tool {
    flex: none;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-size: 11px;
    padding: 2px 8px;
    cursor: default;
  }
  .panel-head .tool:hover { background: var(--hover); color: var(--text); }
  .panel-head .tool.on { color: var(--text); background: var(--accent-sel); }
  .panel-head .vsep {
    flex: none;
    width: 1px;
    height: 12px;
    background: var(--border);
    margin: 0 2px;
  }
  .tterms.hidden { display: none; }
  /* 工具页整块叠在一起，只切可见性 —— 终端不能卸载 */
  .tool-slot { position: absolute; inset: 0; }
  .tool-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }
  .panel-head .gap { flex: 1; }
  .panel-head button {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10.5px;
    padding: 2px 6px;
    border-radius: var(--r-sm);
    cursor: default;
  }
  .panel-head button:hover { background: var(--hover); color: var(--text); }
  .panel-body { overflow: hidden; position: relative; }
  .term-slot { position: absolute; inset: 0; }
  /* 用 visibility 而不是 display:none —— 后者会让 xterm 的尺寸计算拿到 0，
     切回来时排版是乱的 */
  .term-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }

  .tterms { display: flex; align-items: center; gap: 2px; overflow-x: auto; }
  .tterms::-webkit-scrollbar { height: 0; }
  .tterm {
    display: flex;
    align-items: center;
    flex: none;
    border-radius: var(--r-sm);
    background: transparent;
  }
  .tterm:hover { background: var(--hover); }
  .tterm.on { background: var(--accent-sel); }
  .tt-label {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10.5px;
    font-family: var(--code-font);
    padding: 2px 3px 2px 7px;
    cursor: default;
    max-width: 120px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tterm.on .tt-label { color: var(--text); }
  .tt-x {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 9px;
    padding: 2px 6px 2px 2px;
    cursor: default;
  }
  .tt-x:hover { color: var(--text); }
  .tt-add {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 12px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    cursor: default;
    flex: none;
  }
  .tt-add:hover { background: var(--hover); color: var(--text); }
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
  /* 窄窗口下先让信息类的格子退场，动作按钮留到最后 */
  @media (max-width: 900px) {
    .statusbar .drop-1 { display: none; }
  }
  @media (max-width: 740px) {
    .statusbar .drop-2 { display: none; }
  }
  .statusbar .dim { color: var(--text-faint); }
  .statusbar .ok { color: var(--accent); }
  .statusbar .err { color: var(--lvl-error); }
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
  .statusbar .btn.on { color: var(--accent); }
  .statusbar .btn.mode { color: var(--text-dim); }
  .statusbar .btn.mode:hover { color: var(--accent); }
  .statusbar .btn.git { color: var(--git-modified); }
  /*
   * 分组竖线。原本八项同字号、同颜色、同间距 —— 哪些是「状态」哪些是「按钮」
   * 得逐项读才知道。左边一组是文档事实，右边一组是动作，中间用 1px 分开。
   */
  .statusbar .vsep {
    flex: none;
    width: 1px;
    height: 11px;
    background: var(--border);
  }
  /* 「已修改」是唯一会改变你下一步动作的那一项，值得提到 accent */
  .statusbar .accent { color: var(--accent); }
  .statusbar .chg {
    display: inline-block;
    margin-left: 4px;
    background: var(--selected);
    border-radius: var(--r-md);
    padding: 0 5px;
    font-size: 10px;
    color: var(--text-dim);
  }
</style>
