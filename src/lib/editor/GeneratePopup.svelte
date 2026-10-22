<script lang="ts">
  /**
   * 生成（⌃↵，照 IDEA 的 Generate；docs/TAKEOVER.md G5）：挂在光标底下。两步 ——
   * 先选生成什么（构造器 / Getter / Setter / Getter 和 Setter / equals() 和 hashCode() / toString() / 覆盖方法），
   * 再勾用哪些字段 / 覆盖哪些方法（问 jdtls 这个类里有什么，`java.rs` 的 `gen_status`），↵ 生成。
   *
   * 字段默认全勾（IDEA 也是）；覆盖方法默认只勾没实现的（接口、抽象类里的）—— Object 的 equals 那些勾上就是给自己找麻烦。
   * 由 java-lsp.ts 用 `mount()` 挂到 body 上（同 DocPopup：编辑器的格子有 `container-type`，挂在里面 fixed 的坐标不对）
   */
  import { imeKey } from "../shell/ime";
  import type { JavaGenChoices } from "../ipc/commands";
  import type { GenWhat } from "../ipc/java";

  let {
    x,
    y,
    load,
    apply,
    onclose,
  }: {
    x: number;
    y: number;
    load: (what: GenWhat) => Promise<JavaGenChoices>;
    apply: (what: GenWhat, raws: string[]) => Promise<void>;
    onclose: () => void;
  } = $props();

  const MENU: { what: GenWhat; label: string }[] = [
    { what: "constructor", label: "构造器" },
    { what: "getter", label: "Getter" },
    { what: "setter", label: "Setter" },
    { what: "accessors", label: "Getter 和 Setter" },
    { what: "equals", label: "equals() 和 hashCode()" },
    { what: "tostring", label: "toString()" },
    { what: "override", label: "覆盖方法…" },
  ];
  /** 一项都没有时为什么（「什么都没发生」要说为什么，ui.md 第十五条） */
  const NONE: Record<GenWhat, string> = {
    constructor: "",
    getter: "每个字段都有 getter 了（或者没有字段）",
    setter: "每个字段都有 setter 了（final 的字段没有 setter）",
    accessors: "每个字段都有 getter 和 setter 了",
    equals: "",
    tostring: "",
    override: "没有能覆盖的方法",
  };
  /** 一项不勾也能生成的：无参构造器、空的 toString / equals */
  const EMPTY_OK = new Set<GenWhat>(["constructor", "tostring", "equals"]);

  let cursor = $state(0);
  let what = $state<GenWhat | null>(null);
  let choices = $state<JavaGenChoices | null>(null);
  let checked = $state<boolean[]>([]);
  let busy = $state(false);
  let error = $state("");
  let el = $state<HTMLDivElement | null>(null);

  // 钳进视口，下面放不下翻到光标上面（同 DocPopup）。换到第二步时高度变了，再钳一次
  $effect(() => {
    const e = el;
    void choices;
    if (!e) return;
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${y + 4 + r.height > window.innerHeight - pad ? Math.max(pad, y - 22 - r.height) : y + 4}px`;
    e.focus({ preventScroll: true });
  });

  async function pick(w: GenWhat) {
    what = w;
    busy = true;
    error = "";
    try {
      const c = await load(w);
      choices = c;
      checked = c.items.map((i) => i.checked);
      cursor = 0;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  async function confirm() {
    if (!what || !choices || busy) return;
    const raws = choices.items.filter((_, i) => checked[i]).map((i) => i.raw);
    if (!raws.length && !EMPTY_OK.has(what)) return;
    busy = true;
    error = "";
    try {
      await apply(what, raws);
      onclose();
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const n = choices ? choices.items.length : MENU.length;
    if (e.key === "Escape") onclose();
    else if (e.key === "ArrowDown") cursor = n ? (cursor + 1) % n : 0;
    else if (e.key === "ArrowUp") cursor = n ? (cursor - 1 + n) % n : 0;
    else if (e.key === " " && choices) checked[cursor] = !checked[cursor];
    else if (e.key === "a" && e.metaKey && choices) checked = checked.map(() => !checked.every(Boolean));
    else if (e.key === "Enter") {
      if (choices) void confirm();
      else if (!busy) void pick(MENU[cursor].what);
    } else return;
    e.preventDefault();
    e.stopPropagation();
  }

  const count = $derived(checked.filter(Boolean).length);
  const title = $derived(MENU.find((m) => m.what === what)?.label ?? "");
</script>

<div class="scrim" onclick={onclose} role="presentation"></div>
<div class="popup gen" bind:this={el} tabindex="-1" role="dialog" aria-label="生成" onkeydown={onKey}>
  {#if !choices}
    <div class="ttl">生成</div>
    {#each MENU as m, i (m.what)}
      <button class="mi" class:sel={i === cursor} onmouseenter={() => (cursor = i)} onclick={() => void pick(m.what)} disabled={busy}>
        {m.label}{#if busy && what === m.what}<span class="spinner sm"></span>{/if}
      </button>
    {/each}
  {:else}
    <div class="ttl">{title}{#if choices.typeName}<span class="in">· {choices.typeName}</span>{/if}</div>
    {#if choices.note}<div class="note">{choices.note}</div>{/if}
    {#if choices.items.length === 0}
      <div class="empty-msg">{NONE[what!] || "这个类没有字段：会生成一个空的"}</div>
    {/if}
    <div class="list">
      {#each choices.items as it, i (it.raw)}
        <label class="mi" class:sel={i === cursor} onmouseenter={() => (cursor = i)}>
          <input type="checkbox" bind:checked={checked[i]} tabindex="-1" />
          <span class="nm">{it.label}</span>
          <span class="dt">{it.detail}</span>
        </label>
      {/each}
    </div>
    <div class="foot">
      <span class="keys"><kbd>空格</kbd> 勾选 · <kbd>⌘A</kbd> 全选 / 全不选</span>
      <button class="btn sm" onclick={onclose}>取消</button>
      <button class="btn sm primary" class:busy disabled={busy || (!count && choices.items.length > 0 && !EMPTY_OK.has(what!)) || (!choices.items.length && !EMPTY_OK.has(what!))} onclick={() => void confirm()}>生成{#if choices.items.length}（{count}）{/if}</button>
    </div>
  {/if}
  {#if error}<div class="err">{error}</div>{/if}
</div>

<style>
  .gen { position: fixed; z-index: 60; min-width: 240px; max-width: 460px; padding: 4px; outline: none; }
  .ttl { padding: 6px 10px 4px; font-size: var(--fs-sm); font-weight: 600; color: var(--text); }
  .ttl .in { margin-left: 6px; font-weight: 400; color: var(--text-faint); }
  .note { margin: 0 10px 4px; font-size: var(--fs-sm); color: var(--lvl-warn); }
  .list { max-height: 300px; overflow: auto; }
  .mi {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 10px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: default;
  }
  .mi.sel { background: var(--accent); color: #fff; }
  .mi.sel .dt { color: rgba(255, 255, 255, 0.8); }
  .nm { white-space: nowrap; }
  .dt { min-width: 0; margin-left: auto; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-sm); }
  .foot { display: flex; align-items: center; gap: 6px; padding: 8px 6px 4px; }
  .keys { margin-right: auto; font-size: var(--fs-xs); color: var(--text-faint); }
  .err { margin: 4px 10px; font-size: var(--fs-sm); color: var(--lvl-error); }
</style>
