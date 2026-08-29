<script lang="ts">
  import { ScrollMap } from "./scroll-map";
  import { LineCache, type Row } from "./line-cache";
  import { parse, highlight, type LogFormat } from "./parse";

  let {
    handle,
    lineCount,
    filtered = false,
    pattern = "",
    caseSensitive = false,
    stickBottom = false,
    gotoLine = null,
    currentLine = 0,
    format = "plain",
    encoding = "utf-8",
    onTop,
  }: {
    handle: number;
    lineCount: number;
    filtered?: boolean;
    pattern?: string;
    caseSensitive?: boolean;
    stickBottom?: boolean;
    /** 搜索结果跳转的目标行（1-based）。带 nonce，连点同一条也能重新定位 */
    gotoLine?: { line: number; nonce: number } | null;
    /**
     * 当前停在哪一行（视图行号，1-based；0 表示没有）。
     *
     * 「跳到下一处」把行滚到中间是不够的 —— 屏幕上十几行长得都一样，
     * 高亮的关键字每行都有，用户没法一眼认出「就是这一行」。
     */
    currentLine?: number;
    /** 日志格式，由 LogPane 从样本行探测后传入 */
    format?: LogFormat;
    /** 文件编码标签，交给 TextDecoder */
    encoding?: string;
    /**
     * 顶部可见行变了就报一次（1-based）。会话快照用它记住「上次读到哪」——
     * 在 1GB 日志里这件事比在代码文件里值钱得多。
     */
    onTop?: (line: number) => void;
  } = $props();

  const LINE_HEIGHT = 20;
  /** 视口外多渲染几行，滚动时不露白 */
  const OVERSCAN = 8;

  const map = new ScrollMap(LINE_HEIGHT);
  // 缓存是 (handle, 是否过滤, 编码) 的派生物：任一变化都要重建
  let cache = $derived(new LineCache(handle, filtered, encoding));

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

  /** 上次报出去的顶部行，把「同一行内的像素级滚动」滤掉 */
  let lastTop = 0;

  function onScroll() {
    if (!viewport) return;
    scrollTop = viewport.scrollTop;
    if (!onTop) return;
    const line = map.topLineAt(scrollTop, viewportHeight) + 1;
    if (line !== lastTop) {
      lastTop = line;
      onTop(line);
    }
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
        {@const seg = row ? parse(row.text, format) : null}
        <div
          class="row"
          class:pending={!row}
          class:stack={seg?.stack}
          class:current={currentLine > 0 && n === currentLine - 1}
          data-lvl={seg?.lvl}
        >
          <span class="gutter" style:width={gutterWidth}>{row ? row.phys + 1 : ""}</span>
          {#if seg}
            <span class="cells">
              {#each seg.parts as part}
                <span class="p" data-cls={part.cls}
                  >{#each highlight(part.text, pattern, caseSensitive) as t, i}{#if i % 2 === 1}<mark
                      >{t}</mark
                    >{:else}{t}{/if}{/each}</span
                >
              {/each}
            </span>
          {:else}
            <span class="cells"></span>
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
  /*
   * 当前停在的那一行。用左侧一条 accent 竖线 + 淡底，而不是整行反色 ——
   * 行内的关键字高亮已经在抢注意力了，再来一层强底色两个都看不清。
   */
  .row.current {
    background: rgba(53, 116, 240, 0.13);
    box-shadow: inset 2px 0 0 var(--accent);
  }
  .row.current .gutter { color: var(--accent); }

  .gutter {
    flex: none;
    text-align: right;
    padding-right: 6px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    user-select: none;
    border-right: 1px solid var(--border-soft);
    margin-right: 6px;
  }

  /* 分段渲染：解析器吐什么段，这里就按 cls 上什么色。
     加一种日志格式不必动这里 */
  .cells { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .p[data-cls="ts"] { color: var(--text-faint); }
  .p[data-cls="level"] { font-weight: 600; }
  .p[data-cls="thread"] { color: var(--text-faint); }
  .p[data-cls="logger"] { color: var(--text-dim); }
  .p[data-cls="key"] { color: var(--lvl-warn); }
  .p[data-cls="meta"] { color: var(--text-dim); }
  .p[data-cls="dim"] { color: var(--text-faint); }
  .p[data-cls="msg"] { color: var(--text); }

  /* 级别色只染级别段；ERROR 例外——整行都该扎眼 */
  .row[data-lvl="error"] .p[data-cls="level"],
  .row[data-lvl="error"] .p[data-cls="msg"] { color: var(--lvl-error); }
  .row[data-lvl="warn"] .p[data-cls="level"] { color: var(--lvl-warn); }
  .row[data-lvl="info"] .p[data-cls="level"] { color: var(--lvl-info); }
  .row[data-lvl="debug"] .p[data-cls="level"],
  .row[data-lvl="trace"] .p[data-cls="level"] { color: var(--lvl-debug); }
  .row[data-lvl="debug"] .p[data-cls="msg"],
  .row[data-lvl="trace"] .p[data-cls="msg"] { color: var(--text-dim); }

  /* 堆栈续行：缩进 + 压暗，一眼看出是上一条的附属 */
  .row.stack .p { color: var(--text-dim); }
  .row.stack .cells { padding-left: 2ch; }
  .row.stack { background: rgba(255, 255, 255, 0.02); }

  mark {
    background: var(--search-hit);
    color: var(--text);
    border-radius: 1px;
  }

  /* 块还在路上：留白而不是跳动 */
  .pending .cells::after {
    content: "";
    display: inline-block;
    width: 34ch;
    height: 9px;
    background: var(--panel-bg);
    border-radius: 2px;
  }
</style>
