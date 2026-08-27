<script lang="ts">
  import { untrack } from "svelte";
  import { listDir, type DirEntry } from "../ipc/commands";

  let {
    root,
    activePath,
    onOpen,
    onSearch,
    onCollapse,
  }: {
    root: string;
    activePath: string;
    onOpen: (path: string, isDir: boolean) => void;
    onSearch: () => void;
    onCollapse: () => void;
  } = $props();

  /**
   * 扁平化渲染：把展开的树拍平成一个带 depth 的列表，而不是递归组件。
   * 渲染就是一个 each，将来要给大仓库加虚拟滚动也直接可用。
   */
  interface Row {
    name: string;
    path: string;
    isDir: boolean;
    depth: number;
  }

  /** path → 子项。未加载过的目录不在表里，展开时才请求 */
  let children = $state(new Map<string, DirEntry[]>());
  let expanded = $state(new Set<string>());
  let loading = $state(new Set<string>());
  let error = $state("");

  async function load(dir: string) {
    if (children.has(dir) || loading.has(dir)) return;
    loading = new Set(loading).add(dir);
    try {
      const items = await listDir(dir, false);
      children = new Map(children).set(dir, items);
    } catch (e) {
      error = String(e);
    } finally {
      const l = new Set(loading);
      l.delete(dir);
      loading = l;
    }
  }

  // 换项目根：清空缓存重新加载。
  //
  // 写操作必须包在 untrack 里：load() 开头会读 children 判重，
  // 而本 effect 又写 children —— 不隔离就是自己依赖自己，直接 update depth 爆栈。
  $effect(() => {
    const r = root;
    untrack(() => {
      children = new Map();
      expanded = new Set([r]);
      void load(r);
    });
  });

  function toggle(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
      void load(path);
    }
    expanded = next;
  }

  /** 深度优先展开成扁平列表 */
  let rows = $derived.by(() => {
    const out: Row[] = [];
    const walk = (dir: string, depth: number) => {
      const items = children.get(dir);
      if (!items) return;
      for (const it of items) {
        out.push({ name: it.name, path: it.path, isDir: it.isDir, depth });
        if (it.isDir && expanded.has(it.path)) walk(it.path, depth + 1);
      }
    };
    walk(root, 0);
    return out;
  });

  const rootName = $derived(root.slice(root.lastIndexOf("/") + 1) || root);

  function click(row: Row) {
    if (row.isDir) toggle(row.path);
    else onOpen(row.path, false);
  }
</script>

<div class="tree">
  <div class="head">
    <span class="proj" title={root}>{rootName}</span>
    <span class="gap"></span>
    <button class="act" onclick={onSearch} title="在项目中搜索 ⌘⇧F" aria-label="搜索">
      <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
        <circle cx="7" cy="7" r="4.2" fill="none" stroke="currentColor" stroke-width="1.4" />
        <path d="M10.2 10.2 L13.5 13.5" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" />
      </svg>
    </button>
    <button class="act" onclick={onCollapse} title="收起侧边栏 ⌘1" aria-label="收起侧边栏">
      <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
        <path d="M9.5 3.5 L5 8 L9.5 12.5" fill="none" stroke="currentColor" stroke-width="1.5"
              stroke-linecap="round" stroke-linejoin="round" />
        <path d="M12.5 3.5 L12.5 12.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" />
      </svg>
    </button>
  </div>
  <div class="list">
    {#each rows as row (row.path)}
      <button
        class="row"
        class:dir={row.isDir}
        class:active={row.path === activePath}
        style:padding-left="{6 + row.depth * 13}px"
        onclick={() => click(row)}
        title={row.name}
      >
        {#if row.isDir}
          <span class="caret" class:open={expanded.has(row.path)}>▸</span>
        {:else}
          <span class="caret spacer"></span>
        {/if}
        <span class="name">{row.name}</span>
      </button>
    {/each}
    {#if error}<div class="err">{error}</div>{/if}
  </div>
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    flex: none;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .proj { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .head .gap { flex: 1; min-width: 6px; }
  .act {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-faint);
    cursor: default;
  }
  .act:hover { background: var(--panel-bg-2); color: var(--text); }
  .act:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .list { flex: 1; overflow-y: auto; padding: 4px 0; }
  .row {
    display: flex;
    align-items: center;
    gap: 3px;
    width: 100%;
    height: 22px;
    padding-right: 8px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .row:hover { background: var(--panel-bg-2); }
  .row.active { background: var(--accent-sel); color: var(--text); }
  .row.dir { color: var(--text); }
  .row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .caret {
    flex: none;
    width: 11px;
    font-size: 9px;
    color: var(--text-faint);
    transition: transform 0.1s;
  }
  .caret.open { transform: rotate(90deg); }
  .caret.spacer { visibility: hidden; }
  .name { overflow: hidden; text-overflow: ellipsis; }
  .err {
    padding: 8px 10px;
    color: var(--lvl-error);
    font-size: 11.5px;
    font-family: var(--code-font);
  }
  @media (prefers-reduced-motion: reduce) { .caret { transition: none; } }
</style>
