<script lang="ts">
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import LogView from "./lib/logview/LogView.svelte";
  import { openLog, logStat, closeLog, initialFile, diag, type LogStat } from "./lib/ipc/commands";

  let handle = $state<number | null>(null);
  let name = $state("");
  let size = $state(0);
  let stat = $state<LogStat | null>(null);
  let error = $state("");
  let hovering = $state(false);

  /** M0 出口标准要盯的两个数：打开耗时、索引跑完耗时 */
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
      stat = await logStat(r.handle);
      startPolling(r.handle, t0);
    } catch (e) {
      error = String(e);
      handle = null;
    }
  }

  function startPolling(h: number, t0: number) {
    if (poll) clearInterval(poll);
    indexMs = 0;
    poll = setInterval(async () => {
      const s = await logStat(h);
      stat = s;
      if (s.complete) {
        indexMs = performance.now() - t0;
        if (poll) clearInterval(poll);
        poll = null;
      }
    }, 100);
  }

  // 命令行带了文件就直接开
  $effect(() => {
    diag("initial-file effect 已触发");
    initialFile()
      .then((p) => {
        diag(`initialFile 返回: ${p}`);
        if (p && handle === null) open(p);
      })
      .catch((e) => diag(`initialFile 失败: ${e}`));
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
</script>

<main class:hovering>
  <header class="titlebar" data-tauri-drag-region>
    <span class="app">lite-ide</span>
    <span class="sep">—</span>
    <span class="file">{name || "M0 垂直切片"}</span>
  </header>

  {#if handle === null}
    <section class="empty">
      <div class="drop">
        <div class="big">把日志拖进来</div>
        <p>mmap + 稀疏索引 + 二进制 IPC + 虚拟滚动，一条线打通</p>
        <p class="goal">出口标准 · 1GB 日志：打开 &lt;1s · 滚动 60fps · 内存 &lt;200MB</p>
        {#if error}<p class="err">{error}</p>{/if}
      </div>
    </section>
  {:else}
    <section class="body">
      <LogView {handle} lineCount={stat?.lineCount ?? 0} />
    </section>
  {/if}

  <footer class="statusbar">
    {#if stat}
      <span class="cell">{fmtBytes(size)}</span>
      <span class="cell">{fmtNum(stat.lineCount)} 行</span>
      {#if !stat.complete}
        <span class="cell idx">
          索引中 {progress.toFixed(0)}%
          <i class="bar"><i style:width="{progress}%"></i></i>
        </span>
      {:else}
        <span class="cell ok">索引完成 {indexMs.toFixed(0)}ms</span>
      {/if}
      <span class="spacer"></span>
      <span class="cell dim">打开 {openMs.toFixed(1)}ms</span>
      <span class="cell dim">索引占用 {fmtBytes(stat.indexBytes)}</span>
    {:else}
      <span class="cell dim">等待文件</span>
      <span class="spacer"></span>
      <span class="cell dim">M0 · log-engine</span>
    {/if}
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
  .titlebar .sep { color: var(--text-faint); }
  .titlebar .file { color: var(--text-dim); }

  .body { overflow: hidden; }

  .empty {
    display: grid;
    place-items: center;
    color: var(--text-dim);
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
