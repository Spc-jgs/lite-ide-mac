<script lang="ts">
  import { parseDiff, segs, toSideBySide, changeBlocks, type DiffFile, type DiffLine } from "./diff";

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
  const ROW_H = 19;

  /** 双栏对照是 IDEA 的默认形态，也确实更好读；统一视图留着给窄窗口用 */
  /**
   * 用户**想要**哪种视图。实际显示的是下面的 `sideOn` ——
   * 窄到读不了的时候会被强制成统一视图，但这里记着的意图不变，
   * 窗口一拉宽就自动回到双栏。（直接改 `side` 的话，
   * 拉宽之后回不去，等于把用户的选择偷偷改掉了。）
   */
  let side = $state(true);
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
    // 目标块放到可视区上三分之一处：改动上下的上下文都看得见
    const y = blocks[cur] * ROW_H - box.clientHeight / 3;
    box.scrollTo({ top: Math.max(0, y), behavior: "smooth" });
  }

  function onKey(e: KeyboardEvent) {
    // F7 / ⇧F7 是 IDEA 的「下一处 / 上一处差异」
    if (e.key === "F7") {
      e.preventDefault();
      jump(e.shiftKey ? -1 : 1);
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
      <div class="grid">
        {#each sideShown as r, i (i)}
          {#if r.kind === "hunk" || r.kind === "meta"}
            <div class="span4 {r.kind}">{r.text || "⋯"}</div>
          {:else}
            {@const L = r.left}
            {@const R = r.right}
            <div class="no {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}">{L?.oldNo ?? ""}</div>
            <div class="tx {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}">
              {#if L}{@const s = segs(L)}{s[0]}{#if s[1]}<mark>{s[1]}</mark>{/if}{s[2]}{/if}
            </div>
            <div class="no mid {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}">{R?.newNo ?? ""}</div>
            <div class="tx {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}">
              {#if R}{@const s = segs(R)}{s[0]}{#if s[1]}<mark>{s[1]}</mark>{/if}{s[2]}{/if}
            </div>
          {/if}
        {/each}
      </div>
    {:else}
      <div class="uni">
        {#each uniShown as l, i (i)}
          {#if l.kind === "hunk" || l.kind === "meta"}
            <div class="row {l.kind}">
              <span class="no"></span><span class="no"></span><span class="sign"></span>
              <span class="txt">{l.text || "⋯"}</span>
            </div>
          {:else}
            {@const s = segs(l)}
            <div class="row {l.kind}">
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
   * 每一行必须**正好** ROW_H 高，「上一处 / 下一处改动」是按 下标 × 行高
   * 算滚动位置的。hunk 行带上下边框，不锁死高度就会比别的行高 2px，
   * 跳几次之后目标就偏出屏幕了。box-sizing 是全局 border-box，
   * 所以边框算在这 19px 里面。
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
