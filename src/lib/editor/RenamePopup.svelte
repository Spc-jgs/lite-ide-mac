<script lang="ts">
  /**
   * 改名的输入框（docs/IDEA.md 第 14 节，⇧F6）：挂在光标底下，预填原名并全选，↵ 改、Esc 取消。
   *
   * IDEA 是在代码里原地改（所有用到的地方跟着一起变色）；这里先是一个挂在名字底下的小框 —— 原地改要在编辑器里另起一套「模板编辑」状态，
   * 而第一次改名 jdtls 要 4 秒才回来（要搜全部索引），框里打字时还不知道会改到哪几处。
   * ↵ 之后框立刻关掉，「正在改名」去状态栏的进度格（ui.md 第十一条：后台在跑什么只有那一格），不让一个框挂在那儿等。
   *
   * 由 java-lsp.ts `mount()` 到 body 上（同 UsagesPopup：编辑器那一格有 `container-type`，挂在里面 `position: fixed` 按格子算坐标）。
   */
  import { imeKey } from "../shell/ime";

  let {
    name,
    x,
    y,
    onrename,
    onclose,
  }: {
    /** 原来的名字（预填、全选） */
    name: string;
    /** 名字左下角的屏幕坐标 */
    x: number;
    y: number;
    onrename: (to: string) => void;
    onclose: () => void;
  } = $props();

  // svelte-ignore state_referenced_locally
  let value = $state(name);
  let el = $state<HTMLDivElement | null>(null);
  let input = $state<HTMLInputElement | null>(null);

  /** Java 的标识符：字母（含中文）、下划线、$ 开头，后面再加数字。不合法的当场说，不发给 jdtls 绕一圈 */
  const ID = /^[\p{L}_$][\p{L}\p{N}_$]*$/u;
  let problem = $derived(value === "" ? "" : !ID.test(value) ? "不是有效的标识符（以字母、下划线或 $ 开头，后面可包含数字）" : "");

  $effect(() => {
    const e = el;
    if (!e) return;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${y + 4 + r.height > window.innerHeight - pad ? Math.max(pad, y - 22 - r.height) : y + 4}px`;
    input?.focus({ preventScroll: true });
    input?.select();
  });

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter") {
      e.preventDefault();
      const to = value.trim();
      if (!to || problem) return;
      if (to === name) return onclose();
      onrename(to);
    }
  }
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup palette s rename" bind:this={el} role="dialog" aria-label="重命名 {name}">
  <div class="title">重命名 <b>{name}</b></div>
  <input class="field" bind:this={input} bind:value spellcheck="false" autocomplete="off" aria-label="新名称" onkeydown={onKey} />
  {#if problem}
    <div class="hint bad">{problem}</div>
  {:else}
    <div class="hint">↵ 重命名 · Esc 取消。所有引用一并更新（Java 类名同时重命名文件），完成后可撤销</div>
  {/if}
</div>

<style>
  .scrim { z-index: 59; }
  /* 宽度在 app.css 的 `.palette.s`；挂在名字底下，把居中的 top / left / transform 关掉（ui.md 第十二条 `anchored`） */
  .rename {
    z-index: 60;
    top: 0;
    left: 0;
    transform: none;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 12px 14px;
  }
  .title { font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .title b { font-family: var(--code-font); font-weight: 600; }
  /* 输入框：ui.md 二之二「输入框一个样子」，值都引令牌 */
  .field {
    height: 28px;
    padding: 0 8px;
    background: var(--field-bg);
    border: 1px solid var(--field-border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: var(--fs-md);
    outline: none;
  }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .hint { font-size: var(--fs-sm); color: var(--text-faint); line-height: 1.45; }
  .hint.bad { color: var(--lvl-error); }
</style>
