<script lang="ts">
  /**
   * TODO 工具窗（docs/TAKEOVER.md G18，IDEA 的 TODO tool window）：项目里注释中的 TODO / FIXME，按文件分组，点一行跳过去。
   * 开的时候搜一遍、点「刷新」再搜 —— 不跟着每次存盘重搜：它是「回头看还欠着什么」的清单，不是实时的报错。
   * 搜的是项目搜索那条路（rg，忽略规则同 ⇧⌘F）；哪些算、怎么分组在 `todo.ts`
   */
  import { grepProject } from "../ipc/search";
  import { nav } from "../state/nav.svelte";
  import { TODO_MAX, TODO_PATTERN, groupTodos, type TodoFile } from "./todo";

  let { root }: { root: string | null } = $props();

  let files = $state<TodoFile[]>([]);
  let loading = $state(false);
  let err = $state("");
  let truncated = $state(false);
  const total = $derived(files.reduce((n, f) => n + f.items.length, 0));

  async function load() {
    const r = root;
    if (!r) return;
    loading = true;
    err = "";
    try {
      const hits = await grepProject(r, TODO_PATTERN, { case: true, word: false, regex: true }, TODO_MAX + 1);
      truncated = hits.length > TODO_MAX;
      files = groupTodos(hits.slice(0, TODO_MAX));
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    void root;
    void load();
  });

  const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const dir = (p: string) => (p.includes("/") ? p.slice(0, p.lastIndexOf("/")) : "");
</script>

<div class="todo">
  <div class="bar">
    <span class="sum">{#if loading}<span class="wait"><span class="spinner sm"></span> 正在找…</span>{:else}{total} 处 · {files.length} 个文件{/if}</span>
    <span class="gap"></span>
    <button class="btn sm" disabled={loading || !root} class:busy={loading} onclick={() => void load()}>刷新</button>
  </div>
  <div class="body">
    {#if !root}
      <div class="empty-msg">打开一个项目才有 TODO 可列</div>
    {:else if err}
      <div class="empty-msg bad">{err}</div>
    {:else if !loading && !files.length}
      <div class="empty-msg">注释里没有 TODO / FIXME<span class="sub">只认注释里的大写 TODO、FIXME（// 、/* 、# 、&lt;!-- 后面），改了代码点「刷新」</span></div>
    {:else}
      {#each files as f (f.path)}
        <div class="file" title={f.path}><span class="fn">{name(f.path)}</span><span class="dir">{dir(f.path)}</span><span class="n">{f.items.length}</span></div>
        {#each f.items as t (t.line)}
          <button class="row" onclick={() => root && void nav.openAt(`${root}/${f.path}`, t.line, false)} title="跳到 {name(f.path)}:{t.line}">
            <span class="tag" class:fixme={t.tag === "FIXME"}>{t.tag}</span>
            <span class="msg">{t.text}</span>
            <span class="loc">{t.line}</span>
          </button>
        {/each}
      {/each}
      {#if truncated}<div class="none">只列了前 {TODO_MAX} 处，后面还有 —— 项目里的 TODO 比这多</div>{/if}
    {/if}
  </div>
</div>

<style>
  .todo { display: flex; flex-direction: column; height: 100%; background: var(--content-bg); }
  .bar { flex: none; display: flex; align-items: center; gap: 8px; padding: 4px 8px 4px 12px; border-bottom: 1px solid var(--border-soft); }
  .sum { font-size: var(--fs-sm); color: var(--text-muted); font-variant-numeric: tabular-nums; }
  .gap { flex: 1; }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 4px 0 8px; }
  .none { padding: 4px 12px; font-size: var(--fs-sm); color: var(--text-muted); }
  .file { position: sticky; top: 0; display: flex; align-items: baseline; gap: 8px; padding: 4px 12px 2px; font-size: var(--fs-sm); overflow: hidden; white-space: nowrap; background: var(--content-solid); }
  .fn { color: var(--text); }
  .dir { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); }
  .n { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .row { display: flex; align-items: center; gap: 8px; width: 100%; min-height: 24px; padding: 2px 12px 2px 28px; border: none; background: none; color: var(--text-dim); font: inherit; font-size: var(--fs-sm); text-align: left; cursor: pointer; }
  .row:hover { background: var(--hover); }
  .row:focus-visible { outline: none; background: var(--selected); }
  .tag { flex: none; font-size: var(--fs-xs); color: var(--accent); }
  .tag.fixme { color: var(--lvl-warn); }
  .msg { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .loc { flex: none; color: var(--text-faint); font-variant-numeric: tabular-nums; }
</style>
