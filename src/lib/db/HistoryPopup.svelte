<script lang="ts">
  /**
   * 查询历史（docs/DATABASE.md 第 5 节 DB-2）：这个连接执行过的 SQL，最近的在前。上面一个框筛，↑↓ 选、↵ 插进控制台的光标处、Esc 关。
   * 骨架同最近位置（ui.md 第五条）；每条是 SQL 的前两行 + 什么时候、多久、成没成
   */
  import { untrack } from "svelte";
  import { imeKey } from "../shell/ime";
  import type { DbHist } from "../ipc/commands";
  import { dbHistory } from "../ipc/db";

  let { conn, connName, onpick, onclose }: { conn: string; connName: string; onpick: (sql: string) => void; onclose: () => void } = $props();

  let all = $state<DbHist[] | null>(null);
  let error = $state("");
  let q = $state("");
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    input?.focus();
  });
  // 打开时读一次（`untrack`：有意只取初值，浮层开着时换不了连接）
  void dbHistory(untrack(() => conn)).then(
    (h) => (all = h),
    (e) => (error = String(e)),
  );

  const shown = $derived.by(() => {
    const w = q.trim().toLowerCase();
    return (all ?? []).filter((h) => !w || h.sql.toLowerCase().includes(w));
  });
  $effect(() => {
    q;
    sel = 0;
  });

  function when(at: number): string {
    const d = new Date(at);
    const p = (n: number) => String(n).padStart(2, "0");
    const today = new Date().toDateString() === d.toDateString();
    return `${today ? "" : `${d.getMonth() + 1}-${p(d.getDate())} `}${p(d.getHours())}:${p(d.getMinutes())}`;
  }

  function pick(h: DbHist | undefined) {
    if (!h) return;
    onclose();
    onpick(h.sql);
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
      pick(shown[sel]);
    }
  }
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup palette m dbhist" role="dialog" aria-label="查询历史">
  <div class="head">
    <span class="title">查询历史</span>
    <span class="who">{connName}</span>
    <input bind:this={input} bind:value={q} class="q" placeholder="筛选 SQL" spellcheck="false" aria-label="筛选查询历史" onkeydown={onKey} />
  </div>
  {#if error}
    <div class="empty-msg bad">{error}</div>
  {:else if !all}
    <div class="empty-msg wait"><span class="spinner sm"></span> 正在读取…</div>
  {:else if !all.length}
    <div class="empty-msg">此连接尚无查询历史<div class="sub">在控制台中按 ⌘↵ 执行的语句将记录在此（最近 1000 条）</div></div>
  {:else if !shown.length}
    <div class="empty-msg">没有包含「{q}」的语句</div>
  {:else}
    <div class="rows" role="listbox">
      {#each shown as h, i (`${h.at}\n${i}`)}
        <button class="row" class:on={i === sel} role="option" aria-selected={i === sel} tabindex="-1" onmousemove={() => (sel = i)} onclick={() => pick(h)} title={h.error ?? h.sql}>
          <span class="sql">{h.sql.split("\n").slice(0, 2).join("\n")}</span>
          <span class="meta">
            <span class:bad={!h.ok}>{h.ok ? (h.count !== null ? `影响 ${h.count} 行` : "成功") : "失败"}</span>
            <span>{h.ms} ms</span>
            <span>{when(h.at)}</span>
          </span>
        </button>
      {/each}
    </div>
  {/if}
</div>

<style>
  .scrim { z-index: 59; }
  .dbhist { z-index: 60; display: flex; flex-direction: column; max-height: min(60vh, 560px); overflow: hidden; }
  .head { display: flex; align-items: center; gap: 10px; padding: 10px 12px 8px; border-bottom: 1px solid var(--border-soft); }
  .title { flex: none; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .who { flex: none; max-width: 30%; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sm); color: var(--text-faint); }
  .q { flex: 1; min-width: 0; height: 26px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font: inherit; outline: none; }
  .q:focus { border-color: var(--accent); }
  .rows { overflow-y: auto; padding: 4px 6px 6px; }
  .row { display: flex; flex-direction: column; align-items: stretch; gap: 2px; width: 100%; padding: 5px 8px; border: none; border-radius: var(--r-sm); background: none; color: var(--text-dim); font: inherit; text-align: left; cursor: pointer; }
  .row.on { background: var(--selected); }
  .sql { font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text); white-space: pre; overflow: hidden; text-overflow: ellipsis; }
  .meta { display: flex; gap: 10px; font-size: var(--fs-xs); color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .meta .bad { color: var(--lvl-error); }
</style>
