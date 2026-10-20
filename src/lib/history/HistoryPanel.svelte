<script lang="ts">
  /**
   * 本地历史（docs/IDEA.md 第 21 节），照 IDEA 的 Local History → Show History：
   *
   * ```
   * 本地历史 · OrderService.java                                       ✕
   * ┌ 14:32 · 3 分钟前    已保存  1.2 KB ┬ 那一版 → 现在（编辑器里此刻的，没保存的也算）
   * │ 14:10 · 25 分钟前   外部改动        │  差异视图那套：双栏、着色、「N 行未改动」折叠
   * │ 昨天 09:10          已保存          │
   * └─────────────────────────────────────┴──────────────────── [恢复成这一版]
   * ```
   *
   * 「恢复成这一版」是编辑器里的**一笔修改**（⌘Z 撤得回、还没保存），不直接写盘 —— 恢复错了一下就能退回来；
   * 现在这份本来就在历史里（它保存的时候记过），没保存的那些改动也能 ⌘Z 回来。
   * 挂在 body 上（history-ops 的 `mount`），不经过 overlay 那张表：那张表在入口包里
   */
  import Icon from "../shell/Icon.svelte";
  import DiffView from "../git/DiffView.svelte";
  import { imeKey } from "../shell/ime";
  import type { HistoryEntry } from "../ipc/commands";
  import { historyList, historyRead } from "../ipc/history";
  import { unifiedDiff } from "../git/linediff";

  let {
    path,
    rel,
    label,
    current,
    onrestore,
    onclose,
  }: {
    path: string;
    /** 差异里显示的路径（相对项目根） */
    rel: string;
    /** 这个文件的编码（读旧版本时按它解码） */
    label: string;
    /** 现在的文字：编辑器里此刻的（没保存的也算），没开着就是盘上的 */
    current: () => Promise<string>;
    onrestore: (text: string, when: string) => void;
    onclose: () => void;
  } = $props();

  let list = $state<HistoryEntry[] | null>(null);
  let err = $state("");
  let sel = $state(0);
  let text = $state<string | null>(null);
  let diff = $state<string | null>(null);
  let now = $state("");
  let readErr = $state("");

  $effect(() => {
    let dead = false;
    void Promise.all([historyList(path), current()])
      .then(([l, c]) => {
        if (dead) return;
        list = l;
        now = c;
      })
      .catch((e) => {
        if (!dead) err = e instanceof Error ? e.message : String(e);
      });
    return () => {
      dead = true;
    };
  });

  // 选中的那一版：读出来、和现在比
  $effect(() => {
    const e = list?.[sel];
    const cur = now;
    if (!e) return;
    let dead = false;
    text = null;
    diff = null;
    readErr = "";
    historyRead(path, e.hash, label)
      .then((t) => {
        if (dead) return;
        text = t;
        diff = unifiedDiff(t, cur, rel);
      })
      .catch((x) => {
        if (!dead) readErr = x instanceof Error ? x.message : String(x);
      });
    return () => {
      dead = true;
    };
  });

  const pad = (n: number) => String(n).padStart(2, "0");
  function when(ms: number): string {
    const d = new Date(ms);
    const t = new Date();
    const hm = `${pad(d.getHours())}:${pad(d.getMinutes())}`;
    const day = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
    const days = Math.round((day(t) - day(d)) / 86_400_000);
    return days === 0 ? `今天 ${hm}` : days === 1 ? `昨天 ${hm}` : `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${hm}`;
  }
  function ago(ms: number): string {
    const s = Math.max(0, Math.round((Date.now() - ms) / 1000));
    if (s < 60) return "刚才";
    if (s < 3600) return `${Math.floor(s / 60)} 分钟前`;
    if (s < 86_400) return `${Math.floor(s / 3600)} 小时前`;
    return `${Math.floor(s / 86_400)} 天前`;
  }
  const size = (n: number) => (n < 1024 ? `${n} B` : `${(n / 1024).toFixed(1)} KB`);

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onclose();
    } else if ((e.key === "ArrowDown" || e.key === "ArrowUp") && list?.length) {
      e.preventDefault();
      sel = Math.max(0, Math.min(list.length - 1, sel + (e.key === "ArrowDown" ? 1 : -1)));
      document.querySelector(`.hist [data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter" && text !== null && diff) {
      e.preventDefault();
      restore();
    }
  }

  function restore() {
    const e = list?.[sel];
    if (!e || text === null) return;
    onrestore(text, when(e.ms));
  }

  function focusOnMount(node: HTMLElement) {
    node.focus({ preventScroll: true });
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- 点暗处关：这是「看一眼」的浮层，不是要你先回答的对话框（ui.md 第二之二条） -->
<div class="scrim dim" role="presentation" onclick={onclose}></div>
<div class="popup palette l hist" role="dialog" aria-label="本地历史" tabindex="-1" use:focusOnMount>
  <div class="head">
    <span class="title">本地历史</span>
    <span class="file">{rel}</span>
    <span class="gap"></span>
    <button class="ibtn" onclick={onclose} title="关闭（Esc）" aria-label="关闭"><Icon name="x" /></button>
  </div>
  <div class="body">
    <div class="list" role="listbox" aria-label="版本">
      {#if err}
        <div class="empty-msg bad">{err}</div>
      {:else if !list}
        <div class="empty-msg wait"><span class="spinner sm"></span> 正在读…</div>
      {:else if list.length === 0}
        <div class="empty-msg">
          还没有存过的版本
          <div class="sub">每次保存（⌘S、自动保存）都会留一份，30 天、最多 200 版；改之前的样子在第一次保存时一起留下</div>
        </div>
      {:else}
        {#each list as e, i (e.hash + e.ms + e.kind)}
          <div class="row" class:on={i === sel} data-i={i} role="option" aria-selected={i === sel} tabindex="-1" onclick={() => (sel = i)} onkeydown={() => {}}>
            <span class="t">{when(e.ms)}</span>
            <span class="ago">{ago(e.ms)}</span>
            <!-- 保存前盘上那份：最早那条是「改之前的样子」（第一次在这儿保存时留的）；夹在两次保存中间的才是外面改过（git 切分支、别的编辑器） -->
            <span class="kind" class:ext={e.kind === "before" && i < list.length - 1}>{e.kind === "saved" ? "已保存" : i === list.length - 1 ? "改之前" : "外部改动"}</span>
            <span class="sz">{size(e.size)}</span>
          </div>
        {/each}
      {/if}
    </div>
    <div class="diff">
      {#if readErr}
        <div class="empty-msg bad">{readErr}</div>
      {:else if list?.length && text === null}
        <div class="empty-msg wait"><span class="spinner sm"></span> 正在读这一版…</div>
      {:else if text !== null && diff === ""}
        <div class="empty-msg">和现在一样<div class="sub">选下面更早的一版看改了什么</div></div>
      {:else if text !== null && diff === null}
        <div class="empty-msg">两边差得太多（几万行整个重写），这里比不了<div class="sub">恢复照样能做：恢复之后 ⌘Z 撤得回</div></div>
      {:else if diff}
        <DiffView raw={diff} path={rel} staged={false} commit="本地历史" onToggleStaged={() => {}} />
      {/if}
    </div>
  </div>
  <div class="foot">
    <span class="hint">左边是那一版，右边是现在（编辑器里此刻的，没保存的也算）。↑↓ 换一版，↵ 恢复</span>
    <span class="gap"></span>
    <button class="btn" onclick={onclose}>关闭</button>
    <button class="btn primary" disabled={text === null || !diff} onclick={restore} title="在编辑器里换成这一版：⌘Z 撤得回，还没保存">恢复成这一版</button>
  </div>
</div>

<style>
  .hist { display: flex; flex-direction: column; height: min(72vh, 680px); outline: none; z-index: 60; }
  .head { flex: none; display: flex; align-items: center; gap: 8px; height: 38px; padding: 0 6px 0 14px; border-bottom: 1px solid var(--border-soft); }
  .head .title { font-weight: 600; color: var(--text); font-size: var(--fs-md); }
  .head .file { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-sm); }
  .head .gap, .foot .gap { flex: 1; }
  .body { flex: 1; min-height: 0; display: flex; }
  /* 版本在左（定宽：时间读得出就够），差异在右（同 IDEA 的 Local History 窗） */
  .list { flex: none; width: 250px; overflow: auto; padding: 4px 6px 8px; border-right: 1px solid var(--border-soft); }
  .diff { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .diff > :global(*) { flex: 1; min-height: 0; }
  .row { display: flex; align-items: baseline; gap: 6px; height: 26px; padding: 0 8px; border-radius: var(--r-sm); color: var(--text-dim); font-size: var(--fs-md); white-space: nowrap; cursor: default; }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .t { font-variant-numeric: tabular-nums; }
  .ago, .sz { color: var(--text-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; }
  .kind { margin-left: auto; font-size: var(--fs-xs); color: var(--text-faint); }
  .kind.ext { color: var(--lvl-warn); }
  .foot { flex: none; display: flex; align-items: center; gap: 8px; height: 44px; padding: 0 12px; border-top: 1px solid var(--border-soft); background: var(--chrome-scrim); }
  .hint { color: var(--text-faint); font-size: var(--fs-xs); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
</style>
