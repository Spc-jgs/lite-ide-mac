<script lang="ts">
  /**
   * git 要凭据（#62）：第一次连一个新远程、钥匙串里还没有它的凭据时。提示串是 git 原话（`Username for 'https://git.example.com':`），
   * 照 IDEA / VS Code 原样给人看 —— 它说清了是哪个地址、要的是什么。密码一类不回显。
   * 答案只交给那条 git（经 `git_askpass_answer`），不存；存不存进钥匙串由你的 git 配置（credential.helper）决定
   */
  import { imeKey } from "../shell/ime";

  let { prompt, secret, onanswer, onclose }: { prompt: string; secret: boolean; onanswer: (v: string | null) => void; onclose: () => void } = $props();
  let value = $state("");

  function answer(v: string | null) {
    onanswer(v);
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      answer(null);
    } else if (e.key === "Enter") {
      e.preventDefault();
      answer(value);
    }
  }

  function focusIn(el: HTMLInputElement) {
    setTimeout(() => el.focus({ preventScroll: true }), 0);
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog askpass" role="dialog" aria-label="Git 凭据" tabindex="-1" onkeydown={onKey}>
    <p class="title">Git 需要凭据</p>
    <p class="msg">{prompt}</p>
    {#if secret}
      <input class="field" type="password" autocomplete="off" spellcheck="false" bind:value use:focusIn />
    {:else}
      <input class="field" type="text" autocomplete="off" spellcheck="false" bind:value use:focusIn />
    {/if}
    <p class="hint">仅用于本次操作。是否保存到钥匙串由 Git 的 credential.helper 配置决定</p>
    <div class="actions">
      <span class="gap"></span>
      <button class="btn" onclick={() => answer(null)}>取消</button>
      <button class="btn primary" onclick={() => answer(value)}>确定</button>
    </div>
  </div>
</div>

<style>
  /* 外层和输入框同数据库的连接对话框（各对话框各写一份，ui.md 二之二） */
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .askpass { width: min(440px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 8px; }
  .msg { margin: 0; color: var(--text-muted); font-family: var(--code-font); font-size: var(--fs-sm); overflow-wrap: anywhere; }
  .field { height: 28px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-md); outline: none; }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .hint { margin: 0; color: var(--text-faint); font-size: var(--fs-sm); }
</style>
