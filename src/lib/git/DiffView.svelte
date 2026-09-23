<script lang="ts">
  import { splitHunks, hunkPatch, pickLines } from "./hunks";
  import Icon from "../shell/Icon.svelte";
  import { highlightDiff, type Tok } from "./diff-highlight";
  import {
    parseDiff,
    linePieces,
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
    toLocal = false,
    untracked = false,
    capped = false,
    onToggleStaged,
    onApplyHunk,
    onRevertHunk,
  }: {
    raw: string;
    path: string;
    /** 当前看的是暂存区还是工作区 */
    staged: boolean;
    /** 非空表示这是某次提交里的差异（只读历史），此时没有暂存/未暂存之分 */
    commit?: string;
    /** 和 `commit` 搭配（issue #39）：比的是「那次提交 → 现在的工作区」。右侧是活的，但块动作照样不给：patch 的基线是那次提交的 blob，不是暂存区，`apply --cached` 对不上 */
    toLocal?: boolean;
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
    /**
     * 按块暂存（issue #33 ⑫）：这一块的 patch 交给上层 `git apply --cached`。
     * `unstage` 跟着 `staged` 走 —— 看的是暂存区那侧，按下去就是从暂存区撤掉。
     * 历史提交、未跟踪文件没有这回事，不传就不画按钮。
     */
    onApplyHunk?: (patch: string, unstage: boolean) => void;
    /**
     * 撤销一块（issue #38）：只在工作区那一侧有 —— 暂存区那侧的「撤」就是取消暂存。
     * 这里只报「要撤哪块」，确认条在上层（不可逆，和「全部丢弃」同一档）。
     */
    onRevertHunk?: (patch: string) => void;
  } = $props();

  /** 每个 hunk 的原文，给按块暂存拼 patch 用。从 `raw` 拆，不从解析后的行反拼 */
  let patches = $derived(splitHunks(raw));
  let canApply = $derived(!!onApplyHunk && !commit && !untracked && !capped && patches.hunks.length > 0);
  let canRevert = $derived(!!onRevertHunk && !staged && !commit && !untracked && !capped && patches.hunks.length > 0);
  /**
   * 按行暂存（issue #38 后半）：选中几行，只暂存 / 取消暂存那几行。
   *
   * 选中集按 `DiffLine` **对象**认 —— 双栏和统一视图渲染的是同一批对象（`toSideBySide`
   * 只是把它们配对，不复制），所以切视图选中不丢。`bodyIdx` 把每个对象映射到「第几块、
   * 块正文里第几行」，拼 patch 时交给 `pickLines`。**只能选同一块里的行**：patch 是按块拼的，
   * 跨块选中就得拼多块，而人一次想暂存的几行几乎总在一处。
   * 只有改动行能选；点上下文行没反应。⇧点选范围（同一块里 anchor 到这行之间的改动行），
   * 再点已选的取消。正在拖选文字（选区非空）时不当成点选 —— 复制一行代码不该顺手选中它。
   */
  let bodyIdx = $derived.by(() => {
    const m = new Map<DiffLine, { hunk: number; idx: number }>();
    let hunk = -1;
    let idx = 0;
    for (const l of files[0]?.lines ?? []) {
      if (l.kind === "hunk") {
        hunk++;
        idx = 0;
        continue;
      }
      if (hunk >= 0) m.set(l, { hunk, idx: idx++ });
    }
    return m;
  });
  let sel = $state<Set<DiffLine>>(new Set());
  let selHunk = $state(-1);
  let anchor: DiffLine | null = null;
  // 差异重拉（暂存完 refresh）就清掉：那些对象已经不是屏幕上的行了
  $effect(() => {
    raw;
    sel = new Set();
    selHunk = -1;
    anchor = null;
  });
  const isChange = (l: DiffLine | null): l is DiffLine => !!l && (l.kind === "add" || l.kind === "del");
  /**
   * ⇧按下时拦掉浏览器的默认动作 —— 它是「把文字选区从上次点的地方扩到这里」，click 事件
   * 到达时选区已经非空，下面那条「正在拖选文字就不算点选」的守卫会把 ⇧点选范围一起挡掉
   * （review 2026-09-20：合成事件没有 mousedown，所以之前的验证没撞上）。
   */
  const noShiftSel = (e: MouseEvent) => {
    if (e.shiftKey) e.preventDefault();
  };
  function pickRow(lines: (DiffLine | null)[], e: MouseEvent) {
    if (!canApply) return;
    if (!(window.getSelection()?.isCollapsed ?? true)) return;
    const chg = lines.filter(isChange);
    if (chg.length === 0) return;
    const h = bodyIdx.get(chg[0])?.hunk ?? -1;
    if (h < 0) return;
    const next = h === selHunk ? new Set(sel) : new Set<DiffLine>();
    const a = anchor ? bodyIdx.get(anchor) : undefined;
    if (e.shiftKey && a && h === selHunk) {
      const idxs = chg.map((l) => bodyIdx.get(l)!.idx);
      const lo = Math.min(a.idx, ...idxs);
      const hi = Math.max(a.idx, ...idxs);
      for (const [l, p] of bodyIdx) if (p.hunk === h && p.idx >= lo && p.idx <= hi && isChange(l)) next.add(l);
    } else {
      const allIn = chg.every((l) => next.has(l));
      for (const l of chg) if (allIn) next.delete(l); else next.add(l);
      anchor = chg[0];
    }
    sel = next;
    selHunk = next.size > 0 ? h : -1;
  }
  function applySel() {
    if (selHunk < 0 || !patches.header) return;
    const keep = new Set([...sel].map((l) => bodyIdx.get(l)!.idx));
    // 取消暂存走 `apply -R`，基线是暂存区 = 新侧，pickLines 的规则要对调
    const body = pickLines(patches.hunks[selHunk] ?? "", keep, staged);
    if (!body) return;
    onApplyHunk?.(patches.header + body, staged);
    // 交出去就清掉：refresh 之后这些对象就不是屏幕上的行了；失败了人也该重新选
    sel = new Set();
    selHunk = -1;
  }

  /** 第 i 行之前有几个 hunk 行 = 这一行是第几块（0-based） */
  function hunkOrdinals(rows: { kind: string }[]): number[] {
    const out = new Array<number>(rows.length);
    let k = -1;
    for (let i = 0; i < rows.length; i++) {
      if (rows[i].kind === "hunk") k++;
      out[i] = k;
    }
    return out;
  }

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
  let box = $state<HTMLElement | null>(null);
  let cur = $state(-1);

  let files = $derived<DiffFile[]>(parseDiff(raw));
  /*
   * **单边差异**：新增或删除整个文件，只有一侧有内容。
   *
   * 这种 diff 用双栏对照是自找难看：左边整栏空着（一大片斜纹）、右边整片绿，中间那条
   * 分栏线把屏幕劈成「一半废的 + 一半有用的」，还多一列永远空着的行号。IDEA 对新文件
   * 根本不给双栏。所以这里**强制统一视图**，「双栏」那段灰掉并说明原因；统一视图里
   * 把那列空行号也去掉（46px 的空列），只剩一条 hunk 的话那条块头也不画 ——
   * 「从第 1 行开始，共 70 行」对一个全新文件不是信息。
   */
  let oneSided = $derived.by<"add" | "del" | null>(() => {
    if (files.length !== 1) return null;
    const f = files[0];
    if (f.isNew || untracked) return "add";
    if (f.isDeleted) return "del";
    return null;
  });
  /** 真正在显示的形态 */
  let sideOn = $derived(side && !narrow && !oneSided);
  /*
   * 语法着色（`diff-highlight.ts`）。异步：语言包要按需加载、解析要时间，先按平的画，
   * 算好了再上色 —— 一份 diff 通常几十毫秒，肉眼看不到那一下。`dead` 同 LogPane 那条：
   * raw 换了，上一趟还在飞的结果不能盖回来。多文件的 diff 各文件各算（路径不同语言不同）。
   */
  let hl = $state<Map<DiffLine, Tok[]> | null>(null);
  $effect(() => {
    const fs = files;
    hl = null;
    let dead = false;
    void (async () => {
      const merged = new Map<DiffLine, Tok[]>();
      for (const f of fs) {
        const m = await highlightDiff(f.lines, f.path).catch(() => null);
        if (dead) return;
        if (m) for (const [k, v] of m) merged.set(k, v);
      }
      if (!dead && merged.size) hl = merged;
    })();
    return () => {
      dead = true;
    };
  });
  const pieces = (l: DiffLine) => linePieces(l, hl?.get(l) ?? null);
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
  let sideHunk = $derived(hunkOrdinals(sideShown));
  let uniHunk = $derived(hunkOrdinals(uniShown));
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
    // Esc 清掉按行暂存的选中
    if (e.key === "Escape" && sel.size > 0) {
      e.preventDefault();
      sel = new Set();
      selHunk = -1;
      return;
    }
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

