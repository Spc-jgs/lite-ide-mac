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

  let {
    open = $bindable(false),
    current,
    logMode = false,
  }: {
    open?: boolean;
    current: { line: number; col: number } | null;
    /**
     * 日志视图里 ⌘L（2026-09-21）：输的是行号，或者时间 `14:32` / `14:32:05`。
     * 两种视图对 `:` 的解释不一样 —— 编辑器里是「行:列」，日志里是时分秒；
     * 日志没有「列」这回事，所以不冲突。
     */
    logMode?: boolean;
  } = $props();

  let text = $state("");
  let input = $state<HTMLInputElement | null>(null);

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
  <div class="box" role="dialog" aria-modal="true" aria-label={logMode ? "跳到行或时间" : "跳到行"}>
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
  .scrim { position: fixed; inset: 0; z-index: 40; }
  .box {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: 260px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 10px 12px;
    /* 浮层不透明 —— 桌面在 webview 之外，半透明只会让壁纸清晰地穿过来 */
    background: var(--elevated);
    border: var(--island-border); /* M8：浮层边线降一档，靠内高光勾边 */
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop), inset 0 0 0 0.5px rgba(255, 255, 255, 0.06);
    z-index: 41;
  }
  label { display: flex; align-items: center; gap: 10px; }
  .lbl { flex: none; font-size: 12px; color: var(--text-dim); }
  input {
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 8px;
    background: var(--content-solid);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: 13px;
    outline: none;
  }
  input:focus { border-color: var(--accent); }
  .hint { font-size: 11px; color: var(--text-faint); }
  .hint.bad { color: var(--lvl-warn); }
</style>
