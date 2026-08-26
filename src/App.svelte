<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import LogView from "./lib/logview/LogView.svelte";
  import FilterBar from "./lib/logview/FilterBar.svelte";
  import {
    openLog,
    logStat,
    closeLog,
    initialFile,
    logFilter,
    logFilterStat,
    logRefresh,
    type LogStat,
    type LevelCounts,
  } from "./lib/ipc/commands";

  const ALL_LEVELS = 0b111111;

  let handle = $state<number | null>(null);
  let name = $state("");
  let size = $state(0);
  let stat = $state<LogStat | null>(null);
  let error = $state("");
  let hovering = $state(false);

  // 过滤条件
  let levelBits = $state(ALL_LEVELS);
  let pattern = $state("");
  let caseSensitive = $state(false);
  let filtered = $state(false);
  let filterHits = $state<number | null>(null);
  let filterRunning = $state(false);

  // tail
  let tailing = $state(false);

  /** M0 起就盯着的两个数：打开耗时、索引跑完耗时 */
  let openMs = $state(0);
  let indexMs = $state(0);

  let poll: ReturnType<typeof setInterval> | null = null;

  async function open(path: string) {
    error = "";
    const t0 = performance.now();
    try {
      if (handle !== null) await closeLog(handle);
      const r = await openLog(path);
      openMs = performance.now() - t0;
      handle = r.handle;
      name = r.name;
      size = r.size;
      // 换文件时把过滤条件复位，否则新文件会莫名其妙是空的
      levelBits = ALL_LEVELS;
      pattern = "";
      filtered = false;
      filterHits = null;
      tailing = false;
      stat = await logStat(r.handle);
      startPolling(r.handle, t0);
    } catch (e) {
      error = String(e);
      handle = null;
    }
  }

  /** 索引与级别扫描都在后台跑，轮询到两者都完成为止 */
  function startPolling(h: number, t0: number) {
    if (poll) clearInterval(poll);
    indexMs = 0;
    poll = setInterval(async () => {
      const s = await logStat(h);
      stat = s;
      if (s.complete && indexMs === 0) indexMs = performance.now() - t0;
      if (s.complete && s.levelsComplete) {
        if (poll) clearInterval(poll);
        poll = null;
      }
    }, 100);
  }

  // 过滤条件变化 → 重跑过滤。输入关键字时 debounce，免得每敲一个字母就扫一遍 1GB
  $effect(() => {
    const bits = levelBits;
    const pat = pattern;
    const cs = caseSensitive;
    const h = handle;
    if (h === null) return;

    const timer = setTimeout(async () => {
      try {
        const active = await logFilter(h, bits, pat, cs);
        filtered = active;
        if (!active) {
          filterHits = null;
          filterRunning = false;
          return;
        }
        filterRunning = true;
        // 过滤在后台跑，轮询它的进度
        const tick = setInterval(async () => {
          const fs = await logFilterStat(h);
          if (!fs) {
            clearInterval(tick);
            return;
          }
          filterHits = fs.hits;
          if (fs.complete) {
            filterRunning = false;
            clearInterval(tick);
          }
        }, 80);
      } catch (e) {
        error = String(e);
      }
    }, 180);

    return () => clearTimeout(timer);
  });

  // tail：轮询文件是否追加。mmap 长度固定，Rust 侧会重新映射
  $effect(() => {
    const h = handle;
    if (!tailing || h === null) return;
    const id = setInterval(async () => {
      try {
        const r = await logRefresh(h);
        if (r.kind === "grew") {
          stat = await logStat(h);
        } else if (r.kind === "rotated") {
          tailing = false;
          error = "文件已被轮转或截断，请重新打开";
        }
      } catch {
        /* 文件临时不可读，下一轮再试 */
      }
    }, 500);
    return () => clearInterval(id);
  });

  $effect(() => {
    initialFile().then((p) => {
      if (p && handle === null) open(p);
    });
  });

  $effect(() => {
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "over") {
        hovering = true;
      } else if (e.payload.type === "drop") {
        hovering = false;
        const p = e.payload.paths[0];
        if (p) open(p);
      } else {
        hovering = false;
      }
    });
    return () => {
      un.then((f) => f());
      if (poll) clearInterval(poll);
    };
  });

  const fmtBytes = (n: number) => {
    if (n < 1024) return `${n} B`;
    if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
    if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
    return `${(n / 1024 ** 3).toFixed(2)} GB`;
  };
  const fmtNum = (n: number) => n.toLocaleString("en-US");

  let progress = $derived(
    stat && stat.totalBytes > 0 ? Math.min(100, (stat.indexedBytes / stat.totalBytes) * 100) : 0,
  );
  /**
   * 过滤态下视图的行数是命中数，不是总行数。
   *
   * 关键在 `filterHits !== null` 这个条件：过滤刚启动时命中数还没回来，
   * 若此时就切到过滤视图，行数是 0，界面会闪一下空白再填上内容。
   * 拿到第一批计数之前继续显示旧视图 —— 与浏览器搜索同样的手感。
   */
  let showFiltered = $derived(filtered && filterHits !== null);
  let viewLines = $derived(showFiltered ? (filterHits ?? 0) : (stat?.lineCount ?? 0));
  let counts = $derived((stat?.levels ?? [0, 0, 0, 0, 0, 0]) as LevelCounts);
