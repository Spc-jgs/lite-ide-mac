<script lang="ts">
  import { untrack } from "svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { copyText, relTo, showInFinder } from "./pathactions";

  export interface Tab {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log" | "diff" | "merge";
    dirty: boolean;
  }

  let {
    tabs,
    activeId,
    root = "",
    onSelect,
    onClose,
    onCloseMany,
    onRevealInTree,
  }: {
    tabs: Tab[];
    activeId: number | null;
    /** 项目根，只用来算「复制相对路径」 */
    root?: string;
    onSelect: (id: number) => void;
    onClose: (id: number) => void;
    /**
     * 批量关闭。**由 App 处理**，因为有未保存改动的标签要逐个问，
     * 而那个确认横幅长在 App 上。这里只负责算出「关哪些」。
     */
    onCloseMany?: (ids: number[]) => void;
    /** 在文件树里定位到这个标签对应的文件 */
    onRevealInTree?: (path: string) => void;
  } = $props();

  /**
   * 差异/合并标签的 path 是 `git-diff:xxx` 这类**合成 key**，不是盘上的路径。
   * 拿它去 Finder 里显示或者复制，给出来的是一串没用的东西 ——
   * 所以那几项只对真实文件出现。判据就是「以 / 开头」。
   */
  const isReal = (p: string) => p.startsWith("/");

  let menu = $state<{ x: number; y: number; tab: Tab; i: number } | null>(null);

  let items = $derived.by(() => {
    const m = menu;
    if (!m) return [] as MenuItem[];
    const { tab, i } = m;
    const out: MenuItem[] = [{ label: "关闭", run: () => onClose(tab.id) }];
    /*
     * 不适用的项**直接不出现**，而不是灰着放在那儿。
     *
     * 灰项要么让键盘游标停在一个按了没反应的条目上，要么就得写跳过逻辑 ——
     * 而「关闭其他」在只有一个标签时本来也没什么可解释的。
     */
    if (tabs.length > 1) {
      out.push({
        label: "关闭其他",
        run: () => onCloseMany?.(tabs.filter((t) => t.id !== tab.id).map((t) => t.id)),
      });
    }
    if (i < tabs.length - 1) {
      out.push({
        label: "关闭右侧的",
        run: () => onCloseMany?.(tabs.slice(i + 1).map((t) => t.id)),
      });
    }
    out.push({ label: "关闭全部", run: () => onCloseMany?.(tabs.map((t) => t.id)) });

    if (isReal(tab.path)) {
      out.push({
        label: "在文件树中定位",
        sep: true,
        run: () => onRevealInTree?.(tab.path),
      });
      out.push({ label: "在 Finder 中显示", run: () => void showInFinder(tab.path) });
      out.push({ label: "复制路径", sep: true, run: () => void copyText(tab.path, "路径") });
      out.push({
        label: "复制相对路径",
        run: () => void copyText(relTo(root, tab.path), "相对路径"),
      });
    }
    return out;
  });

  function openMenu(e: MouseEvent, tab: Tab, i: number) {
    e.preventDefault();
    // 右键也要切过去 —— 与 IDEA 一致。菜单作用在哪个标签上不能只靠人自己记
    onSelect(tab.id);
    menu = { x: e.clientX, y: e.clientY, tab, i };
  }

  function closeMenu(refocus: boolean) {
    const id = menu?.tab.id;
    menu = null;
    if (refocus && id !== undefined) {
      els[id]?.querySelector<HTMLElement>("button.label")?.focus();
    }
  }

  // 标签被关掉/换了一批之后，菜单可能指着一个已经不存在的标签
  $effect(() => {
    const ids = tabs.map((t) => t.id).join(",");
    untrack(() => {
      if (menu && !ids.split(",").includes(String(menu.tab.id))) menu = null;
    });
  });

  let bar = $state<HTMLElement | null>(null);
  let els = $state<Record<number, HTMLElement>>({});

  /**
   * 让当前标签始终可见。
   *
   * 标签多到溢出时，用 ⌘P 打开一个已存在但滚出视野的标签，界面上会「什么都没发生」
   * —— 其实切过去了，只是那个标签在屏幕外。
   */
  $effect(() => {
    const id = activeId;
    if (id === null) return;
    const el = els[id];
    if (el) el.scrollIntoView({ block: "nearest", inline: "nearest" });
  });

  /** 竖着滚滚轮就横向滚标签栏 —— 触控板上这是最自然的手势 */
  function onWheel(e: WheelEvent) {
    if (!bar) return;
    const d = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.deltaY;
    if (d === 0) return;
    e.preventDefault();
    bar.scrollLeft += d;
  }
