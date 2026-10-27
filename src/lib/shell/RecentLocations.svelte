<script lang="ts">
  /**
   * 最近位置的浮层（⇧⌘E）：每条是「文件名:行 + 那一行代码」，上面一个框按文件名 / 代码筛。↑↓ 选、↵ 跳、Esc 关。
   * 那一行代码：开着的文件从编辑器里拿（没存的改动也算），没开的读盘（只读这几个文件，读不了的那条只显示位置）
   */
  import { imeKey } from "./ime";
  import type { Spot } from "./recent-spots";
  import { viewOf } from "../editor/live-views";
  import { readText } from "../ipc/commands";
  import { nav } from "../state/nav.svelte";
  import { project } from "../state/project.svelte";

  let { spots, onclose }: { spots: Spot[]; onclose: () => void } = $props();

  let q = $state("");
  let sel = $state(0);
  let texts = $state<Record<string, string>>({});
  let input = $state<HTMLInputElement | null>(null);

  const key = (s: Spot) => `${s.path}\n${s.line}`;
  const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const rel = (p: string) => {
    const r = project.root;
    const d = p.slice(0, p.lastIndexOf("/"));
    return r && d.startsWith(r) ? d.slice(r.length + 1) : d;
  };

  $effect(() => {
    input?.focus();
    const want = spots;
    void (async () => {
      const byFile = new Map<string, Spot[]>();
      for (const s of want) byFile.set(s.path, [...(byFile.get(s.path) ?? []), s]);
      for (const [path, ss] of byFile) {
        let lines: string[] | null = null;
        const v = viewOf(path);
        if (v) lines = v.state.doc.toString().split("\n");
        else lines = await readText(path).then((t) => t.content.split("\n")).catch(() => null);
        if (!lines) continue;
        // 只写键、不读整份：`texts = { ...texts, … }` 会在这个 effect 里读到 texts 又写它 —— 开着的文件是同步拿的、读被跟踪了，
        // effect 自己把自己叫醒、转到 effect_update_depth_exceeded（浏览器里撞见的：筛选和 ↵ 跟着全不动了）
        for (const s of ss) texts[key(s)] = (lines[s.line - 1] ?? "").trim();
      }
    })();
  });

  const shown = $derived(
    q.trim()
      ? spots.filter((s) => {
          const w = q.trim().toLowerCase();
          return name(s.path).toLowerCase().includes(w) || (texts[key(s)] ?? "").toLowerCase().includes(w);
        })
      : spots,
  );
  $effect(() => {
    q;
    sel = 0;
  });

  function go(s: Spot | undefined) {
    if (!s) return;
    onclose();
    void nav.openAt(s.path, s.line, false);
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = Math.min(sel + 1, shown.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(sel - 1, 0);
    } else if (e.key === "Enter") {
      e.preventDefault();
      go(shown[sel]);
    }
  }
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup palette m recent" role="dialog" aria-label="最近位置">
  <div class="head">
    <span class="title">最近位置</span>
    <input bind:this={input} bind:value={q} class="q" placeholder="按文件名、代码筛" spellcheck="false" aria-label="筛最近位置" onkeydown={onKey} />
  </div>
  {#if !spots.length}
    <div class="empty-msg">尚无最近位置（通过 ⌘B 跳转、点击搜索结果或切换文件后将显示在此）</div>
  {:else if !shown.length}
    <div class="empty-msg">没有对得上「{q}」的</div>
  {:else}
    <div class="rows" role="listbox">
      {#each shown as s, i (key(s))}
        <button class="row" class:on={i === sel} role="option" aria-selected={i === sel} tabindex="-1" onmousemove={() => (sel = i)} onclick={() => go(s)} title={s.path}>
          <span class="file">{name(s.path)}<span class="ln">:{s.line}</span><span class="dir">{rel(s.path)}</span></span>
          <span class="code">{texts[key(s)] ?? ""}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .scrim { z-index: 59; }
  .recent { z-index: 60; display: flex; flex-direction: column; max-height: min(60vh, 560px); overflow: hidden; }
  .head { display: flex; align-items: center; gap: 10px; padding: 10px 12px 8px; border-bottom: 1px solid var(--border-soft); }
  .title { flex: none; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .q { flex: 1; min-width: 0; height: 26px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font: inherit; outline: none; }
  .q:focus { border-color: var(--accent); }
  .rows { overflow-y: auto; padding: 4px 6px 6px; }
  .row {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
    width: 100%;
    padding: 5px 8px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text-dim);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .row.on { background: var(--selected); }
  .file { display: flex; align-items: baseline; gap: 2px; font-size: var(--fs-sm); color: var(--text); }
  .ln { color: var(--text-faint); }
  .dir { margin-left: 8px; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-xs); }
  .code { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-dim); }
</style>
