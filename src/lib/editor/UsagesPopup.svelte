<script lang="ts">
  /**
   * 查找用法的结果（docs/IDEA.md 第 2 步）：照 IDEA 的 Show Usages（⌥⌘F7）—— 挂在光标底下的一张列表，↑↓ 选、↵ 跳、Esc 关。
   * IDEA 的 ⌥F7 开的是「Find」工具窗；那个工具窗还没有，这一步 ⌥F7 也先开这张（keymap.ts 里写着）。
   *
   * 由 java-lsp.ts 用 `mount()` 挂到 body 上（同 PatternPopup 的 `toBody`：编辑器所在的格子有 `container-type`，
   * 挂在里面的话 `position: fixed` 按格子算坐标，frontend.md）。只有它自己的状态，关掉就 `unmount`。
   */
  import { imeKey } from "../shell/ime";
  import type { JavaLocation } from "../ipc/commands";

  let {
    name,
    items,
    total,
    x,
    y,
    onpick,
    onclose,
  }: {
    /** 查的是谁（光标底下那个名字） */
    name: string;
    items: JavaLocation[];
    /** 真条数。比 `items` 多 = 截断了，要说出来（AGENTS.md「截断了要说出来」） */
    total: number;
    /** 光标的屏幕坐标（左下角） */
    x: number;
    y: number;
    onpick: (loc: JavaLocation) => void;
    onclose: () => void;
  } = $props();

  let sel = $state(0);
  let el = $state<HTMLDivElement | null>(null);
  let list = $state<HTMLDivElement | null>(null);

  /** 文件名 + 相对那个文件所在模块的一段路径，同 IDEA 列表的左右两栏 */
  const fileOf = (l: JavaLocation) => l.label;
  const dirOf = (l: JavaLocation) => {
    if (!l.path) return "";
    const parts = l.path.split("/");
    const src = parts.lastIndexOf("src");
    return (src > 0 ? parts.slice(src - 1, -1) : parts.slice(-3, -1)).join("/");
  };

  // 钳进视口：读一次矩形、写一次 style（同 ContextMenu）。下面放不下就翻到光标上面
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
    const row = list?.children[sel] as HTMLElement | undefined;
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
<div class="popup palette m usages" bind:this={el} tabindex="-1" role="dialog" aria-label="{name} 的用法" onkeydown={onKey}>
  <div class="head">
    <span class="title">{name} 的用法</span>
    <span class="count">{total} 处{total > items.length ? `，只列前 ${items.length}` : ""}</span>
  </div>
  {#if items.length === 0}
    <div class="empty-msg">没有别处用到它（不含声明本身）</div>
  {:else}
    <div class="rows" bind:this={list} role="listbox">
      {#each items as l, i (i)}
        <button
          class="row"
          class:on={i === sel}
          role="option"
          aria-selected={i === sel}
          tabindex="-1"
          onmousemove={() => (sel = i)}
          onclick={() => onpick(l)}
          title={l.path ?? l.label}
        >
          <span class="file">{fileOf(l)}<span class="ln">:{l.line + 1}</span></span>
          <span class="text">{l.text}</span>
          <span class="dir">{dirOf(l)}</span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .scrim { z-index: 59; }
  /* 宽度在 app.css 的 `.palette.m`；挂在光标底下，所以把居中的 top / left / transform 关掉（ui.md 第十二条 `anchored`） */
  .usages {
    z-index: 60;
    top: 0;
    left: 0;
    transform: none;
    display: flex;
    flex-direction: column;
    max-height: min(52vh, 480px);
    overflow: hidden;
    outline: none;
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 10px 14px 8px;
    border-bottom: 1px solid var(--border-soft);
  }
  .title { font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .count { font-size: var(--fs-sm); color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .rows { overflow-y: auto; padding: 4px 6px 6px; }
  /* 行：同随处搜索的结果行（内缩圆角块 + --selected，ui.md 第一条） */
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
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
    cursor: default;
  }
  .row.on { background: var(--selected); color: var(--text); }
  .file { flex: none; color: var(--text); }
  .ln { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  /* 那一行的代码：等宽（ui.md 八之二：代码类内容才等宽） */
  .text { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-dim); }
  .dir { flex: none; max-width: 30%; overflow: hidden; text-overflow: ellipsis; font-size: var(--fs-xs); color: var(--text-faint); }
</style>
