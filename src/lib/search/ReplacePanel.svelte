<script lang="ts">
  /**
   * 跨文件替换（#42，⇧⌘R）。设计在 docs/REPLACE.md，这里只是把它画出来：
   * 上面两行是「找什么」「换成什么」，中间按文件分组列出每一处「前文 ~~旧的~~ 新的 后文」，可以逐处 / 按文件勾掉，
   * 底下一个按钮写清「替换 N 处 · M 个文件」。**不加确认框**：做完是一张带「撤销」的卡片（Confirms），
   * 确认框点多了人会条件反射地点「是」，撤销才是真的退路（第 2½ 节调研）。
   *
   * 查询、替换串、勾选都在 `state/replace.svelte.ts` 里：浮层 Esc 关掉，⇧⌘R 再开回到原样。
   * 盘上的事全在 Rust（replacesvc），开着的标签怎么跟上在 `state/replace-ops.ts`。
   */
  import Icon from "../shell/Icon.svelte";
  import FileGlyph from "../shell/FileGlyph.svelte";
  import { overlay } from "../state/overlay.svelte";
  import { replace } from "../state/replace.svelte";
  import * as ops from "../state/replace-ops";
  import { counts, fileState, flatten, hitKey, inlineDiff, toggleFile, visibleRange } from "./replace-model";
  import type { SearchOpts } from "../ipc/search";

  let { open = $bindable() }: { open: boolean } = $props();

  let qInput: HTMLInputElement | undefined = $state();
  let rInput: HTMLInputElement | undefined = $state();
  let listEl: HTMLDivElement | undefined = $state();
  let scrollTop = $state(0);
  let viewH = $state(0);
  /** 行高固定：虚拟滚动靠乘法定位（`visibleRange`）。改这个要连 CSS 里 `.row` 的高一起改 */
  const ROW = 24;

  // 打开时：有词就把光标放进「替换为」（从 ⇧⌘F 带着词切过来，下一步就是打替换串），没词放进搜索框
  $effect(() => {
    if (!open) return;
    queueMicrotask(() => (replace.query ? rInput : qInput)?.focus());
  });

  /*
   * 查询或开关变了：重新扫（debounce 250ms —— 一次完整扫描在 5 万个文件上约 0.45s，每敲一个字扫一次不行）。
   * 每次打开也扫一次：上次关掉之后盘上可能变了，拿旧的扫描结果去执行只会全被判成「预览之后被改过」
   */
  $effect(() => {
    const q = replace.query;
    const o: SearchOpts = { ...overlay.searchOpts };
    if (!open) return;
    void q;
    void o;
    const t = setTimeout(() => void ops.rescan(), 250);
    return () => clearTimeout(t);
  });

  // 替换串变了：只重算「改后」，不重新扫盘（Rust 留着上一次的扫描）
  $effect(() => {
    const r = replace.replacement;
    if (!open || !replace.scan) return;
    void r;
    const t = setTimeout(() => void ops.refreshAfter(), 120);
    return () => clearTimeout(t);
  });

  let rows = $derived(replace.scan ? flatten(replace.scan, replace.collapsed, replace.expanded, replace.after) : []);
  let range = $derived(visibleRange(rows.length, ROW, scrollTop, viewH));
  let n = $derived(replace.scan ? counts(replace.scan, replace.unchecked) : { hits: 0, files: 0 });
  let canRun = $derived(!!replace.scan && !replace.scan.truncated && n.hits > 0 && !replace.scanning && !replace.applying);
  let showSkipped = $state(false);

  const TOGGLE: Record<string, keyof SearchOpts> = { KeyC: "case", KeyW: "word", KeyX: "regex" };
  const TOGGLES: { key: keyof SearchOpts; icon: "match-case" | "whole-word" | "regex"; title: string }[] = [
    { key: "case", icon: "match-case", title: "区分大小写（⌥C）" },
    { key: "word", icon: "whole-word", title: "整词（⌥W）—— 前后都不是字母数字下划线才算；中文字也算字母，一串中文里找不到" },
    { key: "regex", icon: "regex", title: "正则（⌥X）—— 替换串里可以用 $1 引用分组" },
  ];

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      // 「仍然替换？」那一问开着时，Esc 先收它
      if (replace.askNoUndo !== null) replace.askNoUndo = null;
      else open = false;
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      if (canRun) void ops.apply();
    } else if (e.altKey && !e.metaKey && !e.ctrlKey && TOGGLE[e.code]) {
      // 同 ⇧⌘F：认 `code`，⌥C 的 `key` 是 ç
      e.preventDefault();
      const k = TOGGLE[e.code];
      overlay.searchOpts[k] = !overlay.searchOpts[k];
    }
  }

  function toggleHit(rel: string, h: { line: number; col: number }) {
    const k = hitKey(rel, h);
    const next = new Set(replace.unchecked);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    replace.unchecked = next;
  }

  /** 点开 / 收起「跨 N 行」：改前改后两块逐行摊在下面（docs/REPLACE.md 12.3） */
  function toggleExpand(rel: string, h: { line: number; col: number }) {
    const k = hitKey(rel, h);
    const next = new Set(replace.expanded);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    replace.expanded = next;
  }

  function toggleCollapse(rel: string) {
    const next = new Set(replace.collapsed);
    if (next.has(rel)) next.delete(rel);
    else next.add(rel);
    replace.collapsed = next;
  }

  /** 原生复选框的「半勾」只能设属性，不能写成 HTML 属性 */
  function indeterminate(el: HTMLInputElement, on: boolean) {
    el.indeterminate = on;
    return { update: (v: boolean) => (el.indeterminate = v) };
  }

  const dirOf = (rel: string) => (rel.includes("/") ? rel.slice(0, rel.lastIndexOf("/")) : "");
  const nameOf = (rel: string) => rel.slice(rel.lastIndexOf("/") + 1);
  const mb = (b: number) => (b / 1048576).toFixed(1);