<!-- 一行代码：语法段 × 行内改动区间。平的段直接印文本，少一层 span -->
{#snippet code(l: DiffLine)}
  {#each pieces(l) as p}
    {#if p.hit}<mark class={p.cls || undefined}>{p.t}</mark>{:else if p.cls}<span class={p.cls}>{p.t}</span>{:else}{p.t}{/if}
  {/each}
{/snippet}

<div class="diff" bind:clientWidth={boxW}>
  <div class="bar">
    <span class="path" title={path}>{path}</span>
    {#if files[0]?.oldPath}
      <span class="renamed">← {files[0].oldPath}</span>
    {/if}
    <!-- 整个文件新增 / 删除是这份 diff 最要紧的一句话，比 `+70 −0` 好读 -->
    {#if oneSided === "add"}
      <span class="tag add">新增的文件</span>
    {:else if oneSided === "del"}
      <span class="tag del">删除的文件</span>
    {/if}
    <span class="gap"></span>
    <span class="stat"><b class="a">+{adds}</b> <b class="d">−{dels}</b></span>

    {#if blocks.length > 0}
      <span class="nav">
        <button class="ibtn sm" onclick={() => jump(-1)} title="上一处改动 ⇧F7" aria-label="上一处改动"><Icon name="chevron-up" size={11} /></button>
        <!-- 还没跳过时说「3 处」，跳过才说「1/3」—— 「—/3」读不出是什么 -->
        <span class="pos">{cur < 0 ? `${blocks.length} 处` : `${cur + 1}/${blocks.length}`}</span>
        <button class="ibtn sm" onclick={() => jump(1)} title="下一处改动 F7" aria-label="下一处改动"><Icon name="chevron-down" size={11} /></button>
      </span>
    {/if}

    <span class="segs">
      <button
        class="btn sm"
        class:on={sideOn}
        disabled={narrow || !!oneSided}
        onclick={() => (side = true)}
        title={oneSided === "add"
          ? "整个文件是新增的，没有旧版可对照"
          : oneSided === "del"
            ? "整个文件被删了，没有新版可对照"
            : narrow
              ? "窗口太窄，双栏每列放不下一行代码；拉宽就会自动切回来"
              : "左右分栏对照"}
      >双栏</button>
      <button class="btn sm" class:on={!sideOn} onclick={() => (side = false)} title="统一视图（窄窗口更合适）">统一</button>
    </span>

    <button
      class="btn sm"
      class:on={wrap}
      onclick={() => (wrap = !wrap)}
      title="软换行 ⌥Z —— 长行折进列宽，不用横向滚"
      aria-pressed={wrap}
    >换行</button>

    {#if commit}
      <span class="sha" title={toLocal ? "那次提交到现在工作区的差异（中间的提交和没提交的都算），只读" : "这是历史提交里的差异，只读"}>
        {toLocal ? `${commit} → 本地` : commit}
      </span>
    {:else}
      <button class="btn sm" onclick={onToggleStaged} title="在「已暂存 ↔ 未暂存」之间切换">
        {staged ? "已暂存的改动" : "未暂存的改动"} <Icon name="swap" size={11} />
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
            <div class="span4 {r.kind}" data-row={i}>
              {#if r.kind === "hunk"}<span class="fold" aria-hidden="true"><Icon name="more-h" size={12} /></span>{/if}
              <span class="htxt">{r.text}</span>
              {#if r.kind === "hunk" && selHunk === sideHunk[i]}
                <button class="btn sm primary hsel" onclick={applySel} title="只暂存选中的那几行（Esc 取消选中）">
                  {staged ? "取消暂存" : "暂存"}选中的 {sel.size} 行
                </button>
              {/if}
              {#if r.kind === "hunk" && canApply}
                <button class="btn sm hbtn" onclick={() => onApplyHunk?.(hunkPatch(patches, sideHunk[i]), staged)}>
                  {staged ? "取消暂存这一块" : "暂存这一块"}
                </button>
              {/if}
              {#if r.kind === "hunk" && canRevert}
                <button class="btn sm danger hbtn" onclick={() => onRevertHunk?.(hunkPatch(patches, sideHunk[i]))}>撤销这一块</button>
              {/if}
            </div>
          {:else}
            {@const L = r.left}
            {@const R = r.right}
            {@const on = r.kind === "change" && ((!!L && sel.has(L)) || (!!R && sel.has(R)))}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="no {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}" class:sel={on} data-row={i} onmousedown={noShiftSel} onclick={(e) => pickRow([L, R], e)}>{L?.oldNo ?? ""}</div>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="tx {L ? (L.kind === 'del' ? 'del' : '') : 'blank'}" class:flat={flat.left[i]} class:sel={on} onmousedown={noShiftSel} onclick={(e) => pickRow([L, R], e)}>
              {#if L}{@render code(L)}{/if}
            </div>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="no mid {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}" class:sel={on} onmousedown={noShiftSel} onclick={(e) => pickRow([L, R], e)}>{R?.newNo ?? ""}</div>
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="tx {R ? (R.kind === 'add' ? 'add' : '') : 'blank'}" class:flat={flat.right[i]} class:sel={on} onmousedown={noShiftSel} onclick={(e) => pickRow([L, R], e)}>
              {#if R}{@render code(R)}{/if}
            </div>
          {/if}
        {/each}
      </div>
    {:else}
      <div class="uni" class:wrap>
        {#each uniShown as l, i (i)}
          {#if l.kind === "hunk" && oneSided && patches.hunks.length <= 1}
            <!-- 单边差异只有一块：那条「从第 1 行开始」的块头不是信息（跳转不指向块头，去掉安全） -->
          {:else if l.kind === "hunk" || l.kind === "meta"}
            <div class="row {l.kind}" data-row={i}>
              {#if oneSided !== "add"}<span class="no"></span>{/if}
              {#if oneSided !== "del"}<span class="no"></span>{/if}
              <span class="sign fold" aria-hidden="true">{#if l.kind === "hunk"}<Icon name="more-h" size={12} />{/if}</span>
              <span class="txt">{l.text}</span>
              {#if l.kind === "hunk" && selHunk === uniHunk[i]}
                <button class="btn sm primary hsel" onclick={applySel} title="只暂存选中的那几行（Esc 取消选中）">
                  {staged ? "取消暂存" : "暂存"}选中的 {sel.size} 行
                </button>
              {/if}
              {#if l.kind === "hunk" && canApply}
                <button class="btn sm hbtn" onclick={() => onApplyHunk?.(hunkPatch(patches, uniHunk[i]), staged)}>
                  {staged ? "取消暂存这一块" : "暂存这一块"}
                </button>
              {/if}
              {#if l.kind === "hunk" && canRevert}
                <button class="btn sm danger hbtn" onclick={() => onRevertHunk?.(hunkPatch(patches, uniHunk[i]))}>撤销这一块</button>
              {/if}
            </div>
          {:else}
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
            <div class="row {l.kind}" class:sel={sel.has(l)} data-row={i} onmousedown={noShiftSel} onclick={(e) => pickRow([l], e)}>
              {#if oneSided !== "add"}<span class="no">{l.oldNo ?? ""}</span>{/if}
              {#if oneSided !== "del"}<span class="no">{l.newNo ?? ""}</span>{/if}
              <span class="sign">{l.kind === "add" ? "+" : l.kind === "del" ? "−" : ""}</span>
              <span class="txt">{@render code(l)}</span>
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
    height: 34px; /* M9：岛内工具栏统一 34（原来 28） */
    padding: 0 10px;
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    border-bottom: 1px solid var(--border-soft);
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
  .bar .stat { font-family: var(--code-font); font-size: 11px; white-space: nowrap; }
  .bar .stat .a { color: var(--diff-add-fg); font-weight: 500; }
  .bar .stat .d { color: var(--diff-del-fg); font-weight: 500; }
  .bar .tag {
    flex: none;
    padding: 1px 7px;
    border-radius: var(--r-sm);
    font-size: 11px;
    font-family: var(--ui-font);
  }
  .bar .tag.add { background: var(--diff-add-strong); color: var(--text); }
  .bar .tag.del { background: var(--diff-del-strong); color: var(--text); }
  .bar .sha {
    font-family: var(--code-font);
    font-size: 11px;
    color: var(--accent);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    padding: 1px 7px;
  }

  .nav { display: inline-flex; align-items: center; gap: 1px; }
  .nav .pos { font-family: var(--code-font); font-size: 10px; color: var(--text-faint); min-width: 30px; text-align: center; }

  /* 分段控件是 app.css 的 `.segs`；「换行」是独立开关，`on` 同一套长相 */
  .bar .btn.on { background: var(--accent-sel); border-color: var(--accent); color: var(--text); }

  /* 字号 / 行高同编辑器（13px × 1.55 ≈ 20）：一个文件在编辑器和差异里切来切去，字不该忽大忽小 */
  .body {
    flex: 1;
    overflow: auto;
    font-family: var(--code-font);
    font-size: 13px;
    line-height: 20px;
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
  .grid > div { white-space: pre; height: 20px; }
  /*
   * 块头：一条 `--hover` 的带子，**不画上下边线**（岛里不用线分项）。左边一个 ⋯ 说「这里折着
   * 没显示的行」，后面跟 git 报的函数上下文（斜体，它是注释性质的）。
   */
  .span4 {
    grid-column: 1 / -1;
    padding-left: 12px;
    background: var(--hover);
    color: var(--text-faint);
    font-size: 11.5px;
    font-style: italic;
  }
  .span4.meta {
    font-style: normal;
    color: var(--text);
    background: var(--selected);
  }
  .no {
    padding-right: 10px;
    text-align: right;
    color: var(--gutter-fg);
    font-size: 11.5px;
    user-select: none;
    /* 行号列吸在左边：横向滚动时仍然知道自己在第几行 */
    position: sticky;
    left: 0;
    background: var(--content-solid);
    /* 同编辑器的行号栏：右边一条 soft 线把行号和正文分开 */
    box-shadow: inset -1px 0 0 var(--border-soft);
  }
  /*
   * 吸住的列**背景必须不透明**，否则横向滚动时正文会从行号底下透出来。
   * 增删行的底色是半透明的（要让它叠在编辑器底色上才是对的颜色），
   * 所以这里把它和一层不透明底色叠起来，而不是另写一组死色值 ——
   * 色值只有一份，改 --diff-*-bg 时不会漏掉这里。
   */
  .no.del { background: linear-gradient(var(--diff-del-bg), var(--diff-del-bg)), var(--content-solid); }
  .no.add { background: linear-gradient(var(--diff-add-bg), var(--diff-add-bg)), var(--content-solid); }
  .no.blank { background: var(--content-solid); }
  /* 右栏的行号：也是分栏线 —— 左边一条 soft 线（左栏正文 → 右栏行号），不用 9% 的 */
  .no.mid {
    left: auto;
    position: static;
    box-shadow: inset 1px 0 0 var(--border-soft), inset -1px 0 0 var(--border-soft);
  }
  .tx { padding: 0 14px 0 8px; }
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
    min-height: 20px;
  }
  .uni.wrap .row {
    height: auto;
    min-height: 20px;
    /* min-content 会按最长那行撑宽整行，留着就等于没开换行 */
    min-width: 0;
  }
  .uni.wrap .txt { white-space: pre-wrap; overflow-wrap: anywhere; }

  /*
   * 锁死 20px 是为了行距整齐（同编辑器的行高）。
   *
   * 这**不再是**跳转的前提。以前「上一处 / 下一处改动」按 下标 × 行高 算，
   * 于是等高变成了一条藏在 CSS 里的隐性契约（改样式的人无从知道自己在动它）；
   * 现在 jump() 直接问 DOM 要位置，软换行开着、行高各不相同也照样对。
   */
  .uni .row {
    display: flex;
    height: 20px;
    white-space: pre;
    min-width: min-content;
  }
  .uni .no {
    flex: none;
    width: 46px;
    position: static;
    box-shadow: none;
  }
  /* 统一视图两列行号是一个栏：线画在最后一列右边（单边差异只有一列，那就是它自己） */
  .uni .no:last-of-type { box-shadow: inset -1px 0 0 var(--border-soft); }
  .uni .sign { flex: none; width: 18px; text-align: center; user-select: none; }
  .uni .txt { flex: 1; padding-right: 16px; }
  .uni .row.add { background: var(--diff-add-bg); }
  .uni .row.del { background: var(--diff-del-bg); }
  .uni .row.add .sign { color: var(--diff-add-fg); }
  .uni .row.del .sign { color: var(--diff-del-fg); }
  .uni .row.ctx { color: var(--text-dim); }
  .uni .row.hunk {
    background: var(--hover);
    color: var(--text-faint);
    font-size: 11.5px;
  }
  .uni .row.hunk .no, .uni .row.hunk .no + .no { background: transparent; box-shadow: none; }
  .uni .row.hunk .txt { font-style: italic; }
  /* 按块暂存的按钮：hover 那一行才出（ui.md 第三条：每一块上都有的东西不常驻） */
  /* 是 `.btn.sm`；这里只管位置和「hover 那一行才出」 */
  .hbtn { margin-left: auto; margin-right: 8px; font-style: normal; opacity: 0; align-self: center; }
  /*
   * 第二个按钮（撤销这一块，issue #38）：不可逆的不许和可逆的并排同色（ui.md 第七条）——
   * 带 `danger`，而且和「暂存这一块」之间留 10px，误点的代价不该只隔着 2px。
   * `margin-left: auto` 只给第一个：两个都 auto 的话剩余空间被平分，第一个会被顶到行中间
   */
  .hbtn + .hbtn { margin-left: 10px; }
  /* 按行暂存的按钮：选中集非空时常驻（它是选中这个状态的唯一出口，藏起来人不知道下一步是什么） */
  .hsel { margin-left: auto; margin-right: 8px; align-self: center; font-style: normal; }
  /*
   * 块头上的按钮吸在**可视区**右边。块头横跨整个网格，而长行会把网格撑得比可视区宽
   * （实测 1152 > 736），`margin-left: auto` 把按钮推到网格最右 —— 在屏幕外。
   * sticky 的参照是滚动容器，横向滚到哪儿按钮都贴着右边
   */
  .hbtn, .hsel { position: sticky; right: 8px; }
  .hsel ~ .hbtn { margin-left: 10px; }
  /*
   * 选中的行：左边一道 accent 竖条 + 把底色提亮一档。不用 --selected 盖上去：
   * 它是白色叠加，会把 add / del 的红绿底洗成灰的，选中之后反而看不出是加还是删
   */
  .uni .row.sel { box-shadow: inset 3px 0 0 var(--accent); filter: brightness(1.35); }
  .grid .sel { filter: brightness(1.35); }
  .grid .no.sel[data-row] { box-shadow: inset 3px 0 0 var(--accent); }
  /* 块头比普通行高 4px：装得下 20 的 `.btn.sm` 还有呼吸位。它本来就是分隔，高一点反而更像分隔 */
  .uni .row.hunk, .span4.hunk { min-height: 24px; }
  .row.hunk:hover .hbtn, .span4.hunk:hover .hbtn, .hbtn:focus-visible { opacity: 1; }
  .span4.hunk { display: flex; align-items: center; gap: 6px; }
  /* 折叠标记：⋯ 图标，说「这上面有没显示的行」 */
  .fold { display: inline-flex; align-items: center; justify-content: center; color: var(--text-faint); }
  .span4.hunk .htxt { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .uni .row.meta { color: var(--text-faint); font-size: 11px; }

  /* 行内高亮：颜色更实，把真正改动的那几个字挑出来 */
  mark { background: transparent; color: inherit; border-radius: var(--r-sm); padding: 0 1px; }
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
