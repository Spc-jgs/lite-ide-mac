<script lang="ts">
  import { ScrollMap } from "./scroll-map";
  import { LineCache, type Row } from "./line-cache";
  import { parse, highlight } from "./parse";

  let {
    handle,
    lineCount,
    filtered = false,
    pattern = "",
    caseSensitive = false,
    stickBottom = false,
    gotoLine = null,
  }: {
    handle: number;
    lineCount: number;
    filtered?: boolean;
    pattern?: string;
    caseSensitive?: boolean;
    stickBottom?: boolean;
    /** 搜索结果跳转的目标行（1-based）。带 nonce，连点同一条也能重新定位 */
    gotoLine?: { line: number; nonce: number } | null;
  } = $props();

  const LINE_HEIGHT = 20;
  /** 视口外多渲染几行，滚动时不露白 */
  const OVERSCAN = 8;

  const map = new ScrollMap(LINE_HEIGHT);
  // 缓存是 (handle, 是否过滤) 的派生物：任一变化都要重建
  let cache = $derived(new LineCache(handle, filtered));

  let viewport: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewportHeight = $state(600);
  /** 块加载完成后自增，触发重新渲染 */
  let revision = $state(0);

  // 换文件或切换过滤态：回到顶部
  $effect(() => {
    handle;
    filtered;
    scrollTop = 0;
    if (viewport) viewport.scrollTop = 0;
  });

  $effect(() => {
    map.lineCount = lineCount;
    cache.invalidateTail(lineCount);
  });

  // tail 吸底：行数一变就贴到最新
  $effect(() => {
    if (!stickBottom || !viewport) return;
    lineCount;
    viewport.scrollTop = map.scrollHeight;
  });

  // 跳到指定行：压缩映射下也能算出正确的 scrollTop
  $effect(() => {
    const g = gotoLine;
    if (!g || !viewport || lineCount === 0) return;
    const target = Math.min(Math.max(0, g.line - 1), Math.max(0, lineCount - 1));
    // 往上留几行上下文，别把目标贴在视口最顶上
    const withContext = Math.max(0, target - 3);
    viewport.scrollTop = map.scrollTopFor(withContext, viewportHeight);
  });

  let topLine = $derived(map.topLineAt(scrollTop, viewportHeight));
  let rowCount = $derived(
    Math.min(Math.ceil(viewportHeight / LINE_HEIGHT) + OVERSCAN, Math.max(0, lineCount - topLine)),
  );

  /**
   * 压缩映射下行的像素位置不能按 line × 行高算（会超出容器），
   * 改以 scrollTop 为基准让可见行始终贴住视口。
   */
  let layerTop = $derived(map.compressed ? scrollTop : topLine * LINE_HEIGHT);

  let rows = $derived.by(() => {
    revision; // 依赖它，块加载完成后重新求值
    const out: { n: number; row: Row | undefined }[] = new Array(rowCount);
    for (let i = 0; i < rowCount; i++) {
      const n = topLine + i;
      out[i] = { n, row: cache.get(n) };
    }
    return out;
  });

  $effect(() => {
    if (rowCount > 0) {
      cache.ensure(topLine, topLine + rowCount - 1, () => revision++);
    }
  });

  function onScroll() {
    if (viewport) scrollTop = viewport.scrollTop;
  }

  function onKeydown(e: KeyboardEvent) {
    if (!viewport) return;
    const page = viewportHeight - LINE_HEIGHT;
    const step: Record<string, number> = {
      ArrowDown: LINE_HEIGHT,
      ArrowUp: -LINE_HEIGHT,
      PageDown: page,
      PageUp: -page,
    };
    if (e.key in step) {
      e.preventDefault();
      viewport.scrollTop += step[e.key];
    } else if (e.key === "Home") {
      e.preventDefault();
      viewport.scrollTop = 0;
    } else if (e.key === "End") {
      e.preventDefault();
      viewport.scrollTop = map.scrollHeight;
    }
  }

  const gutterWidth = $derived(`${Math.max(5, String(lineCount).length)}ch`);

  // 视口那两条 a11y_ 抑制说明：日志区是 role="log" 的只读区域，同时必须可聚焦，
  // 否则方向键 / PageUp / PageDown 都用不了 —— 而键盘滚动正是 less 式浏览的基本
  // 交互。Svelte 的两条规则在这里互相矛盾（有 role 则禁 tabindex，无 role 又要求
  // 有 role），可聚焦滚动容器是公认的合法模式，故按元素抑制。
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="viewport"
  bind:this={viewport}
  bind:clientHeight={viewportHeight}
  onscroll={onScroll}
  onkeydown={onKeydown}
  tabindex="0"
  role="log"
  aria-label="日志内容"
