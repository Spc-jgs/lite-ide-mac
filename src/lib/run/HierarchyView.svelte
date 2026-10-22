<script lang="ts">
  /**
   * 层级工具窗（docs/TAKEOVER.md G10）：Java 文件里光标放在方法上按 ⌃⌥H（调用层级）、类型上按 ⌃H（类型层级）打开。
   * 照 IDEA 的 Hierarchy 窗：上面切方向（谁调了它 / 它调了谁；父类型 / 子类型），下面一棵懒展开的树。
   *
   * 单击选中、双击或 ↵ 跳过去（「谁调了它」那一层跳到**调用的那一行**，不是那个方法的开头 —— IDEA 也是这样，你要看的是怎么调的）。
   * 键盘：↑↓ 走，→ 展开 / 进第一个孩子，← 收起 / 回父节点。状态在 `state/hierarchy.svelte.ts`
   */
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import { hierarchy, type HierDir, type HNode } from "../state/hierarchy.svelte";
  import { nav } from "../state/nav.svelte";

  let { root }: { root: string | null } = $props();

  let list: HTMLElement | undefined = $state();

  const KIND: Record<number, string> = { 5: "类", 6: "方法", 9: "构造", 10: "枚举", 11: "接口", 12: "函数" };
  const DIRS: { id: HierDir; label: string; title: string }[] = $derived(
    hierarchy.types
      ? [
          { id: "sub", label: "子类型", title: "谁继承了它 / 实现了它" },
          { id: "super", label: "父类型", title: "它继承的、实现的" },
        ]
      : [
          { id: "in", label: "调用方", title: "谁调了它（往上追：调用方的调用方…）" },
          { id: "out", label: "被调用", title: "它调了谁" },
        ],
  );
  const EMPTY: Record<HierDir, string> = { in: "没有谁调它", out: "它没调别的方法", super: "没有父类型（除了 Object）", sub: "没有谁继承 / 实现它" };

  const rows = $derived(hierarchy.rows);
  const curIdx = $derived(rows.findIndex((r) => r.id === hierarchy.cur));

  function go(n: HNode) {
    const at = n.node.site ?? n.node.at;
    const target = at.path === null
      ? { rel: "", uri: at.uri, label: at.label, line: at.line + 1, col: at.character + 1, why: "语言服务器" as const }
      : root && at.path.startsWith(`${root}/`)
        ? { rel: at.path.slice(root.length + 1), line: at.line + 1, col: at.character + 1, why: "语言服务器" as const }
        : null;
    if (target) void nav.jumpTo({ from: 0, to: 0, text: n.node.name, target });
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const n = rows[curIdx];
    if (!n) return;
    const pick = (i: number) => {
      const r = rows[Math.max(0, Math.min(rows.length - 1, i))];
      if (r) hierarchy.cur = r.id;
      queueMicrotask(() => list?.querySelector(".hrow.sel")?.scrollIntoView({ block: "nearest" }));
    };
    if (e.key === "ArrowDown") pick(curIdx + 1);
    else if (e.key === "ArrowUp") pick(curIdx - 1);
    else if (e.key === "ArrowRight") {
      if (!n.open && !n.cycle) void hierarchy.expand(n);
      else if (n.kids?.length) pick(curIdx + 1);
    } else if (e.key === "ArrowLeft") {
      if (n.open && n.depth > 0) hierarchy.collapse(n);
      else {
        // 回父节点：往上找第一个浅一层的
        for (let i = curIdx - 1; i >= 0; i--) if (rows[i].depth < n.depth) return pick(i);
      }
    } else if (e.key === "Enter") go(n);
    else return;
    e.preventDefault();
  }

  const fileOf = (n: HNode) => n.node.at.path?.slice(n.node.at.path.lastIndexOf("/") + 1) ?? n.node.at.label;
</script>

