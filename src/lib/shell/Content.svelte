<script lang="ts">
  /**
   * 内容区：活动标签的视图（编辑器 / 日志 / 差异 / 合并），以及一个标签都没有时
   * 的起点卡片。四种视图都是懒加载的（理由各在自己的 lazy 上）。
   *
   * 从 App.svelte 搬出来（issue #9 第 6 步）。`Merge` / `Diff` 属于 Git 那组
   * `lazyGroup`，那组还在 App（侧边栏、面板、确认条也用它），以组件类型传进来。
   * `showMinimap` 是 App 上的偏好，传 prop；
   * 日志视图的状态行和编辑器的大纲往上报，状态栏和大纲浮层要。
   */
  import Crash from "./Crash.svelte";
  import type MergeView from "../git/MergeView.svelte";
  import type DiffView from "../git/DiffView.svelte";
  import type { Sym } from "../editor/outline";
  import type { ChangeKind } from "../git/diff";
  import type { KeyDef } from "../state/keymap";
  import { lazy } from "../lazy/lazy.svelte";
  import { gitDiff, listProjectFiles } from "../ipc/commands";
  import { notify } from "../state/notify.svelte";
  import { tabs } from "../state/tabs.svelte";
  import { docs } from "../state/docs.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { project } from "../state/project.svelte";
  import { worktree } from "../state/worktree.svelte";
  import { git } from "../state/git.svelte";
  import { nav } from "../state/nav.svelte";
  import { lang } from "../state/lang.svelte";

  let {
    Merge,
    Diff,
    showMinimap,
    outlineTick,
    keyHints,
    onLogStatus,
    onOutline,
  }: {
    Merge: typeof MergeView | undefined;
    Diff: typeof DiffView | undefined;
    showMinimap: boolean;
    /** 大纲浮层里点了一条，让编辑器重算一次符号 */
    outlineTick: number;
    /** 起点卡片上那几条键位 */
    keyHints: KeyDef[];
    onLogStatus: (text: string) => void;
    onOutline: (syms: Sym[]) => void;
  } = $props();

  /**
   * CodeMirror 6 核心约 340KB，日志模式一点也用不上 —— 静态引入会把入口包
   * 从 71KB 顶到 412KB，与"秒开"的立身之本冲突。改成打开第一个可编辑文件时
   * 才 import，本地加载只有几毫秒。
   */
  const editor = lazy(() => import("../editor/Editor.svelte"), "编辑器");

  /*
   * 日志视图同样按需加载 —— 和 Editor 对称。
   * 早先它是静态引入的，等于只写代码的人一直在为整套日志视图
   * （虚拟滚动 + 过滤条 + 8 种格式的解析着色）付钱。
   */
  const logPane = lazy(() => import("../logview/LogPane.svelte"), "日志视图");

  $effect(() => {
    if (tabs.active?.mode === "edit") editor.load();
  });
  $effect(() => {
    if (tabs.active?.mode === "log") logPane.load();
  });
  // 按需加载失败要说出来（App 那张汇总名单的本地版，同 Panel）
  $effect(() => {
    const e = editor.error || logPane.error;
    if (e) notify.fail(e);
  });

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
    const st = git.status;
    const r = git.repo;
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
    void Promise.all([gitDiff(r, rel, false, false), import("../git/diff")])
      .then(([d, m]) => (editorMarks = m.changedLines(d.text)))
      .catch(() => (editorMarks = null));
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
    // worktree.treeTick 一变就重拉：切分支之后新增的文件也得跳得过去
    worktree.treeTick;
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
</script>

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
          <button class="primary" onclick={() => void tabflow.openFolder()}>打开文件夹…</button>
          <kbd>⌘O</kbd>
          <span class="gap"></span>
          {#if project.recent.length > 0}
            <span class="lastly">最近：</span>
            <button class="link" onclick={() => void tabflow.openRecent(project.recent[0])}>
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
  {:else if tabs.active.mode === "merge" && Merge}
    {#key tabs.active.id}
      <Merge
        text={tabs.active.mergeText ?? ""}
        path={tabs.active.rel ?? tabs.active.name}
        onResolve={(c, r) => void git.resolveMerge(tabs.active!, c, r)}
      />
    {/key}
  {:else if tabs.active.mode === "merge"}
    <div class="empty"><p>正在载入合并视图…</p></div>
  {:else if tabs.active.mode === "diff" && Diff}
    {#key tabs.active.id}
      <Diff
        raw={tabs.active.diffRaw ?? ""}
        capped={!!tabs.active.diffCapped}
        path={tabs.active.rel ?? tabs.active.name}
        staged={!!tabs.active.diffStaged}
        commit={tabs.active.diffShort ?? ""}
        untracked={!!tabs.active.diffUntracked}
        onToggleStaged={() => void git.toggleDiffSide(tabs.active!.id)}
      />
    {/key}
  {:else if tabs.active.mode === "diff"}
    <div class="empty"><p>正在载入差异视图…</p></div>
  {:else if tabs.active.mode === "log" && tabs.active.handle !== undefined && logPane.comp}
    {#key tabs.active.id}
      <logPane.comp
        handle={tabs.active.handle}
        gotoLine={nav.gotoLine}
        encoding={tabs.active.encoding ?? "utf-8"}
        onStatus={onLogStatus}
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
        gotoLine={nav.gotoLine}
        {outlineTick}
        marks={editorMarks}
        {showMinimap}
        onChange={(d) => (tabs.active!.dirty = d)}
        onSave={(c) => docs.save(c)}
        onStash={(p, t) => docs.stashDraft(p, t)}
        onLive={(p, g) => docs.onEditorLive(p, g)}
        onWordProbe={(p, g) => docs.onEditorWordProbe(p, g)}
        onOutline={onOutline}
        onCursor={(l) => docs.markPos(tabs.active!.path, l)}
        jumpFiles={projectFiles}
        jumpRel={project.root && tabs.active.path.startsWith(`${project.root}/`)
          ? tabs.active.path.slice(project.root.length + 1)
          : null}
        jumpLang={lang.mod?.langOf(tabs.active.path) ?? ""}
        onJump={(hit) => void nav.jumpTo(hit)}
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

<style>
  .content { flex: 1; min-height: 0; overflow: hidden; }

  .empty {
    height: 100%;
    display: grid;
    place-content: center;
    text-align: center;
    color: var(--text-dim);
  }
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
</style>
