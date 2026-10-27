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
  import { layout, type PanelTool } from "../state/layout.svelte";
  import { runs } from "../state/runs.svelte";
  import { notify } from "../state/notify.svelte";
  import { files } from "../state/files.svelte";

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
    panelTool: PanelTool;
    onSearch: () => void;
    onTogglePanel: (v: PanelTool) => void;
  } = $props();

  /** Maven 项目：pom.xml 在项目根或第一层子目录（同 Rust 的 `maven::reactors`）。按 ⌘P 那份文件索引判，不为它多问一次盘 */
  let maven = $derived(files.list.some((p) => p === "pom.xml" || /^[^/]+\/pom\.xml$/.test(p)));
  /** Gradle 项目：settings / build 文件在项目根或第一层子目录；项目根有 pom.xml 的按 Maven 算（同 Rust 的 `gradle::roots`） */
  let gradle = $derived(!files.list.includes("pom.xml") && files.list.some((p) => /^([^/]+\/)?(settings|build)\.gradle(\.kts)?$/.test(p)));
</script>

<!--
  常驻的工具竖条。所有侧边栏控件都住在这里，收起侧边栏时竖条留着 ——
  于是按钮在两个状态下位置完全一致。

  早先的做法是「展开时按钮在侧边栏头部右侧、收起时在标题栏左边」，
  结果同一个按钮在两个状态间横跳约 290 像素，每次都要重新找它在哪。
  控件的位置必须是肌肉记忆能记住的。
-->
<!--
  顶上原来还有一个「展开 / 收起侧边栏」的开关（2026-10-26 用户问「这个按钮存在的必要」，删了）：
  点导轨上当前亮着的那个视图就是收起、再点就展开 —— 同一件事印了两遍（ui.md 第六条）。键位（⌘1）和「视图」菜单那一项照旧
