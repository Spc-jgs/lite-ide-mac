<script lang="ts">
  import { pinAfterScroll, wantsUp } from "./pin";
  import { ScrollMap } from "./scroll-map";
  import { LineCache, type Row } from "./line-cache";
  import { cutNote } from "./block";
  import { parse, type LogFormat } from "./parse";
  import { parseQuery, highlightQuery } from "./query";
  import { withLink } from "./stack-frame";

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
    epoch = 0,
    onTop,
    onGotoDone,
    frameAt,
    onFrame,
    sources = null,
  }: {
    handle: number;
    lineCount: number;
    filtered?: boolean;
    pattern?: string;
    caseSensitive?: boolean;
    stickBottom?: boolean;
    /**
     * 跳转的目标行（1-based）。带 nonce，连点同一条也能重新定位。
     * 默认往上留三行上下文（跳命中要看前因）；`top` 是「这一行贴顶」——
     * 恢复上次的顶行用，留上下文会每恢复一次往上漂三行。
     */
    gotoLine?: { line: number; nonce: number; top?: boolean } | null;
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
    /** 句柄背后的文件被换掉了几次（轮转后按名重开）。变了就丢掉行缓存：行号还是那个行号，内容不是了 */
    epoch?: number;
    /**
     * 顶部可见行变了就报一次（1-based）。会话快照用它记住「上次读到哪」——
     * 在 1GB 日志里这件事比在代码文件里值钱得多。
     */
    onTop?: (line: number) => void;
    /** `gotoLine` 真的落到目标行了（不是被行数夹住那次）；上层据此销掉指令 */
    onGotoDone?: () => void;
    /**
     * 堆栈行里哪一段能跳到项目源码（`OrderService.java:142`）；跳不了（在 jar 里、认不准）就 null。
     * 判据在 LogPane（它有文件索引），这里只管画和点 —— 见 stack-frame.ts
     */
    frameAt?: (text: string) => { from: number; to: number } | null;
    /** 点了那段链接。给的是整行原文，由上层再解析一次去开文件 */
    onFrame?: (text: string) => void;
    /**
     * 合并视图（docs/MERGE.md）：每份的短名和颜色。有它时每行行首多一格「来自哪份」，
     * 行号栏显示的是那一份里的行号（对着原文件找得到），不是合并后的第几行
     */
    sources?: { name: string; color: string }[] | null;
  } = $props();

  /** 过滤框那串字切好的样子（语法见 `query.ts`）；每行画高亮时不重切 */
  let query = $derived(parseQuery(pattern));

  const LINE_HEIGHT = 20;
  /** 视口外多渲染几行，滚动时不露白 */
  const OVERSCAN = 8;

  const map = new ScrollMap(LINE_HEIGHT);
  // 缓存是 (handle, 是否过滤, 编码, 轮转代) 的派生物：任一变化都要重建
  let cache = $derived.by(() => {
    epoch;
    return new LineCache(handle, filtered, encoding, !!sources);
  });
  /** 来源那一格多宽：按最长的短名，最多 14 个字宽（再长的省略，图例上有全名） */
  const tagWidth = $derived(sources ? `${Math.min(14, Math.max(...sources.map((s) => s.name.length)))}ch` : "");

  /**
   * 撑开滚动区的高度，**必须**从 `lineCount` 派生，不能在模板里直接读 `map.scrollHeight`：
   * `map` 是普通对象，模板不会跟着它变。原来那么写能工作纯属巧合 —— 同一块模板里
   * `layerTop` 一变顺带重算了它，而第一次滚动之前它一直是 0px。挂载时恢复顶行的
   * 跳转正好发生在第一次滚动之前：scrollTop 被夹在一个没撑开的容器里，1GB 日志
   * 恢复到第 112867 行落在了第 188 行。
   */
  let spacerHeight = $derived(ScrollMap.heightFor(lineCount, LINE_HEIGHT));

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

  /**
   * 跟随尾部时这一刻跟不跟：人往上翻就松开、翻回底部附近再跟上（判法在 pin.ts）。不用 `$state`：
   * 它变了不该触发任何东西 —— 松开时不用动，跟上时人自己已经滚到底了
   */
  let pinned = true;
  let prevScroll = 0;
  // 跟随刚打开（点了按钮、新开一个运行）：先跟上
  $effect(() => {
    if (stickBottom) pinned = true;
  });
  // tail 吸底：行数一变就贴到最新 —— 人往上翻着的时候不贴
  $effect(() => {
    if (!stickBottom || !viewport) return;
    lineCount;
    if (pinned) viewport.scrollTop = map.scrollHeight;
  });
  function noteIntent(e: WheelEvent | KeyboardEvent) {
    if (stickBottom && wantsUp(e)) pinned = false;
  }

  /**
   * 同一条跳转指令已经落到过目标行就不再重放。这个 effect 读 `lineCount`
   * 是为了「索引还没扫到目标行时先等着」，但索引每 100ms 长一截它就重跑一次 ——
   * 原来在 1GB 文件上跳完一行，索引没扫完之前的十几秒里视口被钉在那儿滚不走。
   */
  let applied = -1;

  // 跳到指定行：压缩映射下也能算出正确的 scrollTop
  $effect(() => {
    const g = gotoLine;
    if (!g || !viewport || lineCount === 0) return;
    if (applied === g.nonce && lineCount >= g.line) return;
    const target = Math.min(Math.max(0, g.line - 1), Math.max(0, lineCount - 1));
    // 往上留几行上下文，别把目标贴在视口最顶上；恢复顶行的那种要的就是贴顶
    const withContext = g.top ? target : Math.max(0, target - 3);
    viewport.scrollTop = map.scrollTopFor(withContext, viewportHeight);
    // 目标行还没索引到就先落在当前末尾，这次不算数 —— 等行数追上再落一次真的
    if (lineCount >= g.line) {
      applied = g.nonce;
      onGotoDone?.();
    }
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
    if (stickBottom) {
      const top = viewport.scrollTop;
      pinned = pinAfterScroll(pinned, { prevTop: prevScroll, top, distance: viewport.scrollHeight - top - viewport.clientHeight });
      prevScroll = top;
    }
    if (!onTop) return;
    const line = map.topLineAt(scrollTop, viewportHeight) + 1;
    if (line !== lastTop) {
      lastTop = line;
      onTop(line);
    }
  }

  /** 链接一律用事件委托：屏幕上几十行，每行挂一个 onclick 在滚动时反复装卸 */
  function onClick(e: MouseEvent) {
    const a = (e.target as HTMLElement).closest<HTMLElement>('.p[data-cls="link"]');
    const n = a?.closest<HTMLElement>(".row")?.dataset.n;
    if (!a || n === undefined || !onFrame) return;
    // 拖选一段文字松手也会落一个 click：有选区就是在复制，不跳
    if (getSelection()?.toString()) return;
    const row = cache.get(Number(n));
    if (row) onFrame(row.text);
  }

  function onKeydown(e: KeyboardEvent) {
    if (!viewport) return;
    noteIntent(e);
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
  onwheel={noteIntent}
  onclick={onClick}
  tabindex="0"
  role="log"
  aria-label="日志内容"
>
  <div class="spacer" style:height="{spacerHeight}px">
    <div class="layer" style:transform="translateY({layerTop}px)">
      {#each rows as { n, row } (n)}
        {@const seg = row ? parse(row.text, format) : null}
        {@const link = seg?.stack && frameAt && row ? frameAt(row.text) : null}
        <div
          class="row"
          data-n={n}
          class:pending={!row}
          class:stack={seg?.stack}
          class:current={currentLine > 0 && n === currentLine - 1}
          data-lvl={seg?.lvl}
        >
          {#if sources}
            {@const s = row?.src !== undefined ? sources[row.src] : undefined}
            <!-- 来源：一道色条 + 短名。界面的字（不可选中）：复制一段合并后的日志时不该带上它 -->
            <span class="src" style:width={tagWidth} style:--c={s?.color ?? "transparent"} title={s?.name}>{s?.name ?? ""}</span>
          {/if}
          <span class="gutter" style:width={gutterWidth}>{row ? row.phys + 1 : ""}</span>
          {#if seg}
            <span class="cells">
              {#each link ? withLink(seg.parts, link.from, link.to) : seg.parts as part}
                <span class="p" data-cls={part.cls}
                  >{#each highlightQuery(part.text, query, caseSensitive) as t, i}{#if i % 2 === 1}<mark
                      >{t}</mark
                    >{:else}{t}{/if}{/each}</span
                >
              {/each}
            </span>
            <!-- 截过的长行：说出来、给真数字（block.ts）。放在 .cells 外面、贴着右边：.cells 超出窗口就是省略号，放里面会跟着被省略掉。
                 不可选中 —— 复制这一行时别把界面上的话带进去（复制正是这条最坑人的地方：以为拿到了整行，其实只有前 64KB） -->
            {#if row?.more}<span class="cut">{cutNote(row.more)}</span>{/if}
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
    font-size: var(--fs-md);
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

  /* 合并视图的来源格：左边 2px 的色条，名字用同一个颜色、压暗一档 —— 一眼扫下去看得出「这一段是哪个服务的」，又不抢正文 */
  .src {
    flex: none;
    box-sizing: content-box;
    padding: 0 6px 0 7px;
    box-shadow: inset 2px 0 0 var(--c);
    color: var(--c);
    opacity: 0.85;
    font-family: var(--ui-font);
    font-size: var(--fs-xs);
    overflow: hidden;
    text-overflow: ellipsis;
    user-select: none;
  }

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
  /* 截过的长行在行尾说一句（界面的话，不是日志内容：界面字体、最淡的字、不可选中） */
  .cut {
    flex: none;
    padding: 0 6px 0 1ch;
    font-family: var(--ui-font);
    font-size: var(--fs-xs);
    color: var(--text-faint);
    user-select: none;
  }
  .p[data-cls="ts"] { color: var(--text-faint); }
  .p[data-cls="level"] { font-weight: 600; }
  .p[data-cls="thread"] { color: var(--text-faint); }
  .p[data-cls="logger"] { color: var(--text-dim); }
  .p[data-cls="key"] { color: var(--lvl-warn); }
  .p[data-cls="meta"] { color: var(--text-dim); }
  .p[data-cls="dim"] { color: var(--text-faint); }
  /*
   * 能跳到项目源码的帧：一直带下划线（IDEA 的控制台也是），不用悬停才出现 ——
   * 一眼扫下来哪几帧是自己的代码，这本身就是信息。跳不了的（jar 里的）不画，所以链接
   * 在堆栈里是稀疏的，不吵。小手而不是箭头：它是文本里的链接，和编辑器 ⌘hover 的下划线同类，
   * 不是按钮（按钮一律箭头，ui.md）
   */
  /* 带上 .row.stack：下面那条 `.row.stack .p` 的灰是三个类的权重，只写 .p[data-cls] 压不过它 */
  .row.stack .p[data-cls="link"] {
    color: var(--accent);
    text-decoration: underline;
    text-decoration-thickness: 1px;
    text-underline-offset: 2px;
    cursor: pointer;
  }
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
    /* 贴在一行字里的高亮：2px，不是控件的 7px —— 7px 在 20px 高的字上，一串命中像一颗颗胶囊（ui.md 第一之三条） */
    border-radius: 2px;
  }

  /* 块还在路上：留白而不是跳动 */
  .pending .cells::after {
    content: "";
    display: inline-block;
    width: 34ch;
    height: 9px;
    background: var(--hover);
    border-radius: var(--r-sm);
  }
</style>
