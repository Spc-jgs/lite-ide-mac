<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import FileTree from "./lib/shell/FileTree.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import QuickSearch, { type Action } from "./lib/search/QuickSearch.svelte";
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
   * Git 面板与差异视图按需加载，和 CM6、xterm 同一条纪律
   * （ARCHITECTURE.md 红线：入口包只放两种模式都要的东西）。
   * 静态引入时入口包从 120KB 涨到 140KB —— 而这两样东西，
   * 只看日志的人一次都不会用到。
   *
   * 文件树上的 git 染色不在这里面：那只是 FileTree 里的一个 $derived，
   * 没有额外模块，打开就该看见。
   */
  let GitPaneComp = $state<typeof import("./lib/git/GitPane.svelte").default | null>(null);
  let DiffViewComp = $state<typeof import("./lib/git/DiffView.svelte").default | null>(null);
  let GitLogComp = $state<typeof import("./lib/git/GitLog.svelte").default | null>(null);
  let MergeViewComp = $state<typeof import("./lib/git/MergeView.svelte").default | null>(null);
  let BranchPickerComp = $state<typeof import("./lib/git/BranchPicker.svelte").default | null>(null);
  let gitLoading = $state(false);

  $effect(() => {
    const need =
      (sideView === "git" && !!repo) ||
      tabs.some((t) => t.mode === "diff" || t.mode === "merge") ||
      (panel && panelView === "log") ||
      branchOpen;
    if (!need || gitLoading || GitPaneComp) return;
    gitLoading = true;
    // 四个一起拉：进了 Git 就基本都会用到，分四次只是多三次往返
    Promise.all([
      import("./lib/git/GitPane.svelte"),
      import("./lib/git/DiffView.svelte"),
      import("./lib/git/GitLog.svelte"),
      import("./lib/git/BranchPicker.svelte"),
      import("./lib/git/MergeView.svelte"),
    ])
      .then(([g, d, l, b, m]) => {
        GitPaneComp = g.default;
        DiffViewComp = d.default;
        GitLogComp = l.default;
        BranchPickerComp = b.default;
        MergeViewComp = m.default;
      })
      .catch((e) => (error = `Git 面板加载失败：${e}`))
      .finally(() => (gitLoading = false));
  });

  /** 分支 / 工作树选择器 */
  let branchOpen = $state(false);

  // ─────────────────────────── 编码 ───────────────────────────

  let encOpen = $state(false);
  let EncPickerComp = $state<
    typeof import("./lib/encoding/EncodingPicker.svelte").default | null
  >(null);

  $effect(() => {
    if (!encOpen || EncPickerComp) return;
    import("./lib/encoding/EncodingPicker.svelte")
      .then((m) => (EncPickerComp = m.default))
      .catch((e) => (error = `编码选择器加载失败：${e}`));
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
        error = "有未保存的改动，请先保存（⌘S）再换编码重新打开";
        setTimeout(() => (error = ""), 3000);
        return;
      }
      const t = await readText(tab.path, label);
      tab.content = t.content;
      tab.encoding = t.encoding;
      tab.bom = t.bom;
      tab.lossy = t.lossy;
      savedTick++;
      saved = `已按 ${t.encoding} 重新打开${t.lossy ? "（仍有解不出的字节）" : ""}`;
      setTimeout(() => (saved = ""), 3000);
    } catch (e) {
      error = String(e);
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
    saved = `下次保存将写成 ${label}${bom ? " + BOM" : ""}，按 ⌘S 生效`;
    setTimeout(() => (saved = ""), 3600);
  }

  function switchBranch(name: string, create = false) {
    void gitDo(create ? "新建分支失败" : "切分支失败", async () => {
      await gitSwitch(repo!, name, create);
      saved = create ? `已新建并切到 ${name}` : `已切到 ${name}`;
      setTimeout(() => (saved = ""), 2600);
      // 切分支会大面积改盘上的文件，打开的标签必须重新对一遍
      await checkExternalChanges();
    });
  }

  function newWorktree(dir: string, branch: string) {
    void gitDo("新建工作树失败", async () => {
      // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
      const path = await gitWorktreeAdd(repo!, dir, branch);
      saved = `工作树已建在 ${path}`;
      setTimeout(() => (saved = ""), 3600);
      await openPath(path);
    });
  }

  /** 待确认移除的工作树 —— 会删目录，必须过用户这一关 */
  let pendingWtRemove = $state<GitWorktree | null>(null);

  function doRemoveWorktree(w: GitWorktree, force: boolean) {
    pendingWtRemove = null;
    void gitDo("移除工作树失败", async () => {
      await gitWorktreeRemove(repo!, w.path, force);
      saved = `已移除工作树 ${w.path}`;
      setTimeout(() => (saved = ""), 2600);
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
      .then(([raw, m]) => (editorMarks = m.changedLines(raw)))
      .catch(() => (editorMarks = null));
  });

  /** 从日志里打开某次提交中某个文件的差异 */
  async function openCommitDiff(sha: string, short: string, rel: string) {
    if (!repo) return;
    const key = `git-commit:${sha}:${rel}`;
    let tab = tabs.find((t) => t.path === key);
    if (!tab) {
      tab = {
        id: nextId++,
        path: key,
        name: rel.slice(rel.lastIndexOf("/") + 1),
        mode: "diff",
        dirty: false,
        size: 0,
        rel,
        diffSha: sha,
        diffShort: short,
      };
      tabs = [...tabs, tab];
    }
    activeId = tab.id;
    await reloadDiff(tab);
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
        tabs.filter((t) => t.mode === "diff" && !t.diffSha).map(reloadDiff),
      );
    } catch (e) {
      error = String(e);
      setTimeout(() => (error = ""), 4000);
    } finally {
      gitBusy = false;
    }
  }

  async function reloadDiff(tab: TabState) {
    if (!repo || tab.mode !== "diff" || !tab.rel) return;
    try {
      tab.diffRaw = tab.diffSha
        ? await gitCommitDiff(repo, tab.diffSha, tab.rel)
        : await gitDiff(repo, tab.rel, !!tab.diffStaged, !!tab.diffUntracked);
    } catch (e) {
      tab.diffRaw = "";
      error = String(e);
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
    let tab = tabs.find((t) => t.path === key);
    try {
      const content = (await readText(full)).content;
      if (!tab) {
        tab = {
          id: nextId++,
          path: key,
          name: e.path.slice(e.path.lastIndexOf("/") + 1),
          mode: "merge",
          dirty: false,
          size: 0,
          rel: e.path,
          mergeText: content,
        };
        tabs = [...tabs, tab];
      } else {
        tab.mergeText = content;
      }
      activeId = tab.id;
    } catch (err) {
      error = String(err);
    }
  }

  /** 冲突解决完写回文件；全部决定完的才 git add 标记已解决 */
  async function resolveMerge(tab: TabState, content: string, resolved: boolean) {
    if (!repo || !tab.rel) return;
    try {
      await writeText(`${repo}/${tab.rel}`, content, tab.encoding);
      if (resolved) {
        await gitStage(repo, [tab.rel]);
        saved = `${tab.name} 已标记为解决`;
        doClose(tab);
      } else {
        tab.mergeText = content;
        saved = `${tab.name} 进度已保存`;
      }
      setTimeout(() => (saved = ""), 2600);
      await refreshGit();
    } catch (e) {
      error = String(e);
    }
  }

  /** 打开（或复用）一个差异标签 */
  async function openDiff(e: GitEntry, staged: boolean) {
    if (!repo || e.isDir) return;
    const key = `git-diff:${e.path}`;
    let tab = tabs.find((t) => t.mode === "diff" && t.path === key);
    if (!tab) {
      tab = {
        id: nextId++,
        path: key,
        name: e.path.slice(e.path.lastIndexOf("/") + 1),
        mode: "diff",
        dirty: false,
        size: 0,
        rel: e.path,
      };
      tabs = [...tabs, tab];
    }
    tab.diffStaged = staged;
    tab.diffUntracked = e.untracked && !staged;
    activeId = tab.id;
    await reloadDiff(tab);
  }

  /** 差异标签上切换「已暂存 ↔ 未暂存」 */
  async function toggleDiffSide(tab: TabState) {
    tab.diffStaged = !tab.diffStaged;
    // 未跟踪文件一旦进了暂存区，就该按普通 diff 读，不能再走 --no-index
    const e = gitSt?.entries.find((x) => x.path === tab.rel);
    tab.diffUntracked = !!e?.untracked && !tab.diffStaged;
    await reloadDiff(tab);
  }

  /** 包一层：任何 git 写操作之后都要刷新状态，也统一收口错误 */
  async function gitDo(what: string, fn: () => Promise<unknown>) {
    if (!repo) return;
    try {
      await fn();
      await refreshGit();
    } catch (e) {
      error = `${what}：${e}`;
      setTimeout(() => (error = ""), 5000);
    }
  }

  async function doDiscard(entries: GitEntry[]) {
    pendingDiscard = null;
    if (!repo) return;
    // 跟踪的走 git restore，未跟踪的只能直接删 —— gitsvc 里分了两条路
    const tracked = entries.filter((e) => !e.untracked).map((e) => e.path);
    const untracked = entries.filter((e) => e.untracked).map((e) => e.path);
    await gitDo("丢弃失败", () => gitDiscard(repo!, tracked, untracked));
    // 丢弃会改磁盘，打开着的编辑标签要跟上
    await checkExternalChanges();
  }

  function doGitCommit(message: string, amend: boolean) {
    void gitDo("提交失败", async () => {
      const out = await gitCommit(repo!, message, amend);
      saved = out.split("\n")[0] || "已提交";
      setTimeout(() => (saved = ""), 3000);
    });
  }
  let panel = $state(false);
  let panelHeight = $state(260);
  /** 底部面板当前是哪个工具窗。终端实例永不卸载，只是藏起来 */
  let panelView = $state<"term" | "log">("term");
  /** xterm.js 约 250KB，不开终端就不该付这个钱 —— 与 CM6 同样按需加载 */
  let TerminalComp = $state<typeof import("./lib/terminal/Terminal.svelte").default | null>(null);
  let terminalLoading = $state(false);
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
  let error = $state("");
  let logStatus = $state("");
  let saved = $state("");
  /** 待确认关闭的脏标签 —— 直接丢弃改动太粗暴，也不该静默保存 */
  let pendingClose = $state<TabState | null>(null);

  /**
   * CodeMirror 6 核心约 340KB，日志模式一点也用不上 —— 静态引入会把入口包
   * 从 71KB 顶到 412KB，与"秒开"的立身之本冲突。改成打开第一个可编辑文件时
   * 才 import，本地加载只有几毫秒。
   */
  let EditorComp = $state<typeof import("./lib/editor/Editor.svelte").default | null>(null);
  let editorLoading = $state(false);

  /*
   * 日志视图同样按需加载 —— 和 Editor 对称。
   * 早先它是静态引入的，等于只写代码的人一直在为整套日志视图
   * （虚拟滚动 + 过滤条 + 8 种格式的解析着色）付钱。
   */
  let LogPaneComp = $state<typeof import("./lib/logview/LogPane.svelte").default | null>(null);
  let logPaneLoading = $state(false);

  $effect(() => {
    if (active?.mode !== "log" || LogPaneComp || logPaneLoading) return;
    logPaneLoading = true;
    import("./lib/logview/LogPane.svelte")
      .then((m) => (LogPaneComp = m.default))
      .catch((e) => (error = `日志视图加载失败：${e}`))
      .finally(() => (logPaneLoading = false));
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
          error = "当前项目不是 Git 仓库";
          setTimeout(() => (error = ""), 2600);
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
          error = "先打开一个文件";
          setTimeout(() => (error = ""), 2200);
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
          error = "当前文件没有未提交的改动";
          setTimeout(() => (error = ""), 2600);
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
    if (!panel || TerminalComp || terminalLoading) return;
    terminalLoading = true;
    import("./lib/terminal/Terminal.svelte")
      .then((m) => (TerminalComp = m.default))
      .catch((e) => (error = `终端加载失败：${e}`))
      .finally(() => (terminalLoading = false));
  });

  $effect(() => {
    if (active?.mode !== "edit" || EditorComp || editorLoading) return;
    editorLoading = true;
    import("./lib/editor/Editor.svelte")
      .then((m) => (EditorComp = m.default))
      .catch((e) => (error = `编辑器加载失败：${e}`))
      .finally(() => (editorLoading = false));
  });

  let active = $derived(tabs.find((t) => t.id === activeId) ?? null);

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
    error = "";
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
      error = String(e);
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
      saved = `已保存 ${tab.name}`;
      setTimeout(() => (saved = ""), 1800);
      // 保存八成改变了 git 状态，顺手刷一下，文件树的标记才跟得上
      void refreshGit();
    } catch (e) {
      error = String(e);
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
          saved = `${tab.name} 已被外部修改，已重新加载`;
          setTimeout(() => (saved = ""), 2600);
        } catch (e) {
          error = String(e);
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
        error = String(e);
      }
    }
    // take === "mine"：什么都不做，保留编辑器里的内容，
    // 下次 ⌘S 会覆盖磁盘 —— 指纹已经更新过，不会再重复告警
  }

  $effect(() => {
    const onFocus = () => {
      void checkExternalChanges();
      // 用户可能刚在终端里 commit / checkout 完切回来
      void refreshGit();
    };
    window.addEventListener("focus", onFocus);
    // 兜底：一直没离开窗口，但文件被后台进程改了
    const id = setInterval(onFocus, 10_000);
    return () => {
      window.removeEventListener("focus", onFocus);
      clearInterval(id);
    };
  });

  /** 待确认的模式切换（大文件切到编辑模式时用） */
  let pendingSwitch = $state<TabState | null>(null);

  function requestSwitchMode(tab: TabState) {
    if (tab.dirty) {
      error = "有未保存的改动，请先保存（⌘S）再切换模式";
      setTimeout(() => (error = ""), 2600);
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
    error = "";
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
      error = String(e);
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
    if (k === "1") {
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

  /** 侧边栏横向拖拽。上限留出编辑区的活路，不让它被挤没 */
  function startSideResize(e: PointerEvent) {
    e.preventDefault();
    const startX = e.clientX;
    const startW = sidebarWidth;
    const move = (ev: PointerEvent) => {
      sidebarWidth = Math.max(140, Math.min(window.innerWidth - 360, startW + (ev.clientX - startX)));
    };
    const up = () => {
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

{#if EncPickerComp && active}
  <EncPickerComp
    bind:open={encOpen}
    current={active.encoding ?? "UTF-8"}
    bom={!!active.bom}
    lossy={!!active.lossy}
    readonly={active.mode !== "edit"}
    onReopen={(l) => void reopenWith(l)}
    onSaveAs={saveAsEncoding}
  />
{/if}

{#if BranchPickerComp && repo}
  <BranchPickerComp
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
    {#if !sidebar}
      <!-- 侧边栏收起后，展开的入口只能放这儿；展开时它在侧边栏头部右侧 -->
      <button class="side-toggle" onclick={() => (sidebar = true)} title="展开侧边栏 ⌘1" aria-label="展开侧边栏">
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
          <path d="M6.5 3.5 L11 8 L6.5 12.5" fill="none" stroke="currentColor" stroke-width="1.5"
                stroke-linecap="round" stroke-linejoin="round" />
          <path d="M3.5 3.5 L3.5 12.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
        </svg>
      </button>
    {/if}
    <span class="app">lite-ide</span>
    {#if active}
      <span class="sep">—</span>
      <span class="file">{active.name}</span>
      {#if active.mode === "log"}
        <span class="why" title={active.forced ? "你手动切到了日志模式" : "自动判定的原因"}>
          只读 · {active.forced ? "手动切换" : active.reason || "自动判定"}
        </span>
      {/if}
    {/if}
  </header>

  <div class="workspace" class:no-side={!sidebar} style:--side-w="{sidebarWidth}px">
    {#if sidebar}
      <aside>
        {#if !root}
          <div class="no-root">把文件夹拖进来</div>
        {:else if sideView === "git" && repo && GitPaneComp}
          <GitPaneComp
            status={gitSt}
            busy={gitBusy}
            onOpenDiff={(e, staged) => void (e.conflicted ? openMerge(e) : openDiff(e, staged))}
            onStage={(paths) => void gitDo("暂存失败", () => gitStage(repo!, paths))}
            onUnstage={(paths) => void gitDo("取消暂存失败", () => gitUnstage(repo!, paths))}
            onDiscard={(es) => (pendingDiscard = es)}
            onCommit={doGitCommit}
            onRefresh={() => void refreshGit()}
            onFiles={() => (sideView = "files")}
            onCollapse={() => (sidebar = false)}
          />
        {:else if sideView === "git" && repo}
          <div class="no-root">正在载入 Git 面板…</div>
        {:else}
          <FileTree
            {root}
            activePath={active?.path ?? ""}
            gitStatus={gitSt}
            onOpen={(p) => void openPath(p)}
            onSearch={() => {
              quickScope = "content";
              quickOpen = true;
            }}
            onGit={repo ? () => (sideView = "git") : undefined}
            onCollapse={() => (sidebar = false)}
          />
        {/if}
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

      <div class="content">
        {#if !active}
          <div class="empty">
            <div class="big">把文件或文件夹拖进来</div>
            <p>代码走编辑模式，大文件与日志自动走只读的日志模式</p>
            <p class="keys">双击 ⇧ 随处搜索 · ⌘P 找文件 · ⌘⇧F 搜内容 · ⌘⇧O 文件结构</p>
            <p class="keys">⌘S 保存 · ⌘W 关闭标签 · ⌘1 侧边栏 · ⌘J 终端 · ⌘⇧G 改动</p>
            {#if error}<p class="err">{error}</p>{/if}
          </div>
        {:else if active.mode === "merge" && MergeViewComp}
          {#key active.id}
            <MergeViewComp
              text={active.mergeText ?? ""}
              path={active.rel ?? active.name}
              onResolve={(c, r) => void resolveMerge(active!, c, r)}
            />
          {/key}
        {:else if active.mode === "merge"}
          <div class="empty"><p>正在载入合并视图…</p></div>
        {:else if active.mode === "diff" && DiffViewComp}
          {#key active.id}
            <DiffViewComp
              raw={active.diffRaw ?? ""}
              path={active.rel ?? active.name}
              staged={!!active.diffStaged}
              commit={active.diffShort ?? ""}
              onToggleStaged={() => void toggleDiffSide(active!)}
            />
          {/key}
        {:else if active.mode === "diff"}
          <div class="empty"><p>正在载入差异视图…</p></div>
        {:else if active.mode === "log" && active.handle !== undefined && LogPaneComp}
          {#key active.id}
            <LogPaneComp
              handle={active.handle}
              {gotoLine}
              encoding={active.encoding ?? "utf-8"}
              onStatus={(s) => (logStatus = s)}
            />
          {/key}
        {:else if active.mode === "log"}
          <div class="empty"><p>正在载入日志视图…</p></div>
        {:else if EditorComp}
          {#key active.id}
            <EditorComp
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
              {#if TerminalComp}
                {#each terms as t (t.id)}
                  <div class="term-slot" class:hidden={t.id !== activeTermId}>
                    <TerminalComp cwd={t.cwd} onExit={() => closeTerm(t.id)} />
                  </div>
                {/each}
              {:else}
                <div class="loading">正在载入终端…</div>
              {/if}
            </div>
            {#if panelView === "log" && repo}
              <div class="tool-slot">
                {#if GitLogComp}
                  <GitLogComp
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
        <span class="cell dim drop-2">{langLabel(langOf(active.path))}</span>
      {/if}
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
        <span class="cell">{logStatus}</span>
      {:else}
        <span class="cell drop-2">{active.dirty ? "已修改" : "无改动"}</span>
      {/if}
      {#if activeEntry}
        <button
          class="cell btn git"
          onclick={() => void openDiff(activeEntry!, false)}
          title="查看这个文件的改动"
        >
          {activeEntry.untracked ? "未跟踪" : "有改动"}
        </button>
      {/if}
    {:else}
      <span class="cell dim">等待文件</span>
    {/if}
    {#if saved}<span class="cell ok">{saved}</span>{/if}
    {#if error}<span class="cell err">{error}</span>{/if}
    <span class="spacer"></span>
    {#if gitSt}
      <button
        class="cell btn branch"
        onclick={() => (branchOpen = true)}
        title={`切换分支 / 工作树${gitSt.upstream ? ` · 跟踪 ${gitSt.upstream}` : "（没有上游分支）"}`}
      >
        <svg viewBox="0 0 16 16" width="11" height="11" aria-hidden="true">
          <circle cx="4.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.4" />
          <circle cx="4.5" cy="12.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.4" />
          <circle cx="11.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.4" />
          <path d="M4.5 5.3 L4.5 10.7" stroke="currentColor" stroke-width="1.4" />
          <path d="M11.5 5.3 Q11.5 8.5 4.5 10.7" fill="none" stroke="currentColor" stroke-width="1.4" />
        </svg>
        {gitSt.branch || "游离"}{gitSt.ahead ? ` ↑${gitSt.ahead}` : ""}{gitSt.behind ? ` ↓${gitSt.behind}` : ""}
        {#if gitSt.entries.length}<span class="chg">{gitSt.entries.length}</span>{/if}
      </button>
      <button
        class="cell btn"
        class:on={sidebar && sideView === "git"}
        onclick={() => {
          sideView = sidebar && sideView === "git" ? "files" : "git";
          sidebar = true;
        }}
        title="改动列表 ⌘⇧G"
      >改动</button>
      <button
        class="cell btn"
        class:on={panel && panelView === "log"}
        onclick={() => {
          panelView = "log";
          panel = true;
        }}
        title="提交历史"
      >历史</button>
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
  .side-toggle {
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    color: var(--text-faint);
    cursor: default;
    border-radius: 3px;
  }
  .side-toggle:hover { background: var(--panel-bg-2); color: var(--text); }
  .titlebar .app { color: var(--text); font-weight: 500; }
  .titlebar .sep { color: var(--text-faint); }
  .titlebar .file { color: var(--text-dim); }
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
    /* 三列：侧边栏 · 拖拽条 · 主区 */
    grid-template-columns: var(--side-w, 240px) 4px 1fr;
    overflow: hidden;
  }
  .workspace.no-side { grid-template-columns: 1fr; }
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
  .empty .big { font-size: 17px; color: var(--text); margin-bottom: 10px; }
  .empty p { margin: 4px 0; font-size: 12.5px; color: var(--text-faint); }
  .empty .keys { margin-top: 14px; font-family: var(--code-font); font-size: 11.5px; }
  .empty .err { margin-top: 14px; color: var(--lvl-error); font-family: var(--code-font); }

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
  .statusbar .btn.branch {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 260px;
  }
  .statusbar .btn.branch .chg {
    background: var(--panel-bg-2);
    border-radius: 7px;
    padding: 0 5px;
    font-size: 10px;
    color: var(--text-dim);
  }
</style>
