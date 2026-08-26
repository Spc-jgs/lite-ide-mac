<script lang="ts">
  import type { Sym } from "../editor/outline";
  import { rank, segments } from "./fuzzy";

  let {
    open = $bindable(),
    symbols,
    fileName,
    supported,
    onPick,
  }: {
    open: boolean;
    symbols: Sym[];
    fileName: string;
    /** 当前语言有没有语法树。没有就明说，不假装 */
    supported: boolean;
    onPick: (line: number) => void;
  } = $props();

  let query = $state("");
  let cursor = $state(0);
  let input: HTMLInputElement | undefined = $state();

  $effect(() => {
    if (!open) return;
    query = "";
    cursor = 0;
    queueMicrotask(() => input?.focus());
  });

  let rows = $derived.by(() => {
    if (!query) return symbols.map((s) => ({ sym: s, seg: [{ t: s.name, hit: false }] }));
    return rank(symbols, query, (s) => s.name, 200).map((r) => ({
      sym: r.item,
      seg: segments(r.item.name, r.positions),
    }));
  });

  $effect(() => {
    if (cursor >= rows.length) cursor = Math.max(0, rows.length - 1);
  });

  function choose(i: number) {
    const row = rows[i];
    if (!row) return;
    open = false;
    onPick(row.sym.line);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      cursor = rows.length ? (cursor + 1) % rows.length : 0;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      cursor = rows.length ? (cursor - 1 + rows.length) % rows.length : 0;
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(cursor);
    }
  }
</script>

{#if open}
  <div class="scrim" onclick={() => (open = false)} role="presentation"></div>
  <div class="popup" role="dialog" aria-modal="true" aria-label="文件结构">
    <div class="head">
      <span class="t">文件结构</span>
      <span class="f">{fileName}</span>
      <span class="gap"></span>
      <span class="hint">↑↓ 选择 · ↵ 跳转 · Esc 关闭</span>
    </div>

    <input
      bind:this={input}
      bind:value={query}
      onkeydown={onKey}
      placeholder="过滤符号…"
      spellcheck="false"
      autocomplete="off"
    />

    <div class="list">
      {#if !supported}
        <div class="none">这门语言没有语法树，无法提取结构<br /><span class="sub">（走 legacy 高亮的语言暂不支持）</span></div>
      {:else if symbols.length === 0}
        <div class="none">没找到符号</div>
      {:else if rows.length === 0}
        <div class="none">没有匹配</div>
      {/if}
      {#each rows as row, i (row.sym.kind + row.sym.name + row.sym.line)}
        <button
          class="row"
          class:sel={i === cursor}
          onclick={() => choose(i)}
          onmouseenter={() => (cursor = i)}
          style:padding-left="{12 + Math.min(row.sym.depth, 6) * 14}px"
        >
          <span class="kind">{row.sym.kind}</span>
          <span class="name">
            {#each row.seg as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}
          </span>
          <span class="line">{row.sym.line}</span>
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.35); z-index: 40; }
  .popup {
    position: fixed;
    top: 12vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(560px, 86vw);
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
    z-index: 41;
    overflow: hidden;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 8px 12px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 11.5px;
    user-select: none;
  }
  .head .t { color: var(--text); }
  .head .f { color: var(--text-faint); font-family: var(--code-font); font-size: 10.5px; }
  .head .gap { flex: 1; }
  .head .hint { color: var(--text-faint); font-family: var(--code-font); font-size: 10px; }

  input {
    border: none;
    border-bottom: 1px solid var(--border-soft);
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 14px;
    padding: 9px 12px;
    outline: none;
  }
  input::placeholder { color: var(--text-faint); }

  .list { overflow-y: auto; padding: 4px 0; }
  .none { padding: 20px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; line-height: 1.8; }
  .none .sub { font-size: 11px; }
  .row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 4px 12px;
    background: transparent;
    border: none;
    text-align: left;
    cursor: default;
    font-size: 12.5px;
  }
  .row.sel { background: var(--accent-sel); }
  .kind {
    flex: none;
    font-size: 9.5px;
    font-family: var(--code-font);
    padding: 1px 5px;
    border-radius: 2px;
    background: var(--panel-bg-2);
    color: var(--text-faint);
    min-width: 3.4em;
    text-align: center;
  }
  .name { color: var(--text); font-family: var(--code-font); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .line {
    margin-left: auto;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }
  mark { background: transparent; color: var(--accent); font-weight: 600; }
</style>
