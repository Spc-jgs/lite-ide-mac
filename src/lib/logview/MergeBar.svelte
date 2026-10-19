<script lang="ts">
  /**
   * 合并视图的图例条（docs/MERGE.md）：几份日志各是什么颜色、只看哪几份、每份的时钟偏移、再加一份。
   *
   * 点一份的手感和级别胶囊一样（`merge-path.ts` 的 `toggleSource`）：全亮时点一份 = 只看它，只剩它时再点 = 复原。
   * 偏移是常驻的（「这一份挪过 +8h」不看会看错时间线，ui.md 第三条：不看会出错的才常驻）；没挪过的那个按钮 hover 才出。
   */
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import type { MergeInfo } from "../ipc/commands";
  import { clockNote, fmtOffset, parseOffset, toggleSource, allSources } from "./merge-path";
  import { toBody } from "./to-body";

  let {
    info,
    names,
    colors,
    mask = $bindable(),
    merging = false,
    onOffsets,
    onAdd,
  }: {
    info: MergeInfo;
    /** 每份的短名（`shortNames`），和行首那一格同一套 */
    names: string[];
    colors: string[];
    /** 只看哪几份（第 i 位 = 第 i 份） */
    mask: number;
    /** 正在（重新）归并：「加一份」和改偏移先不让点 —— 句柄背后的东西正在换 */
    merging?: boolean;
    onOffsets: (offsets: number[]) => Promise<void>;
    onAdd: () => void;
  } = $props();

  const n = $derived(info.sources.length);
  const all = $derived(allSources(n));
  const on = (i: number) => (mask & (1 << i)) !== 0;

  /** 借了日期的那几份，说出来（不说的话人会以为几份日志本来就对得上） */
  const notes = $derived(
    info.sources
      .map((s, i) => ({ i, text: clockNote(s.clock, info.fillDate) }))
      .filter((x) => x.text),
  );

  /** 改偏移的小面板：挂在那一份的按钮底下（ui.md 第十二条） */
  let edit = $state<{ i: number; x: number; y: number; text: string; error: string; busy: boolean } | null>(null);

  function openEdit(i: number, e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    // 300 宽的面板别伸出窗口右边
    const x = Math.min(r.left, window.innerWidth - 316);
    edit = { i, x: Math.max(8, x), y: r.bottom + 4, text: fmtOffset(info.sources[i].offsetMs), error: "", busy: false };
  }

  // 小面板挂到 body 上、挪完再给焦点：为什么见 to-body.ts

  async function apply() {
    const e = edit;
    if (!e) return;
    const ms = parseOffset(e.text);
    if (ms === null) {
      edit = { ...e, error: "读不懂。写成 +8h、-350ms、+1m30s 这样" };
      return;
    }
    const offsets = info.sources.map((s, i) => (i === e.i ? ms : s.offsetMs));
    edit = { ...e, busy: true, error: "" };
    try {
      await onOffsets(offsets);
      edit = null;
    } catch (err) {
      edit = edit && { ...edit, busy: false, error: String(err) };
    }
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void apply();
    } else if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      edit = null;
    }
  }
</script>

