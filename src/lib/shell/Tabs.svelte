<script lang="ts">
  import { untrack } from "svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import FileGlyph from "./FileGlyph.svelte";
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

  const MODE_LABEL: Record<Tab["mode"], string> = {
    edit: "",
    log: "日志",
    diff: "差异",
    merge: "冲突",
  };

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

        <!--
          类型字形和文件树、随处搜索是同一个组件。标签栏恰恰是文件名被截得
          最狠的地方（上限 200px），这 14px 里信息密度最高的就是它。
          模式（日志/差异/冲突）给字形上色，不再单占一条 2px 竖带 ——
          竖线整排去掉之后，那条带看着像标签又被切了一刀。
        -->
        <span class="glyphwrap {tab.mode}">
          <FileGlyph name={tab.name} size={13} />
        </span>
        <span class="name">{tab.name}</span>
        {#if tab.id === activeId && tab.mode !== "edit"}
          <!-- 徽章只给当前标签：其余标签的模式由字形颜色说清，
               而当前标签有的是横向余地 -->
          <span class="badge {tab.mode}">{MODE_LABEL[tab.mode]}</span>
        {/if}
      </button>
      <!--
        关闭钮 hover 才出，但**「有未保存改动」的圆点必须常驻** ——
        它不是装饰，是「这个标签关掉要问你」。
        圆点和 ✕ 占同一个格子：hover 时原地互换，位置不跳。
      -->
      <button
        class="close"
        class:dirty={tab.dirty}
        onclick={() => onClose(tab.id)}
        title={tab.dirty ? "有未保存的改动（关闭前会问）" : "关闭"}
        aria-label="关闭 {tab.name}"
      >
        <span class="x">✕</span>
        {#if tab.dirty}<span class="dot" aria-hidden="true"></span>{/if}
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
  /*
   * # 标签是「摞上去的块」，不是「切出来的格子」
   *
   * 原来每个标签右边一条 1px 竖线、底下一条 2px accent 线，八个标签就是
   * 八道竖线 —— **那是表格的语言**。当前项已经由一块底色说清楚了，
   * 再画线就是同一件事说两遍，而线是常驻的、底色只有一块。
   *
   * 两条都跟着材质走：2px 的蓝下划线飘在半透明的条上尤其不成立，
   * 而圆角块和文件树的选中行是同一套 —— 两边挨着，做法该一样。
   */
  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    /* 32 → 38：28px 的圆角块要有呼吸位，贴着上下边看着像被切掉一半 */
    height: 38px;
    padding: 0 6px;
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
    height: 28px;
    max-width: 200px;
    padding-right: 4px;
    border-radius: var(--r-sm);
    background: transparent;
  }
  .tab:hover { background: var(--hover); }
  .tab.active { background: var(--selected); }
  /* 标签溢出时给个细滚动条，否则完全看不出还有更多标签 */
  .tabs::-webkit-scrollbar { height: 3px; }
  .tabs::-webkit-scrollbar-thumb { background: var(--border); border-radius: var(--r-sm); }
  .tabs:hover::-webkit-scrollbar-thumb { background: var(--text-faint); }

  .glyphwrap { flex: none; display: flex; }
  /* 模式给字形上色，与 git 状态色同源：黄=日志、蓝=差异、紫=冲突 */
  .glyphwrap.log :global(.glyph) { color: var(--lvl-warn); opacity: 1; }
  .glyphwrap.diff :global(.glyph) { color: var(--git-modified); opacity: 1; }
  .glyphwrap.merge :global(.glyph) { color: var(--git-renamed); opacity: 1; }
  .tab.active .glyphwrap :global(.glyph) { color: var(--text-dim); }
  .tab.active .glyphwrap.log :global(.glyph) { color: var(--lvl-warn); }
  .tab.active .glyphwrap.diff :global(.glyph) { color: var(--git-modified); }
  .tab.active .glyphwrap.merge :global(.glyph) { color: var(--git-renamed); }

  .label {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 4px 0 10px;
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
    background: var(--hover);
    color: var(--text-faint);
    font-family: var(--code-font);
  }
  .badge.diff { color: var(--git-modified); }
  .badge.merge { color: var(--lvl-warn); }
  .badge.log { color: var(--lvl-warn); }

  /*
   * 关闭钮与未保存圆点共用一个格子。
   *
   * ✕ 常驻的话，八个标签就是八个常驻的 ✕，而任何一刻最多只关得掉一个。
   * 但圆点不能藏：它说的是「这个标签关掉要问你」。
   * 于是两者叠在同一个 16px 里 —— 平时露圆点，hover/当前标签露 ✕，
   * **点击目标始终在同一个位置**。
   */
  .close {
    position: relative;
    flex: none;
    width: 16px;
    height: 16px;
    display: grid;
    place-content: center;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-faint);
    font-size: 9px;
    cursor: default;
    opacity: 0;
  }
  .tab:hover .close, .tab.active .close, .close.dirty { opacity: 1; }
  .close:hover { background: var(--selected); color: var(--text); }
  .close .x { line-height: 1; }
  .close .dot {
    position: absolute;
    inset: 0;
    margin: auto;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--text-dim);
  }
  /* 有改动时平时只看得见圆点；鼠标进来（或它是当前标签）才换成 ✕ */
  .close.dirty .x { opacity: 0; }
  .tab:hover .close.dirty .x, .close.dirty:focus-visible .x { opacity: 1; }
  .tab:hover .close.dirty .dot, .close.dirty:focus-visible .dot { opacity: 0; }

  .label:focus-visible, .close:focus-visible { outline: 1px solid var(--accent); outline-offset: -2px; }
</style>
