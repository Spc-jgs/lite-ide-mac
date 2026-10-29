<script lang="ts">
  import HideSidebar from "../shell/HideSidebar.svelte";
  /**
   * 结构工具窗（⌘7，docs/TAKEOVER.md G17，IDEA 的 Structure）：当前文件的类、方法、字段一棵缩进的列表，常驻在侧边栏。
   * 和 ⌘F12（浮层，用完就走）是同一份数据：Lezer 的语法树（`file-symbols.ts`），**不等 jdtls** —— 所有有语法树的语言都有，打开文件当场就有。
   *
   * 跟着光标走：光标在哪个方法里，那一项亮着（IDEA 的「Autoscroll from Source」默认开）。点一项跳过去（「Autoscroll to Source」）。
   *
   * 怎么知道文本、光标变了：每 400ms 看一眼当前编辑器（`live-views`）—— 文档对象换了才重新解析（CM6 的文档不可变，没改就是同一个对象），
   * 光标行变了才重算亮哪一项。没走「编辑器每次更新通知我」：那要往每个编辑器里加一个扩展，而这个窗不开的时候它白跑；轮询只在这个窗开着时跑，
   * 一次是两个比较
   */
  import { tabs } from "../state/tabs.svelte";
  import { nav } from "../state/nav.svelte";
  import { viewOf } from "../editor/live-views";
  import type { Sym } from "../editor/outline";
  import type { Text } from "@codemirror/state";

  let { root: _root }: { root: string } = $props();

  let syms = $state<Sym[]>([]);
  let caretLine = $state(0);
  let busy = $state(false);
  let list: HTMLElement | undefined = $state();

  const path = $derived(tabs.active?.mode === "edit" ? tabs.active.path : null);
  const name = $derived(path ? path.slice(path.lastIndexOf("/") + 1) : "");

  $effect(() => {
    const p = path;
    syms = [];
    caretLine = 0;
    if (!p) return;
    let dead = false;
    let lastDoc: Text | null = null;
    const tick = async () => {
      const v = viewOf(p);
      if (!v) return;
      caretLine = v.state.doc.lineAt(v.state.selection.main.head).number;
      if (v.state.doc === lastDoc) return;
      lastDoc = v.state.doc;
      busy = true;
      const { fileSymbols } = await import("../editor/file-symbols");
      const s = await fileSymbols(p, v.state.doc.toString()).catch(() => [] as Sym[]);
      if (dead) return;
      syms = s;
      busy = false;
    };
    void tick();
    const timer = setInterval(() => void tick(), 400);
    return () => {
      dead = true;
      clearInterval(timer);
    };
  });

  /** 光标所在的那一项：行号不超过光标行的最后一项（符号按行号排着，嵌套的在外层后面 —— 最后一个就是最里层的那个） */
  const here = $derived.by(() => {
    let best = -1;
    for (let i = 0; i < syms.length; i++) if (syms[i].line <= caretLine) best = i;
    return best;
  });

  // 亮的那一项滚进视野（只在它变了的时候；用户自己往下翻着看时不拽回来）
  $effect(() => {
    if (here < 0) return;
    queueMicrotask(() => list?.querySelector(".srow.here")?.scrollIntoView({ block: "nearest" }));
  });

  /** 点一项：编辑器跳过去，焦点留在这儿（IDEA 的「Autoscroll to Source」：单击只是带着编辑器走，接着还能点下一项） */
  function go(s: Sym) {
    if (path) nav.goto(s.line);
  }
</script>

<div class="structure">
  <div class="head">
    <span class="title">结构</span>
    {#if name}<span class="file">{name}</span>{/if}
    <HideSidebar />
  </div>
  <div class="body" bind:this={list} aria-label="结构">
    {#if !path}
      <div class="empty-msg">没有打开的文件。打开文件后将在此列出其类、方法和字段</div>
    {:else if syms.length === 0 && !busy}
      <div class="empty-msg">此文件没有可显示的结构<span class="sub">只有带语法树的语言才有（Java、Python、TS、Rust、Go…）</span></div>
    {/if}
    {#each syms as s, i (`${s.line}:${s.kind}:${s.name}`)}
      <button class="srow" class:here={i === here} aria-current={i === here ? "location" : undefined} style:padding-left="{10 + Math.min(s.depth, 8) * 14}px" onclick={() => go(s)} title="第 {s.line} 行">
        <span class="kind">{s.kind}</span>
        <span class="name">{s.name}</span>
      </button>
    {/each}
  </div>
</div>

<style>
  .structure { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  /* 头和文件树、Maven、草稿的一模一样（38 高、大写、灰）：同一个侧边栏里几个视图的头不一样，人会以为是几种东西 */
  .head {
    flex: none;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 10px;
    font-size: var(--fs-sm);
    color: var(--text-dim);
  }
  .title { letter-spacing: 0.06em; text-transform: uppercase; }
  .file { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 0 0 8px; }
  .srow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 12px);
    height: 24px;
    margin: 0 6px;
    padding-right: 8px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    white-space: nowrap;
    cursor: default;
  }
  .srow:hover { background: var(--hover); }
  .srow.here { background: var(--selected); color: var(--text); }
  .kind { flex: none; min-width: 2.4em; font-size: var(--fs-xs); color: var(--text-faint); }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text); }
</style>
