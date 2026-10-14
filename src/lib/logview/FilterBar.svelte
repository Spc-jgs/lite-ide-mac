<script lang="ts">
  import { parseQuery } from "./query";
  import { imeKey } from "../shell/ime";
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
    canTail = true,
    onMerge,
    onPatterns,
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
    /** 合并视图不跟 tail（docs/MERGE.md），不给这个按钮 */
    canTail?: boolean;
    /** 有它就给「合并…」：选几份别的日志，和这一份按时间合在一起看 */
    onMerge?: () => void;
    /** 「归类」：把行归成几类模板（docs/PATTERNS.md）。给的是按钮左下角，浮层挂在它底下 */
    onPatterns?: (at: { x: number; y: number }) => void;
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
    <!-- 语法在 query.ts：空格 AND、-词 排除、"x y" 整串、/re/ 正则。写坏的正则让字变红，不弹东西 -->
    <input
      type="text"
      class:bad={parseQuery(pattern).bad}
      placeholder="过滤：a b · -c · /re/"
      title={"空格分开的都得有；-词 排除；\"x y\" 整串；/正则/。大小写跟右边的 Aa"}
      bind:value={pattern}
      spellcheck="false"
      autocomplete="off"
      onkeydown={(e) => {
        // 敲完关键字直接回车就跳到第一处，不用再摸鼠标
        if (e.key !== "Enter" || imeKey(e)) return;
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
      <button class="ibtn sm" onclick={() => (pattern = "")} title="清除" aria-label="清除过滤"><Icon name="x" /></button>
    {/if}
  </div>

  {#if filterHits !== null}
    <div class="segs">
      <button
        class="btn sm"
        class:on={onlyHits}
        onclick={() => (onlyHits = true)}
        title="只显示命中的行"
      >只看命中</button>
      <button
        class="btn sm"
        class:on={!onlyHits}
        onclick={() => (onlyHits = false)}
        title="显示全文，在命中之间跳转 —— 看得到上下文"
      >全文</button>
    </div>
    <!-- 扫描还在跑时计数会一直长；进度本身在状态栏的进度格里（`progress.svelte.ts`），这儿不再挂环 -->
    <span class="hits" class:running={filterRunning}>
      {#if hitIndex > 0}<b>{fmt(hitIndex)}</b>/{/if}{fmt(filterHits)} 条
    </span>
    <div class="jump">
      <button class="ibtn sm" onclick={() => onJump?.(-1)} disabled={!filterHits} title="上一处 ⇧↵ / ⇧F3" aria-label="上一处">
        <Icon name="chevron-up" />
      </button>
      <button class="ibtn sm" onclick={() => onJump?.(1)} disabled={!filterHits} title="下一处 ↵ / F3" aria-label="下一处">
        <Icon name="chevron-down" />
      </button>
    </div>
  {/if}

  <span class="gap"></span>

  {#if onPatterns}
    <button
      class="fold"
      onclick={(e) => {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        onPatterns({ x: r.left, y: r.bottom });
      }}
      title="把这份日志的记录归成几类模板：都发生了些什么、各多少。点一类就按它过滤"
    >归类</button>
  {/if}

  <button
    class="fold"
    class:on={collapseStacks}
    onclick={() => (collapseStacks = !collapseStacks)}
    title="折叠异常堆栈：连续的 at 帧只留第一帧，异常类型与 Caused by always 保留"
  >
    折叠堆栈
  </button>

  {#if onMerge}
    <button class="fold merge" onclick={onMerge} title="再选几份日志，和这一份按时间交织成一条时间线（网关、服务、数据库的日志对着看）">合并…</button>
  {/if}

  {#if canTail}
    <button class="tail" class:on={tailing} onclick={() => (tailing = !tailing)} title="跟随尾部：文件长了就贴到最新一行" aria-label="跟随尾部">
      <i class="live"></i><span class="txt">跟随尾部</span>
    </button>
  {/if}
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
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    border-bottom: 1px solid var(--border-soft); /* M8：岛内分区线 5% */
    font-size: var(--fs-sm);
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
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-sm);
    cursor: default;
  }
  .chip:hover { background: var(--hover); }
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

  /* 基准宽 120：换行容器按基准宽判「这一行放不放得下」，不按最窄；反正它会长满剩下的地方（2026-10-14 从 150 调下来，加「归类」那天差 10px 换行） */
  .search { display: flex; align-items: center; gap: 2px; flex: 1 1 120px; min-width: 110px; }
  .search input {
    width: 100%;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: var(--fs-sm);
    outline: none;
  }
  .search input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px rgba(10, 132, 255, 0.25); }
  /* 正则写坏了：和编辑器查找面板同一个说法 —— 字变红，不弹任何东西；边打边写的正则大半时间是坏的 */
  .search input.bad { color: var(--lvl-error); }
  .search input::placeholder { color: var(--text-faint); }
  .cs {
    height: 22px;
    min-width: 22px;
    background: transparent;
    border: 1px solid transparent;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: var(--fs-xs);
    cursor: default;
  }
  .cs:hover { background: var(--hover); color: var(--text); }
  .cs.on { background: var(--accent-sel); color: var(--text); }

  .hits {
    font-family: var(--ui-font); font-variant-numeric: tabular-nums;
    color: var(--accent);
    font-variant-numeric: tabular-nums;
    flex: none;
  }
  .hits.running { color: var(--text-faint); }
  .hits b { color: var(--text); font-weight: 600; }

  /* 「只看命中 / 全文」是二选一，做成连在一起的分段控件而不是两个独立按钮 */
  /* 「只看命中 / 全文」是 app.css 的 `.segs` */

  .jump { display: flex; gap: 1px; flex: none; }

  /* 极窄时先牺牲级别计数 —— 它是参考信息，而按钮是操作入口 */
  @container (max-width: 640px) {
    .chip .num { display: none; }
  }
  /*
   * 「合并…」是这条栏上最不常按的：窄了先让它走，别把「跟随尾部」挤到第二行（2026-10-14 在 800 宽的窗口里撞上的）。
   * 合并还有文件树多选那条入口，不会因此做不了
   */
  @container (max-width: 760px) {
    .merge { display: none; }
  }
  /*
   * 再窄一档，「跟随尾部」只留那个圆点（开着时它在闪，悬停有全名）。加「归类」那天量的：日志面板 736 宽时，
   * 级别胶囊 388 + 搜索框最窄 110 + 三个按钮 + 间距 = 760，正好挤掉一行。级别计数比这几个字值钱，先让它们
   */
  @container (max-width: 800px) {
    .tail .txt { display: none; }
    .tail { padding: 0 9px; }
  }

  /*
   * 开关按钮（折叠堆栈、跟随尾部）：关着是和 `.btn` 一样的实心浅灰；开着是淡强调色底 —— 2026-10-12 之前是描边 + 开着描一圈蓝，
   * 按钮改成实心之后同一排就成了两种按钮
   */
  .fold {
    height: 24px;
    padding: 0 10px;
    background: rgba(255, 255, 255, 0.08);
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    cursor: default;
    flex: none;
  }
  .fold:hover { background: rgba(255, 255, 255, 0.12); color: var(--text); }
  .fold.on { color: var(--text); background: rgba(10, 132, 255, 0.28); }

  .tail {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    background: rgba(255, 255, 255, 0.08);
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    cursor: default;
    flex: none;
  }
  .tail:hover { background: rgba(255, 255, 255, 0.12); color: var(--text); }
  .tail .live {
    width: 6px; height: 6px; border-radius: 50%;
    background: var(--text-faint); flex: none;
  }
  .tail.on { color: var(--text); background: rgba(10, 132, 255, 0.28); }
  .tail.on .live { background: var(--accent); animation: pulse 1.6s ease-in-out infinite; }
  @keyframes pulse { 50% { opacity: 0.25; } }
  @media (prefers-reduced-motion: reduce) { .tail.on .live { animation: none; } }
</style>
