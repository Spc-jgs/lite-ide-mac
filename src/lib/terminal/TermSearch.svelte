<script lang="ts">
  /**
   * 终端里的查找框（issue #34）。**长得和编辑器那个一样**（`editor/search-panel.ts`）：
   * 输入框里嵌开关和计数、右边上一个 / 下一个 / 关闭。样子照抄不复用代码 —— 那个是 CM6 的 Panel，
   * 靠 `EditorView.theme` 注入样式、靠 `runScopeHandlers` 转键；这里是普通 DOM。
   *
   * **浮在终端右上角，不把终端往下推。** 编辑器的面板是 `top: true` 挤开正文的，
   * 终端不能这么做：终端一变高 pty 就要 resize，vim / less 会整屏重排 ——
   * 为了一个搜索框让正在看的东西跳一下，不值。VS Code 的终端搜索也是浮的。
   *
   * 搜索本身在 `Terminal.svelte`（它持有 xterm 和 SearchAddon），这里只管
   * 「人输入了什么、按了什么」，通过回调报过去；命中计数从那边灌回来。
   */
  import Icon from "../shell/Icon.svelte";

  let {
    initial = "",
    count,
    onQuery,
    onNext,
    onPrev,
    onClose,
  }: {
    /** 上次关掉时框里的词：再按 ⌘F 多半还是找它，全选着放回来，一打就换掉 */
    initial?: string;
    /** 「3/12」「无匹配」那格；空串不显示 */
    count: string;
    onQuery: (term: string, opts: { caseSensitive: boolean; wholeWord: boolean; regex: boolean }) => void;
    onNext: () => void;
    onPrev: () => void;
    onClose: () => void;
  } = $props();

  let input = $state<HTMLInputElement | null>(null);
  // 只取初值是有意的：框开着的时候外面不会改它
  // svelte-ignore state_referenced_locally
  let term = $state(initial);
  let caseSensitive = $state(false);
  let wholeWord = $state(false);
  let regex = $state(false);

  // 边打边搜；开关一动也重搜
  $effect(() => {
    onQuery(term, { caseSensitive, wholeWord, regex });
  });

  /** 开着的时候再按 ⌘F：全选框里的字，一打就换掉（和编辑器那个 `select()` 同一个理由） */
  export function focus() {
    input?.focus();
    input?.select();
  }

  function onKey(e: KeyboardEvent) {
    /*
     * 这三个键在框里就地消化，**不许冒到终端**：Esc 冒过去 vim 会收到一个多余的
     * Esc，↵ 冒过去等于在 shell 里回车 —— 两种都不报错，只是「怎么多了一行提示符」。
     */
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      e.stopPropagation();
      if (e.shiftKey) onPrev();
      else onNext();
    } else if (e.metaKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === "f") {
      e.preventDefault();
      e.stopPropagation();
      focus();
    }
  }
</script>

<!-- 键盘处理挂在整个框上：点过开关之后焦点在开关按钮上，Esc / ↵ 照样要管用 -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="ts" role="search" onkeydown={onKey}>
  <div class="box">
    <input
      class="in"
      bind:this={input}
      bind:value={term}
      placeholder="在终端里查找"
      aria-label="在终端里查找"
      spellcheck="false"
    />
    {#if count}<span class="count">{count}</span>{/if}
    <!-- 开关点完把焦点还给输入框（编辑器那个也是）：接着按 ↵ 找下一个不用先点回去 -->
    <button class="tg" class:on={caseSensitive} type="button" title="区分大小写" onclick={() => { caseSensitive = !caseSensitive; input?.focus(); }}>Aa</button>
    <button class="tg" class:on={wholeWord} type="button" title="全词匹配" onclick={() => { wholeWord = !wholeWord; input?.focus(); }}>W</button>
    <button class="tg" class:on={regex} type="button" title="正则表达式" onclick={() => { regex = !regex; input?.focus(); }}>.*</button>
  </div>
  <!-- 箭头和 ✕ 走 Icon + .ibtn（ui.md 八），和编辑器的查找面板一起从字体符号换过来的 -->
  <button class="ibtn nav" type="button" title="上一个（⇧↵）" aria-label="上一个" onclick={onPrev}><Icon name="chevron-up" size={12} /></button>
  <button class="ibtn nav" type="button" title="下一个（↵）" aria-label="下一个" onclick={onNext}><Icon name="chevron-down" size={12} /></button>
  <button class="ibtn nav" type="button" title="关闭（esc）" aria-label="关闭" onclick={onClose}><Icon name="x" size={12} /></button>
</div>

<style>
  /*
   * 尺寸、颜色全对着 `search-panel.ts` 的 panelTheme 抄：框 26 高、开关 20、导航 24×26。
   * 浮层的外框按 ui.md 二之二：5% 描边 + 内高光，底是 --elevated。
   */
  .ts {
    position: absolute;
    top: 6px;
    right: 14px;
    max-width: calc(100% - 28px);
    /* 要压过 xterm 自己的层：命中高亮是 z-index 6、概览尺 8（xterm.css）。低于它们的话
       高亮块会盖在框上，点 Aa 点到的是高亮 —— 实测就是这样，浏览器里点了没反应。
       12 是因为它的滚动条壳是 11 */
    z-index: 12;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 5px 6px;
    background: var(--elevated);
    border: var(--island-border);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop), inset 0 0 0 0.5px rgba(255, 255, 255, 0.06);
    font-family: var(--ui-font);
  }
  .box {
    display: flex;
    align-items: center;
    gap: 2px;
    flex: 1;
    width: 260px;
    min-width: 120px;
    height: 26px;
    padding: 0 3px 0 8px;
    background: var(--elevated-hi);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
  }
  .box:focus-within { border-color: var(--accent); }
  .in {
    flex: 1;
    min-width: 0;
    height: 100%;
    padding: 0;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-family: var(--code-font);
    font-size: 12px;
  }
  .count {
    flex: none;
    padding: 0 4px;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 11px;
    white-space: nowrap;
  }
  .tg {
    flex: none;
    display: grid;
    place-content: center;
    width: 20px;
    height: 20px;
    padding: 0;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 1;
    cursor: default;
  }
  .tg:hover { background: var(--hover); color: var(--text-dim); }
  .tg.on { background: var(--accent); color: #fff; }
  /* 导航按钮就是 app.css 的 `.ibtn`，这里只在 26 高的行里摆正 */
  .nav { align-self: center; }
</style>
