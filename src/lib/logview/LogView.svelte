<script lang="ts">
  import { ScrollMap } from "./scroll-map";
  import { LineCache } from "./line-cache";
  import { levelOf } from "./colorize";

  let { handle, lineCount }: { handle: number; lineCount: number } = $props();

  const LINE_HEIGHT = 20;
  /** 视口外多渲染几行，滚动时不露白 */
  const OVERSCAN = 8;

  const map = new ScrollMap(LINE_HEIGHT);
  // 缓存是 handle 的派生物：换文件自动重建，无需手动同步
  let cache = $derived(new LineCache(handle));

  let viewport: HTMLDivElement;
  let scrollTop = $state(0);
  let viewportHeight = $state(600);
  /** 块加载完成后自增，触发重新渲染 */
  let revision = $state(0);

  // 换文件回到顶部
  $effect(() => {
    handle;
    scrollTop = 0;
    if (viewport) viewport.scrollTop = 0;
  });

  // 索引还在跑时行数持续增长，末块需要失效重取
  $effect(() => {
    map.lineCount = lineCount;
    cache.invalidateTail(lineCount);
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
    // 依赖 revision，让块加载完成后重新求值
    revision;
    const out: { n: number; text: string | undefined }[] = new Array(rowCount);
    for (let i = 0; i < rowCount; i++) {
      const n = topLine + i;
      out[i] = { n, text: cache.get(n) };
    }
    return out;
  });

  $effect(() => {
    if (rowCount > 0) {
      cache.ensure(topLine, topLine + rowCount - 1, () => revision++);
    }
  });

  function onScroll() {
    scrollTop = viewport.scrollTop;
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
    } else if (e.key === "Home" || (e.key === "ArrowUp" && e.metaKey)) {
      e.preventDefault();
      viewport.scrollTop = 0;
    } else if (e.key === "End" || (e.key === "ArrowDown" && e.metaKey)) {
      e.preventDefault();
      viewport.scrollTop = map.scrollHeight;
    }
  }

  const gutterWidth = $derived(`${Math.max(5, String(lineCount).length)}ch`);
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_to_interactive_role -->
<div
  class="viewport"
  bind:this={viewport}
  bind:clientHeight={viewportHeight}
  onscroll={onScroll}
  onkeydown={onKeydown}
  tabindex="0"
  role="listbox"
  aria-label="日志内容"
>
  <div class="spacer" style:height="{map.scrollHeight}px">
    <div class="layer" style:transform="translateY({layerTop}px)">
      {#each rows as row (row.n)}
        {@const lvl = row.text ? levelOf(row.text) : null}
        <div class="row" class:pending={row.text === undefined}>
          <span class="gutter" style:width={gutterWidth}>{row.n + 1}</span>
          <span class="text" data-lvl={lvl}>{row.text ?? ""}</span>
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
    height: var(--line-height);
    line-height: var(--line-height);
    font-family: var(--code-font);
    font-size: 12px;
    white-space: pre;
  }
  .row:hover { background: rgba(255, 255, 255, 0.035); }
  .gutter {
    flex: none;
    text-align: right;
    padding-right: 14px;
    color: var(--text-faint);
    font-variant-numeric: tabular-nums;
    user-select: none;
  }
  .text {
    flex: 1;
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .text[data-lvl="error"] { color: var(--lvl-error); }
  .text[data-lvl="warn"] { color: var(--lvl-warn); }
  .text[data-lvl="debug"] { color: var(--lvl-debug); }
  /* 块还在路上：留白而不是跳动 */
  .pending .text::after {
    content: "";
    display: inline-block;
    width: 30ch;
    height: 9px;
    background: var(--panel-bg);
    border-radius: 2px;
  }
</style>
