<script lang="ts">
  export interface Tab {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log" | "diff" | "merge";
    dirty: boolean;
  }

  let {
    tabs,
    activeId,
    onSelect,
    onClose,
  }: {
    tabs: Tab[];
    activeId: number | null;
    onSelect: (id: number) => void;
    onClose: (id: number) => void;
  } = $props();

  let bar = $state<HTMLElement | null>(null);
  let els = $state<Record<number, HTMLElement>>({});

  /**
   * 让当前标签始终可见。
   *
   * 标签多到溢出时，用 ⌘P 打开一个已存在但滚出视野的标签，界面上会「什么都没发生」
   * —— 其实切过去了，只是那个标签在屏幕外。
   */
  $effect(() => {
    const id = activeId;
    if (id === null) return;
    const el = els[id];
    if (el) el.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  /** 竖着滚滚轮就横向滚标签栏 —— 触控板上这是最自然的手势 */
  function onWheel(e: WheelEvent) {
    if (!bar) return;
    const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (d === 0) return;
    e.preventDefault();
    bar.scrollLeft += d;
  }
</script>

<div class="tabs" role="tablist" bind:this={bar} onwheel={onWheel}>
  {#each tabs as tab (tab.id)}
    <!-- 中键关标签，浏览器和各家编辑器通用的手势 -->
    <div
      class="tab"
      class:active={tab.id === activeId}
      role="presentation"
      bind:this={els[tab.id]}
      onauxclick={(e) => {
        if (e.button === 1) {
          e.preventDefault();
          onClose(tab.id);
        }
      }}
    >
      <button
        class="label"
        role="tab"
        aria-selected={tab.id === activeId}
        onclick={() => onSelect(tab.id)}
        title={tab.path}
      >
        {#if tab.mode === "log"}<span class="badge">日志</span>{/if}
        {#if tab.mode === "diff"}<span class="badge diff">差异</span>{/if}
        {#if tab.mode === "merge"}<span class="badge merge">冲突</span>{/if}
        <span class="name">{tab.name}</span>
      </button>
      <button
        class="close"
        class:dirty={tab.dirty}
        onclick={() => onClose(tab.id)}
        title={tab.dirty ? "有未保存的改动" : "关闭"}
        aria-label="关闭 {tab.name}"
      >
        {tab.dirty ? "●" : "✕"}
      </button>
    </div>
  {/each}
</div>

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    height: 32px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    overflow-y: hidden;
    user-select: none;
  }
  .tab {
    display: flex;
    align-items: center;
    flex: none;
    max-width: 200px;
    border-right: 1px solid var(--border-soft);
    /* IDEA 是平角标签，底部一条 accent 线表示选中 */
    border-bottom: 2px solid transparent;
    background: transparent;
  }
  .tab:hover { background: var(--panel-bg-2); }
  /* 标签溢出时给个细滚动条，否则完全看不出还有更多标签 */
  .tabs::-webkit-scrollbar { height: 3px; }
  .tabs::-webkit-scrollbar-thumb { background: var(--border); border-radius: 2px; }
  .tabs:hover::-webkit-scrollbar-thumb { background: var(--text-faint); }
  .tab.active {
    background: var(--editor-bg);
    border-bottom-color: var(--accent);
  }
  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 11px;
    height: 100%;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    cursor: default;
    overflow: hidden;
  }
  .tab.active .label { color: var(--text); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge {
    flex: none;
    font-size: 9px;
    padding: 1px 4px;
    border-radius: 2px;
    background: var(--panel-bg-2);
    color: var(--text-faint);
    font-family: var(--code-font);
  }
  .badge.diff { color: var(--git-modified); }
  .badge.merge { color: var(--lvl-warn); }
  .close {
    flex: none;
    width: 20px;
    height: 100%;
    padding: 0 11px 0 0;
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10px;
    cursor: default;
  }
  .close:hover { color: var(--text); }
  .close.dirty { color: var(--accent); font-size: 9px; }
  .label:focus-visible, .close:focus-visible { outline: 1px solid var(--accent); outline-offset: -2px; }
</style>
