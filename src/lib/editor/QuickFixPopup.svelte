<script lang="ts">
  /**
   * 快速修复（docs/IDEA.md 第 16 节，⌥↵）：照 IDEA 的 Show Context Actions —— 挂在光标底下的一张列表，↑↓ 选、↵ 做、Esc 关。
   * 修报错的（quickfix）在前，顺手的改法（quickassist）和整个文件的（整理 import）在后，各一个分组头。标题是 jdtls 给的英文原话（IDEA 也是英文的）。
   *
   * 由 java-lsp.ts `mount()` 到 body 上（同 UsagesPopup：编辑器那一格有 `container-type`，挂在里面 `position: fixed` 按格子算坐标）。
   */
  import { imeKey } from "../shell/ime";
  import type { JavaAction } from "../ipc/commands";

  let {
    items,
    x,
    y,
    onpick,
    onclose,
  }: {
    items: JavaAction[];
    x: number;
    y: number;
    onpick: (a: JavaAction) => void;
    onclose: () => void;
  } = $props();

  let sel = $state(0);
  let el = $state<HTMLDivElement | null>(null);
  let list = $state<HTMLDivElement | null>(null);

  const groupOf = (a: JavaAction) => (a.kind === "quickfix" ? "修复" : a.kind.startsWith("source") ? "整个文件" : "改法");

  $effect(() => {
    const e = el;
    if (!e) return;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${y + 4 + r.height > window.innerHeight - pad ? Math.max(pad, y - 22 - r.height) : y + 4}px`;
    e.focus({ preventScroll: true });
  });

  $effect(() => {
    const row = list?.querySelector(`[data-i="${sel}"]`) as HTMLElement | null;
    row?.scrollIntoView({ block: "nearest" });
  });

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = Math.min(sel + 1, items.length - 1);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = Math.max(sel - 1, 0);
    } else if (e.key === "Enter" && items[sel]) {
      e.preventDefault();
      onpick(items[sel]);
    }
  }
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup palette m fixes" bind:this={el} tabindex="-1" role="dialog" aria-label="可用的修复" onkeydown={onKey}>
  <div class="rows" bind:this={list} role="listbox">
    {#each items as a, i (i)}
      {#if i === 0 || groupOf(items[i - 1]) !== groupOf(a)}
        <div class="group">{groupOf(a)}</div>
      {/if}
      <button
        class="row"
        class:on={i === sel}
        data-i={i}
        role="option"
        aria-selected={i === sel}
        tabindex="-1"
        onmousemove={() => (sel = i)}
        onclick={() => onpick(a)}
      >{a.title}</button>
    {/each}
  </div>
  <div class="foot">↑↓ 选 · ↵ 做 · Esc 关</div>
</div>

<style>
  .scrim { z-index: 59; }
  /* 宽度在 app.css 的 `.palette.m`；挂在光标底下，把居中的 top / left / transform 关掉（ui.md 第十二条 `anchored`） */
  .fixes {
    z-index: 60;
    top: 0;
    left: 0;
    transform: none;
    display: flex;
    flex-direction: column;
    max-height: min(52vh, 460px);
    overflow: hidden;
    outline: none;
  }
  .rows { overflow-y: auto; padding: 4px 6px 6px; }
  /* 分组名：浮层里吸顶的那行（ui.md 八之二：11 号、常规、字距、最淡） */
  .group {
    position: sticky;
    top: 0;
    padding: 6px 8px 3px;
    background: var(--elevated);
    color: var(--text-faint);
    font-size: var(--fs-xs);
    letter-spacing: 0.04em;
  }
  /* 行：同随处搜索 / 用法列表（内缩圆角块，ui.md 第一条） */
  .row {
    display: block;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: default;
  }
  .row.on { background: var(--accent); color: #fff; }
  .foot { flex: none; padding: 5px 12px; border-top: 1px solid var(--border-soft); background: var(--chrome-scrim); color: var(--text-faint); font-size: var(--fs-xs); }
</style>
