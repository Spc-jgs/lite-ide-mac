<script lang="ts">
  import Icon from "./Icon.svelte";
  import { diag } from "../ipc/commands";

  let {
    error,
    /** 出错的是哪一块，用来告诉用户「什么还能用」 */
    scope = "界面",
    onReset,
  }: {
    error: unknown;
    scope?: string;
    onReset?: () => void;
  } = $props();

  /**
   * 把异常摊平成一段能直接贴给别人的文本。
   *
   * release 构建没有 devtools —— WebView 里出了错，用户能拿到的就只有这块屏幕。
   * 所以这里要把「定位问题真正需要的东西」一次给全：消息、调用栈、
   * 构建时间（否则又要花半天才发现对方跑的是旧构建）。
   */
  let detail = $derived.by(() => {
    const e = error as { message?: string; stack?: string } | null;
    const lines = [
      `位置：${scope}`,
      `构建：${__BUILD_TIME__}`,
      `消息：${e?.message ?? String(error)}`,
    ];
    if (e?.stack) lines.push("", e.stack);
    return lines.join("\n");
  });

  let message = $derived(
    (error as { message?: string } | null)?.message ?? String(error),
  );

  let copied = $state(false);

  // 同时回传一份到 Rust 侧 stderr（LITE_IDE_DEBUG=1 时可见）
  $effect(() => {
    void diag(`crash [${scope}] ${detail}`);
  });

  async function copy() {
    try {
      await navigator.clipboard.writeText(detail);
      copied = true;
      setTimeout(() => (copied = false), 1800);
    } catch {
      // 剪贴板被拒时退回到「全选那块文本」，至少让人能手动复制
      const el = document.querySelector(".crash pre");
      if (el) {
        const r = document.createRange();
        r.selectNodeContents(el);
        const s = window.getSelection();
        s?.removeAllRanges();
        s?.addRange(r);
      }
    }
  }
</script>

<div class="crash">
  <div class="box">
    <div class="head">
      <Icon name="warn" size={16} />
      <span>{scope}出错了</span>
    </div>

    <p class="msg">{message}</p>
    <p class="hint">
      其余部分应该还能用。{#if onReset}可以先重试这一块；{/if}实在不行就重载窗口 ——
      <b>未保存的改动会丢</b>。
    </p>

    <pre>{detail}</pre>

    <div class="row">
      {#if onReset}
        <button class="primary" onclick={onReset}>重试这一块</button>
      {/if}
      <button onclick={copy}>{copied ? "已复制" : "复制详情"}</button>
      <span class="gap"></span>
      <button class="danger" onclick={() => location.reload()}>重载窗口</button>
    </div>
  </div>
</div>

<style>
  .crash {
    height: 100%;
    display: grid;
    place-content: center;
    padding: 24px;
    overflow: auto;
    /* place-content: center 下子项默认按内容定宽，得让它铺满可用宽度 */
    justify-items: center;
    grid-template-columns: minmax(0, 1fr);
    background: var(--editor-bg);
  }
  .box {
    /*
     * 宽度要跟着**容器**走，不能用 vw —— 这块屏是渲染在内容区里的，
     * 而内容区可能只有窗口的一半宽（侧边栏 + 终端面板都开着的时候）。
     * 用 90vw 会直接从容器里溢出去，标题被切掉一截。
     */
    width: 100%;
    max-width: 680px;
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    padding: 18px 20px 16px;
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    color: var(--lvl-error);
    font-size: 14px;
    margin-bottom: 10px;
  }
  .msg {
    margin: 0 0 8px;
    font-family: var(--code-font);
    font-size: 12.5px;
    line-height: 1.6;
    color: var(--text);
    word-break: break-word;
  }
  .hint { margin: 0 0 12px; font-size: 12px; line-height: 1.7; color: var(--text-faint); }
  .hint b { color: var(--lvl-warn); font-weight: 500; }
  pre {
    margin: 0 0 14px;
    padding: 10px 12px;
    max-height: 220px;
    overflow: auto;
    background: var(--content-solid);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    font-family: var(--code-font);
    font-size: 11px;
    line-height: 1.65;
    color: var(--text-dim);
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }
  .row { display: flex; align-items: center; gap: 8px; }
  .row .gap { flex: 1; }
  button {
    padding: 4px 12px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  button:hover { background: var(--hover); color: var(--text); }
  button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  button.danger:hover { background: var(--lvl-error); border-color: var(--lvl-error); color: #fff; }
</style>
