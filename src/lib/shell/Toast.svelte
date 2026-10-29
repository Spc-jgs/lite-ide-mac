<script lang="ts">
  /**
   * 做完 / 没做成 / 什么都没发生的那一句：编辑区底部居中的胶囊（2026-10-13 盘点 B1，用户选的方案 B）。
   *
   * 原来画在状态栏最左那一格、顶掉面包屑 2.6 秒 —— 眼睛这时候在编辑区或者刚点的那块，左下角是视野最边上，
   * 很多提示没人看见就走了。照 macOS 的 HUD（Safari「已复制」、Xcode「构建成功」）：浮在内容上、不抢焦点、
   * 不挡正文主体（底边、居中、一行），一次只有一条。哪条该顶掉哪条、停多久由 `notify` 定，这里只管画和进退场。
   *
   * 鼠标停上去不走（人在读），离开再给剩下的时间；点一下关掉。
   * 在入口包里（启动时的失败也走它），所以只用入口里已有的东西：`Icon`、notify。
   */
  import { untrack } from "svelte";
  import Icon from "./Icon.svelte";
  import { notify, type Toast } from "../state/notify.svelte";

  /**
   * 退场 140ms 淡出要求「notify 已经清掉了，胶囊还在」—— 自己留一份。不用 Svelte 的 `out:` 过渡：
   * 那会把过渡的运行时拽进入口包（frontend.md：懒 chunk 用到的运行时函数也落在入口里）
   */
  let hovered = false, focused = false;
  function pause() { if (hovered || focused) notify.hold(); else notify.release(); }
  let shown = $state<Toast | null>(null);
  let leaving = $state(false);
  $effect(() => {
    const t = notify.toast;
    if (t) {
      hovered = false;
      focused = false;
      shown = t;
      leaving = false;
      return;
    }
    if (!untrack(() => shown)) return;
    leaving = true;
    const timer = setTimeout(() => {
      shown = null;
      leaving = false;
    }, 140);
    return () => clearTimeout(timer);
  });
</script>

<!-- 读屏：常驻一个 live 区域，内容换了它会念；失败用 alert 打断念 -->
<div class="host" aria-live="polite">
  {#if shown}
    {#key shown.id}
      <!-- svelte-ignore a11y_click_events_have_key_events a11y_no_static_element_interactions -->
      <div
        class="toast {shown.kind}"
        class:leaving
        role={shown.kind === "fail" ? "alert" : "status"}
        title={shown.text}
        onpointerenter={() => { hovered = true; pause(); }}
        onpointerleave={() => { hovered = false; pause(); }}
        onfocusin={() => { focused = true; pause(); }}
        onfocusout={(e) => { focused = e.currentTarget.contains(e.relatedTarget as Node); pause(); }}
        onclick={() => notify.dismiss()}
      >
        {#if shown.kind === "ok"}<span class="ic"><Icon name="check" /></span>
        {:else if shown.kind === "fail"}<span class="ic"><Icon name="x" /></span>{/if}
        <span class="tx">{shown.text}</span>
        {#if shown.act}
          {@const act = shown.act}
          <!-- 附的那条退路（「撤销」）：先关胶囊再做，点它不算「点掉」 -->
          <button class="act" onclick={(e) => (e.stopPropagation(), notify.dismiss(), act.run())}>{act.label}</button>
        {/if}
        <button class="ibtn xs close" aria-label="关闭通知" title="关闭通知" onclick={(e) => { e.stopPropagation(); notify.dismiss(); }}><Icon name="x" /></button>
      </div>
    {/key}
  {/if}
</div>

<style>
  /* 编辑岛（`.editor-island`，position: relative）的底边居中。卡片（30）之上、模态层（40）之下：先回答对话框，再操作通知 */
  .host {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 14px;
    z-index: 36;
    display: flex;
    justify-content: center;
    pointer-events: none;
  }
  .toast {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 7px;
    max-width: min(560px, calc(100% - 32px));
    padding: 6px 14px;
    border-radius: 16px;
    background: var(--elevated-hi);
    box-shadow: var(--shadow-pop), var(--pop-ring);
    color: var(--text);
    font-size: var(--fs-md);
    line-height: 1.4;
    cursor: default;
    animation: rise-in 140ms ease-out;
  }
  .toast.leaving { animation: fade-out 140ms ease-in forwards; }
  .toast.hint { color: var(--text-muted); }
  .ic { flex: none; display: inline-flex; }
  /* 胶囊里的动作：一个字按钮，强调色、不画底（ui.md 第十七条：浮层里的按钮不再套框） */
  .act { flex: none; margin: -3px -6px -3px 2px; padding: 3px 6px; border: none; border-radius: var(--r-sm); background: none; color: var(--accent); font: inherit; font-weight: 600; cursor: pointer; }
  .act:hover { background: var(--hover); }
  .ok .ic { color: var(--accent); }
  .fail .ic { color: var(--lvl-error); }
  @media (prefers-reduced-motion: reduce) {
    .toast, .toast.leaving { animation: none; }
  }
  /* 长消息有界滚动，全文可读；多步骤决策仍走卡片。 */
  .tx {
    min-width: 0;
    overflow-wrap: anywhere;
    max-height: 7em;
    overflow: auto;
  }
</style>
