<script lang="ts">
  /**
   * 快速文档（F1，照 IDEA 的 Quick Documentation；docs/TAKEOVER.md G3）：挂在光标底下的一张卡片，签名 + Javadoc。Esc / 点外面关。
   * 由 java-lsp.ts 用 `mount()` 挂到 body 上（同 UsagesPopup：编辑器的格子有 `container-type`，挂在里面 fixed 的坐标不对）。
   * Markdown 只拆成几块、全走文本插值（`doc-md.ts` 头上写着为什么不插 HTML）
   */
  import { imeKey } from "../shell/ime";
  import { docBlocks } from "./doc-md";

  let { md, x, y, onclose }: { md: string; x: number; y: number; onclose: () => void } = $props();

  const blocks = $derived(docBlocks(md));
  let el = $state<HTMLDivElement | null>(null);

  // 钳进视口，下面放不下翻到光标上面（同 UsagesPopup）
  $effect(() => {
    const e = el;
    if (!e) return;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${y + 4 + r.height > window.innerHeight - pad ? Math.max(pad, y - 22 - r.height) : y + 4}px`;
    e.focus({ preventScroll: true });
  });

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape" || e.key === "F1") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup doc" bind:this={el} tabindex="-1" role="dialog" aria-label="快速文档" onkeydown={onKey}>
  {#each blocks as b, i (i)}
    {#if b.kind === "code"}
      <pre class="sig">{b.text}</pre>
    {:else}
      <p class:item={b.kind === "item"} style:padding-left={b.kind === "item" ? `${10 + b.depth * 14}px` : null}>
        {#if b.kind === "item"}<span class="bullet" aria-hidden="true">·</span>{/if}{#each b.spans as s, j (j)}{#if s.code}<code>{s.t}</code>{:else if s.bold}<b>{s.t}</b>{:else}{s.t}{/if}{/each}
      </p>
    {/if}
  {/each}
</div>

<style>
  .scrim { z-index: 59; }
  .doc {
    position: fixed;
    z-index: 60;
    top: 0;
    left: 0;
    width: min(560px, calc(100vw - 32px));
    max-height: min(50vh, 420px);
    overflow-y: auto;
    padding: 10px 14px 12px;
    outline: none;
    font-size: var(--fs-md);
    line-height: 1.55;
    color: var(--text-dim);
    user-select: text;
  }
  .sig {
    margin: 0 0 8px;
    padding: 6px 8px;
    border-radius: var(--r-sm);
    background: var(--hover);
    font-family: var(--code-font);
    font-size: var(--fs-sm);
    color: var(--text);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  p { margin: 0 0 6px; }
  p.item { margin: 0 0 2px; }
  .bullet { display: inline-block; width: 10px; color: var(--text-faint); }
  code { font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text); }
  b { color: var(--text); font-weight: 600; }
</style>
