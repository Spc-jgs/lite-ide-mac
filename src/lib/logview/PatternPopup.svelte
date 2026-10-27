<script lang="ts">
  /**
   * 日志归类（docs/PATTERNS.md）：这份日志里都发生了些什么、各多少。点一类 = 按它的模板过滤。
   *
   * 骨架同任务列表（ui.md 第五条：输入 → 结果 → 脚栏），挂在「归类」按钮底下（第十二条：由控件点开的挂在控件底下，不压暗）。
   * 大文件上归类要跑几秒：边归边画（Rust 每 26 万行发布一次快照），进度在状态栏的进度格里（第十一条：别处不画进度）。
   *
   * **对比**：脚栏「和另一份比…」选基准（昨天的、上线前的），两边放进同一棵树里归，每一类说「新出现 / 没了 / ×8 / ÷3 / 持平」，
   * 按这个排（Rust 的 `change_rank`）。点一类照样是按它过滤这一份。
   */
  import { untrack } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import { logPatterns, logPatternsCompare, pickFiles } from "../ipc/log";
  import { notify } from "../state/notify.svelte";
  import type { LogPatterns } from "../ipc/commands";
  import { progress, type TaskHandle } from "../state/progress.svelte";
  import { templateParts, templateShown, changeLabel } from "./pattern-query";
  import { toBody } from "./to-body";

  let {
    handle,
    encoding = "utf-8",
    anchor,
    dir = null,
    onPick,
    onClose,
  }: {
    handle: number;
    encoding?: string;
    /** 按钮左下角（屏幕坐标） */
    anchor: { x: number; y: number };
    /** 选基准时面板从哪个目录开（这份日志所在的目录：昨天那份多半就在旁边）；合并视图没有 */
    dir?: string | null;
    /** 点了一类：模板原文 */
    onPick: (template: string) => void;
    onClose: () => void;
  } = $props();

  /** 一次最多画多少类：再往下是长尾，没人翻；搜索在这些里面搜（Rust 侧留了前 1000） */
  const LIMIT = 300;

  let data = $state<LogPatterns | null>(null);
  let error = $state("");
  let q = $state("");
  let sel = $state(0);
  /** 对比的基准（几份文件的路径）；null 是不对比 */
  let base = $state<string[] | null>(null);
  const nameOf = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  // 模板是原始字节：按这个标签的编码解（GBK 的日志模板是 GBK 的字节，同日志行）
  // svelte-ignore state_referenced_locally
  const decoder = (() => {
    try {
      return new TextDecoder(encoding);
    } catch {
      return new TextDecoder("utf-8");
    }
  })();
  const rows = $derived.by(() => {
    if (!data) return [];
    const total = Math.max(1, data.lines);
    const needle = q.trim().toLowerCase();
    return data.patterns
      .map((p) => {
        const text = decoder.decode(new Uint8Array(p.template));
        // 主级别：数量最多的那一级（一样多时靠前的级别 —— ERROR 先）
        let lvl = 0;
        p.levels.forEach((n, i) => (n > p.levels[lvl] ? (lvl = i) : 0));
        const change = base && data ? changeLabel(p.count, p.base, data.lines, data.baseLines) : null;
        return { text, count: p.count, base: p.base, pct: (p.count * 100) / total, lvl, change };
      })
      .filter((r) => !needle || r.text.toLowerCase().includes(needle));
  });
  $effect(() => {
    rows.length;
    if (sel >= rows.length) sel = Math.max(0, rows.length - 1);
  });

  /** 第几次 load：换了模式（开始 / 不再对比）时旧的那个轮询自己走人，不和新的抢着写 data */
  let gen = 0;

  async function load(restart: boolean) {
    const my = ++gen;
    const b = base;
    error = "";
    let task: TaskHandle | null = null;
    try {
      for (let first = true; ; first = false) {
        const d = b ? await logPatternsCompare(handle, b, LIMIT, restart && first) : await logPatterns(handle, LIMIT, restart && first);
        if (dead || my !== gen) return;
        data = d;
        if (d.done) break;
        const percent = d.totalBytes > 0 ? Math.floor((d.scannedBytes * 100) / d.totalBytes) : 0;
        if (task) task.set({ percent });
        else task = progress.start(b ? "对比日志" : "归类日志", { percent });
        await new Promise((r) => setTimeout(r, 150));
        if (dead || my !== gen) return;
      }
    } catch (e) {
      if (!dead) error = String(e);
    } finally {
      task?.end();
    }
  }

  let dead = false;
  /*
   * 挂载时起一次。**`load` 要套在 `untrack` 里**：它同步读了 `base`，effect 的依赖是第一次跑时读到的全部信号（frontend.md「effect 的依赖集」）——
   * 不套的话点「和另一份比」一改 `base`，这个 effect 当场重跑，它的 cleanup 把 `dead` 置位，之后每一次 load 都在第一个 await 后面走人，
   * 浮层永远停在「正在归类…」（2026-10-14 在浏览器里撞上的）
   */
  $effect(() => {
    untrack(() => void load(false));
    return () => {
      dead = true;
    };
  });

  function pick(i: number) {
    const r = rows[i];
    if (!r) return;
    // 只在基准里有的类：这一份里一条都没有，按它筛只会得到一个空视图
    if (r.count === 0) {
      notify.hint("此类别仅存在于基准日志中，当前日志中没有");
      return;
    }
    onPick(r.text);
  }

  /** 选基准、开始对比；取消面板就什么都不变 */
  async function startCompare() {
    const picked = await pickFiles("和哪份比？选昨天的、上线前的那份（可以多选）", dir).catch(() => [] as string[]);
    if (picked.length === 0) return;
    base = picked;
    data = null;
    sel = 0;
    void load(false);
  }

  function stopCompare() {
    base = null;
    data = null;
    sel = 0;
    void load(false);
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = rows.length;
      if (n) sel = (sel + (e.key === "ArrowDown" ? 1 : n - 1)) % n;
      document.querySelector(`.pattern-pop [data-i="${sel}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick(sel);
    }
  }

  const fmt = (n: number) => n.toLocaleString("en-US");
  const LVL = ["error", "warn", "info", "debug", "trace", "other"];
  // 600 宽的浮层别伸出窗口右边
  const left = $derived(Math.max(8, Math.min(anchor.x, window.innerWidth - 648)));
</script>

<div class="layer" use:toBody>
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={onClose}></div>
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="popup palette m pattern-pop"
    role="dialog"
    tabindex="-1"
    aria-label="日志归类"
    style:left="{left}px"
    style:top="{anchor.y + 4}px"
    onkeydown={onKey}
  >
    <div class="input">
      <Icon name="search" />
      <input bind:value={q} oninput={() => (sel = 0)} placeholder="在模板里找…" aria-label="搜模板" spellcheck="false" />
    </div>
    {#if base}
      <div class="vs">
        <span class="lbl">对比</span>
        <span class="files" title={base.join("\n")}>这一份 ↔ {base.map(nameOf).join("、")}</span>
        {#if data}<span class="tot">{fmt(data.lines)} 条 ↔ {fmt(data.baseLines)} 条</span>{/if}
        <span class="gap"></span>
        <button class="btn sm quiet" onclick={stopCompare}>不比了</button>
      </div>
    {/if}
    <div class="results" role="listbox">
      {#each rows as r, i (r.text)}
        <button
          class="row"
          class:on={i === sel}
          data-i={i}
          role="option"
          aria-selected={i === sel}
          onmouseenter={() => (sel = i)}
          onclick={() => pick(i)}
          title={r.text}
        >
          <i class="dot {LVL[r.lvl]}"></i>
          {#if r.change}
            <span class="chg {r.change.kind}">{r.change.text}</span>
            <span class="ab">{fmt(r.base)} → {fmt(r.count)}</span>
          {:else}
            <span class="n">{fmt(r.count)}</span>
            <span class="pct">{r.pct >= 0.1 ? r.pct.toFixed(1) : "<0.1"}%</span>
          {/if}
          <span class="tpl">{#each templateParts(templateShown(r.text)) as p, k (k)}{#if p.wild}<b>{p.text}</b>{:else}{p.text}{/if}{/each}</span>
        </button>
      {/each}
      {#if error}
        <div class="empty-msg bad">{error}</div>
      {:else if data && data.done && rows.length === 0}
        <div class="empty-msg">{q ? "没有含这几个字的模板。换个词，或者清掉再看" : "这份日志里没有能归类的行（空的，或者只有堆栈）"}</div>
      {:else if !data}
        <div class="empty-msg wait"><span class="spinner"></span>正在归类…</div>
      {/if}
    </div>
    <div class="foot">
      {#if data}
        <span class="sum">
          {fmt(data.total)} 类 · {fmt(data.lines)} 条记录{data.done ? "" : "（还在归）"}{#if data.total > data.patterns.length}，列出最多的 {fmt(data.patterns.length)} 类{/if}{#if data.overflow > 0}，<em>还有 {fmt(data.overflow)} 条没归进去（类数到上限了）</em>{/if}
        </span>
      {/if}
      <span class="gap"></span>
      <span><kbd>↵</kbd> 按这一类过滤</span>
      {#if !base}
        <button class="btn sm quiet" onclick={() => void startCompare()} title="和昨天的、上线前的日志比：新出现了哪几类、哪一类涨了、哪一类没了">和另一份比…</button>
      {/if}
      <button class="btn sm quiet" disabled={!data?.done} onclick={() => void load(true)} title="日志长了、或者想从头再归一遍">重新归类</button>
    </div>
  </div>
</div>

<style>
  .layer { display: contents; }
  .scrim { z-index: 59; }
  /* 宽度在 app.css 的 `.palette.m`；挂在按钮底下，所以把居中的 top / left / transform 关掉（ui.md 第十二条之二 `anchored`） */
  .pattern-pop {
    z-index: 60;
    transform: none;
    display: flex;
    flex-direction: column;
    max-height: min(62vh, 560px);
    overflow: hidden;
    outline: none;
  }
  .input {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-soft);
  }
  .input input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: var(--fs-lg);
  }
  .results { overflow-y: auto; padding: 4px 6px 6px; min-height: 60px; }
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
    cursor: default;
    white-space: nowrap;
  }
  .row.on { background: var(--selected); color: var(--text); }
  .dot { width: 6px; height: 6px; border-radius: 50%; flex: none; background: var(--text-faint); }
  .dot.error { background: var(--lvl-error); }
  .dot.warn { background: var(--lvl-warn); }
  .dot.info { background: var(--lvl-info); }
  .dot.debug, .dot.trace { background: var(--lvl-debug); }
  /* 次数和占比：右对齐、等宽数字，一列扫下去比得出大小 */
  .n { flex: none; width: 7ch; text-align: right; font-variant-numeric: tabular-nums; color: var(--text); }
  .pct { flex: none; width: 5ch; text-align: right; font-variant-numeric: tabular-nums; color: var(--text-faint); font-size: var(--fs-sm); }
  /* 模板是日志原文的样子：等宽；变量压暗，一眼看出哪些是固定的话 */
  .tpl {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--code-font);
    font-size: var(--fs-sm);
  }
  .tpl b { font-weight: 400; color: var(--text-faint); }
  /* 对比：一条在输入框下面说「和谁比」；每行先说变化，再说两边各几次 */
  .vs {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px 4px 14px;
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    white-space: nowrap;
  }
  .vs .lbl { color: var(--text-faint); }
  .vs .files { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .vs .tot { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .chg { flex: none; width: 5ch; text-align: right; font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  /* 新出现 / 涨了是坏消息的颜色（多半是报错多了），没了 / 跌了是琥珀（要你看一眼：不该没的没了），持平最淡 */
  .chg.new, .chg.up { color: var(--lvl-error); }
  .chg.gone, .chg.down { color: var(--lvl-warn); }
  .chg.flat { color: var(--text-faint); }
  .ab { flex: none; width: 15ch; text-align: right; color: var(--text-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .foot {
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 5px 8px 5px 14px;
    border-top: 1px solid var(--border-soft);
    color: var(--text-faint);
    font-size: var(--fs-xs);
  }
  .sum { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-variant-numeric: tabular-nums; }
  .sum em { font-style: normal; color: var(--lvl-warn); }
  .gap { flex: 1; }
  kbd { font-family: var(--ui-font); color: var(--text-dim); }
</style>
