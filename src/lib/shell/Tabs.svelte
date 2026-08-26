<script lang="ts">
  export interface Tab {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log";
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
</script>

<div class="tabs" role="tablist">
  {#each tabs as tab (tab.id)}
    <div class="tab" class:active={tab.id === activeId} role="presentation">
      <button
        class="label"
        role="tab"
        aria-selected={tab.id === activeId}
        onclick={() => onSelect(tab.id)}
        title={tab.path}
      >
        {#if tab.mode === "log"}<span class="badge">日志</span>{/if}
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
  .tabs::-webkit-scrollbar { height: 0; }
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
