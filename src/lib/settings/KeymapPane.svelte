<script lang="ts">
  /**
   * 设置里「键位」那一页（docs/TAKEOVER.md R3，IDEA 的 Settings › Keymap）：整张表按分组列出来，点键帽录一个新的键，
   * 撞了当场说和谁撞、不能改的说为什么。改的先攒在对话框里（`edits`），「应用 / 确定」才写进 keymap.json —— 同别的设置。
   *
   * **录键时把菜单上的键全摘掉**（`keymapRecord(true)`）：菜单项挂着的键 AppKit 先吃掉、页面收不到，不摘的话 ⌘S ⌘W 这种
   * 「已经有人用着的」永远录不进来 —— 而「把一个键从 A 挪给 B」正是改键位最常见的事。录完（按下、Esc、点走、关对话框）挂回去。
   */
  import { untrack } from "svelte";
  import { KEYS, accelFromEvent, accelOf, conflicts, type KeyDef } from "../state/keymap";
  import { judge, lockedWhy, matches, staged, type KeyEdits } from "./keymap-form";
  import { keymapRecord } from "../ipc/keymap";
  import { imeKey } from "../shell/ime";

  let {
    over,
    edits,
    onedit,
    onresetall,
  }: { over: Readonly<Record<string, string>>; edits: KeyEdits; onedit: (id: string, accel: string | null) => void; onresetall: () => void } = $props();

  let q = $state("");
  /** 「按键找」录到的键：只列键位正好是它的 */
  let exact = $state<string | null>(null);
  /** 正在给哪一行录键（"?find" = 按键找） */
  let rec = $state<string | null>(null);
  /** 录到的键不收时的那句话，挂在那一行下面 */
  let why = $state<{ id: string; text: string } | null>(null);

  const next = $derived(staged(over, edits));
  const clash = $derived(conflicts(next));
  const labelOf = new Map(KEYS.map((k) => [k.id, k.label]));
  const GROUPS = [...new Set(KEYS.map((k) => k.group))];
  const shown = $derived(KEYS.filter((k) => matches(k, next, q, exact)));
  const changedCount = $derived(Object.keys(next).length);

  /** 开始 / 停止录键：菜单的键跟着摘 / 挂（浏览器里没有菜单，命令是空的） */
  function record(id: string | null) {
    const was = rec;
    rec = id;
    why = null;
    if (!!was !== !!id) void keymapRecord(!!id).catch(() => {});
  }
  // 关对话框时还在录：把菜单的键挂回去
  $effect(() => () => {
    if (untrack(() => rec)) void keymapRecord(false).catch(() => {});
  });

  function onRecKey(e: KeyboardEvent, d: KeyDef | null) {
    if (imeKey(e)) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.key === "Escape" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) return record(null);
    const a = accelFromEvent(e);
    if (!a) return; // 只按了修饰键：等主键
    if (!d) {
      exact = a;
      q = "";
      return record(null);
    }
    const r = judge(a);
    if ("why" in r) {
      why = { id: d.id, text: r.why };
      return;
    }
    onedit(d.id, r.accel);
    record(null);
  }

  /** 和这一行撞的别的几行的名字 */
  function clashWith(d: KeyDef): string[] {
    const a = accelOf(d, next);
    const ids = a ? clash[a] : undefined;
    return ids ? ids.filter((x) => x !== d.id).map((x) => labelOf.get(x) ?? x) : [];
  }

  const focusNow = (n: HTMLElement) => {
    queueMicrotask(() => n.focus({ preventScroll: true }));
  };
</script>

