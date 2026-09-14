<script lang="ts">
  import { untrack, tick } from "svelte";
  import { Channel } from "@tauri-apps/api/core";
  import FileTree from "./lib/shell/FileTree.svelte";
  import Rail from "./lib/shell/Rail.svelte";
  import Sidebar from "./lib/shell/Sidebar.svelte";
  import Panel from "./lib/shell/Panel.svelte";
  import StatusBar from "./lib/shell/StatusBar.svelte";
  import TitleBar from "./lib/shell/TitleBar.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import type { Action } from "./lib/search/QuickSearch.svelte";
  import type { JumpHit } from "./lib/editor/jump";
  import { lazy, lazyGroup } from "./lib/lazy/lazy.svelte";
  import { notify } from "./lib/state/notify.svelte";
  import { layout } from "./lib/state/layout.svelte";
  import { tabs } from "./lib/state/tabs.svelte";
  import { tabflow } from "./lib/state/tabflow.svelte";
  import { project } from "./lib/state/project.svelte";
  import { terms } from "./lib/state/terms.svelte";
  import * as session from "./lib/state/session";
  import { stashed } from "./lib/state/doc";
  import { docs } from "./lib/state/docs.svelte";
  import Crash from "./lib/shell/Crash.svelte";
  import { KEYS, byId as keyById } from "./lib/state/keymap";
  import type { Sym } from "./lib/editor/outline";
  import type { ChangeKind } from "./lib/git/diff";
  import {
    probePath,
    ignoredDirs,
    appLogPath,
    clearAppLog,
    readText,
    writeText,
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
    openLog,
    closeLog,
    reportBudget,
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
    listProjectFiles,
    scratchDir,
    createScratch,
    type GitEntry,
    type GitStatus,
    type GitWorktree,
  } from "./lib/ipc/commands";
  import type { TabState } from "./lib/state/tab";


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
  // 布局状态住在 layout.svelte.ts（导轨 / 侧边栏 / 面板各自直接读写），这里只灌一次
  layout.restore(saved?.layout ?? session.DEFAULT_LAYOUT);
  project.recent = saved?.recent ?? [];
  // 文档生命周期往外的两个钩子：保存完刷 git，光标动了安排存快照（见 docs.svelte.ts 文件头）
  docs.hooks.afterSave = () => void refreshGit();
  docs.hooks.afterPos = () => scheduleSave();




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
    const r = project.root;
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
      (layout.sideView === "git" && !!repo) ||
      tabs.list.some((t) => t.mode === "diff" || t.mode === "merge") ||
      (layout.panel && panelTool === "git" && layout.gitTab === "log") ||
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
    const tab = tabs.active;
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
      docs.savedTick++;
      notify.ok(`已按 ${t.encoding} 重新打开${t.lossy ? "（仍有解不出的字节）" : ""}`, 3000);
    } catch (e) {
      notify.fail(String(e));
    }
  }

  /** 只改「将来存成什么编码」，不动当前内容 */
  function saveAsEncoding(label: string, bom: boolean) {
    const tab = tabs.active;
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
    await tabflow.openPath(path);
    const name = path.slice(path.lastIndexOf("/") + 1) || path;
    notify.ok(`项目根已切到 ${name}（打开的标签没有动）`, 3200);
  }

  function newWorktree(dir: string, branch: string) {
    void gitDo("新建工作树失败", async () => {
      // 分支存不存在由 gitsvc 判，这里只管「要一个跑着这个分支的目录」
      const path = await gitWorktreeAdd(repo!, dir, branch);
      notify.ok(`工作树已建在 ${path}`, 3600);
      await tabflow.openPath(path);
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
    const tab = tabs.active;
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
    let id = tabs.list.find((t) => t.path === key)?.id;
    if (id === undefined) {
      id = tabs.add({
        path: key,
        name: rel.slice(rel.lastIndexOf("/") + 1),
        mode: "diff",
        dirty: false,
        size: 0,
        rel,
        diffSha: sha,
        diffShort: short,
      });
    }
    tabs.activeId = id;
    await reloadDiff(id);
  }

  /** 换项目根就重新找仓库。找不到时把 Git 的一切都清干净 */
  $effect(() => {
    const r = project.root;
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
          layout.sideView = "files";
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
        tabs.list.filter((t) => t.mode === "diff" && !t.diffSha).map((t) => reloadDiff(t.id)),
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
   * 原始对象上写（`const tab = {...}; tabs.list = [...tabs.list, tab]; tab.x = 1`）
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
    await docs.checkExternalChanges();
    treeTick++;
  }

  /**
   * 一个标签是不是「在 p 底下」。目录要连子树一起算 ——
   * 改名或删掉一个目录，里面开着的每个文件都受影响。
   */


  /**
   * 在文件树里改完名，打开着的标签要跟着走。
   *
   * 少了这一步的表现是：标签还挂着旧名字，按 ⌘S 报「文件不在盘上了」——
   * 而名字是人刚刚亲手改的，最不会去怀疑的就是这件事。
   */
  async function renameOpenTabs(from: string, to: string, isDir: boolean) {
    const moved: number[] = [];
    for (const t of tabs.under(from, isDir)) {
      const np = to + t.path.slice(from.length);
      // 位置记忆的 key 也是路径，一起搬 —— 不搬的话切回这个文件会跳回第一行
      const pos = docs.posByPath.get(t.path);
      if (pos !== undefined) {
        docs.posByPath.delete(t.path);
        docs.posByPath.set(np, pos);
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
      const before = tabs.byId(id);
      if (!before || before.mode !== "log" || before.handle === undefined) continue;
      const stale = before.handle;
      try {
        const h = (await openLog(before.path)).handle;
        // await 回来必须按 id 重新取一次 —— 手上那个引用可能已经不是
        // 响应式的那一份了（AGENTS.md 里那条 $state 数组的坑）
        const now = tabs.byId(id);
        if (now) now.handle = h;
        void closeLog(stale);
      } catch (e) {
        notify.fail(`${before.name} 改名后重开日志失败，tail 会停：${String(e)}`);
      }
    }
  }

  /** 进废纸篓的东西，开着的标签一并关掉（确认框已经说过会关几个未保存的） */
  function closeTabsUnder(p: string, isDir: boolean) {
    for (const t of tabs.under(p, isDir)) tabflow.doClose(t);
  }

  /**
   * 文件树里改完盘之后的收尾。
   *
   * 三件事一起做：重读打开的文件 + 重列目录（`workingTreeChanged`），
   * 再刷一次 git 状态 —— 新建出来的文件是未跟踪的，删掉的要显示成 D，
   * 少这一下文件树上的染色就停在改动之前。
   */
  async function afterFsChange(openThis: string | null) {
    if (openThis) await tabflow.openPath(openThis);
    await workingTreeChanged();
    void refreshGit();
  }


  async function reloadDiff(id: number) {
    const tab = tabs.byId(id);
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
      let id = tabs.list.find((t) => t.path === key)?.id;
      if (id === undefined) {
        id = tabs.add({
          path: key,
          name: e.path.slice(e.path.lastIndexOf("/") + 1),
          mode: "merge",
          dirty: false,
          size: 0,
          rel: e.path,
          mergeText: content,
        });
      } else {
        const t = tabs.byId(id);
        if (t) t.mergeText = content;
      }
      tabs.activeId = id;
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
        tabflow.doClose(tab);
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
    let id = tabs.list.find((t) => t.mode === "diff" && t.path === key)?.id;
    if (id === undefined) {
      id = tabs.add({
        path: key,
        name: e.path.slice(e.path.lastIndexOf("/") + 1),
        mode: "diff",
        dirty: false,
        size: 0,
        rel: e.path,
      });
    }
    // 从数组里重新取一次，拿到的才是响应式的那份
    const tab = tabs.byId(id);
    if (!tab) return;
    tab.diffStaged = staged;
    tab.diffUntracked = e.untracked && !staged;
    tabs.activeId = id;
    await reloadDiff(id);
  }

  /** 差异标签上切换「已暂存 ↔ 未暂存」 */
  async function toggleDiffSide(id: number) {
    const tab = tabs.byId(id);
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
  /**
   * 实际在渲染的那个工具窗（开合 / 偏好本身在 `layout` 里）。
   *
   * `layout.panelView` 是**存下来的偏好**，它可以是 `git` 而当下并没有仓库 ——
   * 上次在一个 git 仓库里看着提交历史退出，这次打开的是个普通文件夹。
   * 那时面板头写着「Git」，底下却是一片空白（历史那块的渲染条件
   * 带着 `&& repo`），而头上已经没有「切回终端」的按钮了（切换搬去了导轨）。
   *
   * 所以渲染一律看这个，写状态才写 `layout.panelView` —— 偏好留着，
   * 下次真打开仓库时提交历史还在。
   */
  let panelTool = $derived<"term" | "git">(layout.panelView === "git" && repo ? "git" : "term");

  let hovering = $state(false);
  let logStatus = $state("");

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
    if (tabs.active?.mode === "log") logPane.load();
  });

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
    if (tabs.active?.mode !== "edit") return;
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
    if (!gitSt || !tabs.active || tabs.active.mode === "diff") return null;
    const prefix = `${gitSt.root}/`;
    if (!tabs.active.path.startsWith(prefix)) return null;
    const rel = tabs.active.path.slice(prefix.length);
    return gitSt.entries.find((e) => e.path === rel) ?? null;
  });






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
    const r = project.root;
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

  /** 此刻在哪儿。`docs.posByPath` 里存的是编辑器最后报上来的光标行 */
  function hereNow(): NavSpot | null {
    if (!tabs.active) return null;
    return { path: tabs.active.path, line: docs.posByPath.get(tabs.active.path) ?? 1 };
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
    const w = docs.wordUnderCursor();
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
    const full = path.startsWith("/") ? path : `${project.root ?? ""}/${path}`;
    await tabflow.openPath(full);
    if (line !== undefined) gotoLine = { line, nonce: ++gotoNonce };
  }

  $effect(() => {
    if (tabs.active?.mode === "edit") editor.load();
  });


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
    const e =
      editor.error ||
      logPane.error ||
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
    if (tabs.active && !langs) void import("./lib/editor/langs").then((m) => (langs = m));
  });

  let outlineSupported = $derived(
    tabs.active?.mode === "edit" && !!langs && LEZER_LANGS.has(langs.langOf(tabs.active.path) ?? ""),
  );






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
      await tabflow.openPath(await createScratch(stem));
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
      await tabflow.openPath(dir);
    } catch (e) {
      notify.fail(String(e));
    }
  }




  /**
   * 开原生的选择文件夹面板。取消了什么也不做。
   *
   * 选中之后走的是 `openPath` —— 它对目录的处理就是把 project.root 设过去，
   * 和拖一个文件夹进来、命令行传目录**是同一条路**。
   * 另起一套的话，「切项目要不要清掉旧标签」这类判断就会有两份。
   */
  async function openFolder() {
    const dir = await pickFolder().catch(() => null);
    if (!dir) return;
    await tabflow.openPath(dir);
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
      project.recent = project.recent.filter((r) => r !== dir);
      return;
    }
    await tabflow.openPath(dir);
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
    await tabflow.openPath(path);
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
      return `在终端里跑一次，输一遍账号密码，之后就一直有效：\n  git -C ${project.root} fetch`;
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
      if (ok) project.root = saved.root;
    }
    /*
     * **先记位置，再开文件。** 反过来写过一版，位置恢复整个不生效：
     * `openPath` 一把标签加进去，tabs.activeId 就变了，兑现位置的那个 effect
     * 当场就跑 —— 而那时 `docs.pendingPos` 里还什么都没有。等 effect 跑完再写进去，
     * tabs.activeId 已经不会再变，effect 也就不会再跑第二次了。
     */
    for (const t of saved.tabs) {
      if (t.line !== undefined) docs.pendingPos.set(t.path, t.line);
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
        await tabflow.openPath(t.path, true);
        if (t.path === wantPath) {
          const hit = tabs.list.find((x) => x.path === t.path);
          // 这一下是整个恢复过程里唯一一次内容区渲染
          if (hit) tabs.activeId = hit.id;
        }
      }
    } finally {
      // 这里必须 finally：漏掉的话 tabs.activeId 就永久失灵，
      // 而 openPath 是会抛的（文件没了、读不动、编码探测失败）
      tabflow.restoringTabs = false;
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
      const tab = tabs.list.find((t) => t.path === snapTab.path && t.mode === "edit");
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
     * 兜底。正常路径上 tabs.activeId 在上面那个循环里就点过了 ——
     * 这里只服务两种情况：上次激活的那个文件这次不在了（循环里没命中），
     * 或者 `saved.active` 越界。那时退到第一个恢复成功的标签，
     * 总比停在一个空内容区上好。
     */
    if (tabs.activeId === null && tabs.list.length > 0) tabs.activeId = tabs.list[0].id;
    // 上次开着、这次已经不在的文件：从记忆里也删掉，不然它们
    // 会一直躺在快照里，每次启动都白试一遍
    for (const t of saved.tabs) {
      if (!tabs.list.some((x) => x.path === t.path)) docs.pendingPos.delete(t.path);
    }
    tabs.audit("会话恢复");
  }

  /**
   * 活动标签换了：如果它带着一个待兑现的恢复位置，跳过去并**销号**。
   *
   * 销号是关键 —— 不删的话，以后每次切回这个标签都会被拽回那一行，
   * 用户在别处读到一半切走再切回来就莫名其妙跳走了。
   */
  $effect(() => {
    const t = tabs.active;
    if (!t) return;
    const line = docs.pendingPos.get(t.path);
    if (line === undefined) return;
    docs.pendingPos.delete(t.path);
    gotoLine = { line, nonce: ++gotoNonce };
  });

  /** 按当前状态拍一张快照 */
  function snapshot(): session.Session {
    return {
      root: project.root,
      tabs: tabs.list.map((t) => {
        const line = docs.posByPath.get(t.path);
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
          snap.draft = docs.liveText(t);
          if (t.stamp) snap.stamp = { mtimeMs: t.stamp.mtimeMs, size: t.stamp.size };
        }
        return snap;
      }),
      active: Math.max(0, tabs.list.findIndex((t) => t.id === tabs.activeId)),
      layout: layout.snapshot(),
      recent: [...project.recent],
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
      const dirty = tabs.list.filter((t) => t.mode === "edit" && t.dirty);
      if (dirty.length === 0) return;
      // 存不下的那种要当面说 —— 不说的话用户以为自己被记住了
      for (const t of dirty) {
        if (warnedBig.has(t.path)) continue;
        if (docs.liveText(t).length <= session.MAX_DRAFT_CHARS) continue;
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
    void [project.root, tabs.list.length, tabs.activeId, layout.snapshot()];
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
      case "recent-clear": project.recent = []; return;
      case "save": return docs.saveActive();
      case "encoding":
        if (tabs.active) encOpen = true;
        return;
      case "close-tab":
        if (tabs.active) tabflow.requestClose(tabs.active.id);
        return;
      case "close-all-tabs": return tabflow.closeMany(tabs.list.map((t) => t.id));
      case "toggle-mode":
        if (tabs.active) tabflow.requestSwitchMode(tabs.active);
        return;
      case "quick-all": quickScope = "all"; quickSeed = ""; quickOpen = true; return;
      case "quick-file": quickScope = "file"; quickSeed = ""; quickOpen = true; return;
      case "quick-content": quickScope = "content"; quickSeed = ""; quickOpen = true; return;
      case "find-word": return findWordAtCursor();
      case "nav-back": return void navGo("back");
      case "nav-fwd": return void navGo("fwd");
      case "outline": return openOutline();
      case "toggle-sidebar": layout.sidebar = !layout.sidebar; return;
      case "toggle-panel": layout.panel = !layout.panel; return;
      case "toggle-minimap": showMinimap = !showMinimap; return;
      case "new-terminal": terms.open(project.root ?? "~"); return;
      case "close-terminal":
        if (terms.activeId !== null) terms.close(terms.activeId);
        return;
      case "git-changes":
        // 已经在 Git 视图上再点一次就切回去，和 ⇧⌘G 是同一个手势
        layout.toggleGitChanges();
        return;
      case "git-file-diff": {
        const en = activeEntry;
        if (en) void openDiff(en, false);
        else notify.fail("当前文件没有未提交的改动", 2600);
        return;
      }
      case "git-log": layout.openGitTab("log"); return;
      case "git-console": layout.openGitTab("console"); return;
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

  $effect(() => {
    // 见 `project.scratchRoot` 的注释：故意不挂在启动那条 await 链上
    void scratchDir()
      .then((d) => (project.scratchRoot = d))
      .catch(() => {});
    initialPath()
      .then(async (p) => {
        if (tabs.list.length > 0 || project.root !== null) return;
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
        await tabflow.openPath(p);
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
    void syncMenuState(tabs.active !== null, repo !== null, terms.activeId !== null).catch(() => {});
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

{#if keysPanel.comp}
  <keysPanel.comp bind:open={keysOpen} />
{/if}

{#if overlays.comps.outline}
  <overlays.comps.outline
    bind:open={outlineOpen}
    {symbols}
    fileName={tabs.active?.name ?? ""}
    supported={outlineSupported}
    onPick={(line) => (gotoLine = { line, nonce: ++gotoNonce })}
  />
{/if}

{#if overlays.comps.quick}
  <overlays.comps.quick
    bind:open={quickOpen}
    bind:scope={quickScope}
    seed={quickSeed}
    root={project.root}
    {actions}
    onOpenFile={openAt}
  />
{/if}

{#if encPicker.comp && tabs.active}
  <encPicker.comp
    bind:open={encOpen}
    current={tabs.active.encoding ?? "UTF-8"}
    bom={!!tabs.active.bom}
    lossy={!!tabs.active.lossy}
    readonly={tabs.active.mode !== "edit"}
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



<main class:hovering>
  <TitleBar
    root={project.root}
    {gitSt}
    recent={project.recent}
    {branchOpen}
    bind:branchBtn
    onOpenRecent={(r) => void openRecent(r)}
    onOpenFolder={() => void openFolder()}
    onClearRecent={() => (project.recent = [])}
    onOpenBranches={openBranchPicker}
  />

  <div
    class="workspace"
    class:no-side={!layout.sidebar}
    class:resizing={layout.resizing}
    style:--side-w="{layout.sidebarWidth}px"
  >
    <Rail
      root={project.root}
      {repo}
      changes={gitSt?.entries.length ?? 0}
      {panelTool}
      onSearch={() => {
        quickScope = "content";
        quickSeed = "";
        quickOpen = true;
      }}
      onTogglePanel={(v) => layout.togglePanel(v, panelTool)}
    />

    {#if layout.sidebar}
      <!--
        侧边栏外壳在 Sidebar.svelte 里；两块内容的数据和回调还接在 App 上
        （标签表、git 动作没搬出去），所以以 snippet 传进去。
      -->
      <Sidebar root={project.root} {repo} gitReady={!!git.comps.pane}>
        {#snippet gitPane()}
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
            onOpenLog={() => layout.openGitTab("log")}
            ahead={gitSt?.ahead ?? 0}
            behind={gitSt?.behind ?? 0}
            onSync={(what) => void (what === "push" ? askPush() : doPull())}
            syncing={syncing ? { what: syncing.what, phase: syncing.phase, percent: syncing.percent } : null}
            onCancelSync={syncing && syncing.what !== "push" ? cancelSync : null}
          />
        {/snippet}
        {#snippet fileTree()}
          <!-- `root!`：这块只在 Sidebar 判过 root 非空之后才渲染，收窄在那个文件里 -->
          <FileTree
            root={project.root!}
            activePath={tabs.active?.path ?? ""}
            gitStatus={gitSt}
            {ignored}
            reloadTick={treeTick}
            {revealPath}
            {revealTick}
            onOpen={(p) => void tabflow.openPath(p)}
            dirtyUnder={(p) => tabs.dirtyUnder(p)}
            onCreated={(p, isDir) => void afterFsChange(isDir ? null : p)}
            onRenamed={(from, to, isDir) =>
              void renameOpenTabs(from, to, isDir).then(() => afterFsChange(null))}
            onTrashed={(p, isDir) => {
              closeTabsUnder(p, isDir);
              void afterFsChange(null);
            }}
          />
        {/snippet}
      </Sidebar>
    {/if}

    <section class="main">
      {#if tabs.list.length > 0}
        <Tabs
          tabs={tabs.list}
          activeId={tabs.activeId}
          root={project.root ?? ""}
          onSelect={(id) => {
            tabs.activeId = id;
            tabs.audit("切标签");
          }}
          onClose={(...a) => tabflow.requestClose(...a)}
          onCloseMany={(...a) => tabflow.closeMany(...a)}
          onRevealInTree={revealInTree}
          onNewScratch={newScratch}
        />
      {/if}

      {#if tabs.active?.conflict}
        <div class="confirm conflict">
          <span><b>{tabs.active.name}</b> 在编辑器外被改过，而你这边也有未保存的改动</span>
          <button class="primary" onclick={() => docs.resolveConflict(tabs.active!, "mine")}>保留我的</button>
          <button onclick={() => docs.resolveConflict(tabs.active!, "disk")}>用磁盘上的</button>
        </div>
      {/if}

      {#if tabflow.pendingSwitch}
        <div class="confirm">
          <span>
            <b>{tabflow.pendingSwitch.name}</b> 有 {(tabflow.pendingSwitch.size / 1048576).toFixed(1)}MB，
            编辑模式会把全文读进内存，可能明显卡顿
          </span>
          <button class="primary" onclick={() => tabflow.doSwitch(tabflow.pendingSwitch!, "edit")}>仍然编辑</button>
          <button onclick={() => (tabflow.pendingSwitch = null)}>取消</button>
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
              layout.showSide("git");
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

      {#if tabflow.pendingClose}
        <div class="confirm">
          <span><b>{tabflow.pendingClose.name}</b> 有未保存的改动</span>
          {#if tabflow.closeQueue.length}
            <!-- 批量关闭时要说清后面还有几个，否则人不知道这个框还要弹几次 -->
            <span class="rest">（后面还有 {tabflow.closeQueue.length} 个）</span>
          {/if}
          <button class="primary" onclick={() => void tabflow.resolveClose("save")}>保存并关闭</button>
          <button onclick={() => void tabflow.resolveClose("discard")}>丢弃改动</button>
          <button onclick={() => void tabflow.resolveClose("cancel")}>取消</button>
        </div>
      {/if}

      <!--
        内容区单独设边界：编辑器 / 日志 / 差异里任何一处抛异常，
        都不该把整个外壳一起带走 —— 文件树、终端、状态栏还得能用。
        boundary 的 reset 会重建这棵子树，多数一次性的渲染错误重试一下就好了。
      -->
      <svelte:boundary onerror={(e) => notify.fail(`内容区出错：${e}`)}>
      <div class="content">
        {#if !tabs.active}
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
                {#if project.recent.length > 0}
                  <span class="lastly">最近：</span>
                  <button class="link" onclick={() => void openRecent(project.recent[0])}>
                    {project.recent[0].slice(project.recent[0].lastIndexOf("/") + 1) || project.recent[0]}
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
        {:else if tabs.active.mode === "merge" && git.comps.merge}
          {#key tabs.active.id}
            <git.comps.merge
              text={tabs.active.mergeText ?? ""}
              path={tabs.active.rel ?? tabs.active.name}
              onResolve={(c, r) => void resolveMerge(tabs.active!, c, r)}
            />
          {/key}
        {:else if tabs.active.mode === "merge"}
          <div class="empty"><p>正在载入合并视图…</p></div>
        {:else if tabs.active.mode === "diff" && git.comps.diff}
          {#key tabs.active.id}
            <git.comps.diff
              raw={tabs.active.diffRaw ?? ""}
              capped={!!tabs.active.diffCapped}
              path={tabs.active.rel ?? tabs.active.name}
              staged={!!tabs.active.diffStaged}
              commit={tabs.active.diffShort ?? ""}
              untracked={!!tabs.active.diffUntracked}
              onToggleStaged={() => void toggleDiffSide(tabs.active!.id)}
            />
          {/key}
        {:else if tabs.active.mode === "diff"}
          <div class="empty"><p>正在载入差异视图…</p></div>
        {:else if tabs.active.mode === "log" && tabs.active.handle !== undefined && logPane.comp}
          {#key tabs.active.id}
            <logPane.comp
              handle={tabs.active.handle}
              {gotoLine}
              encoding={tabs.active.encoding ?? "utf-8"}
              onStatus={(s) => (logStatus = s)}
              onTop={(l) => docs.markPos(tabs.active!.path, l)}
            />
          {/key}
        {:else if tabs.active.mode === "log"}
          <div class="empty"><p>正在载入日志视图…</p></div>
        {:else if editor.comp}
          {#key tabs.active.id}
            <editor.comp
              path={tabs.active.path}
              initial={tabs.active.draft ?? tabs.active.content ?? ""}
              baseline={tabs.active.content ?? ""}
              savedTick={docs.savedTick}
              {gotoLine}
              {outlineTick}
              marks={editorMarks}
              {showMinimap}
              onChange={(d) => (tabs.active!.dirty = d)}
              onSave={(c) => docs.save(c)}
              onStash={(p, t) => docs.stashDraft(p, t)}
              onLive={(p, g) => docs.onEditorLive(p, g)}
              onWordProbe={(p, g) => docs.onEditorWordProbe(p, g)}
              onOutline={(s) => (symbols = s)}
              onCursor={(l) => docs.markPos(tabs.active!.path, l)}
              jumpFiles={projectFiles}
              jumpRel={project.root && tabs.active.path.startsWith(`${project.root}/`)
                ? tabs.active.path.slice(project.root.length + 1)
                : null}
              jumpLang={langs?.langOf(tabs.active.path) ?? ""}
              onJump={(hit) => void jumpTo(hit)}
            />
          {/key}
        {:else}
          <div class="empty"><p>正在载入编辑器…</p></div>
        {/if}
      </div>

      {#snippet failed(err, reset)}
        <div class="content">
          <Crash error={err} scope={tabs.active ? `${tabs.active.name} 的视图` : "内容区"} onReset={reset} />
        </div>
      {/snippet}
      </svelte:boundary>

      <!--
        底部工具窗在 Panel.svelte 里。提交历史那块要这边的 git lazyGroup 和活动标签，
        以 snippet 传进去（同侧边栏的两块内容）。
      -->
      <Panel root={project.root} {repo} {panelTool} gitLogReady={!!git.comps.log}>
        {#snippet gitLog()}
          <git.comps.log
            repo={repo!}
            filePath={tabs.active?.mode === "edit" ? tabs.active.path : ""}
            onOpenCommitDiff={(sha, short, p) => void openCommitDiff(sha, short, p)}
          />
        {/snippet}
      </Panel>
    </section>
  </div>

  <StatusBar
    active={tabs.active}
    {activeEntry}
    root={project.root}
    {langs}
    {logStatus}
    onReveal={revealInTree}
    onSwitchMode={() => tabflow.requestSwitchMode(tabs.active!)}
    onOpenEncoding={() => (encOpen = true)}
    onOpenDiff={() => void openDiff(activeEntry!, false)}
  />
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


  /*
   * 用 flex 列而不是 grid：这一列里的元素是**条件渲染**的（标签栏、三种确认条、
   * 拖拽条、终端面板都可能不在），固定行数的 grid 会让后面的元素往前占位 ——
   * 曾经导致终端面板抢到 1fr 跑到内容区上面去。
   * flex 天然按实际存在的元素排布，content 吃掉剩余空间就行。
   */
  .main { display: flex; flex-direction: column; overflow: hidden; }
  .content { flex: 1; min-height: 0; overflow: hidden; }


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

</style>
