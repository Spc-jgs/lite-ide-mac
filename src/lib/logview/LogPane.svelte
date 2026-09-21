<script lang="ts">
  import LogView from "./LogView.svelte";
  import FilterBar from "./FilterBar.svelte";
  import { decodeBlock } from "./block";
  import { detectFormat, FORMAT_LABEL, type LogFormat } from "./parse";
  import { type LogStat, type LevelCounts } from "../ipc/commands";
  import type { LogViewState } from "../state/tab";
  import { logStat, logLines, logFilter, logFilterStat, logRefresh, logFilterMap, logSeekTime } from "../ipc/log";
  import { notify } from "../state/notify.svelte";

  let {
    handle,
    gotoLine = null,
    seekTime = null,
    onSeekDone,
    onSeek,
    encoding = "utf-8",
    initialFilter = null,
    initialTop = null,
    onFilter,
    onGotoDone,
    onStatus,
    onTop,
  }: {
    handle: number;
    gotoLine?: { line: number; nonce: number } | null;
    /** 跳到时间（`nav.seekTime`）：行号由 Rust 二分出来，这里把它变成一次 `jumpTo` */
    seekTime?: { q: string; nonce: number } | null;
    onSeekDone?: () => void;
    /** 二分出来的行（1-based）。交给上层走 `nav.goto`，和 ⌘L 输行号是同一条路 */
    onSeek?: (line: number) => void;
    /** 文件编码标签；由上层探测后传下来 */
    encoding?: string;
    /** 上次切走时的过滤 / tail 状态（`TabState.logView`），没有就全默认 */
    initialFilter?: LogViewState | null;
    /** 上次切走时顶部那一行（1-based），挂载后原样摆回去；null 就从头开始 */
    initialTop?: number | null;
    /** 过滤 / tail 状态变了，交回标签 —— 切走再切回来靠它 */
    onFilter?: (s: LogViewState) => void;
    /** `gotoLine` 已经落到位了，上层把它销掉（`nav.done`） */
    onGotoDone?: () => void;
    onStatus: (s: string) => void;
    /**
     * 顶部可见的**物理**行号，给会话快照记「上次读到哪」。
     *
     * 过滤态下**不报**：那时的视图行号是「第几条命中」，换个关键字
     * 就完全对不上了，拿它当位置恢复出来会落在一个毫不相干的地方。
     */
    onTop?: (line: number) => void;
  } = $props();

  const ALL_LEVELS = 0b111111;

  let stat = $state<LogStat | null>(null);
  // 两个 initial* 只在挂载时读一次，这是有意的：之后的变化由本组件自己持有
  // svelte-ignore state_referenced_locally
  const init = initialFilter;
  // svelte-ignore state_referenced_locally
  const initTop = initialTop;
  // 下面六个是 `LogViewState`：初值从标签上取，每次变化交回去（见 tab.ts 那段注释）
  let levelBits = $state(init?.levelBits ?? ALL_LEVELS);
  let pattern = $state(init?.pattern ?? "");
  let caseSensitive = $state(init?.caseSensitive ?? false);
  let tailing = $state(init?.tailing ?? false);
  let collapseStacks = $state(init?.collapseStacks ?? false);
  /**
   * 只看命中，还是看全文、在命中之间跳。
   *
   * 这两件事在 GB 级日志里是不同的需求：「这个订单号出现过几次」要过滤，
   * 「这条报错前后发生了什么」要上下文。以前只有前者 —— 一输关键字整个文件
   * 就只剩命中行，想看上下文只能把关键字删掉，然后自己找回刚才那个位置。
   */
  let onlyHits = $state(init?.onlyHits ?? true);
  let filtered = $state(false);
  let filterHits = $state<number | null>(null);
  let filterRunning = $state(false);
  let error = $state("");
  let format = $state<LogFormat>("plain");
  /**
   * 文件被轮转 / 截断、Rust 侧按名重开了几次。每次 +1，让依赖 `handle` 的那几个
   * effect（格式投票、索引轮询、过滤）再跑一遍 —— 句柄没变，它们自己不会醒。
   * 也传给 LogView 让它丢掉行缓存：同一个句柄、同一个行号，内容已经是另一个文件的了
   */
  let epoch = $state(0);

  $effect(() => {
    onFilter?.({ levelBits, pattern, caseSensitive, tailing, collapseStacks, onlyHits });
  });
  /** 当前停在第几条命中，1-based；0 表示还没跳过 */
  let hitIndex = $state(0);
  /** 传给 LogView 的跳转指令 */
  let jumpTo = $state<{ line: number; nonce: number; top?: boolean } | null>(null);
  let jumpNonce = 0;
  /**
   * 挂载时把上次的顶行摆回去。走 `top: true`（那一行贴顶，不留上下文）：
   * 跳命中那种「往上留三行」用在这儿会每切一次标签往上漂三行，
   * 会话恢复原来就是这么漂的 —— 记的是顶行，恢复出来是顶行往上三行，再记、再漂。
   */
  /*
   * **一次性的**：任何一次跳转落到位（`onGotoDone`）就把它清掉。它是 `jumpTo ?? gotoLine ?? restoreTo`
   * 里的兜底，不清的话外面来的跳转（⌘L、跳到时间、搜索结果）落到位、`nav.done()` 把 gotoLine 置空，
   * 兜底又冒出来把视口拽回上次的顶行 —— 2026-09-21 加跳到时间时撞上的：每跳一次都弹回第 47 行。
   */
  let restoreTo = $state(initTop !== null && initTop > 1 ? { line: initTop, nonce: ++jumpNonce, top: true } : null);

  /**
   * 跳到上/下一处命中。
   *
   * 两种视图下「行号」的含义不同，这是这段唯一需要小心的地方：
   * - 只看命中：视图第 i 行**就是**第 i 条命中，直接跳
   * - 看全文：得问 Rust 要第 i 条命中的**物理行号**（logFilterMap）
   *
   * ⚠️ 两边的基数不一样，踩过一次：
   * `gotoLine.line` 全程按 **1-based** 用（LogView 里做 `line - 1`），
   * 而 `logFilterMap` 返回的是 **0-based** 物理行号（与 `row.phys` 同源，
   * 行号栏显示的是 `phys + 1`）。所以走全文那条路必须 +1。
   * 只看命中那条恰好没踩到 —— 命中序号本来就是 1-based，纯属运气。
   *
   * 不缓存整张命中表 —— 900 万行的文件上它可能有几百万条，
   * 传到前端纯属浪费。每次只取一条。
   */
  /** 视口顶上那一行（0-based 物理行）。跳时间不带日期时，Rust 拿它附近的日期补 */
  let lastTop = 0;

  /*
   * 外面来了新的跳行指令（⌘L 输行号、跳到时间）就把 F3 留下的 `jumpTo` 让开：
   * 下面 `jumpTo ?? gotoLine` 是 F3 优先，不让的话 ⌘L 永远被上一次 F3 挡住
   * （2026-09-21 加跳到时间时撞上的：⌘L 输 50 不动，因为 jumpTo 还停在上一次）。
   * 只在指令非空时让 —— 落到位后 `nav.done()` 把它置空，那时不能把 F3 的当前行高亮也清掉。
   */
  $effect(() => {
    if (gotoLine) jumpTo = null;
  });

  $effect(() => {
    const st = seekTime;
    const h = handle;
    if (!st) return;
    let dead = false;
    logSeekTime(h, st.q, lastTop)
      .then((line) => {
        if (dead) return;
        onSeekDone?.();
        if (line === null) {
          notify.fail("这个文件里没认出时间戳（认 2026-08-24 14:03:21 / Aug 24 14:03:21 / nginx / JSON 那几种）", 3600);
          return;
        }
        // 「只看命中」下视图行是命中序号，物理行跳不过去 —— 先切回全文
        if (onlyHits) onlyHits = false;
        onSeek?.(line + 1);
      })
      .catch((e) => {
        if (!dead) error = String(e);
      });
    return () => {
      dead = true;
    };
  });

  async function jumpHit(dir: 1 | -1) {
    const total = filterHits ?? 0;
    if (!filtered || total === 0) return;
    // 循环：到底了回到第一条，符合「一直按下一处」的直觉
    let next = hitIndex + dir;
    if (next < 1) next = total;
    if (next > total) next = 1;
    hitIndex = next;

    if (onlyHits) {
      jumpTo = { line: next, nonce: ++jumpNonce };
      return;
    }
    try {
      const [physical] = await logFilterMap(handle, next - 1, 1);
      // physical 是 0-based，换成全局约定的 1-based
      if (physical !== undefined) jumpTo = { line: physical + 1, nonce: ++jumpNonce };
    } catch (e) {
      error = String(e);
    }
  }

  /*
   * F3 / ⇧F3 跳命中。挂在 window 上而不是某个元素上：翻日志时焦点可能在
   * 滚动区、过滤框、级别按钮上的任何一个，绑到具体元素就会时灵时不灵。
   * 这个组件只在日志模式下挂载，卸载时监听跟着走，不会串到编辑模式去。
   */
  $effect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "F3") return;
      e.preventDefault();
      void jumpHit(e.shiftKey ? -1 : 1);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  });

  // 换关键字 / 换文件就把游标归零，否则「3/12」会停在一个已经不存在的位置
  $effect(() => {
    pattern;
    levelBits;
    handle;
    hitIndex = 0;
  });

  // 取开头几十行投票选格式。只看第一行容易被启动横幅、空行带偏
  $effect(() => {
    const h = handle;
    epoch;
    logLines(h, 0, 60)
      .then((buf) => {
        format = detectFormat(decodeBlock(buf, encoding).lines);
      })
      .catch(() => (format = "plain"));
  });

  // 索引与级别扫描都在后台跑，轮询到两者都完成为止
  $effect(() => {
    const h = handle;
    epoch;
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
    epoch;
    let tick: ReturnType<typeof setInterval> | null = null;
    /*
     * `dead` 不能省。
     *
     * cleanup 只能清掉它**当时看得见**的东西：如果它正好赶在 `logFilter`
     * 的 await 中间跑，`tick` 还是 null，清了个寂寞 —— 而 await 回来之后
     * 那行 `tick = setInterval(...)` 照样执行，装出一个再也没人清的轮询。
     * 在 1GB 文件上连打十个字，就是十个 80ms 的轮询一起烧 IPC。
     */
    let dead = false;

    const timer = setTimeout(async () => {
      try {
        const active = await logFilter(h, bits, pat, cs, fold, encoding);
        if (dead) return;
        filtered = active;
        if (!active) {
          filterHits = null;
          filterRunning = false;
          return;
        }
        filterRunning = true;
        tick = setInterval(async () => {
          const fs = await logFilterStat(h);
          if (dead) return;
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
        if (!dead) error = String(e);
      }
    }, 180);

    return () => {
      dead = true;
      clearTimeout(timer);
      if (tick) clearInterval(tick);
    };
  });

  /** 轮转过几次，状态栏上说一声就够 —— 不打断、不关 tail */
  let rotations = $state(0);

  /*
   * tail：轮询文件是否追加。mmap 长度固定，Rust 侧会重新映射。
   *
   * 轮转 / 截断时 Rust 侧已经按名重开、句柄不变（`AppState::reopen`）：
   * 这里只需要让依赖句柄的那几个 effect 再跑一遍（`epoch`），过滤条件原样
   * 重跑，tail 继续 —— `tail -F` 的语义。原来是关掉 tail、报「请重新打开」，
   * logback 每晚滚一次日志就每晚断一次，过滤和位置也一起没了。
   *
   * 改名和新建之间那个空档 `logRefresh` 会抛（文件不在），走下面的 catch，
   * 下一轮再试 —— 就是 tail -F 等新文件出现的那段。
   */
  $effect(() => {
    const h = handle;
    if (!tailing) return;
    const id = setInterval(async () => {
      try {
        const r = await logRefresh(h);
        if (r.kind === "grew") stat = await logStat(h);
        else if (r.kind === "rotated") {
          rotations += 1;
          epoch += 1;
        }
      } catch {
        /* 文件临时不可读（轮转的空档、权限抖动），下一轮再试 */
      }
    }, 500);
    return () => clearInterval(id);
  });

  const fmtNum = (n: number) => n.toLocaleString("en-US");

  /**
   * 拿到首批命中计数之前继续显示旧视图 —— 否则行数为 0，
   * 界面会闪一下空白再填上内容。
   */
  let showFiltered = $derived(filtered && filterHits !== null && onlyHits);
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
    if (rotations > 0) parts.push(`轮转过 ${rotations} 次，已跟上新文件`);
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
    {hitIndex}
    bind:onlyHits
    onJump={(d) => void jumpHit(d)}
  />
  <div class="body">
    <LogView
      {handle}
      lineCount={viewLines}
      filtered={showFiltered}
      {pattern}
      {caseSensitive}
      stickBottom={tailing}
      {epoch}
      gotoLine={jumpTo ?? gotoLine ?? restoreTo}
      onGotoDone={() => {
        restoreTo = null;
        onGotoDone?.();
      }}
      currentLine={jumpTo?.line ?? 0}
      {format}
      {encoding}
      onTop={(l) => {
        // 只看命中时视图行是命中序号，不是物理行，记了反而误导跳时间那条补日期
        if (showFiltered) return;
        lastTop = Math.max(0, l - 1); // onTop 报的是 1-based，Rust 那边按 0-based 物理行
        onTop?.(l);
      }}
    />
  </div>
</div>

<style>
  /* container-type 让过滤栏能按**自己所在容器**的宽度退化，而不是看整个窗口 ——
     侧边栏和终端面板都会吃掉宽度，vw 在这里是错的参照物 */
  .pane {
    display: grid;
    grid-template-rows: auto 1fr;
    height: 100%;
    overflow: hidden;
    container-type: inline-size;
  }
  .body { overflow: hidden; }
</style>