<div class="bar">
  <input class="field" placeholder="按名字或键位找" spellcheck="false" bind:value={() => q, (v) => ((q = v), (exact = null))} aria-label="搜索键位" />
  {#if rec === "?find"}
    <button class="btn sm recording" use:focusNow onkeydown={(e) => onRecKey(e, null)} onblur={() => record(null)}>按下要找的键…</button>
  {:else if exact}
    <button class="btn sm" title="不按键找了" onclick={() => (exact = null)}>找 {exact} ✕</button>
  {:else}
    <button class="btn sm" title="按一下键，列出用着它的（IDEA 的 Find Shortcut）" onclick={() => record("?find")}>按键找</button>
  {/if}
  <span class="gap"></span>
  {#if changedCount}
    <span class="count">改过 {changedCount} 个</span>
    <button class="btn quiet sm" title="这一页全回到默认（应用之后才生效）" onclick={onresetall}>全部恢复默认</button>
  {/if}
</div>
<p class="doc">点键帽，按下新的键（Esc 取消）。菜单和编辑器里的键都改得了，改了当场生效、开着的编辑器也是；存在应用目录的 keymap.json，只存改过的。</p>

{#each GROUPS as g (g)}
  {@const rows = shown.filter((k) => k.group === g)}
  {#if rows.length}
    <h3 class="group">{g}</h3>
    {#each rows as d (d.id)}
      {@const a = accelOf(d, next)}
      {@const locked = lockedWhy(d)}
      {@const changed = d.id in next}
      {@const others = clashWith(d)}
      <div class="row" class:set={changed}>
        <span class="name" title={d.id}>{d.label}</span>
        {#if locked}
          <span class="cap locked" title={locked}>{d.gesture ?? d.accel ?? "—"}</span>
        {:else if rec === d.id}
          <button class="cap recording" use:focusNow onkeydown={(e) => onRecKey(e, d)} onblur={() => record(null)}>按下新的键…</button>
        {:else}
          <button class="cap" class:none={!a} class:bad={others.length > 0} title="点了按下新的键" onclick={() => record(d.id)}>{a ?? "—"}</button>
        {/if}
        <span class="acts">
          {#if !locked && a}<button class="btn quiet sm" title="这个动作不要键位" onclick={() => onedit(d.id, "")}>不要键位</button>{/if}
          {#if changed}<button class="btn quiet sm" title="回到 {d.accel ?? "没有键位"}" onclick={() => onedit(d.id, null)}>恢复默认</button>{/if}
        </span>
        {#if why?.id === d.id}<p class="msg">{why.text}</p>{/if}
        {#if others.length}<p class="msg">和「{others.join("」「")}」撞了：菜单的键会先被吃掉，编辑器里两个只有一个能用。改掉一个才能应用</p>{/if}
        {#if locked && !d.gesture}<p class="note">{locked}</p>{/if}
      </div>
    {/each}
  {/if}
{/each}
{#if shown.length === 0 && (q.trim() || exact)}
  <div class="empty-msg">{exact ? `没有动作用着 ${exact}` : `没有名字或键位里带「${q.trim()}」的`}</div>
{/if}

<style>
  .bar { display: flex; align-items: center; gap: 8px; }
  .bar .field { flex: 0 1 280px; }
  .gap { flex: 1; }
  .count { font-size: var(--fs-sm); color: var(--text-faint); }
  .field {
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--field-border);
    border-radius: var(--r-sm);
    background: var(--field-bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .doc { margin: 8px 0 4px; font-size: var(--fs-sm); line-height: 1.5; color: var(--text-muted); }
  .group {
    position: sticky;
    top: -16px;
    z-index: 1;
    margin: 14px 0 2px;
    padding: 4px 0;
    background: var(--elevated);
    font-size: var(--fs-xs);
    font-weight: normal;
    letter-spacing: 0.04em;
    color: var(--text-faint);
  }
  .row { position: relative; display: flex; flex-wrap: wrap; align-items: center; gap: 8px; min-height: 30px; padding: 2px 0 2px 12px; }
  /* 改过的：左边一条强调色（同别的设置项） */
  .row.set::before { content: ""; position: absolute; left: 0; top: 7px; height: 16px; width: 2px; border-radius: 99px; background: var(--accent); }
  .name { flex: 1 1 240px; min-width: 0; font-size: var(--fs-md); color: var(--text); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .cap {
    flex: none;
    min-width: 72px;
    height: 24px;
    padding: 0 8px;
    border: none;
    border-radius: var(--r-xs);
    background: var(--hover);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-sm);
    white-space: nowrap;
    cursor: pointer;
  }
  .cap:hover { background: var(--pressed); }
  .cap.none { color: var(--text-faint); }
  .cap.bad { color: var(--lvl-error); }
  .cap.locked { display: inline-flex; align-items: center; justify-content: center; background: none; color: var(--text-faint); cursor: default; }
  .cap.recording, .btn.recording { background: color-mix(in srgb, var(--accent) 22%, transparent); color: var(--text); box-shadow: 0 0 0 1.5px var(--accent); }
  .acts { flex: none; display: flex; gap: 4px; width: 172px; justify-content: flex-end; }
  .row:not(:hover):not(.set) .acts { visibility: hidden; }
  .msg { flex-basis: 100%; margin: 0 0 4px; font-size: var(--fs-sm); color: var(--lvl-error); }
  /* 不能改的理由：十来行一样的话常驻是噪音（ui.md 第三条），鼠标停上去才出 */
  .note { flex-basis: 100%; margin: 0 0 4px; font-size: var(--fs-xs); color: var(--text-faint); }
  .row:not(:hover) .note { display: none; }
</style>