</script>

<main class:hovering>
  <header class="titlebar" data-tauri-drag-region>
    <span class="app">lite-ide</span>
    <span class="sep">—</span>
    <span class="file">{name || "M1 日志模式"}</span>
  </header>

  {#if handle === null}
    <section class="empty">
      <div class="drop">
        <div class="big">把日志拖进来</div>
        <p>级别着色 · chips 过滤 · 文本搜索 · 跟随尾部</p>
        <p class="goal">GB 级日志秒开，内存与文件大小无关</p>
        {#if error}<p class="err">{error}</p>{/if}
      </div>
    </section>
  {:else}
    <FilterBar
      {counts}
      levelsReady={stat?.levelsComplete ?? false}
      bind:levelBits
      bind:pattern
      bind:caseSensitive
      bind:tailing
      {filterHits}
      {filterRunning}
    />
    <section class="body">
      <LogView
        {handle}
        lineCount={viewLines}
        filtered={showFiltered}
        {pattern}
        {caseSensitive}
        stickBottom={tailing}
      />
    </section>
  {/if}

  <footer class="statusbar">
    {#if stat}
      <span class="cell">{fmtBytes(size)}</span>
      <span class="cell">{fmtNum(stat.lineCount)} 行</span>
      {#if showFiltered}
        <span class="cell ok">筛出 {fmtNum(viewLines)}</span>
      {/if}
      {#if !stat.complete}
        <span class="cell idx">
          索引中 {progress.toFixed(0)}%
          <i class="bar"><i style:width="{progress}%"></i></i>
        </span>
      {:else if !stat.levelsComplete}
        <span class="cell idx">级别扫描中…</span>
      {:else}
        <span class="cell ok">就绪 {indexMs.toFixed(0)}ms</span>
      {/if}
      {#if error}<span class="cell err">{error}</span>{/if}
      <span class="spacer"></span>
      <span class="cell dim">打开 {openMs.toFixed(1)}ms</span>
      <span class="cell dim">索引 {fmtBytes(stat.indexBytes)}</span>
    {:else}
      <span class="cell dim">等待文件</span>
      <span class="spacer"></span>
      <span class="cell dim">M1 · 日志模式</span>
    {/if}
  </footer>
</main>

<style>
  main {
    height: 100%;
    display: grid;
    grid-template-rows: 38px auto 1fr 24px;
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
  .titlebar .sep { color: var(--text-faint); }
  .titlebar .file { color: var(--text-dim); }

  .body { overflow: hidden; grid-row: 3; }

  .empty {
    display: grid;
    place-items: center;
    color: var(--text-dim);
    grid-row: 2 / 4;
  }
  .drop { text-align: center; padding: 32px 40px; }
  .drop .big { font-size: 17px; color: var(--text); margin-bottom: 10px; }
  .drop p { margin: 4px 0; font-size: 12.5px; color: var(--text-faint); }
  .drop .goal { margin-top: 14px; font-family: var(--code-font); font-size: 11.5px; }
  .drop .err { margin-top: 14px; color: var(--lvl-error); font-family: var(--code-font); }

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
  .statusbar .idx { display: flex; align-items: center; gap: 7px; }
  .bar {
    display: block;
    width: 70px;
    height: 3px;
    background: var(--panel-bg-2);
    border-radius: 2px;
    overflow: hidden;
  }
  .bar > i { display: block; height: 100%; background: var(--accent); }
</style>