<div class="bar">
  <span class="lead">合并 {n} 份</span>
  <div class="srcs">
    {#each info.sources as s, i (s.path)}
      <div class="src" class:off={!on(i)} style:--c={colors[i]}>
        <button
          class="name"
          onclick={() => (mask = toggleSource(mask, i, n))}
          title={`${s.path}\n点击只看这一份，再点复原`}
        >
          <i class="dot"></i>{names[i]}
        </button>
        <button
          class="ofs"
          class:set={s.offsetMs !== 0}
          disabled={merging}
          onclick={(e) => openEdit(i, e)}
          title="时钟偏移：这一份的时间整体挪多少再合（时区不同、机器钟不准时用）"
          aria-label="{names[i]} 的时钟偏移"
        >{s.offsetMs !== 0 ? fmtOffset(s.offsetMs) : "±"}</button>
      </div>
    {/each}
    {#if mask !== all}
      <button class="btn sm quiet" onclick={() => (mask = all)}>全部</button>
    {/if}
  </div>
  {#if notes.length}
    <span class="notes" title={notes.map((x) => `${names[x.i]}：${x.text}`).join("\n")}>
      <Icon name="warn" />
      <span class="nt">{#each notes as x, k (x.i)}{k > 0 ? "；" : ""}<b style:color={colors[x.i]}>{names[x.i]}</b> {x.text}{/each}</span>
    </span>
  {/if}
  {#if !notes.length}<span class="gap"></span>{/if}
  <button class="btn sm" disabled={merging} onclick={onAdd} title="再选几份日志一起合（新开一个标签，这一个关掉）">加一份…</button>
</div>

{#if edit}
  <div class="layer" use:toBody>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (edit = null)}></div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="pop popup" role="dialog" tabindex="-1" aria-label="时钟偏移" style:left="{edit.x}px" style:top="{edit.y}px" onkeydown={onKey}>
    <div class="ptitle">时钟偏移</div>
    <div class="pobj" style:--c={colors[edit.i]}><i class="dot"></i><span class="oname">{names[edit.i]}</span></div>
    <input
      class="pinput"
      bind:value={edit.text}
      oninput={() => edit && (edit.error = "")}
      placeholder="+8h、-350ms、+1m30s，0 是不挪"
      spellcheck="false"
      autocomplete="off"
    />
    {#if edit.error}
      <div class="perr">{edit.error}</div>
    {:else}
      <div class="phint">这一份的时间整体加上它再合。一份 UTC、别的是北京时间：给 UTC 那份 +8h。挪完行号会变，过滤会按原条件重跑。</div>
    {/if}
    <div class="prow">
      <button class="btn" onclick={() => (edit = null)}>取消</button>
      <button class="btn primary" disabled={edit.busy} class:busy={edit.busy} aria-busy={edit.busy} onclick={() => void apply()}>重新合并</button>
    </div>
  </div>
  </div>
{/if}

<style>
  /* 和过滤条同一种工具条（34 高、岛内分区线），叠在它上面：先选看哪几份，再在里面筛 */
  .bar {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 6px 10px;
    padding: 5px 10px;
    min-height: 34px;
    border-bottom: 1px solid var(--border-soft);
    font-size: var(--fs-sm);
    user-select: none;
  }
  .lead { color: var(--text-faint); flex: none; }
  .gap { flex: 1; }
  .srcs { display: flex; flex-wrap: wrap; align-items: center; gap: 4px; min-width: 0; }

  /* 一份 = 名字（点了筛）+ 偏移（点了改），两个按钮拼成一块，和级别胶囊同一个底 */
  .src { display: flex; align-items: center; border-radius: var(--r-sm); }
  .src:hover { background: var(--hover); }
  .src.off { opacity: 0.38; }
  .src button {
    height: 24px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-sm);
    cursor: default;
  }
  .name { display: flex; align-items: center; gap: 6px; padding: 0 4px 0 8px; }
  .name:hover { color: var(--text); }
  .dot { width: 7px; height: 7px; border-radius: 2px; background: var(--c); flex: none; }
  .ofs {
    padding: 0 7px 0 3px;
    font-variant-numeric: tabular-nums;
    color: var(--text-faint) !important;
    visibility: hidden;
  }
  /* 没挪过的「±」hover 才出；挪过的常驻（不看会读错时间线） */
  .src:hover .ofs, .ofs:focus-visible, .ofs.set { visibility: visible; }
  .ofs.set { color: var(--c) !important; }
  .ofs:hover:not(:disabled) { color: var(--text) !important; }
  .src button:focus-visible { outline: 1px solid var(--accent); border-radius: var(--r-sm); }

  /* 说明占掉剩下的宽度、太长就省略（全文在 title 上）：flex-basis 是 0，不然换行容器会先把「加一份」挤到第二行，而不是先让它缩 */
  .notes {
    flex: 1 1 0;
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    color: var(--text-faint);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .notes .nt { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .notes b { font-weight: 500; }

  .layer { display: contents; }
  /* 小面板：同文件树的新建 / 改名（标题 → 对象行 → 输入 → 说明 → 按钮），面在 app.css 的 `.popup` */
  .scrim { z-index: 59; }
  .pop { position: fixed; z-index: 60; width: 300px; padding: 14px 14px 12px; outline: none; }
  .ptitle { margin: 0 0 10px; color: var(--text); font-size: var(--fs-md); font-weight: 600; }
  .pobj {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 24px;
    padding: 0 8px;
    margin-bottom: 4px;
    border-radius: var(--r-sm);
    background: var(--hover);
    color: var(--text);
    font-size: var(--fs-md);
  }
  .oname { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .pinput {
    width: 100%;
    margin: 4px 0 0;
    height: 28px;
    padding: 0 9px;
    background: var(--field-bg);
    border: 1px solid var(--field-border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: var(--fs-md);
  }
  .pinput:focus { outline: none; border-color: var(--accent); box-shadow: var(--focus-glow); }
  .perr { padding: 6px 0 1px; color: var(--lvl-error); font-size: var(--fs-sm); line-height: 1.5; }
  .phint { margin-top: 6px; color: var(--text-faint); font-size: var(--fs-sm); line-height: 1.5; }
  .prow { display: flex; justify-content: flex-end; gap: 6px; margin-top: 10px; }
</style>
