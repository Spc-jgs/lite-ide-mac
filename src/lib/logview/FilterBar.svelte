<script lang="ts">
  import type { LevelCounts } from "../ipc/commands";

  // 位序必须与 Rust 侧 Level 一致：error/warn/info/debug/trace/other
  const CHIPS = [
    { bit: 0, key: "error", label: "ERROR" },
    { bit: 1, key: "warn", label: "WARN" },
    { bit: 2, key: "info", label: "INFO" },
    { bit: 3, key: "debug", label: "DEBUG" },
    { bit: 5, key: "other", label: "其他" },
  ] as const;

  let {
    counts,
    levelsReady,
    levelBits = $bindable(),
    pattern = $bindable(),
    caseSensitive = $bindable(),
    tailing = $bindable(),
    filterHits,
    filterRunning,
  }: {
    counts: LevelCounts;
    levelsReady: boolean;
    levelBits: number;
    pattern: string;
    caseSensitive: boolean;
    tailing: boolean;
    filterHits: number | null;
    filterRunning: boolean;
  } = $props();

  const ALL = 0b111111;

  function toggle(bit: number) {
    const mask = 1 << bit;
    // 只剩这一个还亮着就整个复原，避免点成空视图
    if (levelBits === mask) {
      levelBits = ALL;
    } else if (levelBits === ALL) {
      levelBits = mask; // 从全选点某一级 = 只看它，这是看日志时最常要的
    } else {
      levelBits ^= mask;
    }
  }

  const fmt = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n.toLocaleString("en-US"));
  const active = (bit: number) => (levelBits & (1 << bit)) !== 0;
</script>

<div class="bar">
  <div class="chips">
    {#each CHIPS as c (c.bit)}
      <button
        class="chip {c.key}"
        class:off={!active(c.bit)}
        onclick={() => toggle(c.bit)}
        title="点击只看这一级，再点复原"
      >
        <i class="dot"></i>
        <span class="lbl">{c.label}</span>
        <span class="num">{levelsReady ? fmt(counts[c.bit]) : "…"}</span>
      </button>
    {/each}
  </div>

  <div class="search">
    <input
      type="text"
      placeholder="过滤内容…"
      bind:value={pattern}
      spellcheck="false"
      autocomplete="off"
    />
    <button
      class="cs"
      class:on={caseSensitive}
      onclick={() => (caseSensitive = !caseSensitive)}
      title="区分大小写"
    >Aa</button>
    {#if pattern}
      <button class="clr" onclick={() => (pattern = "")} title="清除">✕</button>
    {/if}
  </div>

  {#if filterHits !== null}
    <span class="hits" class:running={filterRunning}>
      {fmt(filterHits)} 条{filterRunning ? " …" : ""}
    </span>
  {/if}

  <span class="gap"></span>

  <button class="tail" class:on={tailing} onclick={() => (tailing = !tailing)}>
    <i class="live"></i> 跟随尾部
  </button>
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 10px;
    height: 34px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11.5px;
    user-select: none;
    overflow: hidden;
  }
  .gap { flex: 1; }

  .chips { display: flex; gap: 4px; flex: none; }
  .chip {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 8px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-dim);
    font-family: var(--code-font);
    font-size: 11px;
    cursor: pointer;
  }
  .chip:hover { background: var(--panel-bg-2); }
  .chip .dot { width: 6px; height: 6px; border-radius: 50%; flex: none; }
  .chip.error .dot { background: var(--lvl-error); }
  .chip.warn .dot { background: var(--lvl-warn); }
  .chip.info .dot { background: var(--lvl-info); }
  .chip.debug .dot { background: var(--lvl-debug); }
  .chip.other .dot { background: var(--text-faint); }
  .chip .num { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .chip.off { opacity: 0.38; }
  .chip.off .dot { background: var(--text-faint) !important; }
  .chip:focus-visible { outline: 1px solid var(--accent); }

  .search { display: flex; align-items: center; gap: 2px; flex: none; }
  .search input {
    width: 190px;
    height: 22px;
    padding: 0 7px;
    background: var(--editor-bg);
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text);
    font-family: var(--code-font);
    font-size: 11.5px;
    outline: none;
  }
  .search input:focus { border-color: var(--accent); }
  .search input::placeholder { color: var(--text-faint); }
  .cs, .clr {
    height: 22px;
    min-width: 22px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: 3px;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 10.5px;
    cursor: pointer;
  }
  .cs:hover, .clr:hover { background: var(--panel-bg-2); color: var(--text); }
  .cs.on { background: var(--accent-sel); color: var(--text); }

  .hits {
    font-family: var(--code-font);
    color: var(--accent);
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .hits.running { color: var(--text-faint); }

  .tail {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 9px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 11px;
    cursor: pointer;
    flex: none;
  }
  .tail:hover { background: var(--panel-bg-2); }
  .tail .live {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--text-faint); flex: none;
  }
  .tail.on { border-color: var(--accent); color: var(--text); }
  .tail.on .live { background: var(--accent); animation: pulse 1.6s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .tail.on .live { animation: none; } }
</style>
