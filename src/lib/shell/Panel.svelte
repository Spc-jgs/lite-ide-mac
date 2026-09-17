<script lang="ts">
  /**
   * 底部工具窗：终端 / Git（提交历史 · 控制台）。
   *
   * 拖高、面板头（名字 + 标签页 + 动作）、终端实例的挂载、Git 控制台的按需加载
   * 都在这里；终端列表本身在 `state/terms.svelte.ts`，开合 / 高度 / 哪个工具窗
   * 在 `state/layout.svelte.ts`。提交历史那块要 App 的 `git` lazyGroup 和活动标签，
   * 以 snippet 传进来（同 Sidebar 的两块内容，等 #9 后面几步把源头搬出来再收）。
   *
   * `panelTool` 是**实际在显示**的工具窗，由 App 按「偏好是 git 且真有仓库」算出来，
   * 导轨也用同一个值 —— 两处各算一遍就会有一天不一致。
   */
  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { lazy } from "../lazy/lazy.svelte";
  import { notify } from "../state/notify.svelte";
  import { layout } from "../state/layout.svelte";
  import { terms } from "../state/terms.svelte";

  let {
    root,
    repo,
    panelTool,
    gitLogReady,
    gitLog,
  }: {
    root: string | null;
    repo: string | null;
    panelTool: "term" | "git";
    /** 提交历史那个懒加载 chunk 到了没 */
    gitLogReady: boolean;
    gitLog: Snippet;
  } = $props();

  /** xterm.js 约 250KB，不开终端就不该付这个钱 —— 与 CM6 同样按需加载 */
  const terminal = lazy(() => import("../terminal/Terminal.svelte"), "终端");
  /**
   * Git 控制台那一页（issue #29）。**按需加载**，和终端、CM6 同一条纪律 ——
   * 一个诊断页面不该让每个人的启动多付钱。
   */
  const gitcon = lazy(() => import("../git/GitConsole.svelte"), "Git 控制台");

  $effect(() => {
    if (layout.panel) terminal.load();
  });
  $effect(() => {
    // Git 控制台只在真的切到那一页时才拉那个 chunk
    if (layout.panel && panelTool === "git" && layout.gitTab === "console") gitcon.load();
  });
  /*
   * 按需加载失败要说出来。App 那张汇总名单「漏一个就是一处静默失败」——
   * 而 `gitcon` 在那儿正好漏了（#29 加的时候忘了回去加一行）。搬到这里之后
   * 两个 lazy 和用它们的地方在同一个文件里，漏不掉。
   */
  $effect(() => {
    const e = terminal.error || gitcon.error;
    if (e) notify.fail(e);
  });

  // 打开面板时若一个终端都没有，自动起一个。
  // 只在终端页上做 —— 冲着 Git 日志来的人不该莫名多出一个 shell
  $effect(() => {
    if (layout.panel && panelTool === "term" && terms.list.length === 0 && root !== null) {
      terms.open(root);
    }
  });

  /** 面板头右边那两个下拉：`list` 是全部终端，`more` 是更多操作 */
  let panelMenu = $state<{ x: number; y: number; kind: "list" | "more" } | null>(null);

  function openPanelMenu(e: MouseEvent, kind: "list" | "more") {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    panelMenu = { x: r.left, y: r.bottom + 2, kind };
  }

  let panelMenuItems = $derived.by<MenuItem[]>(() => {
    if (!panelMenu) return [];
    if (panelMenu.kind === "list") {
      // 前面那个格子标出当前项。全角空格占位，切换时标题不会左右跳
      return terms.list.map((t) => ({
        label: `${t.id === terms.activeId ? "●" : "\u3000"} ${t.title}`,
        run: () => (terms.activeId = t.id),
      }));
    }
    const items: MenuItem[] = [{ label: "新建终端", run: () => terms.open(root ?? "~") }];
    if (terms.activeId !== null) {
      const id = terms.activeId;
      items.push({ label: "关闭当前终端", run: () => terms.close(id) });
    }
    if (terms.list.length > 1) {
      items.push({ label: "关闭其他终端", run: () => terms.closeOthers() });
    }
    /*
     * 「全部关闭」带 danger，和 Git 栏的「全部丢弃」同一条判据：
     * 关掉一个终端等于 kill 掉里面正在跑的东西，撤不回来。一个一个关，
     * 每一下都还在看着标题；一下关掉全部，跑着的 gradle build 就没了。
     */
    if (terms.list.length > 1) {
      items.push({ label: "全部关闭", sep: true, danger: true, run: () => terms.closeAll() });
    }
    return items;
  });

  function startResize(e: PointerEvent) {
    e.preventDefault();
    const startY = e.clientY;
    const startH = layout.panelHeight;
    const move = (ev: PointerEvent) => {
      // 往上拖变高：面板贴在底部，位移要反号
      layout.panelHeight = Math.max(90, Math.min(window.innerHeight - 200, startH - (ev.clientY - startY)));
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

<!--
  条件是 `layout.panel || terms.list.length > 0`，不是 `layout.panel`。

  收起面板**不能卸载**这一块：组件一销毁 Session 就 drop，shell 被 kill。
  跑着 gradle build 的时候按 ⌘J 腾点地方，构建就没了 —— 而且没有任何提示。
  （下面切 Git 日志页那处早就想到了这一层，这里漏了一级。）

  `terms.list.length > 0` 那半边保证「从没开过终端」时不会白挂一块 DOM，
  也保证关掉最后一个终端后这块能真正消失（`terms.close` 会清空列表）。
-->
{#if layout.panel || terms.list.length > 0}
  <div
    class="resizer"
    class:hidden={!layout.panel}
    role="separator"
    aria-label="调整终端高度"
    onpointerdown={startResize}
  ></div>
  <div class="panel" class:hidden={!layout.panel} style:height="{layout.panelHeight}px">
    <!--
      工具窗的头：**名字在最左，标签页跟在后面，动作靠右**。

      工具窗之间的切换不在这里（在导轨上），所以这一行只讲一件事：
      「你现在看的是哪个工具窗、它有哪几个标签页」。名字比标签亮一档 ——
      面板收起再展开时，第一眼要能认出这是哪个工具窗。
    -->
    <div class="panel-head">
      <span class="tw-name">{panelTool === "term" ? "终端" : "Git"}</span>
      {#if panelTool === "git"}
        <!--
          Git 窗的两个标签。和终端标签同一套样式，只是没有 ✕ ——
          它们不是开出来的东西，关不掉。
        -->
        <div class="ptabs" role="tablist">
          <div class="ptab" class:on={layout.gitTab === "log"}>
            <button
              class="pt-label fixed"
              role="tab"
              aria-selected={layout.gitTab === "log"}
              onclick={() => (layout.gitTab = "log")}
            >提交历史</button>
          </div>
          <div class="ptab" class:on={layout.gitTab === "console"}>
            <button
              class="pt-label fixed"
              role="tab"
              aria-selected={layout.gitTab === "console"}
              onclick={() => (layout.gitTab = "console")}
              title="跑过的每一条 git，完整 argv"
            >控制台</button>
          </div>
        </div>
      {:else}
        <div class="ptabs">
          {#each terms.list as t (t.id)}
            <div class="ptab" class:on={t.id === terms.activeId}>
              <button class="pt-label" onclick={() => (terms.activeId = t.id)} title={t.cwd}>
                {t.title}
              </button>
              <button
                class="pt-x"
                onclick={() => terms.close(t.id)}
                aria-label="关闭 {t.title}"
                title="关闭 {t.title}"
              >✕</button>
            </div>
          {/each}
        </div>
        <button
          class="phbtn"
          onclick={() => terms.open(root ?? "~")}
          title="新建终端 ⌃⇧`"
          aria-label="新建终端"
        >
          <Icon name="plus" />
        </button>
        <!--
          标签页多到溢出时，横向滚动条是看不见的（高度 0）——
          这个下拉是唯一能一眼看全、并且直接跳过去的路
        -->
        {#if terms.list.length > 1}
          <button
            class="phbtn"
            onclick={(e) => openPanelMenu(e, "list")}
            title="全部终端"
            aria-label="全部终端"
          >
            <Icon name="chevron-down" />
          </button>
        {/if}
      {/if}
      <span class="gap"></span>
      <!--
        「更多」只在终端页出 —— 提交历史那边一条真动作都没有，
        摆一个点开是空的按钮，比没有这个按钮糟。
      -->
      {#if panelTool === "term" && terms.list.length > 0}
        <button
          class="phbtn"
          onclick={(e) => openPanelMenu(e, "more")}
          title="更多操作"
          aria-label="更多操作"
        >
          <Icon name="more-v" />
        </button>
      {/if}
      <button
        class="phbtn"
        onclick={() => (layout.panel = false)}
        title="收起 ⌘J"
        aria-label="收起面板"
      >
        <Icon name="minus" />
      </button>
    </div>
    <div class="panel-body">
      <!--
        终端整块只藏不卸载：组件一销毁 Session 就 drop，shell 直接被 kill。
        切到 Git 日志页时正在跑的命令必须还在跑。
      -->
      <div class="tool-slot" class:hidden={panelTool !== "term"}>
        {#if terminal.comp}
          {#each terms.list as t (t.id)}
            <div class="term-slot" class:hidden={t.id !== terms.activeId}>
              <terminal.comp cwd={t.cwd} onExit={() => terms.close(t.id)} />
            </div>
          {/each}
        {:else}
          <div class="loading">正在载入终端…</div>
        {/if}
      </div>
      <!-- 收起时别去拉 git log：那是一串没人看的子进程 -->
      <!-- 同上：切走就整个销毁，那条 1.5 秒的轮询跟着停 -->
      {#if layout.panel && panelTool === "git" && layout.gitTab === "console" && repo}
        <div class="tool-slot">
          {#if gitcon.comp}
            <gitcon.comp />
          {:else}
            <div class="loading">正在载入 Git 控制台…</div>
          {/if}
        </div>
      {/if}
      {#if layout.panel && panelTool === "git" && layout.gitTab === "log" && repo}
        <div class="tool-slot">
          {#if gitLogReady}
            {@render gitLog()}
          {:else}
            <div class="loading">正在载入 Git 日志…</div>
          {/if}
        </div>
      {/if}
    </div>
  </div>
{/if}

{#if panelMenu}
  <ContextMenu
    x={panelMenu.x}
    y={panelMenu.y}
    label={panelMenu.kind === "list" ? "全部终端" : "终端的操作"}
    items={panelMenuItems}
    onclose={() => (panelMenu = null)}
  />
{/if}

<style>
  /* 与 .side-resizer 同一条判据（M8）：热区就是岛之间那条 6px 的缝，平时不画线 */
  .resizer {
    position: relative;
    flex: none;
    height: var(--island-gap);
    background: transparent;
    cursor: row-resize;
  }
  .resizer::after {
    content: "";
    position: absolute;
    left: 0;
    right: 0;
    top: 2.5px;
    height: 1px;
    background: transparent;
    transition: background 0.1s;
  }
  .resizer:hover::after { background: var(--accent); }
  .resizer:active::after { background: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .resizer::after { transition: none; } }
  /* 收起时整块不占位也不可见，但**仍然挂在 DOM 上** —— 见上面那段注释 */
  .resizer.hidden,
  .panel.hidden { display: none; }
  /*
   * **上边不画线。** 和侧边栏那条是同一个毛病：`.resizer` 已经用伪元素画了
   * 一条，这里再来一条，两条隔 1.5px。`.resizer` 和 `.panel` 共用同一个
   * `class:hidden={!panel}`，收起时一起走，线不会落单。
   */
  /* 底部工具窗整块是一座岛（M8），头在岛里 —— 头和内容是同一件东西的两层，不该隔着缝 */
  .panel {
    flex: none;
    display: grid;
    /* 26 → 32 → 38（M8）：和侧边栏头、标签栏同一个高度，三处的头在一条线上 */
    /* `minmax(0, 1fr)` 不是 `1fr`：理由见 App.svelte 的 main 那条 —— 终端里回显一行也别去问整座岛多高 */
    grid-template-rows: 38px minmax(0, 1fr);
    overflow: hidden;
    contain: strict;
    border: var(--island-border);
    border-radius: var(--island-radius);
  }
  .panel-head {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 9px;
    /* 头在岛里，画内容层的底（下面的终端 / 日志各画各的，不叠） */
    background: var(--content-bg);
    border-bottom: var(--island-border);
    color: var(--text-dim);
    user-select: none;
  }
  /*
   * 工具窗的名字。**它不是按钮** —— 切工具窗在导轨上，这里只回答
   * 「你现在看的是哪个」。比标签亮一档，右边那点留白就是分隔，
   * 不画竖线：线只用来分区，不用来分项。
   */
  .tw-name {
    flex: none;
    font-size: 12px;
    color: var(--text);
    padding-right: 7px;
  }
  .panel-head .gap { flex: 1; }
  /*
   * 头上的动作按钮：＋ / ⌄ / ⋮ / —。都是 22px 的方格子，
   * 和标签一样高 —— 一行里两种高度会让人以为它们不是一类东西。
   */
  .phbtn {
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
  .phbtn:hover { background: var(--hover); color: var(--text); }
  .phbtn:active { background: var(--pressed); }
  .phbtn:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }

  /*
   * 终端标签页。和上面的编辑器标签栏是**同一套**：内缩的圆角块 + `--selected`，
   * 没有竖线也没有下划线。两条标签栏在同一个窗口里，长相必须一致 ——
   * 否则人会以为它们是两种不同的东西。
   */
  .ptabs {
    display: flex;
    align-items: center;
    gap: 2px;
    height: 100%;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .ptabs::-webkit-scrollbar { height: 0; }
  .ptab {
    display: flex;
    align-items: center;
    flex: none;
    height: 24px; /* M8：和头里的按钮一样高 */
    border-radius: var(--r-sm);
    background: transparent;
  }
  .ptab:hover { background: var(--hover); }
  .ptab.on { background: var(--selected); }
  .pt-label {
    height: 100%;
    max-width: 140px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-size: 11.5px;
    padding: 0 2px 0 9px;
    cursor: default;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ptab.on .pt-label { color: var(--text); }
  /* 关不掉的标签（Git 窗那两个）没有 ✕ 占位，右边补回和左边一样的内缩 */
  .pt-label.fixed { padding-right: 9px; }
  /*
   * ✕ 的格子固定 16px，平时透明，hover / 当前项才显形 ——
   * 常驻的话每个标签一个 ✕，而任何一刻最多只关得掉一个；
   * 而格子固定，点击目标就不会跟着 hover 左右挪。
   */
  .pt-x {
    flex: none;
    display: grid;
    place-content: center;
    width: 16px;
    height: 16px;
    margin-right: 3px;
    background: transparent;
    border: none;
    border-radius: 5px;
    color: var(--text-faint);
    font-size: 9px;
    line-height: 1;
    cursor: default;
    opacity: 0;
  }
  .ptab:hover .pt-x, .ptab.on .pt-x { opacity: 1; }
  /* 当前标签的底已经是 --selected 了，hover 再用它等于没反馈 */
  .pt-x:hover { background: var(--pressed); color: var(--text); }
  .pt-x:focus-visible { opacity: 1; outline: 1px solid var(--accent); outline-offset: -1px; }

  /* 工具页整块叠在一起，只切可见性 —— 终端不能卸载 */
  .tool-slot { position: absolute; inset: 0; }
  .tool-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }
  .panel-body { overflow: hidden; position: relative; }
  .term-slot { position: absolute; inset: 0; }
  /* 用 visibility 而不是 display:none —— 后者会让 xterm 的尺寸计算拿到 0，
     切回来时排版是乱的 */
  .term-slot.hidden { visibility: hidden; pointer-events: none; z-index: -1; }

  .loading {
    display: grid;
    place-content: center;
    height: 100%;
    color: var(--text-faint);
    font-size: 12px;
  }
</style>
