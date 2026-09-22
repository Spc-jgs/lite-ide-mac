<script lang="ts">
  import { listEncodings } from "../ipc/fs";

  let {
    open = $bindable(false),
    current,
    bom,
    lossy,
    /** 只读（日志模式）时不给「以此编码保存」 */
    readonly = false,
    onReopen,
    onSaveAs,
    anchor = null,
  }: {
    open?: boolean;
    /** 状态栏那格的位置（左、上沿减 4）：有就挂在它上面，没有就居中（菜单开的） */
    anchor?: { x: number; y: number } | null;
    current: string;
    bom: boolean;
    lossy: boolean;
    readonly?: boolean;
    onReopen: (label: string) => void;
    onSaveAs: (label: string, bom: boolean) => void;
  } = $props();

  let list = $state<[string, string][]>([]);
  /*
   * 挂在状态栏那格上面时：钳进视口，抄 ContextMenu 的 `up` —— 读一次矩形、写一次 style，
   * 改 DOM 不写回 props。`y` 是格子上沿，浮层的下边贴着它。
   */
  let popEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    const e = popEl;
    const a = anchor;
    if (!e || !a) return;
    // 编码表是异步拉的：列表一到高度就变，钳完的 top 会过时 —— 依赖里带上它
    void list.length;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(a.x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${Math.max(pad, a.y - r.height)}px`;
  });

  let sel = $state(0);
  let withBom = $state(false);

  $effect(() => {
    if (!open) return;
    withBom = bom;
    sel = 0;
    void listEncodings()
      .then((l) => {
        list = l;
        const i = l.findIndex(([label]) => label.toLowerCase() === current.toLowerCase());
        if (i >= 0) sel = i;
      })
      .catch(() => (list = []));
  });

  /** UTF-8 / UTF-16 才有 BOM 的说法，别在 GBK 上摆一个没意义的开关 */
  let bomApplies = $derived(/^utf-?(8|16)/i.test(list[sel]?.[0] ?? ""));

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = (sel + 1) % Math.max(1, list.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = (sel - 1 + list.length) % Math.max(1, list.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      // ↵ 走「重新打开」—— 绝大多数时候用户是发现乱码了想换个编码看，
      // 而不是想改变这个文件将来的存法
      reopen();
    }
  }

  function reopen() {
    const l = list[sel]?.[0];
    if (l) onReopen(l);
    open = false;
  }
  function saveAs() {
    const l = list[sel]?.[0];
    if (l) onSaveAs(l, bomApplies && withBom);
    open = false;
  }
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <!-- 从格子上开的不压暗：它是随手换一下的东西，不是要你先处理完的对话框（ui.md 十二） -->
  <div class="scrim" class:dim={!anchor} onclick={() => (open = false)}></div>
  <div class="popup" class:anchored={!!anchor} bind:this={popEl} role="dialog" aria-label="文件编码">
    <div class="head">
      <span>文件编码</span>
      <span class="cur">当前：{current}{bom ? " + BOM" : ""}</span>
    </div>
    {#if lossy}
      <div class="warn">
        按 {current} 解码时有解不出的字节，界面上显示成 �。
        <b>保存会把它们永久写成 U+FFFD</b> —— 换个编码重新打开试试。
      </div>
    {/if}
    <div class="list">
      {#each list as [label, desc], i (label)}
        <button
          class="row"
          class:on={i === sel}
          onmouseenter={() => (sel = i)}
          onclick={() => (sel = i)}
          ondblclick={reopen}
        >
          <span class="lb">{label}</span>
          <span class="ds">{desc}</span>
          {#if label.toLowerCase() === current.toLowerCase()}<span class="now">当前</span>{/if}
        </button>
      {/each}
    </div>
    <div class="foot">
      {#if bomApplies}
        <label class="bom"><input type="checkbox" bind:checked={withBom} /> 带 BOM</label>
      {/if}
      <span class="gap"></span>
      <button class="btn" onclick={reopen} title="按这个编码重新解码文件内容">重新打开 ↵</button>
      {#if !readonly}
        <button class="btn primary" onclick={saveAs} title="下次保存时按这个编码写回">以此编码保存</button>
      {/if}
    </div>
  </div>
{/if}

<style>
  .popup {
    position: fixed;
    top: 16vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(500px, 88vw);
    max-height: 62vh;
    display: flex;
    flex-direction: column;
    /* 面（底、边、圆角、投影、淡入）在 app.css 的 `.popup`，这里只管位置和尺寸 */
    z-index: 41;
    overflow: hidden;
  }
  /* 挂在状态栏格子上时：不居中，left/top 由 effect 写进 style */
  .popup.anchored { top: 0; left: 0; transform: none; max-height: min(62vh, calc(100vh - 60px)); }
  .head {
    display: flex;
    align-items: baseline;
    gap: 10px;
    padding: 10px 14px 8px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 13px;
    color: var(--text);
    user-select: none;
  }
  .head .cur { font-family: var(--code-font); font-size: 11px; color: var(--text-faint); }
  .warn {
    padding: 8px 14px;
    background: rgba(247, 84, 100, 0.10);
    border-bottom: 1px solid var(--lvl-error);
    font-size: 11.5px;
    line-height: 1.6;
    color: var(--text-dim);
  }
  .warn b { color: var(--lvl-error); }
  .list { overflow-y: auto; padding: 4px 0; }
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 26px;
    padding: 0 14px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .row.on { background: var(--accent-sel); color: var(--text); }
  .lb { flex: none; width: 110px; font-family: var(--code-font); font-size: 12px; }
  .ds { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis;
        color: var(--text-faint); font-size: 11px; }
  .now { flex: none; font-size: 10px; color: var(--accent); }
  .foot {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 8px 14px;
    border-top: 1px solid var(--border-soft);
  }
  .foot .gap { flex: 1; }
  .bom { display: flex; align-items: center; gap: 4px; font-size: 11px; color: var(--text-faint); }
  .bom input { margin: 0; accent-color: var(--accent); }
</style>
