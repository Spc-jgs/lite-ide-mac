<script lang="ts">
  import Icon from "../shell/Icon.svelte";
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
    collapseStacks = $bindable(),
    filterHits,
    filterRunning,
    hitIndex = 0,
    onlyHits = $bindable(true),
    onJump,
  }: {
    counts: LevelCounts;
    levelsReady: boolean;
    levelBits: number;
    pattern: string;
    caseSensitive: boolean;
    tailing: boolean;
    collapseStacks: boolean;
    filterHits: number | null;
    filterRunning: boolean;
    /** 当前停在第几条命中，1-based；0 表示还没跳过 */
    hitIndex?: number;
    /**
     * 只看命中（过滤视图）还是看全文（命中之间跳）。
     *
     * 这两件事在 GB 级日志里是**不同的需求**：
     * 「这个订单号出现过几次」要过滤，「这条报错前后发生了什么」要上下文。
     */
    onlyHits?: boolean;
    onJump?: (dir: 1 | -1) => void;
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
      onkeydown={(e) => {
        // 敲完关键字直接回车就跳到第一处，不用再摸鼠标
        if (e.key !== "Enter") return;
        e.preventDefault();
        onJump?.(e.shiftKey ? -1 : 1);
      }}
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
    <div class="nav">
      <button
        class="seg"
        class:on={onlyHits}
        onclick={() => (onlyHits = true)}
        title="只显示命中的行"
      >只看命中</button>
      <button
        class="seg"
        class:on={!onlyHits}
        onclick={() => (onlyHits = false)}
        title="显示全文，在命中之间跳转 —— 看得到上下文"
      >全文</button>
    </div>
    <span class="hits" class:running={filterRunning}>
      {#if hitIndex > 0}<b>{fmt(hitIndex)}</b>/{/if}{fmt(filterHits)} 条{filterRunning ? " …" : ""}
    </span>
    <div class="jump">
      <button onclick={() => onJump?.(-1)} disabled={!filterHits} title="上一处 ⇧↵ / ⇧F3" aria-label="上一处">
        <Icon name="chevron-up" size={11} />
      </button>
      <button onclick={() => onJump?.(1)} disabled={!filterHits} title="下一处 ↵ / F3" aria-label="下一处">
        <Icon name="chevron-down" size={11} />
      </button>
    </div>
  {/if}

  <span class="gap"></span>

  <button
    class="fold"
    class:on={collapseStacks}
    onclick={() => (collapseStacks = !collapseStacks)}
    title="折叠异常堆栈：连续的 at 帧只留第一帧，异常类型与 Caused by always 保留"
  >
    折叠堆栈
  </button>

  <button class="tail" class:on={tailing} onclick={() => (tailing = !tailing)}>
    <i class="live"></i> 跟随尾部
  </button>
</div>

<style>
  /*
   * 窄窗口下必须优雅退化。这条栏上有六个级别 chip、搜索框、两个分段按钮、
   * 计数、两个跳转按钮 —— 加起来约 1180px，而侧边栏和终端都开着时
   * 内容区可能只有 600px。原本是 overflow: hidden，溢出的部分**直接看不见** ——
   * 用户不会知道有个「下一处」按钮被挤到了屏幕外。
   *
   * 退化顺序按「丢了最不心疼」排：先让级别计数消失，再让搜索框收窄，
   * 最后整条栏换行 —— 会变高，但每个控件都还在。
   */
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    padding: 5px 10px;
    min-height: 34px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11.5px;
    user-select: none;
  }
  .gap { flex: 1; }

  .chips { display: flex; gap: 4px; flex: 0 1 auto; min-width: 0; flex-wrap: wrap; }
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

  .search { display: flex; align-items: center; gap: 2px; flex: 1 1 150px; min-width: 110px; }
  .search input {
    width: 100%;
    min-width: 0;
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
  .hits b { color: var(--text); font-weight: 600; }

  /* 「只看命中 / 全文」是二选一，做成连在一起的分段控件而不是两个独立按钮 */
  .nav { display: flex; flex: none; }
  .seg {
    padding: 3px 9px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11.5px;
    cursor: default;
    white-space: nowrap;
  }
  .seg:first-child { border-radius: 3px 0 0 3px; }
  .seg:last-child { border-radius: 0 3px 3px 0; border-left: none; }
  .seg:hover { background: var(--panel-bg-2); color: var(--text); }
  .seg.on { background: var(--accent-sel); color: var(--text); border-color: var(--accent); }
  .seg.on + .seg { border-left-color: var(--accent); }

  .jump { display: flex; gap: 1px; flex: none; }
  .jump button {
    display: grid;
    place-content: center;
    width: 22px;
    height: 21px;
    background: transparent;
    border: 1px solid var(--border);
    color: var(--text-dim);
    cursor: default;
  }
  .jump button:first-child { border-radius: 3px 0 0 3px; }
  .jump button:last-child { border-radius: 0 3px 3px 0; border-left: none; }
  .jump button:hover:not(:disabled) { background: var(--panel-bg-2); color: var(--text); }
  .jump button:disabled { opacity: 0.35; }

  /* 极窄时先牺牲级别计数 —— 它是参考信息，而按钮是操作入口 */
  @container (max-width: 640px) {
    .chip .num { display: none; }
  }

  .fold {
    padding: 3px 9px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 11px;
    cursor: default;
    flex: none;
  }
  .fold:hover { background: var(--panel-bg-2); }
  .fold.on { border-color: var(--accent); color: var(--text); background: var(--accent-sel); }

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
