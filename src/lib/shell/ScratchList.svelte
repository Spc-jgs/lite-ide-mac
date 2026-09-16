<script lang="ts">
  /**
   * 侧边栏的「草稿」视图（issue #40）：草稿目录里有什么，最近的在上。
   *
   * 为什么不是「把草稿目录当项目根打开」（原来「打开草稿目录」那条路）：
   * 那会切走当前项目 —— 翻一条笔记的代价是换工作区，多数人选不翻。
   * 这里点一条只是**叠一个标签**，文件树、git、终端都还在原地。
   *
   * 长相照文件树：30px 的头 + 内缩圆角行（ui.md 第一条）。每行两段：
   * 上面是第一行摘要（没有就显示文件名），下面是「文件名 · 多久之前」——
   * 翻回来时唯一记得的线索就是「大概什么时候记的、记了什么」。
   *
   * 按需加载：它和文件树一样只在侧边栏露出来时才需要。
   */
  import Icon from "./Icon.svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { scratches } from "../state/scratches.svelte";
  import { ago } from "../state/ago";
  import type { ScratchEntry } from "../ipc/commands";

  let {
    activePath,
    onOpen,
    onNew,
    onTrash,
    onReveal,
  }: {
    activePath: string;
    /** `keep` 为真是双击 —— 保留而不是预览，和文件树同一套约定 */
    onOpen: (path: string, keep: boolean) => void;
    onNew: () => void;
    onTrash: (path: string) => void;
    onReveal: (path: string) => void;
  } = $props();

  // 露出来就拉一次；之后由写盘的那几处自己叫 refresh
  $effect(() => {
    void scratches.refresh();
  });

  let menu = $state<{ x: number; y: number; row: ScratchEntry } | null>(null);
  let menuFrom: HTMLElement | null = null;

  function openMenu(e: MouseEvent | { clientX: number; clientY: number }, row: ScratchEntry, from: HTMLElement) {
    menuFrom = from;
    menu = { x: e.clientX, y: e.clientY, row };
  }
  function closeMenu(refocus: boolean) {
    menu = null;
    if (refocus) menuFrom?.focus();
  }

  const items = $derived.by<MenuItem[]>(() => {
    const row = menu?.row;
    if (!row) return [];
    return [
      { label: "打开", run: () => onOpen(row.path, true) },
      { label: "在 Finder 中显示", run: () => onReveal(row.path) },
      { label: "移到废纸篓", danger: true, sep: true, run: () => onTrash(row.path) },
    ];
  });

  /** 标签里显示的名字：`2026-09-10 1644.md` → `09-10 16:44` */
  function shortName(name: string): string {
    const m = /^(\d{4})-(\d{2})-(\d{2}) (\d{2})(\d{2})(?:-(\d+))?\.md$/.exec(name);
    if (!m) return name;
    return `${m[2]}-${m[3]} ${m[4]}:${m[5]}${m[6] ? ` (${m[6]})` : ""}`;
  }
</script>

<div class="scratch">
  <div class="head">
    <span class="title">草稿</span>
    <span class="gap"></span>
    <button class="hb" onclick={onNew} title="新建草稿 ⌘N" aria-label="新建草稿">
      <Icon name="plus" size={14} />
    </button>
  </div>
  <div class="list">
    {#if scratches.loaded && scratches.list.length === 0}
      <!-- 空态要给下一步，不是给句号（ui.md 第六条） -->
      <div class="empty">还没有草稿 —— ⌘N 记第一条</div>
    {/if}
    {#each scratches.list as row (row.path)}
      <button
        class="row"
        class:active={row.path === activePath}
        title={row.path}
        onclick={() => onOpen(row.path, false)}
        ondblclick={() => onOpen(row.path, true)}
        oncontextmenu={(e) => {
          e.preventDefault();
          openMenu(e, row, e.currentTarget as HTMLElement);
        }}
        onkeydown={(e) => {
          if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
            e.preventDefault();
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            openMenu({ clientX: r.left + 12, clientY: r.bottom - 2 }, row, e.currentTarget as HTMLElement);
          }
        }}
      >
        <span class="line" class:faint={row.firstLine === ""}>{row.firstLine || "（空）"}</span>
        <span class="meta">{shortName(row.name)} · {row.mtimeMs ? ago(row.mtimeMs / 1000) : ""}</span>
      </button>
    {/each}
  </div>
</div>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    title={menu.row.name}
    titleTip={menu.row.path}
    label="{menu.row.name} 的操作"
    {items}
    onclose={closeMenu}
  />
{/if}

<style>
  .scratch {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    overflow: hidden;
  }
  /* 头和文件树的一模一样：同一个侧边栏里两个视图的头长得不一样，人会以为是两种东西 */
  .head {
    flex: none;
    height: 38px; /* 同 FileTree（M8） */
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .head .gap { flex: 1; min-width: 6px; }
  .head .hb {
    flex: none;
    display: grid;
    place-content: center;
    width: 24px;
    height: 24px; /* M8：工具按钮统一 24 */
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    cursor: default;
  }
  .head .hb:hover { background: var(--hover); color: var(--text); }
  .head .hb:active { background: var(--pressed); }
  .head .hb:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .list { flex: 1; overflow: auto; padding: 4px 6px; }
  .empty { padding: 10px 6px; font-size: 12px; color: var(--text-faint); }
  /* 两行一条，内缩圆角块；当前项的长相和文件树、标签栏同一套（ui.md 第一条） */
  .row {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 1px;
    width: 100%;
    padding: 4px 8px;
    border: none;
    border-radius: var(--r-sm); /* M8 */
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  .row:hover { background: var(--hover); }
  .row.active { background: var(--selected); color: var(--text); }
  .row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text);
  }
  .row .line.faint { color: var(--text-faint); }
  .row .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--text-faint);
    font-family: var(--code-font);
  }
</style>
