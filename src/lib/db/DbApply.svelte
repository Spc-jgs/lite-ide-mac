<script lang="ts">
  /**
   * 提交改过的格子之前（docs/DATABASE.md 第 5 节，DB-3）：把要执行的 UPDATE 一条条列出来，点「提交」才执行。
   * 真执行的是参数化的那一份（值不拼进 SQL），这里列的是等价的写法，好让人对着看。
   * 默认按钮是「取消」；生产库上「提交」是 danger（ui.md 二之二：不可逆的动作），别的库是 primary
   */
  let { connName, prod, table, sqls, onanswer, onclose }: { connName: string; prod: boolean; table: string; sqls: string[]; onanswer: (go: boolean) => void; onclose: () => void } = $props();

  function answer(go: boolean) {
    onanswer(go);
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      answer(false);
    }
  }

  function focusIn(el: HTMLButtonElement) {
    el.focus({ preventScroll: true });
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog dbapply" role="alertdialog" aria-label="提交更改" tabindex="-1" onkeydown={onKey}>
    <p class="title">{prod ? `在生产库「${connName}」上提交对 ${table} 的更改？` : `提交对 ${table} 的更改？`}</p>
    <p class="body">将在一个事务中执行以下 {sqls.length} 条语句；任一条未恰好影响 1 行时全部撤回。</p>
    <pre class="sqls">{sqls.join("\n")}</pre>
    <div class="actions">
      <span class="gap"></span>
      <button class="btn" data-dismiss data-default use:focusIn onclick={() => answer(false)}>取消</button>
      <button class="btn" class:danger={prod} class:primary={!prod} onclick={() => answer(true)}>提交</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .dbapply { width: min(640px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .body { margin: 0; font-size: var(--fs-sm); color: var(--text-muted); }
  .sqls { margin: 0; max-height: 40vh; overflow: auto; padding: 8px 10px; border-radius: var(--r-sm); background: var(--field-bg); font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text); white-space: pre-wrap; overflow-wrap: anywhere; }
</style>
