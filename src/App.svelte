<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import FileTree from "./lib/shell/FileTree.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import QuickSearch, { type Action } from "./lib/search/QuickSearch.svelte";
  import { lazy, lazyGroup } from "./lib/lazy/lazy.svelte";
  import { notify } from "./lib/state/notify.svelte";
  import Crash from "./lib/shell/Crash.svelte";
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
    /** edit 模式打开时的磁盘内容 */
    content?: string;
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

  let sidebar = $state(true);
  let sidebarWidth = $state(240);
  /** 侧边栏当前显示哪个视图。不在仓库里时强制回文件树 */
  let sideView = $state<"files" | "git">("files");

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
  let panel = $state(false);
  let panelHeight = $state(260);
  /** 底部面板当前是哪个工具窗。终端实例永不卸载，只是藏起来 */
  let panelView = $state<"term" | "log">("term");
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
    { id: "close-all", label: "关闭所有标签", run: () => closeAll() },
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

  function saveActive() {
    // 触发编辑器自己的保存路径：内容以编辑器里的为准，这里只能存已知内容
    if (active?.mode === "edit") void save(active.content ?? "");
  }

  function closeAll() {
    for (const t of [...tabs]) {
      if (t.mode === "log" && t.handle !== undefined) void closeLog(t.handle);
    }
    tabs = [];
    activeId = null;
    pendingClose = null;
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

  async function openPath(path: string) {
    if (opening.has(path)) return;
    opening.add(path);
    notify.clear();
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
      notify.fail(String(e));
    } finally {
      opening.delete(path);
    }
  }

  async function save(content: string) {
    const tab = active;
    if (!tab || tab.mode !== "edit") return;
    try {
      // 保存返回新指纹，必须记下来，否则下次检查会把自己的保存当成外部修改
      tab.stamp = await writeText(tab.path, content, tab.encoding, tab.bom);
      tab.dirty = false;
      tab.content = content;
      tab.conflict = false;
      savedTick++;
      notify.ok(`已保存 ${tab.name}`, 1800);
      // 保存八成改变了 git 状态，顺手刷一下，文件树的标记才跟得上
      void refreshGit();
    } catch (e) {
      notify.fail(String(e));
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
          tab.content = t.content;
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
        tab.content = (await readText(tab.path, tab.encoding)).content;
        tab.stamp = await fileStamp(tab.path);
        tab.dirty = false;
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
    initialPath().then((p) => {
      if (p && tabs.length === 0 && root === null) void openPath(p);
    });
  });

  $effect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over") hovering = true;
      else if (e.payload.type === "drop") {
        hovering = false;
        for (const p of e.payload.paths) void openPath(p);
      } else hovering = false;
    });
    /*
     * 立刻挂上 catch，而不是只在清理函数里挂。
     * 浏览器里跑（没有 Tauri）时这个 promise 会直接 reject，
     * 而清理函数要等 effect 销毁才跑 —— 中间这段时间就是一条
     * "Uncaught (in promise)"，把控制台的真错误淹掉。
     */
    const reg = un.catch(() => null);
    return () => void reg.then((f) => f?.());
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
            <button class="crumb" onclick={() => void openPath(c.path)} title={c.path}>{c.name}</button>
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
        <svg viewBox="0 0 16 16" width="12" height="12" aria-hidden="true">
          <circle cx="4.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <circle cx="4.5" cy="12.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <circle cx="11.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M4.5 5.3 L4.5 10.7" stroke="currentColor" stroke-width="1.3" />
          <path d="M11.5 5.3 Q11.5 8.5 4.5 10.7" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
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
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <rect x="1.6" y="2.6" width="12.8" height="10.8" rx="1.6"
                fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M6.4 2.6 L6.4 13.4" stroke="currentColor" stroke-width="1.3" />
          <rect x="1.6" y="2.6" width="4.8" height="10.8" fill="currentColor" opacity="0.28" />
        </svg>
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
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <path d="M1.8 12.5 V4.2 a1 1 0 0 1 1-1 h3.1 l1.4 1.6 h5.9 a1 1 0 0 1 1 1 v6.7 a1 1 0 0 1-1 1 H2.8 a1 1 0 0 1-1-1 z"
                  fill="none" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round" />
          </svg>
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
            <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
              <circle cx="4.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
              <circle cx="4.5" cy="12.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
              <circle cx="11.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
              <path d="M4.5 5.3 L4.5 10.7" stroke="currentColor" stroke-width="1.3" />
              <path d="M11.5 5.3 Q11.5 8.5 4.5 10.7" fill="none" stroke="currentColor" stroke-width="1.3" />
            </svg>
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
          <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
            <circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.4" />
            <path d="M10.2 10.2 L13.5 13.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
          </svg>
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
        <svg viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          <rect x="1.6" y="2.6" width="12.8" height="10.8" rx="1.6"
                fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M1.6 9.6 L14.4 9.6" stroke="currentColor" stroke-width="1.3" />
          <rect x="1.6" y="9.6" width="12.8" height="3.8" fill="currentColor" opacity="0.28" />
        </svg>
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
            onOpen={(p) => void openPath(p)}
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
        <Tabs {tabs} {activeId} onSelect={(id) => (activeId = id)} onClose={requestClose} />
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
          <button class="primary" onclick={() => { const t = pendingClose!; save(t.content ?? "").then(() => doClose(t)); }}>
            保存并关闭
          </button>
          <button onclick={() => doClose(pendingClose!)}>丢弃改动</button>
          <button onclick={() => (pendingClose = null)}>取消</button>
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
            />
          {/key}
        {:else if active.mode === "log"}
          <div class="empty"><p>正在载入日志视图…</p></div>
        {:else if editor.comp}
          {#key active.id}
            <editor.comp
              path={active.path}
              initial={active.content ?? ""}
              {savedTick}
              {gotoLine}
              {outlineTick}
              marks={editorMarks}
              {showMinimap}
              onChange={(d) => (active!.dirty = d)}
              onSave={save}
              onOutline={(s) => (symbols = s)}
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

      {#if panel}
        <div
          class="resizer"
          role="separator"
          aria-label="调整终端高度"
          onpointerdown={startResize}
        ></div>
        <div class="panel" style:height="{panelHeight}px">
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
            {#if panelView === "log" && repo}
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
    background: var(--editor-bg);
  }
  main.hovering { outline: 2px solid var(--accent); outline-offset: -2px; }

  .titlebar {
    display: flex;
    align-items: center;
    gap: 8px;
    /* 给 macOS 红绿灯让位 */
    padding: 0 12px 0 78px;
    background: var(--panel-bg);
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
    border-radius: 3px;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /* 只有目录段可点（点了把它设成项目根），文件段是 span，不该有 hover 反馈 */
  button.crumb:hover { background: var(--panel-bg-2); color: var(--text); }
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
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 11px;
    cursor: default;
  }
  .tbranch:hover { background: var(--panel-bg-2); color: var(--text-dim); }
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
    border-radius: 3px;
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
    border-radius: 5px;
    color: var(--text-faint);
    cursor: default;
    transition: background 0.09s, color 0.09s;
  }
  .rbtn:hover { background: var(--panel-bg-2); color: var(--text); }
  .rbtn.on { color: var(--accent); background: var(--accent-sel); }
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
  .side-resizer {
    background: var(--border);
    cursor: col-resize;
  }
  .side-resizer:hover { background: var(--accent); }
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

  .resizer {
    flex: none;
    height: 4px;
    background: var(--border);
    cursor: row-resize;
  }
  .resizer:hover { background: var(--accent); }
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
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 11px;
    padding: 2px 8px;
    cursor: default;
  }
  .panel-head .tool:hover { background: var(--panel-bg-2); color: var(--text); }
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
    border-radius: 3px;
    cursor: default;
  }
  .panel-head button:hover { background: var(--panel-bg-2); color: var(--text); }
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
    border-radius: 3px;
    background: transparent;
  }
  .tterm:hover { background: var(--panel-bg-2); }
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
    border-radius: 3px;
    cursor: default;
    flex: none;
  }
  .tt-add:hover { background: var(--panel-bg-2); color: var(--text); }
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
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 8px;
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
    background: var(--panel-bg-2);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }
  .confirm b { color: var(--text); font-weight: 600; }
  .confirm button {
    padding: 3px 10px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 11.5px;
    cursor: default;
  }
  .confirm button:hover { background: var(--panel-bg); color: var(--text); }
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
    background: var(--panel-bg);
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
    border-radius: 3px;
    cursor: default;
  }
  .statusbar .btn:hover { background: var(--panel-bg-2); color: var(--text); }
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
    background: var(--panel-bg-2);
    border-radius: 7px;
    padding: 0 5px;
    font-size: 10px;
    color: var(--text-dim);
  }
</style>