-->
<nav class="rail" aria-label="侧边栏工具">
  {#if root}
    <!--
      侧边栏的四个视图一个模板（2026-10-22 结构窗 ⌘7 进来时合的，同底部那组 10-19 的做法：同样的一段写四遍要付四份，入口包离告警线只剩几百字节）。
      - Git 改动只在有仓库时出现；角标带数字（原来是不带数字的圆点，条数印在状态栏的「改动 9」上 —— 删那个按钮时计数不能跟着没；
        IDEA 的提交工具窗图标就是这么标的）。99 封顶：三位数会把 26px 的按钮撑变形，「128 还是 132」在这儿没人要看
      - tooltip 里的 ⌘K、⌘7 是写死的：从 keymap.ts 取（byId）会把整张键位表拽进入口包（2026-09-23 实测 +6.8 KB）。改键位时这里跟着改
      - Maven 工具窗（docs/IDEA.md 第 1 步）：Maven 项目才有。IDEA 里它是独立的工具窗，这里也是（抄层级）
      - 结构（⌘7，docs/TAKEOVER.md G17）：上游的 structure 图标（2026-10-23 拉下来的，D11）
      - AI 助手（docs/TAKEOVER.md MVP-6）：开着项目就在。图标自绘（IDEA 社区版没有）
    -->
    {#each [{ v: "files", on: true, i: "files", l: "文件树", k: "", n: 0 }, { v: "git", on: !!repo, i: "git", l: "Git 更改", k: " ⌘K", n: changes }, { v: "maven", on: maven, i: "maven", l: "Maven", k: "", n: 0 }, { v: "gradle", on: gradle, i: "gradle", l: "Gradle", k: "", n: 0 }, { v: "structure", on: true, i: "structure", l: "结构", k: " ⌘7", n: 0 }, { v: "agent", on: true, i: "ai", l: "AI 助手", k: "", n: 0 }] as const as b}
      {#if b.on}
        <button
          class="ibtn lg rbtn"
          class:on={layout.sidebar && layout.sideView === b.v}
          onclick={() => layout.showSide(b.v)}
          title={b.l + b.k}
          aria-label={b.l}
        >
          <Icon name={b.i} />
          {#if b.n > 0}
            <span class="badge">{b.n > 99 ? "99+" : b.n}</span>
          {/if}
        </button>
      {/if}
    {/each}
    <button class="ibtn lg rbtn" onclick={onSearch} title="在项目中搜内容 ⌘⇧F" aria-label="搜索">
      <Icon name="search" />
    </button>
  {/if}
  <!--
    草稿（issue #40）。**不在 `{#if root}` 里** —— 草稿不属于任何项目，
    没开项目也要能记、能翻。放在项目那组的后面：它是「我的东西」，不是「这个项目的东西」。
    M10 给草稿加了锚点（记下在哪个项目 / 分支 / 文件行写的），**这条不变**：
    锚点是软的、存放位置不按项目分，只是列表会把当前项目的排在前面。见 docs/SCRATCH.md。
  -->
  <button
    class="ibtn lg rbtn"
    class:on={layout.sidebar && layout.sideView === "scratch"}
    onclick={() => layout.showSide("scratch")}
    title="草稿"
    aria-label="草稿"
  >
    <Icon name="note" />
  </button>
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
    class="ibtn lg rbtn"
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
      class="ibtn lg rbtn"
      class:on={layout.panel && panelTool === "git"}
      onclick={() => onTogglePanel("git")}
      title="Git：提交历史 · 控制台"
      aria-label="Git"
    >
      <Icon name="history" />
    </button>
  {/if}
  <!--
    Services（docs/IDEA.md 第 12 节）认出了 Spring Boot 服务才出现；运行窗（#48）跑过任务才出现（ui.md 第十条「工具窗有内容才出现」）：
    一个永远空着的按钮只是噪音。角标是还活着的几个（跑着 / 正在停），各数各的，全停了就不画 —— 同 Git 改动那个角标的判据。
    两个按钮一个模板：入口包离告警线只剩几百字节，同样的一段写两遍要多付一份（2026-10-19 量的，-284 B）
  -->
  <!-- 通知记录（I11）：说过一句才出现；角标是上次看过之后又来了几条坏消息（没做成、卡片），看一眼就清 -->
  {#each [{ v: "services", i: "services", on: runs.services > 0, n: runs.alive(true), l: "Services", k: "" }, { v: "run", i: "run", on: runs.plain().length > 0, n: runs.alive(), l: "运行", k: " ⌃⌥R" }, { v: "notifications", i: "bell", on: notify.log.length > 0, n: notify.unseen, l: "通知", k: "" }] as const as b}
    {#if b.on}
      <button
        class="ibtn lg rbtn"
        class:on={layout.panel && panelTool === b.v}
        onclick={() => onTogglePanel(b.v)}
        title={b.l + b.k}
        aria-label={b.l}
      >
        <Icon name={b.i} />
        {#if b.n > 0}
          <span class="badge">{b.n}</span>
        {/if}
      </button>
    {/if}
  {/each}
</nav>

<style>
  .rail {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 5px 0 6px;
    background: var(--panel-bg);
    /* 不画右边线（M8）：导轨和侧边栏都是玻璃，一条线分不出层，只是多一条线 */
    overflow: hidden;
  }
  .rail .rgap { flex: 1; }
  /*
   * 导轨按钮是 `.ibtn.lg`（26，导轨是整条竖栏唯一的内容，34 宽里 24 显得缩着 —— 唯一的例外）。
   * 选中态 `.ibtn.on` 用中性白不用 accent：accent 在这一列里已经有活儿干了，旁边那个
   * 「有未提交改动」的角标。两个都上色就分不出哪个是状态、哪个是「你现在在这儿」。
   */
  .rbtn { position: relative; transition: background 0.09s, color 0.09s; }
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
    /* 计数角标是胶囊：写成 99px，不靠「高 13 时 7px 刚好像胶囊」（数字变成两位、高度一改就不是了） */
    border-radius: 99px;
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
    font-family: var(--ui-font); font-variant-numeric: tabular-nums;
    font-size: var(--fs-xs);
    font-weight: 600;
  }
  @media (prefers-reduced-motion: reduce) { .rbtn { transition: none; } }
</style>
