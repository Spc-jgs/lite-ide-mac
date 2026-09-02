<script lang="ts">
  import {
    parseDiff,
    segs,
    toSideBySide,
    changeBlocks,
    blankRuns,
    type DiffFile,
    type DiffLine,
  } from "./diff";

  let {
    raw,
    path,
    staged,
    commit = "",
    untracked = false,
    capped = false,
    onToggleStaged,
  }: {
    raw: string;
    path: string;
    /** 当前看的是暂存区还是工作区 */
    staged: boolean;
    /** 非空表示这是某次提交里的差异（只读历史），此时没有暂存/未暂存之分 */
    commit?: string;
    /**
     * 这是个未跟踪（新增）的文件。
     *
     * 用来把两种「什么都没有」分开：新增的**空文件**本来就没有内容可显示，
     * 和「这一侧没有改动」是完全不同的两件事，却都渲染成一片空白。
     */
    untracked?: boolean;
    /**
     * 差异在 Rust 侧就被 1MB 上限掐断了，`raw` 只是前半截。
     *
     * 跟下面的 `truncated`（前端渲染行数上限）是两件事，措辞必须分开 ——
     * 「共 N 行」在这种情况下是假的，N 本身就已经不全了。
     */
    capped?: boolean;
    onToggleStaged: () => void;
  } = $props();

  /**
   * 渲染行数上限。一次 refactor 改上万行是有的，全渲染会让切标签明显卡顿；
   * 但也不能不给看 —— 截断并说清楚还剩多少。
   */
  const MAX_ROWS = 3000;

  /** 双栏对照是 IDEA 的默认形态，也确实更好读；统一视图留着给窄窗口用 */
  /**
   * 用户**想要**哪种视图。实际显示的是下面的 `sideOn` ——
   * 窄到读不了的时候会被强制成统一视图，但这里记着的意图不变，
   * 窗口一拉宽就自动回到双栏。（直接改 `side` 的话，
   * 拉宽之后回不去，等于把用户的选择偷偷改掉了。）
   */
  let side = $state(true);
  /**
   * 软换行。默认关着 —— 代码差异按行读，换行会把「这是一行」这个信息弄糊。
   * 开着的时候长行折进列宽里，整块差异不再横向溢出。
   *
   * 这两件事是配套的，缺一条都不成立：折行之后每行高度不再是常数，
   * 所以 `jump()` 必须去问真实几何（见那边的注释）。
   */
  let wrap = $state(false);
  let boxW = $state(0);
  /**
   * 双栏的下限宽度。
   *
   * 实测 760px 窗口下内容区只有 482px，每列剩四十来个字符 ——
   * 一行 Java 代码都放不下，两栏都在横向滚动，比统一视图还难读。
   * 而模板里那句「统一视图（窄窗口更合适）」一直只是句提示，
   * 从来没人替用户按下去过。
   */
  const SIDE_MIN = 720;
  let narrow = $derived(boxW > 0 && boxW < SIDE_MIN);
  /** 真正在显示的形态 */
  let sideOn = $derived(side && !narrow);
  let box = $state<HTMLElement | null>(null);
  let cur = $state(-1);

  let files = $derived<DiffFile[]>(parseDiff(raw));
  let adds = $derived(files.reduce((n, f) => n + f.adds, 0));
  let dels = $derived(files.reduce((n, f) => n + f.dels, 0));

  /** 双栏行；多文件时按顺序接起来，中间插一条文件名分隔 */
  let sideRows = $derived.by(() => {
    const out: ReturnType<typeof toSideBySide> = [];
    for (const f of files) {
      if (files.length > 1) out.push({ kind: "meta", left: null, right: null, text: f.path });
      out.push(...toSideBySide(f.lines));
    }
    return out;
  });

  let uniRows = $derived.by(() => {
    const out: DiffLine[] = [];
    for (const f of files) {
      if (files.length > 1) out.push({ kind: "meta", text: f.path });
      out.push(...f.lines);
    }
    return out;
  });

  let total = $derived(sideOn ? sideRows.length : uniRows.length);
  let truncated = $derived(total > MAX_ROWS);
  /** 实际渲染出来的那些行 —— 两种视图各切一份，类型才不会退化成 { kind } */
  let sideShown = $derived(sideRows.slice(0, MAX_ROWS));
  let uniShown = $derived(uniRows.slice(0, MAX_ROWS));
  /**
   * 跳转目标只能取**渲染出来的**那些块。
   * 早先是在全部行上算的，于是超过 MAX_ROWS 的差异里，「下一处改动」会把
   * 视图滚到一片空白 —— 那些行根本没渲染。
   */
  let blocks = $derived(changeBlocks(sideOn ? sideShown : uniShown));
  /** 哪些空白行落在够长的连续空白块里 —— 那些换纯色底，见 blankRuns 的注释 */
  let flat = $derived(blankRuns(sideShown));

  // 换文件 / 换视图就把跳转游标归零，否则会停在一个已经不存在的位置
  $effect(() => {
    raw;
    sideOn;
    cur = -1;
  });

  function jump(dir: 1 | -1) {
    if (blocks.length === 0 || !box) return;
    let next = cur + dir;
    if (next < 0) next = blocks.length - 1;
    if (next >= blocks.length) next = 0;
    cur = next;
    /*
     * 位置**问 DOM**，不按「下标 × 行高」算。
     *
     * 算术版要求每一行正好等高，而软换行一开这个前提就没了 ——
     * 一条 120 字符的 import 占两行、旁边的上下文占一行，
     * 算出来的 y 会越跳越偏。（这正是 issue #7 里判断「软换行和跳转冲突、
     * 不是调 CSS 能收尾的」的由来，其实冲突的只是这一行算术。）
     *
     * 用 rect 差而不是 offsetTop：行号列是 position: sticky，
     * offsetTop 在吸住的元素上各家浏览器口径不一；而 rect 和 scrollTop
     * 是同一时刻的同一套坐标，平滑滚动进行中读也是自洽的。
     */
    const el = box.querySelector<HTMLElement>(`[data-row="${blocks[cur]}"]`);
    if (!el) return;
    const top = el.getBoundingClientRect().top - box.getBoundingClientRect().top + box.scrollTop;
    // 目标块放到可视区上三分之一处：改动上下的上下文都看得见
    box.scrollTo({ top: Math.max(0, top - box.clientHeight / 3), behavior: "smooth" });
  }

  function onKey(e: KeyboardEvent) {
    /*
     * 这个监听挂在 window 上，而差异标签是打开着的时候底部终端可能正被聚焦 ——
     * 不排除掉可输入的目标，⌥Z 就会被我们截走，用户以为终端吞了键。
     */
    const t = e.target as HTMLElement | null;
    if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
    // F7 / ⇧F7 是 IDEA 的「下一处 / 上一处差异」
    if (e.key === "F7") {
      e.preventDefault();
      jump(e.shiftKey ? -1 : 1);
      return;
    }
    // ⌥Z 跟 VS Code 一致。macOS 上 ⌥z 的 e.key 是「Ω」，只能认 code
    if (e.altKey && e.code === "KeyZ") {
      e.preventDefault();
      wrap = !wrap;
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="diff" bind:clientWidth={boxW}>
  <div class="bar">
    <span class="path" title={path}>{path}</span>
    {#if files[0]?.oldPath}
      <span class="renamed">← {files[0].oldPath}</span>
    {/if}
    <span class="gap"></span>
    <span class="stat"><b class="a">+{adds}</b> <b class="d">−{dels}</b></span>

    {#if blocks.length > 0}
      <span class="nav">
        <button onclick={() => jump(-1)} title="上一处改动 ⇧F7" aria-label="上一处改动">⌃</button>
        <span class="pos">{cur < 0 ? "—" : cur + 1}/{blocks.length}</span>
        <button onclick={() => jump(1)} title="下一处改动 F7" aria-label="下一处改动">⌄</button>
      </span>
    {/if}

    <span class="segs">
      <button
        class:on={sideOn}
        disabled={narrow}
        onclick={() => (side = true)}
        title={narrow ? "窗口太窄，双栏每列放不下一行代码；拉宽就会自动切回来" : "左右分栏对照"}
      >双栏</button>
      <button class:on={!sideOn} onclick={() => (side = false)} title="统一视图（窄窗口更合适）">统一</button>
    </span>

    <button
      class="seg"
      class:on={wrap}
      onclick={() => (wrap = !wrap)}
      title="软换行 ⌥Z —— 长行折进列宽，不用横向滚"
      aria-pressed={wrap}
    >换行</button>

    {#if commit}
      <span class="sha" title="这是历史提交里的差异，只读">{commit}</span>
    {:else}
      <button class="seg" onclick={onToggleStaged} title="在「已暂存 ↔ 未暂存」之间切换">
        {staged ? "已暂存的改动" : "未暂存的改动"} ⇄
      </button>
    {/if}
  </div>

  <div class="body" bind:this={box}>
    {#if files.length === 0}
      <div class="none">
        {#if untracked}新增的文件，内容为空{:else}这一侧没有改动{/if}
      </div>
    {:else if files[0].binary}
      <div class="none">二进制文件，不显示差异</div>
    {:else if sideOn}
      <!--
        data-row 是跳转的锚点。双栏里一行是 4 个 grid 子元素拼出来的，
        没有「行」这个元素，所以标在每行的**第一个**子元素上 ——
        两个分支都要标，漏掉 span4 的话落在 hunk 行上的跳转会静默失败。
      -->
      <div class="grid" class:wrap>
        {#each sideShown as r, i (i)}
          {#if r.kind === "hunk" || r.kind === "meta"}
            <div class="span4 {r.kind}" data-row={i}>{r.text || "⋯"}</div>
          {:else}
            {@const L = r.left}
            {@const R = r.right}
            <div class="no {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}" data-row={i}>{L?.oldNo ?? ""}</div>
            <div class="tx {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}" class:flat={flat.left[i]}>
              {#if L}{@const s = segs(L)}{s[0]}{#if s[1]}<mark>{s[1]}</mark>{/if}{s[2]}{/if}
            </div>
            <div class="no mid {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}">{R?.newNo ?? ""}</div>
            <div class="tx {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}" class:flat={flat.right[i]}>
              {#if R}{@const s = segs(R)}{s[0]}{#if s[1]}<mark>{s[1]}</mark>{/if}{s[2]}{/if}
            </div>
          {/if}
        {/each}
      </div>
    {:else}
      <div class="uni" class:wrap>
        {#each uniShown as l, i (i)}
          {#if l.kind === "hunk" || l.kind === "meta"}
            <div class="row {l.kind}" data-row={i}>
              <span class="no"></span><span class="no"></span><span class="sign"></span>
              <span class="txt">{l.text || "⋯"}</span>
            </div>
          {:else}
            {@const s = segs(l)}
            <div class="row {l.kind}" data-row={i}>
              <span class="no">{l.oldNo ?? ""}</span>
              <span class="no">{l.newNo ?? ""}</span>
              <span class="sign">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : ""}</span>
              <span class="txt">{s[0]}{#if s[1]}<mark>{s[1]}</mark>{/if}{s[2]}</span>
            </div>
          {/if}
        {/each}
      </div>
    {/if}
    {#if capped}
      <div class="none">
        这份差异过大，只取了前 1MB{#if truncated}，其中显示了前 {MAX_ROWS} 行{/if}
        —— 完整内容请用 <code>git diff</code> 看
      </div>
    {:else if truncated}
      <div class="none">差异太长，只显示了前 {MAX_ROWS} 行（共 {total} 行）</div>
    {/if}
  </div>
</div>

<style>
  .diff {
    display: flex;
    flex-direction: column;
    height: 100%;
    overflow: hidden;
    background: var(--editor-bg);
  }
  .bar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 28px;
    padding: 0 10px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11.5px;
    color: var(--text-dim);
    user-select: none;
  }
  .bar .path {
    font-family: var(--code-font);
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .bar .renamed { font-family: var(--code-font); color: var(--text-faint); font-size: 11px; }
  .bar .gap { flex: 1; }
  .bar .stat { font-family: var(--code-font); font-size: 11px; }
  .bar .stat .a { color: var(--diff-add-fg); font-weight: 500; }
  .bar .stat .d { color: var(--diff-del-fg); font-weight: 500; }
  .bar .sha {
    font-family: var(--code-font);
    font-size: 11px;
    color: var(--accent);
    border: 1px solid var(--border);
    border-radius: 3px;
    padding: 1px 7px;
  }

  .nav { display: inline-flex; align-items: center; gap: 1px; }
  .nav button {
    width: 20px;
    height: 18px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 10px;
    cursor: default;
  }
  .nav button:hover { background: var(--panel-bg-2); color: var(--text); }
  .nav .pos { font-family: var(--code-font); font-size: 10px; color: var(--text-faint); min-width: 30px; text-align: center; }

  .segs { display: inline-flex; border: 1px solid var(--border); border-radius: 3px; overflow: hidden; }
  .segs button {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 11px;
    padding: 2px 8px;
    cursor: default;
    /* 窄窗口下「双栏」会断成两行，把 28px 的工具条撑到 34px 还错位 */
    white-space: nowrap;
  }
  .segs button:disabled { opacity: .45; }
  .segs button:hover { background: var(--panel-bg-2); color: var(--text); }
  .segs button.on { background: var(--accent-sel); color: var(--text); }
  .seg {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 11px;
    padding: 2px 8px;
    cursor: default;
    white-space: nowrap;
  }
  .seg:hover { background: var(--panel-bg-2); color: var(--text); }
  .seg.on { background: var(--accent-sel); color: var(--text); border-color: transparent; }

  .body {
    flex: 1;
    overflow: auto;
    font-family: var(--code-font);
    font-size: 12.5px;
    line-height: 19px;
  }

  /*
   * 双栏用一个 grid 而不是两个并排的滚动容器：
   * 一个容器意味着纵向滚动天生同步，不用写一行同步代码，也不会有
   * 「两边差一帧」的抖动。列宽取 max-content，横向滚动时两栏一起动。
   */
  .grid {
    display: grid;
    /* minmax(max-content, 1fr)：下限是最长那行的宽度（长行不被截断，
       溢出时整个 grid 横向滚动），上限是 1fr（窗口宽时两栏平分、
       背景色铺满，不会在右边留一条没上色的空白） */
    grid-template-columns: 46px minmax(max-content, 1fr) 46px minmax(max-content, 1fr);
    min-width: 100%;
    align-items: stretch;
  }
  .grid > div { white-space: pre; height: 19px; }
  .span4 {
    grid-column: 1 / -1;
    padding-left: 12px;
    background: var(--panel-bg);
    color: var(--text-faint);
    font-size: 11.5px;
    font-style: italic;
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
  }
  .span4.meta {
    font-style: normal;
    color: var(--text);
    background: var(--panel-bg-2);
  }
  .no {
    padding-right: 8px;
    text-align: right;
    color: var(--text-faint);
    font-size: 11px;
    user-select: none;
    /* 行号列吸在左边：横向滚动时仍然知道自己在第几行 */
    position: sticky;
    left: 0;
    background: var(--editor-bg);
  }
  /*
   * 吸住的列**背景必须不透明**，否则横向滚动时正文会从行号底下透出来。
   * 增删行的底色是半透明的（要让它叠在编辑器底色上才是对的颜色），
   * 所以这里把它和一层不透明底色叠起来，而不是另写一组死色值 ——
   * 色值只有一份，改 --diff-*-bg 时不会漏掉这里。
   */
  .no.del { background: linear-gradient(var(--diff-del-bg), var(--diff-del-bg)), var(--editor-bg); }
  .no.add { background: linear-gradient(var(--diff-add-bg), var(--diff-add-bg)), var(--editor-bg); }
  .no.blank { background: var(--editor-bg); }
  .no.mid {
    left: auto;
    position: static;
    border-left: 1px solid var(--border);
  }
  .tx { padding: 0 14px 0 4px; }
  .tx.del { background: var(--diff-del-bg); }
  .tx.add { background: var(--diff-add-bg); }
  /* 对面没有对应行：画成静音的斜纹底，一眼看出「这里本来就没东西」 */
  .tx.blank {
    background: repeating-linear-gradient(
      45deg,
      transparent,
      transparent 5px,
      rgba(255, 255, 255, 0.028) 5px,
      rgba(255, 255, 255, 0.028) 10px
    );
  }
  /*
   * 连着好几行的空白块改成纯色。斜纹的视觉重量是按面积累加的：
   * 一两行合适，五行 import 那么一整块就盖过了旁边真正的代码。
   * 哪些行算「连着好几行」由 diff.ts 的 blankRuns 决定（那边有测试）。
   */
  .tx.blank.flat { background: rgba(255, 255, 255, 0.022); }

  /*
   * 软换行。两件事一起做才有意义：
   *
   * 1. 列宽从 minmax(max-content, 1fr) 换成 1fr —— max-content 的下限就是
   *    「最长那行有多宽」，只要它还在，折不折行都一样会溢出。
   * 2. overflow-wrap: anywhere 兜住没有空格可断的长串（压缩过的一行、
   *    一条长 URL）。只写 pre-wrap 的话它们照样顶出去。
   *
   * 双栏左右对齐是白拿的：一行的 4 个格子在同一个 grid row 里，
   * 行高取两边的较大者，左边折成 3 行右边就跟着长到 3 行。
   * （这也是当初选「一个 grid」而不是「两个并排容器」的红利。）
   */
  .grid.wrap { grid-template-columns: 46px 1fr 46px 1fr; }
  .grid.wrap > div {
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    height: auto;
    min-height: 19px;
  }
  .uni.wrap .row {
    height: auto;
    min-height: 19px;
    /* min-content 会按最长那行撑宽整行，留着就等于没开换行 */
    min-width: 0;
  }
  .uni.wrap .txt { white-space: pre-wrap; overflow-wrap: anywhere; }

  /*
   * 锁死 19px 是为了行距整齐 —— hunk 行带上下边框，不锁就比别的行高 2px，
   * 一屏里几条 hunk 就是几处台阶。box-sizing 是全局 border-box，
   * 所以边框算在这 19px 里面。
   *
   * 这**不再是**跳转的前提。以前「上一处 / 下一处改动」按 下标 × 行高 算，
   * 于是等高变成了一条藏在 CSS 里的隐性契约（改样式的人无从知道自己在动它）；
   * 现在 jump() 直接问 DOM 要位置，软换行开着、行高各不相同也照样对。
   */
  .uni .row {
    display: flex;
    height: 19px;
    white-space: pre;
    min-width: min-content;
  }
  .uni .no {
    flex: none;
    width: 46px;
    position: static;
  }
  .uni .sign { flex: none; width: 14px; text-align: center; user-select: none; }
  .uni .txt { flex: 1; padding-right: 16px; }
  .uni .row.add { background: var(--diff-add-bg); }
  .uni .row.del { background: var(--diff-del-bg); }
  .uni .row.add .sign { color: var(--diff-add-fg); }
  .uni .row.del .sign { color: var(--diff-del-fg); }
  .uni .row.ctx { color: var(--text-dim); }
  .uni .row.hunk {
    background: var(--panel-bg);
    color: var(--text-faint);
    font-size: 11.5px;
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
  }
  .uni .row.hunk .txt { font-style: italic; }
  .uni .row.meta { color: var(--text-faint); font-size: 11px; }

  /* 行内高亮：颜色更实，把真正改动的那几个字挑出来 */
  mark { background: transparent; color: inherit; border-radius: 2px; padding: 0 1px; }
  .tx.del mark, .uni .row.del mark { background: var(--diff-del-strong); color: var(--text); }
  .tx.add mark, .uni .row.add mark { background: var(--diff-add-strong); color: var(--text); }

  .none {
    padding: 20px;
    text-align: center;
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 12.5px;
  }
</style>
