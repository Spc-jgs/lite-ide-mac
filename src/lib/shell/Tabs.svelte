<script lang="ts">
  import { untrack } from "svelte";
  import type { MenuItem } from "./ContextMenu.svelte";
  import type { Group } from "../state/tab";
  import { cmenu } from "./context-menu.svelte";
  import Icon from "./Icon.svelte";
  import FileGlyph from "./FileGlyph.svelte";
  import { copyText, relTo, showInFinder } from "./pathactions";
  import { scratchSaveState } from "../state/autosave";

  export interface Tab {
    id: number;
    path: string;
    name: string;
    mode: "edit" | "log" | "diff" | "merge";
    dirty: boolean;
    /** 预览标签：名字斜体，双击保留（issue #33 ⑯） */
    preview?: boolean;
    /** 钉住：图钉占 ✕ 的位置，排最左（issue #33 ⑰） */
    pinned?: boolean;
    /** 显示名。草稿用第一行代替 `2026-09-16 1103.md` 那种时间戳（见 TabState.title） */
    title?: string;
    /** 草稿自动保存失败了（见 TabState.saveFailed） */
    saveFailed?: boolean;
  }

  let {
    tabs,
    activeId,
    root = "",
    onSelect,
    onClose,
    onCloseMany,
    onRevealInTree,
    onNewScratch,
    onKeep,
    onPin,
    onMoveToOther,
    moveLabel = "移到另一组",
    dim = false,
    group = 0,
    canDrop = () => true,
    onDrop,
    isScratch = () => false,
  }: {
    tabs: Tab[];
    activeId: number | null;
    /** 项目根，只用来算「复制相对路径」 */
    root?: string;
    /** 这个路径是不是草稿：草稿的圆点只在自动保存失败时亮（见 autosave.ts 的 scratchSaveState） */
    isScratch?: (path: string) => boolean;
    onSelect: (id: number) => void;
    /** `force`：钉住的标签也关（右键菜单里的「关闭」才传） */
    onClose: (id: number, force?: boolean) => void;
    /**
     * 批量关闭。**由 App 处理**，因为有未保存改动的标签要逐个问，
     * 而那个确认横幅长在 App 上。这里只负责算出「关哪些」。
     */
    onCloseMany?: (ids: number[]) => void;
    /** 在文件树里定位到这个标签对应的文件 */
    onRevealInTree?: (path: string) => void;
    /** 标签条末尾那个加号：新建一份草稿 */
    onNewScratch?: () => void;
    /** 双击预览标签：保留它（不再是预览） */
    onKeep?: (id: number) => void;
    /** 钉住 / 取消钉住 */
    onPin?: (id: number, on: boolean) => void;
    /** 挪到另一组（issue #35）。单栏时就是向右分屏，菜单项的字由 `moveLabel` 说 */
    onMoveToOther?: (id: number) => void;
    moveLabel?: string;
    /**
     * 非焦点组的标签条（issue #35，设计图②）：当前标签从 `--selected` 退到 `--hover`、
     * 字退到 dim、✕ 收起 —— 屏上「当前项」只有焦点组那一块是亮的（ui.md 第一条）。
     */
    dim?: boolean;
    /** 这条标签条是哪一组的（分屏）。写在 DOM 上，拖拽按它认落点 */
    group?: Group;
    /** 拖拽：能不能落到某一组（`tabs.moveTo` 的前置条件） */
    canDrop?: (id: number, g: Group) => boolean;
    /** 拖拽落点：目标组 + 组内序号（不含被拖的那个） */
    onDrop?: (id: number, g: Group, index: number) => void;
  } = $props();

  /**
   * 拖拽（2026-09-21）：这里只管「按下、挪过 5px」，过了阈值才 `import()` `tab-drag.ts` ——
   * 幽灵标签、插入线、落点计算都在那边，不进入口包。没过阈值松手就是一次普通点击。
   */
  let press: { x: number; y: number; tab: Tab } | null = null;
  const DRAG_START = 5;
  /** 按下就开始拉模块（缓存的，第二次起是同步的）；过了阈值时多半已经到了 */
  const dragMod = () => import("./tab-drag");
  function onTabPointerDown(e: PointerEvent, tab: Tab) {
    if (e.button !== 0 || e.metaKey || e.shiftKey || e.altKey || e.ctrlKey || !onDrop) return;
    press = { x: e.clientX, y: e.clientY, tab };
    void dragMod().catch(() => {});
    window.addEventListener("pointermove", onPressMove);
    window.addEventListener("pointerup", onPressUp, { once: true });
  }
  function onPressUp() {
    press = null;
    window.removeEventListener("pointermove", onPressMove);
  }
  function onPressMove(e: PointerEvent) {
    const p = press;
    if (!p || Math.hypot(e.clientX - p.x, e.clientY - p.y) < DRAG_START) return;
    onPressUp();
    const spec = { id: p.tab.id, label: p.tab.title ?? p.tab.name, canDrop: (g: Group) => canDrop(p.tab.id, g) };
    const drop = (g: Group, i: number) => onDrop?.(p.tab.id, g, i);
    /*
     * 模块还在路上时鼠标可能已经松开（一下快拖，第一次要等 chunk）。拿不到 `dragTab`
     * 之前先记着最后的位置和有没有松手：到了再决定是接着拖，还是直接按松手位置落。
     */
    let last = e;
    let released: PointerEvent | null = null;
    const track = (ev: PointerEvent) => (last = ev);
    const up = (ev: PointerEvent) => (released = ev);
    window.addEventListener("pointermove", track);
    window.addEventListener("pointerup", up, { once: true });
    void dragMod().then((m) => {
      window.removeEventListener("pointermove", track);
      window.removeEventListener("pointerup", up);
      if (released) m.dropAt(released.clientX, released.clientY, spec, drop);
      else m.dragTab(last, spec, drop);
    });
  }

  /**
   * 圆点亮不亮、亮成什么色、标题说什么。项目文件：脏 = 未保存 = 关闭前会问。
   * 草稿：自动存的，脏是半秒内的过渡态，不亮 —— 亮了还写着「关闭前会问」是撒谎
   * （`autosaveBeforeClose` 静默存）；只有写失败才亮，警示色，因为那才是要人管的。
   */
  function dotOf(tab: Tab): { on: boolean; warn: boolean; title: string } {
    const st = scratchSaveState({ scratch: isScratch(tab.path), dirty: tab.dirty, failed: !!tab.saveFailed });
    if (st === null) return { on: tab.dirty, warn: false, title: "有未保存的改动（关闭前会问）" };
    if (st === "failed") return { on: true, warn: true, title: "自动保存失败 —— 改动还在编辑器里，⌘S 重试" };
    return { on: false, warn: false, title: "" };
  }

  /**
   * 差异/合并标签的 path 是 `git-diff:xxx` 这类**合成 key**，不是盘上的路径。
   * 拿它去 Finder 里显示或者复制，给出来的是一串没用的东西 ——
   * 所以那几项只对真实文件出现。判据就是「以 / 开头」。
   */
  const isReal = (p: string) => p.startsWith("/");

  /** 在当前项目根底下。草稿住在根外面，跟它有关的菜单项要跟着让开 */
  const inRoot = (p: string) => !!root && p.startsWith(root.endsWith("/") ? root : `${root}/`);

  const MODE_LABEL: Record<Tab["mode"], string> = {
    edit: "",
    log: "日志",
    diff: "差异",
    merge: "冲突",
  };

  let menu = $state<{ x: number; y: number; tab: Tab; i: number } | null>(null);
  $effect(() => {
    if (menu) cmenu.load();
  });

  let items = $derived.by(() => {
    const m = menu;
    if (!m) return [] as MenuItem[];
    const { tab, i } = m;
    // 钉住的也能从这儿关（`force`）：钉住防的是误关，不是不许关
    const out: MenuItem[] = [{ label: "关闭", run: () => onClose(tab.id, true) }];
    out.push({ label: tab.pinned ? "取消钉住" : "钉住", run: () => onPin?.(tab.id, !tab.pinned) });
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
    // 分屏（issue #35）：只有一个标签时不出现 —— 挪走它这一组就空了、立刻收起，等于白做。
    // 判据同上面那句：不适用的项直接不出现
    if (onMoveToOther && tabs.length > 1) {
      out.push({ label: moveLabel, sep: true, run: () => onMoveToOther(tab.id) });
    }

    if (isReal(tab.path)) {
      /*
       * 「定位」和「相对路径」只对**项目根底下**的文件成立。
       *
       * 草稿就住在根外面（`~/Library/Application Support/…/scratches`）：
       * 文件树里根本没有它那一行，定位必然落空；而 `relTo` 对根外的路径
       * 是原样返回绝对路径的 —— 一个叫「复制相对路径」的菜单项给你一串
       * `/Users/…/Application Support/…`，那是菜单在说谎。
       * 判据同上面那句：**不适用的项直接不出现**。
       */
      if (inRoot(tab.path)) {
        out.push({
          label: "在文件树中定位",
          sep: true,
          run: () => onRevealInTree?.(tab.path),
        });
      }
      out.push({
        label: "在 Finder 中显示",
        sep: !inRoot(tab.path),
        run: () => void showInFinder(tab.path),
      });
      out.push({ label: "复制路径", sep: true, run: () => void copyText(tab.path, "路径") });
      if (inRoot(tab.path)) {
        out.push({
          label: "复制相对路径",
          run: () => void copyText(relTo(root, tab.path), "相对路径"),
        });
      }
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

  /*
   * 溢出提示：滚出去的那一边渐隐。
   *
   * `overflow-x: auto` 配 macOS 的覆盖式滚动条 = 不滚的时候什么都不画，藏起来的标签
   * 就是不存在。IDEA 用一个 ⋯ 下拉列出藏起来的，这里先用最便宜的：哪边还有东西哪边
   * 渐隐 28px（滚到头的那一边不渐隐，不然最后一个标签永远蒙着一层）。
   * 三个时机都要量：滚动、条本身变宽窄（ResizeObserver）、标签增减。
   */
  let fadeL = $state(false);
  let fadeR = $state(false);
  function measure() {
    const b = bar;
    if (!b) return;
    fadeL = b.scrollLeft > 1;
    fadeR = b.scrollLeft + b.clientWidth < b.scrollWidth - 1;
  }
  $effect(() => {
    const b = bar;
    if (!b) return;
    const ro = new ResizeObserver(measure);
    ro.observe(b);
    b.addEventListener("scroll", measure, { passive: true });
    measure();
    return () => {
      ro.disconnect();
      b.removeEventListener("scroll", measure);
    };
  });
  $effect(() => {
    void tabs.length;
    // 等这一轮 DOM 更新完再量，不然量到的是加标签之前的宽度
    queueMicrotask(measure);
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

<div class="tabs" class:dim class:fade-l={fadeL} class:fade-r={fadeR} role="tablist" data-group={group} bind:this={bar} onwheel={onWheel}>
  {#each tabs as tab, i (tab.id)}
    {@const dot = dotOf(tab)}
    <!-- 中键关标签，浏览器和各家编辑器通用的手势 -->
    <div
      class="tab"
      class:active={tab.id === activeId}
      role="presentation"
      data-id={tab.id}
      bind:this={els[tab.id]}
      onpointerdown={(e) => onTabPointerDown(e, tab)}
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
        ondblclick={() => onKeep?.(tab.id)}
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
        <span class="name" class:preview={tab.preview}>{tab.title ?? tab.name}</span>
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
      {#if tab.pinned}
        <!-- 钉住的：图钉常驻在 ✕ 的位置，点了取消钉住（VS Code 同款）。脏的圆点仍要看得见 -->
        <button
          class="ibtn xs close pin"
          class:dirty={dot.on}
          class:warn={dot.warn}
          onclick={() => onPin?.(tab.id, false)}
          title={dot.on ? `已钉住，${dot.title} —— 点击取消钉住` : "已钉住 —— 点击取消钉住"}
          aria-label="取消钉住 {tab.name}"
        >
          <span class="x"><Icon name="pin" size={11} /></span>
          {#if dot.on}<span class="dot" aria-hidden="true"></span>{/if}
        </button>
      {:else}
        <button
          class="ibtn xs close"
          class:dirty={dot.on}
          class:warn={dot.warn}
          onclick={() => onClose(tab.id)}
          title={dot.on ? dot.title : "关闭"}
          aria-label="关闭 {tab.name}"
        >
          <span class="x"><Icon name="x" size={10} /></span>
          {#if dot.on}<span class="dot" aria-hidden="true"></span>{/if}
        </button>
      {/if}
    </div>
  {/each}

  <!--
    **加号跟着标签一起滚，不钉在右边。**

    钉在右边要么脱出这个 `overflow-x: auto` 的容器（那就得再套一层壳），
    要么 `position: sticky`（在横向滚动的 flex 里它会盖住最后一个标签的 ✕）。
    而它跟着滚的代价很小：标签多到要滚的时候，⌘N 就在手上。

    这也**破了 ui.md 第三条**（「常驻的只留不看会出错的那些」）——
    那条管的是**每一项上重复出现**的东西（每个标签一个 ✕、每一行一个 ＋），
    常驻它们等于满屏噪音；整条栏上只有一个的入口不在此列。
    规矩那边补了这句，不是这里悄悄破的例。
  -->
  {#if onNewScratch}
    <button class="ibtn" onclick={onNewScratch} title="新建草稿（⌘N）" aria-label="新建草稿">
      <Icon name="plus" size={13} />
    </button>
  {/if}
</div>

{#if menu && cmenu.comp}
  <cmenu.comp
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
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    /* 不画下边线（M8）：下面那块岛的圆角上沿就是边界 */
    overflow-x: auto;
    overflow-y: hidden;
    user-select: none;
  }
  /* 溢出的那一边渐隐（见 measure）。mask 套在滚动容器上，滚出去的内容跟着一起被蒙 */
  .tabs.fade-l {
    -webkit-mask-image: linear-gradient(to right, transparent, #000 28px);
    mask-image: linear-gradient(to right, transparent, #000 28px);
  }
  .tabs.fade-r {
    -webkit-mask-image: linear-gradient(to left, transparent, #000 28px);
    mask-image: linear-gradient(to left, transparent, #000 28px);
  }
  .tabs.fade-l.fade-r {
    -webkit-mask-image: linear-gradient(to right, transparent, #000 28px, #000 calc(100% - 28px), transparent);
    mask-image: linear-gradient(to right, transparent, #000 28px, #000 calc(100% - 28px), transparent);
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
  /* 和标签同一套：28px 的圆角块 + hover 底色（ui.md 第一条） */
  /* 末尾的「新建草稿」和各处头里的工具按钮同一个 `.ibtn` */
  .tab.active { background: var(--selected); }
  /* 非焦点组（issue #35）：当前项退一档，✕ 收起；hover 照常 —— 它仍然是能操作的 */
  .tabs.dim .tab.active { background: var(--hover); }
  .tabs.dim .tab.active .label { color: var(--text-dim); }
  .tabs.dim .tab.active:not(:hover) .close:not(.dirty):not(.pin) { opacity: 0; }
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
  /* 预览标签斜体 —— VS Code 的约定，看一眼就懂「这一格是临时的」 */
  .name.preview { font-style: italic; }
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
  /* 标签上的 ✕ / 图钉是 `.ibtn.xs`；这里只管「平时藏着、圆点和 ✕ 原地互换」 */
  .close { position: relative; opacity: 0; }
  .tab:hover .close, .tab.active .close, .close.dirty { opacity: 1; }
  /* 图钉常驻：它不是「每一项上都有」的装饰，是这个标签和别的不一样的唯一标记 */
  .close.pin { opacity: 0.7; }
  .close.pin .x { display: inline-flex; }
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
  /* 草稿自动保存失败：这颗点是唯一的常驻提醒，得和「普通的未保存」分得开 */
  .close.warn .dot { background: var(--lvl-warn); }
  /* 有改动时平时只看得见圆点；鼠标进来（或它是当前标签）才换成 ✕ */
  .close.dirty .x { opacity: 0; }
  .tab:hover .close.dirty .x, .close.dirty:focus-visible .x { opacity: 1; }
  .tab:hover .close.dirty .dot, .close.dirty:focus-visible .dot { opacity: 0; }

  .label:focus-visible, .close:focus-visible { outline: 1px solid var(--accent); outline-offset: -2px; }
</style>
