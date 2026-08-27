<script lang="ts">
  import { untrack } from "svelte";
  import { listDir, type DirEntry, type GitEntry, type GitStatus } from "../ipc/commands";

  let {
    root,
    activePath,
    gitStatus = null,
    onOpen,
    onSearch,
    onGit,
    onCollapse,
  }: {
    root: string;
    activePath: string;
    /** 有仓库就给文件染色；没有就是 null，整块装饰不存在 */
    gitStatus?: GitStatus | null;
    onOpen: (path: string, isDir: boolean) => void;
    onSearch: () => void;
    /** 切到 Git 视图；不在仓库里时上层不传，按钮就不出现 */
    onGit?: () => void;
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

  /**
   * git 状态 → 绝对路径查找表。三样东西一起算，因为都要遍历同一份 entries：
   *
   * - `own`  —— 文件/目录**自身**的状态
   * - `roll` —— 祖先目录的「里面有东西改了」冒泡标记。IDE 里最有用的那个提示：
   *   目录收着也知道里面有动静
   * - `utDirs` —— 被折叠的未跟踪目录前缀。git 把整个未跟踪目录报成一条 `dir/`，
   *   里面的文件根本不在 entries 里，只能靠前缀匹配补上
   */
  let git = $derived.by(() => {
    const own = new Map<string, string>();
    const roll = new Set<string>();
    const utDirs: string[] = [];
    const st = gitStatus;
    if (!st) return { own, roll, utDirs };

    for (const e of st.entries) {
      const rel = e.isDir ? e.path.slice(0, -1) : e.path;
      const abs = `${st.root}/${rel}`;
      own.set(abs, klass(e));
      if (e.isDir) utDirs.push(`${abs}/`);
      // 一路冒泡到仓库根为止
      let p = abs;
      for (;;) {
        const i = p.lastIndexOf("/");
        if (i < 0) break;
        p = p.slice(0, i);
        if (p.length <= st.root.length) break;
        roll.add(p);
      }
    }
    return { own, roll, utDirs };
  });

  function klass(e: GitEntry): string {
    if (e.conflicted) return "conflict";
    if (e.untracked) return "untracked";
    // 工作区的状态更贴近「我现在看到的这个文件怎么了」，优先它
    const c = e.work !== "." && e.work !== " " ? e.work : e.index;
    switch (c) {
      case "A": return "added";
      case "D": return "deleted";
      case "R":
      case "C": return "renamed";
      default: return "modified";
    }
  }

  const LETTER: Record<string, string> = {
    modified: "M",
    added: "A",
    deleted: "D",
    untracked: "?",
    renamed: "R",
    conflict: "!",
  };

  /** 一行显示什么装饰：自身状态优先，其次未跟踪目录前缀，最后才是冒泡点 */
  function deco(path: string): { cls: string; ch: string } | null {
    const own = git.own.get(path);
    if (own) return { cls: own, ch: LETTER[own] ?? "·" };
    for (const d of git.utDirs) {
      if (path.startsWith(d)) return { cls: "untracked", ch: "?" };
    }
    if (git.roll.has(path)) return { cls: "roll", ch: "" };
    return null;
  }

  function click(row: Row) {
    if (row.isDir) toggle(row.path);
    else onOpen(row.path, false);
  }
</script>

<div class="tree">
  <div class="head">
    <span class="proj" title={root}>{rootName}</span>
    <span class="gap"></span>
    {#if onGit}
      <button class="act" onclick={onGit} title="Git ⌘⇧G" aria-label="Git">
        <svg viewBox="0 0 16 16" width="13" height="13" aria-hidden="true">
          <circle cx="4.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <circle cx="4.5" cy="12.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <circle cx="11.5" cy="3.5" r="1.8" fill="none" stroke="currentColor" stroke-width="1.3" />
          <path d="M4.5 5.3 L4.5 10.7" stroke="currentColor" stroke-width="1.3" />
          <path d="M11.5 5.3 Q11.5 8.5 4.5 10.7" fill="none" stroke="currentColor" stroke-width="1.3" />
        </svg>
      </button>
    {/if}
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
      {@const d = deco(row.path)}
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
        <span class="name g-{d?.cls ?? 'none'}">{row.name}</span>
        {#if d}
          <span class="gap"></span>
          {#if d.ch}
            <span class="gmark g-{d.cls}">{d.ch}</span>
          {:else}
            <!-- 目录自身没改，但里面有东西改了：一个点，不喧宾夺主 -->
            <span class="gdot" aria-label="内含改动"></span>
          {/if}
        {/if}
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
  .row .gap { flex: 1; min-width: 4px; }

  /* git 装饰：文件名染色 + 右端一个状态字母。
     两样都给是有意的 —— 颜色扫得快，字母说得准（红绿色觉障碍也读得出） */
  .gmark {
    flex: none;
    font-family: var(--code-font);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 1;
  }
  .gdot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--git-modified);
    opacity: 0.55;
  }
  .g-modified { color: var(--git-modified); }
  .g-added { color: var(--git-added); }
  .g-deleted { color: var(--git-deleted); }
  .g-untracked { color: var(--git-untracked); }
  .g-renamed { color: var(--git-renamed); }
  .g-conflict { color: var(--git-conflict); }
  /* 删除的文件划掉，但右端那个 D 字母不划 */
  .name.g-deleted { text-decoration: line-through; }
  .err {
    padding: 8px 10px;
    color: var(--lvl-error);
    font-size: 11.5px;
    font-family: var(--code-font);
  }
  @media (prefers-reduced-motion: reduce) { .caret { transition: none; } }
</style>
