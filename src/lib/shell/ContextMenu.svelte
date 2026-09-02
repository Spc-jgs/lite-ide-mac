<script lang="ts">
  /**
   * 右键菜单的壳：定位、键盘、点外面关掉。**只管这些，不管菜单里是什么。**
   *
   * M22/M24 时这套逻辑长在 FileTree 里，M25 加标签栏右键菜单时抽了出来 ——
   * 抄第二份的代价不是行数，是「两份里只修好一份」：钳进视口、捕获阶段听滚动、
   * 关掉时把焦点还回去，这几条每一条都是踩出来的，抄漏一条不报错，
   * 只是某个方向上不好使。
   */
  export interface MenuItem {
    label: string;
    run: () => void;
    /** 在这一项**上面**画一条分隔线 */
    sep?: boolean;
    /** 危险操作，常驻红色 */
    danger?: boolean;
  }

  let {
    x,
    y,
    title = "",
    titleTip = "",
    label,
    items,
    onclose,
  }: {
    x: number;
    y: number;
    /** 菜单顶部那行灰字，通常是被操作对象的名字 */
    title?: string;
    /** 那行灰字的 tooltip，通常是完整路径 */
    titleTip?: string;
    /** 给读屏用的菜单名 */
    label: string;
    items: MenuItem[];
    /**
     * 关掉。`refocus` 为真表示是键盘或 Esc 关的 —— 调用方该把焦点收回到
     * 打开菜单的那个元素上，否则焦点掉到 body，接着按 Tab 会从头开始走。
     */
    onclose: (refocus: boolean) => void;
  } = $props();

  let el = $state<HTMLElement | null>(null);
  let cursor = $state(0);

  // 换了一批条目（换了对象）就把游标收回顶上
  $effect(() => {
    void items;
    cursor = 0;
  });

  /*
   * 位置钳进视口。**改的是 DOM 而不是 props** —— 写回状态会让这个 effect
   * 依赖自己写的值，一不小心就是 update 循环。读一次布局、写一次样式，一帧完事。
   */
  $effect(() => {
    const e = el;
    if (!e) return;
    const r = e.getBoundingClientRect();
    const pad = 6;
    e.style.left = `${Math.max(pad, Math.min(x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${Math.max(pad, Math.min(y, window.innerHeight - r.height - pad))}px`;
    // 开完就把焦点交给菜单，否则 Esc 和方向键都落不到它身上
    e.focus();
  });

  /*
   * scroll 用捕获阶段：滚动的容器（文件树的 .list、标签栏）不冒泡到 window，
   * 不捕获的话菜单就飘在半空中指着一个早已滚走的东西。
   */
  $effect(() => {
    const onDown = (ev: PointerEvent) => {
      if (el && !el.contains(ev.target as Node)) onclose(false);
    };
    const onGone = () => onclose(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("scroll", onGone, true);
    window.addEventListener("resize", onGone);
    window.addEventListener("blur", onGone);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("scroll", onGone, true);
      window.removeEventListener("resize", onGone);
      window.removeEventListener("blur", onGone);
    };
  });

  function onKey(e: KeyboardEvent) {
    const n = items.length;
    if (n === 0) return;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        onclose(true);
        break;
      case "ArrowDown":
        e.preventDefault();
        cursor = (cursor + 1) % n;
        break;
      case "ArrowUp":
        e.preventDefault();
        cursor = (cursor - 1 + n) % n;
        break;
      case "Home":
        e.preventDefault();
        cursor = 0;
        break;
      case "End":
        e.preventDefault();
        cursor = n - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        items[cursor]?.run();
        onclose(true);
        break;
    }
  }
</script>

<!--
  菜单画在触发它的容器**外面**：那些容器多半是 overflow: auto，画在里面会被
  裁掉半截还跟着滚。position: fixed 逃得出 overflow，但逃不出 transform 祖先 ——
  将来给侧边栏或标签栏加动画时要留意这条。
-->
<div
  class="menu"
  role="menu"
  tabindex="-1"
  aria-label={label}
  bind:this={el}
  style:left="{x}px"
  style:top="{y}px"
  onkeydown={onKey}
>
  {#if title}
    <div class="mhead" title={titleTip}>{title}</div>
  {/if}
  {#each items as it, i (it.label)}
    <button
      class="mitem"
      class:on={i === cursor}
      class:sep={it.sep}
      class:danger={it.danger}
      role="menuitem"
      tabindex="-1"
      onmouseenter={() => (cursor = i)}
      onclick={() => {
        it.run();
        onclose(false);
      }}
    >
      {it.label}
    </button>
  {/each}
</div>

<style>
  .menu {
    position: fixed;
    z-index: 60;
    min-width: 168px;
    padding: 4px;
    background: var(--panel-bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    outline: none;
  }
  .mhead {
    padding: 3px 9px 5px;
    margin-bottom: 3px;
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
  }
  .mitem {
    display: block;
    width: 100%;
    padding: 4px 9px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: default;
  }
  /* 鼠标和键盘共用一个高亮：菜单里同时有两个高亮是最容易看错的写法 */
  .mitem.on { background: var(--accent-sel); }
  .mitem.sep {
    margin-top: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--border-soft);
    border-radius: 0 0 3px 3px;
  }
  /*
   * 危险项常驻红色，不是只在 hover 时才红：手滑点中的那一下发生在 hover 之后，
   * 而人是靠「扫一眼菜单」决定往哪儿点的
   */
  .mitem.danger { color: var(--lvl-error); }
  .mitem.danger.on { background: rgba(247, 84, 100, 0.16); }
</style>
