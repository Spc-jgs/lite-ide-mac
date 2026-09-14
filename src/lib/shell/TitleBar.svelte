<script lang="ts">
  /**
   * 标题栏 = IDEA 的 main toolbar：**「哪个项目 / 哪个分支」，只有这两件事。**
   * 右边整片是窗口拖动区。
   *
   * 项目挂件的下拉在这里（最近项目 / 打开 / 清除），三件事的逻辑都在 App
   * （`recent` / `openRecent` / `openFolder`），以回调传进来。分支挂件的 DOM
   * 元素通过 `bind:branchBtn` 交回 App —— 分支浮层要挂在它底下，而开浮层的
   * 有三条路（挂件、Git 栏、菜单），位置得由同一个元素定。
   */
  import Icon from "./Icon.svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { devtoolsBuild, type GitStatus } from "../ipc/commands";
  import { projectName } from "../state/crumbs";

  let {
    root,
    gitSt,
    recent,
    branchOpen,
    branchBtn = $bindable(null),
    onOpenRecent,
    onOpenFolder,
    onClearRecent,
    onOpenBranches,
  }: {
    root: string | null;
    gitSt: GitStatus | null;
    recent: string[];
    /** 分支浮层开着时挂件保持点亮 */
    branchOpen: boolean;
    branchBtn?: HTMLElement | null;
    onOpenRecent: (dir: string) => void;
    onOpenFolder: () => void;
    onClearRecent: () => void;
    onOpenBranches: () => void;
  } = $props();

  /**
   * 标题栏项目挂件的下拉。
   *
   * 照 IDEA 的 project widget：显示当前项目名，点开是最近项目 + 打开 + 清除。
   * 这三件事的逻辑**一条都不是新写的** —— `recent` / `openRecent` / `openFolder`
   * 早就在了，以前只有 macOS 菜单栏的「最近打开」子菜单用得着它们，
   * 而 `pnpm dev` 跑在浏览器里，那儿一个菜单项都没有。
   */
  let projMenu = $state<{ x: number; y: number } | null>(null);

  function openProjMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    projMenu = { x: r.left, y: r.bottom + 2 };
  }

  let projName = $derived(projectName(root));
  /** 方块里那个字。IDEA 用项目名首字母，中文项目名就直接用第一个字 */
  let projInitial = $derived((projName[0] ?? "?").toUpperCase());

  let projMenuItems = $derived.by<MenuItem[]>(() => {
    if (!projMenu) return [];
    // 前面那个格子标出当前项目。全角空格占位，切换时名字不会左右跳
    const items: MenuItem[] = recent.map((r) => ({
      label: `${r === root ? "●" : "\u3000"} ${r.slice(r.lastIndexOf("/") + 1) || r}`,
      run: () => onOpenRecent(r),
    }));
    items.push({ label: "打开文件夹…", sep: items.length > 0, run: onOpenFolder });
    if (recent.length > 0) items.push({ label: "清除最近记录", run: onClearRecent });
    return items;
  });

  /**
   * 这份构建带不带 Web Inspector（issue #20）。
   *
   * 调试版和正式版**装在同一个路径上**（`pnpm app:bundle:devtools` 覆盖
   * `pnpm app:bundle` 的产物），而「盘上只留一份 .app」是这个仓库的硬纪律 ——
   * 两条加起来的结果是：忘了打回去的话，你双击的那份一直开着 inspector，
   * 而界面上**没有任何迹象**。
   *
   * 挂在项目挂件的 tooltip 上，和构建时间并排：排查「你跑的是哪个构建」时
   * 本来就要看那一眼，不多一个新习惯。
   */
  let devtools = $state(false);
  /**
   * 项目挂件的 tooltip。
   *
   * **换行必须写在表达式里，不能在模板里写 `&#10;`。** 原来就是后者，
   * 而实测它出来的是一个**空格**（`charCodeAt` 是 32 不是 10）——
   * 也就是说「tooltip 第二行是构建时间」这句话从来没成立过，三行全挤在一行里。
   * 一条挂在界面上、用来确认「你跑的是哪个构建」的信息，自己却在说谎。
   */
  let projTip = $derived(
    [
      root ?? "还没打开文件夹",
      `lite-ide · 构建于 ${__BUILD_TIME__}`,
      ...(devtools ? ["⚠︎ 调试版：带 Web Inspector，别拿它当正式版用"] : []),
    ].join("\n"),
  );
  $effect(() => {
    let dead = false;
    void devtoolsBuild().then((v) => {
      if (!dead) devtools = v;
    });
    return () => {
      dead = true;
    };
  });
