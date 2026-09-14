<script lang="ts">
  /**
   * 左边那条常驻的工具竖条：打开哪个工具窗，**只有这一处**（ui.md 第十一条）。
   *
   * 侧边栏的开合 / 视图直接读写 `layout`；底部面板那两个按钮的「正在显示哪个」
   * 要看有没有仓库（偏好是 git 而当下没仓库时亮着的是终端），那个判断在 App
   * 的 `panelTool` 里，所以这里收 `panelTool` 和 `onTogglePanel` ——
   * issue #9 第 2 步把底部面板抽出去时一起挪走。
   */
  import Icon from "./Icon.svelte";
  import { layout } from "../state/layout.svelte";

  let {
    root,
    repo,
    changes,
    panelTool,
    onSearch,
    onTogglePanel,
  }: {
    root: string | null;
    repo: string | null;
    /** 未提交改动的条数，画在 Git 改动图标的角标上 */
    changes: number;
    /** 底部面板**实际在显示**的工具窗 */
    panelTool: "term" | "git";
    onSearch: () => void;
    onTogglePanel: (v: "term" | "git") => void;
  } = $props();
</script>

<!--
  常驻的工具竖条。所有侧边栏控件都住在这里，收起侧边栏时竖条留着 ——
  于是按钮在两个状态下位置完全一致。

  早先的做法是「展开时按钮在侧边栏头部右侧、收起时在标题栏左边」，
  结果同一个按钮在两个状态间横跳约 290 像素，每次都要重新找它在哪。
  控件的位置必须是肌肉记忆能记住的。
-->
<nav class="rail" aria-label="侧边栏工具">
  <button
    class="rbtn"
    class:on={layout.sidebar}
    onclick={() => (layout.sidebar = !layout.sidebar)}
    title={layout.sidebar ? "收起侧边栏 ⌘1" : "展开侧边栏 ⌘1"}
    aria-label={layout.sidebar ? "收起侧边栏" : "展开侧边栏"}
    aria-expanded={layout.sidebar}
  >
    <Icon name="sidebar" />
  </button>
  {#if root}
    <button
      class="rbtn"
      class:on={layout.sidebar && layout.sideView === "files"}
      onclick={() => layout.showSide("files")}
      title="文件树"
      aria-label="文件树"
    >
      <Icon name="files" />
    </button>
    {#if repo}
      <button
        class="rbtn"
        class:on={layout.sidebar && layout.sideView === "git"}
        onclick={() => layout.showSide("git")}
        title="Git 改动 ⌘⇧G"
        aria-label="Git 改动"
      >
        <Icon name="git" />
        <!--
          角标带数字。原来是个不带数字的 5px 圆点，而改动条数印在状态栏的
          「改动 9」按钮上 —— 那个按钮和这个图标是同一件事的两份入口，
          删掉按钮时计数不能跟着一起没。IDEA 的提交工具窗图标就是这么标的。
          99 是上限：三位数会把 26px 的按钮撑变形，而「到底是 128 还是 132」
          在这个位置上没人要看。
        -->
        {#if changes > 0}
          <span class="badge">{changes > 99 ? "99+" : changes}</span>
        {/if}
      </button>
    {/if}
    <button class="rbtn" onclick={onSearch} title="在项目中搜内容 ⌘⇧F" aria-label="搜索">
      <Icon name="search" />
    </button>
  {/if}
  <span class="rgap"></span>
  <!--
    底部工具窗的开关住在导轨上，和上面的文件树 / Git 改动同一套。

    原来这里是一个笼统的「面板」开关，而**切哪个工具窗**摆在面板头上 ——
    于是「换一个工具窗」这件事在同一个应用里有两种长相：侧边栏在导轨上换，
    底部在面板头上换。IDEA 只有一处，就是导轨；面板头腾出来留给
    工具窗自己的名字和它的标签页。

    点当前这个就收起 —— 和最上面 sidebar 那个开关是同一个手势。
  -->
  <button
    class="rbtn"
    class:on={layout.panel && panelTool === "term"}
    onclick={() => onTogglePanel("term")}
    title="终端 ⌘J"
    aria-label="终端"
  >
    <Icon name="terminal" />
  </button>
  <!--
    Git 工具窗（提交历史 + 控制台两个标签）。只在有仓库时出现 ——
    没有仓库时两个标签都是空的，一个永远空着的按钮只是噪音。
    图标用 `history` 不用 `git`：上面「Git 改动」已经占着分支图标了，
    同一列里出现两个一样的形状，人只能靠位置记忆去分。
  -->
  {#if repo}
    <button
      class="rbtn"
      class:on={layout.panel && panelTool === "git"}
      onclick={() => onTogglePanel("git")}
      title="Git：提交历史 · 控制台"
      aria-label="Git"
    >
      <Icon name="history" />
    </button>
  {/if}
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 5px 0 6px;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .rail .rgap { flex: 1; }
  .rbtn {
    position: relative;
    flex: none;
    display: grid;
    place-content: center;
    width: 26px;
    height: 26px;
    background: transparent;
    border: none;
    border-radius: var(--r-md);
    color: var(--text-faint);
    cursor: default;
    transition: background 0.09s, color 0.09s;
  }
  .rbtn:hover { background: var(--hover); color: var(--text); }
  /*
   * 选中态用中性白，不用 accent —— accent 在这一列里已经有活儿干了：
   * 旁边那个「有未提交改动」的红点。两个都上色就分不出哪个是状态、
   * 哪个是"你现在在这儿"。
   */
  .rbtn.on { color: var(--text); background: var(--selected); }
  .rbtn:active { background: var(--pressed); }
  .rbtn:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* 有未提交改动时给 Git 图标一个角标，收起侧边栏也知道有几处 */
  .rbtn .badge {
    position: absolute;
    right: 0;
    top: 0;
    display: grid;
    place-content: center;
    min-width: 13px;
    height: 13px;
    padding: 0 3px;
    border-radius: 7px;
    background: var(--git-modified);
    /*
      深色字压在 --git-modified（#6ba1e8，浅蓝）上，深浅两套主题里这个底色是同一个，
      所以这里直接写死一个近黑而不是用 --text：--text 在浅色主题下是深的、
      深色主题下是白的，而白字压在浅蓝上读不清。

      **不描边。** 角标会盖住图标右上那个结点，直觉是用外壳色描一圈把它抠出来 ——
      但外壳层是 transparent（后面是 NSVisualEffectView），描一圈实色就是在玻璃上
      凿一个洞。角标本身不透明，压住一段描边足够说清「它在上面」。
    */
    color: #101014;
    font-family: var(--code-font);
    font-size: 9px;
    font-weight: 600;
  }
  @media (prefers-reduced-motion: reduce) { .rbtn { transition: none; } }
</style>