>
  <div class="spacer" style:height="{map.scrollHeight}px">
    <div class="layer" style:transform="translateY({layerTop}px)">
      {#each rows as { n, row } (n)}
        {@const seg = row ? parse(row.text) : null}
        <div class="row" class:pending={!row} class:stack={seg?.stack}>
          <span class="gutter" style:width={gutterWidth}>{row ? row.phys + 1 : ""}</span>
          {#if seg}
            {#if seg.ts}<span class="ts">{seg.ts}</span>{/if}
            {#if seg.level}<span class="lvl" data-lvl={seg.lvl}>{seg.level}</span>{/if}
            {#if seg.thread}<span class="thread">{seg.thread}</span>{/if}
            {#if seg.logger}<span class="logger">{seg.logger}</span><span class="dash">-</span>{/if}
            <span class="msg" data-lvl={seg.stack ? null : seg.lvl}>
              {#each highlight(seg.msg, pattern, caseSensitive) as part, i}
                {#if i % 2 === 1}<mark>{part}</mark>{:else}{part}{/if}
              {/each}
            </span>
          {:else}
            <span class="msg"></span>
          {/if}
        </div>
      {/each}
    </div>
  </div>
</div>

<style>
  .viewport {
    height: 100%;
    overflow-y: auto;
    overflow-x: hidden;
    outline: none;
    background: var(--editor-bg);
    /* 交给合成器，滚动时不触发布局 */
    will-change: scroll-position;
  }
  .spacer { position: relative; }
  .layer {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    will-change: transform;
  }
  .row {
    display: flex;
    gap: 8px;
    height: var(--line-height);
    line-height: var(--line-height);
    font-family: var(--code-font);
    font-size: 12px;
    white-space: pre;
    padding-right: 12px;
  }
  .row:hover { background: rgba(255, 255, 255, 0.035); }

  .gutter {
    flex: none;
    text-align: right;
    padding-right: 6px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    user-select: none;
    border-right: 1px solid var(--border-soft);
    margin-right: 4px;
  }
  .ts { flex: none; color: var(--text-faint); }
  .lvl { flex: none; font-weight: 600; width: 5ch; }
  .lvl[data-lvl="error"] { color: var(--lvl-error); }
  .lvl[data-lvl="warn"] { color: var(--lvl-warn); }
  .lvl[data-lvl="info"] { color: var(--lvl-info); }
  .lvl[data-lvl="debug"] { color: var(--lvl-debug); }
  .lvl[data-lvl="trace"] { color: var(--lvl-debug); }
  .thread { flex: none; color: var(--text-faint); }
  .logger { flex: none; color: var(--text-dim); }
  .dash { flex: none; color: var(--text-faint); }
  .msg {
    flex: 1;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
  }
  /* ERROR 整行都该扎眼，其余只染级别列 */
  .msg[data-lvl="error"] { color: var(--lvl-error); }
  .msg[data-lvl="debug"] { color: var(--text-dim); }

  /* 堆栈续行：缩进 + 压暗，一眼能看出是上一条的附属 */
  .row.stack .msg { color: var(--text-dim); padding-left: 2ch; }
  .row.stack { background: rgba(255, 255, 255, 0.02); }

  mark {
    background: var(--search-hit);
    color: var(--text);
    border-radius: 1px;
  }

  /* 块还在路上：留白而不是跳动 */
  .pending .msg::after {
    content: "";
    display: inline-block;
    width: 34ch;
    height: 9px;
    background: var(--panel-bg);
    border-radius: 2px;
  }
</style>