</script>

{#if open}
  <div class="scrim dim" onclick={() => (open = false)} role="presentation"></div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="popup" role="dialog" aria-modal="true" aria-label="在项目中替换" tabindex="-1" onkeydown={onKey}>
    <div class="q">
      <span class="qic"><Icon name="search" /></span>
      <input bind:this={qInput} bind:value={replace.query} placeholder="在项目中查找…" spellcheck="false" autocomplete="off" aria-label="查找" />
      <span class="toggles">
        {#each TOGGLES as t (t.key)}
          <button
            class="ibtn"
            class:on={overlay.searchOpts[t.key]}
            title={t.title}
            aria-label={t.title}
            aria-pressed={overlay.searchOpts[t.key]}
            onclick={() => (overlay.searchOpts[t.key] = !overlay.searchOpts[t.key])}
          ><Icon name={t.icon} /></button>
        {/each}
      </span>
    </div>
    <div class="q r">
      <span class="qic"><Icon name="swap" /></span>
      <input bind:this={rInput} bind:value={replace.replacement} placeholder="替换为…（可以是空的）" spellcheck="false" autocomplete="off" aria-label="替换为" />
      <button class="btn primary sm" class:busy={replace.applying} disabled={!canRun} onclick={() => void ops.apply()} title="替换勾选的那些（⌘↵）">
        {n.hits ? `替换 ${n.hits} 处 · ${n.files} 个文件` : "替换"}
      </button>
    </div>

    <div class="info">
      {#if replace.scanning}<span class="wait"><span class="spinner sm"></span>正在找…</span>
      {:else if replace.error}<span class="err">{replace.error}</span>
      {:else if replace.scan}
        <span>{replace.scan.total} 处 · {replace.scan.files.length} 个文件</span>
        {#if replace.scan.truncated}<span class="warn">命中超过 5000 处，只列了前面的 —— 缩小范围再替换（不替换一半）</span>{/if}
        {#if replace.scan.indexTruncated}<span class="warn">项目超过 5 万个文件，之后的没看</span>{/if}
        {#if replace.scan.skipped.length}
          <button class="btn quiet sm" onclick={() => (showSkipped = !showSkipped)}>跳过 {replace.scan.skipped.length} 个{showSkipped ? " ▴" : " ▾"}</button>
        {/if}
        {#if replace.scan.binary}<span class="dim">另有 {replace.scan.binary} 个二进制文件没看</span>{/if}
      {:else if !replace.query}<span class="dim">输入要找的词；替换只改勾着的那些，做完能撤销</span>{/if}
    </div>
    {#if showSkipped && replace.scan?.skipped.length}
      <ul class="skipped">
        {#each replace.scan.skipped as k (k.rel)}<li><b>{k.rel}</b> —— {k.text}</li>{/each}
      </ul>
    {/if}

    <div class="list" bind:this={listEl} bind:clientHeight={viewH} onscroll={() => (scrollTop = listEl?.scrollTop ?? 0)}>
      {#if replace.scan && rows.length === 0 && !replace.scanning}
        <div class="none">没有找到 —— 换个词，或者看看上面三个开关</div>
      {/if}
      <div class="inner" style:height="{rows.length * ROW}px">
        {#each rows.slice(range[0], range[1]) as row, k (row.kind === "file" ? `f${row.fi}` : row.kind === "hit" ? `h${row.fi}:${row.hi}` : `b${range[0] + k}`)}
          {@const f = replace.scan!.files[row.fi]}
          {#if row.kind === "file"}
            {@const st = fileState(f, replace.unchecked)}
            <div class="row file" style:top="{(range[0] + k) * ROW}px">
              <button class="ibtn sm caret" class:open={!replace.collapsed.has(f.rel)} onclick={() => toggleCollapse(f.rel)} aria-label="展开或折叠 {f.rel}">
                <Icon name="chevron-right" />
              </button>
              <input
                type="checkbox"
                checked={st === "all"}
                use:indeterminate={st === "some"}
                onchange={() => (replace.unchecked = toggleFile(f, replace.unchecked))}
                aria-label="勾选 {f.rel} 里的全部"
              />
              <FileGlyph name={nameOf(f.rel)} />
              <span class="fname">{nameOf(f.rel)}</span>
              {#if f.editor}<span class="tag" title="这个文件开着、有没存的改动：替换只改编辑器里那份，不存盘">未保存</span>{/if}
              <span class="side">{dirOf(f.rel)}</span>
              <span class="cnt">{f.hits.length}</span>
            </div>
          {:else if row.kind === "block"}
            <!-- 跨行命中点开之后的片段：改前（-）/ 改后（+）各一块，一行一行，像 diff 那样 -->
            <div class="row blk" class:minus={row.side === "-"} style:top="{(range[0] + k) * ROW}px">
              <span class="sign">{row.side}</span>
              <span class="code">{row.text}</span>
            </div>
          {:else}
            {@const h = f.hits[row.hi]}
            {@const d = inlineDiff(h, replace.after[row.fi]?.[row.hi])}
            <label class="row hit" class:off={replace.unchecked.has(hitKey(f.rel, h))} style:top="{(range[0] + k) * ROW}px">
              <input type="checkbox" checked={!replace.unchecked.has(hitKey(f.rel, h))} onchange={() => toggleHit(f.rel, h)} />
              <span class="ln">{h.line}</span>
              <span class="code">{d.pre}<del>{d.old}</del>{#if d.neu !== null}<ins>{d.neu}</ins>{/if}{d.post}</span>
              {#if h.lines > 1}
                <!-- 跨行的命中在列表里仍然只占一行（不让一个大块把列表撑乱），点这个看整块 -->
                <button
                  class="btn quiet sm multi"
                  onclick={(e) => {
                    e.preventDefault();
                    toggleExpand(f.rel, h);
                  }}
                >跨 {h.lines} 行 {replace.expanded.has(hitKey(f.rel, h)) ? "▴" : "▾"}</button>
              {/if}
            </label>
          {/if}
        {/each}
      </div>
    </div>

    <div class="foot">
      {#if replace.askNoUndo !== null}
        <span class="ask">这次要改的文件合起来 {mb(replace.askNoUndo)} MB，超过替换日志的上限 —— <b>撤销和中断恢复都做不了</b></span>
        <span class="gap"></span>
        <button class="btn sm" onclick={() => (replace.askNoUndo = null)}>取消</button>
        <button class="btn sm danger" onclick={() => void ops.apply(true)}>仍然替换</button>
      {:else}
        <span><kbd>⌘↵</kbd> 替换</span>
        <span><kbd>⌥C</kbd> <kbd>⌥W</kbd> <kbd>⌥X</kbd> 开关</span>
        <span class="gap"></span>
        <span><kbd>esc</kbd> 关闭（查询和勾选都留着）</span>
      {/if}
    </div>
  </div>
{/if}

<style>
  .popup {
    position: fixed;
    top: 10vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(820px, 92vw);
    height: min(640px, 78vh);
    display: flex;
    flex-direction: column;
    /* 面（底、边、圆角、投影、淡入）在 app.css 的 `.popup`，这里只管位置和尺寸 */
    z-index: 41;
    overflow: hidden;
    outline: none;
  }
  .q { flex: none; display: flex; align-items: center; gap: 10px; padding: 0 16px; }
  .q.r { border-top: 1px solid var(--border-soft); }
  .qic { flex: none; display: flex; color: var(--text-faint); }
  input:not([type]) {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-family: var(--code-font);
    font-size: var(--fs-md);
    padding: 11px 0;
    outline: none;
  }
  input::placeholder { color: var(--text-faint); font-family: var(--ui-font); }
  .toggles { flex: none; display: flex; gap: 2px; }

  .info {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: 30px;
    padding: 0 16px;
    border-top: 1px solid var(--border-soft);
    border-bottom: 1px solid var(--border-soft);
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }
  .info .wait { display: inline-flex; align-items: center; gap: 6px; }
  .info .err { color: var(--lvl-error); }
  .info .warn { color: var(--lvl-warn); }
  .info .dim, .dim { color: var(--text-faint); }
  .skipped {
    flex: none;
    max-height: 120px;
    overflow: auto;
    margin: 0;
    padding: 6px 16px 6px 32px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
    border-bottom: 1px solid var(--border-soft);
  }

  .list { flex: 1; min-height: 0; overflow: auto; position: relative; }
  .inner { position: relative; }
  .none { padding: 18px 16px; color: var(--text-faint); font-size: var(--fs-md); }
  .row {
    position: absolute;
    left: 6px;
    right: 6px;
    height: 24px;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 8px;
    border-radius: var(--r-sm);
    font-size: var(--fs-md);
    white-space: nowrap;
  }
  .row:hover { background: var(--hover); }
  .row.file { color: var(--text); }
  .row.hit { padding-left: 46px; cursor: default; }
  .row.hit.off .code { opacity: 0.45; }
  .caret :global(.icon) { transition: transform 90ms; }
  .caret.open :global(.icon) { transform: rotate(90deg); }
  .fname { flex: none; }
  .tag { flex: none; font-size: var(--fs-xs); color: var(--lvl-warn); }
  .side { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; direction: rtl; text-align: left; color: var(--text-faint); font-size: var(--fs-sm); }
  .cnt { flex: none; color: var(--text-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .ln { flex: none; width: 36px; text-align: right; color: var(--text-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .code { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-dim); }
  del { color: var(--lvl-error); text-decoration: line-through; background: color-mix(in srgb, var(--lvl-error) 14%, transparent); }
  ins { color: var(--text); text-decoration: none; background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .multi { flex: none; }
  .row.blk { padding-left: 64px; }
  .row.blk .sign { flex: none; width: 10px; font-family: var(--code-font); color: color-mix(in srgb, var(--accent) 80%, var(--text)); }
  .row.blk.minus .sign { color: var(--lvl-error); }
  .row.blk .code { white-space: pre; color: var(--text); }
  .row.blk.minus .code { color: var(--text-dim); }
  .row.blk:hover { background: transparent; }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 7px 16px;
    font-size: var(--fs-xs);
    color: var(--text-faint);
    background: var(--chrome-scrim);
  }
  .foot .gap { flex: 1; }
  .foot .ask { font-size: var(--fs-sm); color: var(--text); }
</style>
