<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import LogPane from "./lib/logview/LogPane.svelte";
  import FileTree from "./lib/shell/FileTree.svelte";
  import Tabs from "./lib/shell/Tabs.svelte";
  import QuickSearch, { type Action } from "./lib/search/QuickSearch.svelte";
  import { langOf, langLabel } from "./lib/editor/langs";
  import {
    probePath,
    readText,
    writeText,
    fileStamp,
    type Stamp,
    openLog,
    closeLog,
    initialPath,
  } from "./lib/ipc/commands";

  interface TabState {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log";
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

  let sidebar = $state(true);
  let panel = $state(false);
  let panelHeight = $state(260);
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

  // 打开面板时若一个终端都没有，自动起一个
  $effect(() => {
    if (panel && terms.length === 0 && root !== null) newTerm(root);
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
  /** 每次保存成功自增，Editor 据此重置 dirty 基线 */
  let savedTick = $state(0);

  let quickOpen = $state(false);
  let quickScope = $state<"all" | "file" | "content" | "action">("all");
  /** 待跳转的行号；带 nonce，连点同一条结果也能重新定位 */
  let gotoLine = $state<{ line: number; nonce: number } | null>(null);
  let gotoNonce = 0;

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
  ];

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
      } else {
        tab.content = await readText(info.path);
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
      tab.stamp = await writeText(tab.path, content);
      tab.dirty = false;
      tab.content = content;
      tab.conflict = false;
      savedTick++;
      saved = `已保存 ${tab.name}`;
      setTimeout(() => (saved = ""), 1800);
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
          tab.content = await readText(tab.path);
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
        tab.content = await readText(tab.path);
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
    const onFocus = () => void checkExternalChanges();
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
        // 非 UTF-8 会在这里明确失败，不会把文件读坏
        tab.content = await readText(tab.path);
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
    if (e.key === "p") {
      e.preventDefault();
      quickScope = "file";
      quickOpen = true;
      return;
    }
    if (e.key === "f" && e.shiftKey) {
      e.preventDefault();
      quickScope = "content";
      quickOpen = true;
      return;
    }
    if (e.key === "1") {
      e.preventDefault();
      sidebar = !sidebar;
    } else if (e.key === "j") {
      e.preventDefault();
      panel = !panel;
    } else if (e.key === "w" && active) {
      e.preventDefault();
      requestClose(active.id);
    }
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
    // 注销失败不该把整个 effect 清理链炸掉
    return () => void un.then((f) => f()).catch(() => {});
  });

</script>

<svelte:window onkeydown={onWindowKey} onkeyup={onWindowKeyUp} />

<QuickSearch bind:open={quickOpen} bind:scope={quickScope} {root} {actions} onOpenFile={openAt} />

<main class:hovering>
  <header class="titlebar" data-tauri-drag-region>
    <button class="side-toggle" class:on={sidebar} onclick={() => (sidebar = !sidebar)} title="侧边栏 ⌘1">☰</button>
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

  <div class="workspace" class:no-side={!sidebar}>
    {#if sidebar}
      <aside>
        {#if root}
          <FileTree {root} activePath={active?.path ?? ""} onOpen={(p) => void openPath(p)} />
        {:else}
          <div class="no-root">把文件夹拖进来</div>
        {/if}
      </aside>
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
            <p class="keys">双击 ⇧ 随处搜索 · ⌘P 找文件 · ⌘⇧F 搜内容</p>
            <p class="keys">⌘S 保存 · ⌘W 关闭标签 · ⌘1 侧边栏 · ⌘J 终端</p>
            {#if error}<p class="err">{error}</p>{/if}
          </div>
        {:else if active.mode === "log" && active.handle !== undefined}
          {#key active.id}
            <LogPane handle={active.handle} {gotoLine} onStatus={(s) => (logStatus = s)} />
          {/key}
        {:else if EditorComp}
          {#key active.id}
            <EditorComp
              path={active.path}
              initial={active.content ?? ""}
              {savedTick}
              {gotoLine}
              onChange={(d) => (active!.dirty = d)}
              onSave={save}
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
            <span class="tag">终端</span>
            <div class="tterms">
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
            {#if TerminalComp}
              {#each terms as t (t.id)}
                <!-- 隐藏而不是卸载：卸载会 kill 掉 shell -->
                <div class="term-slot" class:hidden={t.id !== activeTermId}>
                  <TerminalComp cwd={t.cwd} onExit={() => closeTerm(t.id)} />
                </div>
              {/each}
            {:else}
              <div class="loading">正在载入终端…</div>
            {/if}
          </div>
        </div>
      {/if}
    </section>
  </div>

  <footer class="statusbar">
    {#if active}
      <button
        class="cell btn mode"
        onclick={() => requestSwitchMode(active!)}
        title={active.mode === "log" ? "切换到编辑模式" : "切换到日志模式（只读，带级别过滤与 tail）"}
      >
        {active.mode === "log" ? "日志模式" : "编辑模式"} ⇄
      </button>
      {#if active.mode === "edit"}
        <span class="cell dim">{langLabel(langOf(active.path))}</span>
      {/if}
      {#if active.mode === "log"}
        <span class="cell">{logStatus}</span>
      {:else}
        <span class="cell">{active.dirty ? "已修改" : "无改动"}</span>
      {/if}
    {:else}
      <span class="cell dim">等待文件</span>
    {/if}
    {#if saved}<span class="cell ok">{saved}</span>{/if}
    {#if error}<span class="cell err">{error}</span>{/if}
    <span class="spacer"></span>
    <button class="cell btn" onclick={() => { quickScope = "all"; quickOpen = true; }}>搜索 ⇧⇧</button>
    <button class="cell btn" class:on={panel} onclick={() => (panel = !panel)}>
      终端 ⌘J{terms.length > 1 ? ` (${terms.length})` : ""}
    </button>
    <span class="cell dim">{tabs.length} 个标签</span>
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
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 12px;
    cursor: default;
    padding: 2px 5px;
    border-radius: 3px;
  }
  .side-toggle:hover { background: var(--panel-bg-2); color: var(--text); }
  .side-toggle.on { color: var(--text-dim); }
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
    grid-template-columns: 240px 1fr;
    overflow: hidden;
  }
  .workspace.no-side { grid-template-columns: 1fr; }
  aside { overflow: hidden; }
  .no-root {
    padding: 14px 12px;
    color: var(--text-faint);
    font-size: 12px;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    height: 100%;
  }

  .main { display: grid; grid-template-rows: auto auto 1fr auto auto; overflow: hidden; }
  .content { overflow: hidden; grid-row: 3; }

  .resizer {
    height: 4px;
    background: var(--border);
    cursor: row-resize;
  }
  .resizer:hover { background: var(--accent); }
  .panel {
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
  .panel-head .tag { letter-spacing: 0.06em; text-transform: uppercase; }
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

  .statusbar {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 0 12px;
    background: var(--panel-bg);
    border-top: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--text-dim);
    font-family: var(--code-font);
    user-select: none;
  }
  .statusbar .spacer { flex: 1; }
  .statusbar .dim { color: var(--text-faint); }
  .statusbar .ok { color: var(--accent); }
  .statusbar .err { color: var(--lvl-error); }
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
</style>
