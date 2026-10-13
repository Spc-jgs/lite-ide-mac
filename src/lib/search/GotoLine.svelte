<script lang="ts">
  /**
   * 跳到行（⌘L，IDEA 的键位）。输 `12` 或 `12:34`，↵ 跳，Esc 关。
   *
   * 照 IDEA 的 Go to Line:Column：一个小输入框，不是随处搜索那种大面板 ——
   * 它只回答一个数字。居中而不是从状态栏那格底下掉：⌘L 按下去时手在键盘上，
   * 眼睛在正文里，居中离正文最近（ui.md 第十二条的例外：没有「触发它的控件」
   * 这一说，键盘和状态栏两个入口都要落在同一处）。
   *
   * 跟着搜索浮层那组一起懒加载：单独一个 chunk 不值，但入口包每一 KB 都在
   * 红线边上数着（CI 卡 150 KiB），搭那组的车不多花一次往返。
   */
  import { nav } from "../state/nav.svelte";
  import { imeKey } from "../shell/ime";

  let {
    open = $bindable(false),
    current,
    logMode = false,
    anchor = null,
  }: {
    open?: boolean;
    current: { line: number; col: number } | null;
    /**
     * 日志视图里 ⌘L（2026-09-21）：输的是行号，或者时间 `14:32` / `14:32:05`。
     * 两种视图对 `:` 的解释不一样 —— 编辑器里是「行:列」，日志里是时分秒；
     * 日志没有「列」这回事，所以不冲突。
     */
    logMode?: boolean;
    /** 状态栏「行:列」那格的位置：从格子点开的挂在它上面，⌘L 开的居中 */
    anchor?: { x: number; y: number } | null;
  } = $props();

  let text = $state("");
  let input = $state<HTMLInputElement | null>(null);

  /*
   * 挂在状态栏那格上面时：钳进视口，抄 ContextMenu 的 `up` —— 读一次矩形、写一次 style，
   * 改 DOM 不写回 props。`y` 是格子上沿，浮层的下边贴着它。
   */
  let popEl = $state<HTMLDivElement | null>(null);
  $effect(() => {
    const e = popEl;
    const a = anchor;
    if (!e || !a) return;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(a.x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${Math.max(pad, a.y - r.height)}px`;
  });

  $effect(() => {
    if (!open) return;
    // 预填当前位置并全选：直接打数字就是覆盖，↵ 就是「回到这一行」
    text = current ? `${current.line}:${current.col}` : "";
    queueMicrotask(() => {
      input?.focus();
      input?.select();
    });
  });

  /** 日志视图里的时间：`14:32`、`14:32:05`、`14:32:05.442`，前面可带 `2026-08-24 ` */
  const TIME = /^\s*(?:\d{4}-\d{2}-\d{2}[ T])?(\d{2}):(\d{2})(?::(\d{2})(?:[.,]\d{1,9})?)?\s*$/;
  /** 时分秒的范围在框里就判掉：`25:99` 送到 Rust 解不出，报回来的却是「文件里没时间戳」（review 2026-09-21） */
  const timeOk = (m: RegExpExecArray) => Number(m[1]) <= 23 && Number(m[2]) <= 59 && Number(m[3] ?? 0) <= 60;

  /** `12` / `12:34` / `12,34` / `:34`（只改列）。别的一律当没输 */
  function parse(s: string): { line: number; col?: number } | { time: string } | null {
    if (logMode) {
      const t = TIME.exec(s);
      if (t) return timeOk(t) ? { time: s.trim() } : null;
      const n = /^\s*(\d+)\s*$/.exec(s);
      return n ? { line: Number(n[1]) } : null;
    }
    const m = /^\s*(\d*)\s*(?:[:,]\s*(\d+))?\s*$/.exec(s);
    if (!m || (m[1] === "" && m[2] === undefined)) return null;
    const line = m[1] === "" ? (current?.line ?? 1) : Number(m[1]);
    const col = m[2] === undefined ? undefined : Number(m[2]);
    if (line < 1 || (col !== undefined && col < 1)) return null;
    return col === undefined ? { line } : { line, col };
  }

  let target = $derived(parse(text));

  function go() {
    if (!target) return;
    if ("time" in target) nav.seekTime(target.time);
    else nav.goto(target.line, target.col);
    open = false;
  }

  function onKey(e: KeyboardEvent) {
    // 输入法在用这一下（确认候选字的回车、取消组字的 Esc）：不是给我们的（shell/ime.ts）
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "Enter") {
      e.preventDefault();
      go();
    }
  }
</script>

{#if open}
  <div class="scrim" onclick={() => (open = false)} role="presentation"></div>
  <div class="box popup palette s" class:anchored={!!anchor} bind:this={popEl} role="dialog" aria-modal="true" aria-label={logMode ? "跳到行或时间" : "跳到行"}>
    <label>
      <span class="lbl">{logMode ? "跳到" : "跳到行"}</span>
      <input
        bind:this={input}
        bind:value={text}
        onkeydown={onKey}
        placeholder={logMode ? "行号，或 14:32" : "行[:列]"}
        spellcheck="false"
        autocomplete="off"
      />
    </label>
    <span class="hint" class:bad={text.trim() !== "" && !target}>
      {text.trim() !== "" && !target
        ? (logMode ? "只认行号，或 14:32 / 14:32:05 这样的时间" : "只认「行」或「行:列」")
        : "↵ 跳过去 · Esc 关"}
    </span>
  </div>
{/if}

<style>
  .box {
    /* 位置和宽度在 app.css 的 `.palette`（上沿一条线、宽度三档，ui.md 第十二条之二）；面在 `.popup`。这里只剩排版 */
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
  }
  /* 挂在状态栏「行:列」格子上时：不居中、只有一个输入框那么宽，left/top 由 effect 写进 style */
  .box.anchored { top: 0; left: 0; transform: none; width: 260px; }
  label { display: flex; align-items: center; gap: 10px; }
  .lbl { flex: none; font-size: var(--fs-md); color: var(--text-dim); }
  input {
    flex: 1;
    min-width: 0;
    height: 28px;
    padding: 0 9px;
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(255, 255, 255, 0.08);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--ui-font); font-variant-numeric: tabular-nums;
    font-size: var(--fs-md);
    outline: none;
  }
  /* 聚焦：一圈柔和的强调色光晕（macOS 文本框的样子），不是把边描成实蓝 */
  input:focus { border-color: var(--accent); box-shadow: 0 0 0 3px rgba(10, 132, 255, 0.25); }
  .hint { font-size: var(--fs-sm); color: var(--text-faint); }
  .hint.bad { color: var(--lvl-warn); }
</style>
