<script lang="ts">
  /**
   * 快捷键速查（帮助 › 快捷键速查，⌘/）。
   *
   * # 为什么要有
   *
   * 这份表原来**只在空态那张卡片上出现过一次** —— 开了文件就再也找不到，
   * 而那恰恰是开始需要它的时候。菜单栏解决了大半，但有三样说不进菜单：
   *
   * - 「连按两下 ⇧」是手势，不是 accelerator
   * - ⌘B 是 ⌘1 的别名，而菜单一项只能挂一个 accelerator
   * - ⌘F / ⌥⌘F 是 CM6 给的，代码里一行都没写，别处一个字都不会出现
   *
   * # 骨架照 AGENTS.md 定死的那套
   *
   * 输入 → 分组结果 → 脚栏。和随处搜索、分支面板是同一副 ——
   * **第三个浮层了，不再另起一套。**
   */
  import { shortcuts, type KeyDef } from "../state/keymap";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let q = $state("");
  let box = $state<HTMLInputElement | null>(null);

  /*
   * 每次打开都从空过滤开始。
   *
   * 留着上次的过滤词的话，下次打开看到的是一张残缺的表，
   * 而人此刻的问题通常和上次不是同一个。
   */
  $effect(() => {
    if (open) {
      q = "";
      // 等浮层进 DOM 再聚焦。用 setTimeout 不用 rAF —— 后台标签页里
      // rAF 永远不回调（AGENTS.md 里那条）
      setTimeout(() => box?.focus(), 0);
    }
  });

  /**
   * 过滤。标签和键位都参与匹配 —— 「⌘」能筛出所有带 Command 的，
   * 「终端」能筛出终端那一摊。
   */
  let rows = $derived.by(() => {
    const needle = q.trim().toLowerCase();
    const all = shortcuts();
    if (!needle) return all;
    return all.filter((k) => {
      const hay = `${k.label} ${k.accel ?? ""} ${k.alias ?? ""} ${k.gesture ?? ""} ${k.group}`;
      return hay.toLowerCase().includes(needle);
    });
  });

  /**
   * 按 group 切成段。
   *
   * **按名字合并，不是遇到新名字就开一段。** KEYS 里同名分组可能不连续
   * （日志的 F3 两条也归「导航」，但它们排在 Git 那一摊前面）——
   * 不合并的话「导航」会印两遍，而且 `{#each ... (g.name)}` 当场就抛
   * `each_key_duplicate`，浮层整个打不开。
   *
   * 段内顺序仍是 KEYS 里的原始顺序，那个顺序就是显示顺序。
   */
  let groups = $derived.by(() => {
    const out: { name: string; items: KeyDef[] }[] = [];
    for (const k of rows) {
      const hit = out.find((g) => g.name === k.group);
      if (hit) hit.items.push(k);
      else out.push({ name: k.group, items: [k] });
    }
    return out;
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    }
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (open = false)} role="presentation"></div>
  <div class="popup" role="dialog" aria-modal="true" aria-label="快捷键速查">
    <div class="head">
      <svg class="ic" width="16" height="16" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <circle cx="7" cy="7" r="4.4" stroke="currentColor" stroke-width="1.4" />
        <path d="M10.3 10.3 14 14" stroke="currentColor" stroke-width="1.4" />
      </svg>
      <input
        bind:this={box}
        bind:value={q}
        onkeydown={onKey}
        placeholder="过滤，比如「终端」或「⌘」…"
        aria-label="过滤快捷键"
      />
    </div>

    <div class="list">
      {#if groups.length === 0}
        <div class="none">没有匹配</div>
      {/if}
      {#each groups as g (g.name)}
        <div class="sec">{g.name}</div>
        {#each g.items as k (k.id)}
          <div class="row">
            <span class="label">{k.label}</span>
            <span class="keys">
              {#if k.gesture}
                <kbd class="wide">{k.gesture}</kbd>
              {:else}
                <kbd>{k.accel}</kbd>
                <!-- 别名并排列出来：菜单里只写得下一个，这是唯一能把两个都说清的地方 -->
                {#if k.alias}<kbd>{k.alias}</kbd>{/if}
              {/if}
            </span>
          </div>
        {/each}
      {/each}
    </div>

    <div class="foot">
      <span><kbd>⌘/</kbd> 随时打开</span>
      <span class="gap"></span>
      <span><kbd>esc</kbd> 关闭</span>
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.35); z-index: 40; }
  .popup {
    position: fixed;
    top: 12vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(560px, 88vw);
    max-height: 70vh;
    display: flex;
    flex-direction: column;
    /* 浮层必须不透明：桌面在 webview 之外，backdrop-filter 模糊不到它 */
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    z-index: 41;
    overflow: hidden;
  }

  /* 输入排第一 —— 面板打开后的下一个动作永远是打字 */
  .head { display: flex; align-items: center; gap: 10px; padding: 13px 16px; }
  .head .ic { flex: none; color: var(--text-faint); }
  input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 15px;
    outline: none;
  }
  input::placeholder { color: var(--text-faint); }

  .list { overflow-y: auto; border-top: 1px solid var(--border-soft); padding-bottom: 4px; }
  .none { padding: 18px 16px; color: var(--text-faint); font-size: 12.5px; text-align: center; }

  /* 分组头吸顶：底色跟着所在层走，填死色会在滚动时拖出一条实心带 */
  .sec {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 9px 16px 4px;
    background: var(--elevated);
    font-size: 10.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
  }

  .row { display: flex; align-items: center; gap: 10px; padding: 5px 16px; font-size: 12.5px; }
  .label { color: var(--text-dim); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .keys { margin-left: auto; display: flex; gap: 5px; flex: none; }
  /*
   * **不许给 kbd 加 direction: rtl。**
   * 那行是给长路径做左省略用的，而 ⌘(U+2318) 在 bidi 里是中性字符 ——
   * 在 RTL 段落里它会跑到字母/数字右边，把 ⌘1 显示成 1⌘（v0.5.0 修过一次）。
   */
  kbd {
    font-family: var(--code-font);
    font-size: 11.5px;
    color: var(--text);
    background: var(--hover);
    border-radius: var(--r-sm);
    padding: 2px 7px;
    white-space: nowrap;
  }
  kbd.wide { color: var(--text-dim); }

  .foot {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 16px;
    border-top: 1px solid var(--border-soft);
    background: var(--chrome-scrim);
    font-size: 10.5px;
    color: var(--text-faint);
    user-select: none;
  }
  .foot .gap { flex: 1; }
  .foot kbd { font-size: 10px; padding: 1px 5px; color: var(--text-faint); }
</style>