</script>

<!--
  标题栏 = IDEA 的 main toolbar：**「哪个项目 / 哪个分支」，只有这两件事。**

  面包屑原来在这儿（那时的理由是「顺带占掉右边那块常年空着的地方」），
  2026-09-06 搬到状态栏左边去了 —— IDEA 的导航栏就在那儿，而且路径属于
  「我在哪个文件」，和底下那排文件状态是同一组信息。
  腾出来的右边不是浪费，是**窗口拖动区**。
-->
<header class="titlebar" data-tauri-drag-region>
  <!--
    项目挂件。没打开项目时显示应用名 —— **按钮位置在两个状态下完全一致**，
    和导轨上那条「控件的位置必须是肌肉记忆能记住的」是同一条。

    tooltip 第二行是构建时间，别删：报上来的 bug 复现不了时，
    第一件事就是确认对方跑的是哪个构建（为此白查过一次代码）。
    它原来挂在这儿那个 `lite-ide` 字样上，而那个字样现在只有空项目时才出现。
  -->
  <button class="twidget proj" onclick={(e) => openProjMenu(e)} title={projTip}>
    <span class="sq" aria-hidden="true">{projInitial}</span>
    <span class="wlabel">{projName}</span>
    <Icon name="chevron-down" size={10} />
  </button>
  {#if gitSt}
    <button
      class="twidget"
      class:on={branchOpen}
      bind:this={branchBtn}
      onclick={onOpenBranches}
      title="切换分支 / 工作树"
    >
      <Icon name="git" size={12} />
      <span class="wlabel">{gitSt.branch || "游离"}</span>
      {#if gitSt.ahead}<span class="ab">↑{gitSt.ahead}</span>{/if}
      {#if gitSt.behind}<span class="ab">↓{gitSt.behind}</span>{/if}
      <Icon name="chevron-down" size={10} />
    </button>
  {/if}
  <span class="tgap" data-tauri-drag-region></span>
</header>

{#if projMenu}
  <ContextMenu
    x={projMenu.x}
    y={projMenu.y}
    label="项目"
    items={projMenuItems}
    onclose={() => (projMenu = null)}
  />
{/if}

<style>
  .titlebar {
    display: flex;
    align-items: center;
    gap: 8px;
    /* 给 macOS 红绿灯让位 */
    padding: 0 12px 0 78px;
    /* 贴着窗口上边，窗口阴影在这条边上最弱 —— 浅色壁纸下不压一层，小字糊进桌面 */
    background: var(--chrome-scrim);
    border-bottom: 1px solid var(--border);
    font-size: 12.5px;
    user-select: none;
  }
  /* tgap 是**拖动区**，不是留白 —— 面包屑搬走之后这一大片正是拿窗口的地方 */
  .titlebar .tgap { flex: 1; min-width: 12px; }

  /*
   * 标题栏挂件（项目 / 分支）。两个长得一模一样，**中间不画竖线** ——
   * 线只分区不分项，而它们本来就是同一组「我现在在哪个项目的哪个分支上」。
   */
  .twidget {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    flex: none;
    max-width: 240px;
    height: 24px;
    padding: 0 6px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .twidget:hover { background: var(--hover); color: var(--text-dim); }
  /* 浮层开着时挂件保持点亮 —— 否则那块浮层看着像凭空冒出来的 */
  .twidget.on { background: var(--selected); color: var(--text-dim); }
  .twidget:active { background: var(--pressed); }
  .twidget:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .twidget .wlabel {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .twidget.proj .wlabel { color: var(--text); }
  /*
   * 项目名首字的方块。IDEA 的项目挂件就是这个形状，它的用处不是装饰：
   * 同时开着两个窗口时，一眼认出「这个窗口是哪个项目」靠的是这个色块，
   * 不是去读那几个字。
   */
  .twidget .sq {
    flex: none;
    display: grid;
    place-content: center;
    width: 16px;
    height: 16px;
    border-radius: 5px;
    background: var(--selected);
    color: var(--text);
    font-size: 9.5px;
    font-weight: 600;
  }
  .twidget:hover .sq { background: var(--pressed); }
  /* 分支名是标识符，用等宽；ahead/behind 用 accent，它是「该做点什么」的信号 */
  .twidget .ab { color: var(--accent); font-family: var(--code-font); flex: none; font-size: 11px; }
</style>
