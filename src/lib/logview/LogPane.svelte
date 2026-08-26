<script lang="ts">
  import LogView from "./LogView.svelte";
  import FilterBar from "./FilterBar.svelte";
  import { decodeBlock } from "./block";
  import { detectFormat, FORMAT_LABEL, type LogFormat } from "./parse";
  import {
    logStat,
    logLines,
    logFilter,
    logFilterStat,
    logRefresh,
    type LogStat,
    type LevelCounts,
  } from "../ipc/commands";

  let {
    handle,
    gotoLine = null,
    onStatus,
  }: {
    handle: number;
    gotoLine?: { line: number; nonce: number } | null;
    onStatus: (s: string) => void;
  } = $props();

  const ALL_LEVELS = 0b111111;

  let stat = $state<LogStat | null>(null);
  let levelBits = $state(ALL_LEVELS);
  let pattern = $state("");
  let caseSensitive = $state(false);
  let filtered = $state(false);
  let filterHits = $state<number | null>(null);
  let filterRunning = $state(false);
  let tailing = $state(false);
  let collapseStacks = $state(false);
  let error = $state("");
  let format = $state<LogFormat>("plain");

  // 取开头几十行投票选格式。只看第一行容易被启动横幅、空行带偏
  $effect(() => {
    const h = handle;
    logLines(h, 0, 60)
      .then((buf) => {
        format = detectFormat(decodeBlock(buf).lines);
      })
      .catch(() => (format = "plain"));
  });

  // 索引与级别扫描都在后台跑，轮询到两者都完成为止
  $effect(() => {
    const h = handle;
    let stop = false;
    logStat(h).then((s) => {
      if (!stop) stat = s;
    });
    const id = setInterval(async () => {
      const s = await logStat(h);
      if (stop) return;
      stat = s;
      if (s.complete && s.levelsComplete) clearInterval(id);
    }, 100);
    return () => {
      stop = true;
      clearInterval(id);
    };
  });

  // 过滤条件变化 → 重跑。输入时 debounce，免得每敲一个字母就扫一遍 1GB
  $effect(() => {
    const bits = levelBits;
    const pat = pattern;
    const cs = caseSensitive;
    const fold = collapseStacks;
    const h = handle;
    let tick: ReturnType<typeof setInterval> | null = null;

    const timer = setTimeout(async () => {
      try {
        const active = await logFilter(h, bits, pat, cs, fold);
        filtered = active;
        if (!active) {
          filterHits = null;
          filterRunning = false;
          return;
        }
        filterRunning = true;
        tick = setInterval(async () => {
          const fs = await logFilterStat(h);
          if (!fs) {
            if (tick) clearInterval(tick);
            return;
          }
          filterHits = fs.hits;
          if (fs.complete) {
            filterRunning = false;
            if (tick) clearInterval(tick);
          }
        }, 80);
      } catch (e) {
        error = String(e);
      }
    }, 180);

    return () => {
      clearTimeout(timer);
      if (tick) clearInterval(tick);
    };
  });

  // tail：轮询文件是否追加。mmap 长度固定，Rust 侧会重新映射
  $effect(() => {
    const h = handle;
    if (!tailing) return;
    const id = setInterval(async () => {
      try {
        const r = await logRefresh(h);
        if (r.kind === "grew") stat = await logStat(h);
        else if (r.kind === "rotated") {
          tailing = false;
          error = "文件已被轮转或截断，请重新打开";
        }
      } catch {
        /* 文件临时不可读，下一轮再试 */
      }
    }, 500);
    return () => clearInterval(id);
  });

  const fmtNum = (n: number) => n.toLocaleString("en-US");

  /**
   * 拿到首批命中计数之前继续显示旧视图 —— 否则行数为 0，
   * 界面会闪一下空白再填上内容。
   */
  let showFiltered = $derived(filtered && filterHits !== null);
  let viewLines = $derived(showFiltered ? (filterHits ?? 0) : (stat?.lineCount ?? 0));
  let counts = $derived((stat?.levels ?? [0, 0, 0, 0, 0, 0]) as LevelCounts);

  // 把状态汇报给外壳的状态栏
  $effect(() => {
    if (!stat) {
      onStatus("");
      return;
    }
    const parts = [`${fmtNum(stat.lineCount)} 行`, FORMAT_LABEL[format]];
    if (showFiltered) parts.push(`筛出 ${fmtNum(viewLines)}`);
    if (collapseStacks) parts.push("堆栈已折叠");
    if (!stat.complete) parts.push("索引中…");
    else if (!stat.levelsComplete) parts.push("级别扫描中…");
    if (error) parts.push(error);
    onStatus(parts.join("  ·  "));
  });
</script>

<div class="pane">
  <FilterBar
    {counts}
    levelsReady={stat?.levelsComplete ?? false}
    bind:levelBits
    bind:pattern
    bind:caseSensitive
    bind:tailing
    bind:collapseStacks
    {filterHits}
    {filterRunning}
  />
  <div class="body">
    <LogView
      {handle}
      lineCount={viewLines}
      filtered={showFiltered}
      {pattern}
      {caseSensitive}
      stickBottom={tailing}
      {gotoLine}
      {format}
    />
  </div>
</div>

<style>
  .pane { display: grid; grid-template-rows: auto 1fr; height: 100%; overflow: hidden; }
  .body { overflow: hidden; }
</style>