<div class="hier">
  {#if !hierarchy.top}
    <div class="empty-msg">
      Java 文件里把光标放在方法上按 <kbd>⌃⌥H</kbd> 看调用层级，放在类型上按 <kbd>⌃H</kbd> 看类型层级
    </div>
  {:else}
    <div class="bar">
      <div class="segs" role="tablist" aria-label="方向">
        {#each DIRS as d (d.id)}
          <button class="btn sm" class:on={hierarchy.dir === d.id} role="tab" aria-selected={hierarchy.dir === d.id} title={d.title} onclick={() => hierarchy.setDir(d.id)}>{d.label}</button>
        {/each}
      </div>
      <span class="what">{hierarchy.types ? "类型层级" : "调用层级"} · {hierarchy.top.node.name}</span>
    </div>
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="body" role="tree" aria-label={hierarchy.types ? "类型层级" : "调用层级"} tabindex="0" bind:this={list} onkeydown={onKey}>
      {#each rows as n (n.id)}
        <div
          class="hrow"
          class:sel={n.id === hierarchy.cur}
          role="treeitem"
          aria-selected={n.id === hierarchy.cur}
          aria-expanded={n.cycle ? undefined : n.open}
          tabindex="-1"
          style:padding-left="{8 + n.depth * 16}px"
          onclick={() => {
            hierarchy.cur = n.id;
            list?.focus({ preventScroll: true });
          }}
          ondblclick={() => go(n)}
          onkeydown={() => {}}
        >
          {#if n.cycle}
            <span class="tw"></span>
          {:else}
            <button
              class="tw ibtn xs"
              class:open={n.open}
              aria-label={n.open ? "收起" : "展开"}
              tabindex="-1"
              onclick={(e) => {
                e.stopPropagation();
                hierarchy.cur = n.id;
                if (n.open) hierarchy.collapse(n);
                else void hierarchy.expand(n);
              }}><Icon name="chevron-right" /></button>
          {/if}
          <span class="kind">{KIND[n.node.kind] ?? ""}</span>
          <span class="name">{n.node.name}</span>
          {#if n.node.detail}<span class="detail">{n.node.detail}</span>{/if}
          <span class="file">{fileOf(n)}{#if n.node.site}:{n.node.site.line + 1}{/if}</span>
          {#if n.cycle}<span class="note">递归</span>{/if}
          {#if n.loading}<span class="spinner sm"></span>{/if}
        </div>
        {#if n.open && !n.loading}
          {#if n.error}
            <div class="sub bad" style:padding-left="{40 + n.depth * 16}px">{n.error}</div>
          {:else if n.kids && n.kids.length === 0}
            <div class="sub" style:padding-left="{40 + n.depth * 16}px">{EMPTY[hierarchy.dir]}</div>
          {:else if n.kids && n.total > n.kids.length}
            <div class="sub" style:padding-left="{40 + n.depth * 16}px">一共 {n.total} 个，只列了前 {n.kids.length} 个</div>
          {/if}
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .hier { display: flex; flex-direction: column; height: 100%; background: var(--content-bg); }
  .bar { flex: none; display: flex; align-items: center; gap: 10px; padding: 4px 8px; border-bottom: 1px solid var(--border-soft); }
  .what { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sm); color: var(--text-faint); }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 4px 0 8px; outline: none; }
  .hrow {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    margin: 0 6px;
    padding-right: 8px;
    border-radius: var(--r-sm);
    font-size: var(--fs-sm);
    color: var(--text-dim);
    white-space: nowrap;
    cursor: default;
  }
  .hrow:hover { background: var(--hover); }
  .hrow.sel { background: var(--selected); color: var(--text); }
  .body:focus-visible .hrow.sel { outline: 1px solid var(--accent); outline-offset: -1px; }
  .tw { flex: none; width: 16px; height: 16px; display: inline-flex; }
  .tw :global(.icon) { transition: transform 70ms; }
  .tw.open :global(.icon) { transform: rotate(90deg); }
  @media (prefers-reduced-motion: reduce) { .tw :global(.icon) { transition: none; } }
  .kind { flex: none; min-width: 2.4em; font-size: var(--fs-xs); color: var(--text-faint); }
  .name { color: var(--text); }
  .detail, .file { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); }
  .file { margin-left: auto; flex: none; font-variant-numeric: tabular-nums; }
  .note { flex: none; font-size: var(--fs-xs); color: var(--lvl-warn); }
  .sub { height: 22px; display: flex; align-items: center; font-size: var(--fs-sm); color: var(--text-muted); }
  .sub.bad { color: var(--lvl-error); }
</style>