</script>

<div class="tabs" role="tablist" bind:this={bar} onwheel={onWheel}>
  {#each tabs as tab, i (tab.id)}
    <!-- 中键关标签，浏览器和各家编辑器通用的手势 -->
    <div
      class="tab"
      class:active={tab.id === activeId}
      role="presentation"
      bind:this={els[tab.id]}
      onauxclick={(e) => {
        if (e.button === 1) {
          e.preventDefault();
          onClose(tab.id);
        }
      }}
      oncontextmenu={(e) => openMenu(e, tab, i)}
    >
      <!--
        模式用左侧色条，不用文字徽章：「差异」「日志」两个汉字要吃掉约 34px，
        而标签上限只有 200px —— 先被挤没的正是文件名。色条占的是本来就有的
        边框位置。蓝=差异，黄=日志，紫=冲突，与 git 状态色同源。
      -->
      <span class="mode {tab.mode}" aria-hidden="true"></span>
      <button
        class="label"
        role="tab"
        aria-selected={tab.id === activeId}
        onclick={() => onSelect(tab.id)}
        onkeydown={(e) => {
          // 只有鼠标能开的菜单等于把功能藏起来了（同文件树那边）
          if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
            e.preventDefault();
            const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
            onSelect(tab.id);
            menu = { x: r.left + 8, y: r.bottom + 2, tab, i };
          }
        }}
        title={tab.path}
      >

        {#if tab.mode === "merge"}<span class="badge merge">冲突</span>{/if}
        <span class="name">{tab.name}</span>
      </button>
      <button
        class="close"
        class:dirty={tab.dirty}
        onclick={() => onClose(tab.id)}
        title={tab.dirty ? "有未保存的改动" : "关闭"}
        aria-label="关闭 {tab.name}"
      >
        {tab.dirty ? "●" : "✕"}
      </button>
    </div>
  {/each}
</div>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    title={menu.tab.name}
    titleTip={menu.tab.path}
    label="{menu.tab.name} 的操作"
    {items}
    onclose={closeMenu}
  />
{/if}

<style>
  .tabs {
    display: flex;
    align-items: stretch;
    height: 32px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    overflow-x: auto;
    overflow-y: hidden;
    user-select: none;
  }
  .tab {
    display: flex;
    align-items: center;
    flex: none;
    max-width: 200px;
    border-right: 1px solid var(--border-soft);
    /* IDEA 是平角标签，底部一条 accent 线表示选中 */
    border-bottom: 2px solid transparent;
    background: transparent;
  }
  .tab:hover { background: var(--hover); }
  /* 标签溢出时给个细滚动条，否则完全看不出还有更多标签 */
  .tabs::-webkit-scrollbar { height: 3px; }
  .tabs::-webkit-scrollbar-thumb { background: var(--border); border-radius: var(--r-sm); }
  .tabs:hover::-webkit-scrollbar-thumb { background: var(--text-faint); }
  .tab.active {
    background: var(--editor-bg);
    border-bottom-color: var(--accent);
  }
  .mode { flex: none; width: 2px; align-self: stretch; background: transparent; }
  .mode.log { background: var(--lvl-warn); }
  .mode.diff { background: var(--git-modified); }
  .mode.merge { background: var(--git-renamed); }
  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 9px;
    height: 100%;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    cursor: default;
    overflow: hidden;
  }
  .tab.active .label { color: var(--text); }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .badge {
    flex: none;
    font-size: 9px;
    padding: 1px 4px;
    border-radius: var(--r-sm);
    background: var(--selected);
    color: var(--text-faint);
    font-family: var(--code-font);
  }
  .badge.diff { color: var(--git-modified); }
  .badge.merge { color: var(--lvl-warn); }
  .close {
    flex: none;
    width: 20px;
    height: 100%;
    padding: 0 11px 0 0;
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10px;
    cursor: default;
  }
  .close:hover { color: var(--text); }
  .close.dirty { color: var(--accent); font-size: 9px; }
  .label:focus-visible, .close:focus-visible { outline: 1px solid var(--accent); outline-offset: -2px; }
</style>
