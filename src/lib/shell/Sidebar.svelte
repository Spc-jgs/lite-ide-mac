<script lang="ts">
  /**
   * 侧边栏的**外壳**：开合、宽度拖拽、哪个视图在显示、崩溃边界。
   *
   * 里面的内容不归它：文件树是 `FileTree.svelte`，Git 改动是 `git/Pane.svelte`，
   * 两块各自的数据和回调都接在 App 上，所以以 snippet 的形式从 App 传进来 ——
   * 这层只决定「现在该露出哪一块」。等 issue #9 后面几步把标签表和 git 动作
   * 也搬出 App，这两个 snippet 就能收进来。
   *
   * 渲染两个根元素：`<aside>` 和右边那根拖拽条。它们是 `.workspace` 那个
   * grid 里并排的两列（`34px var(--side-w) 4px 1fr`），不能包一层。
   */
  import type { Snippet } from "svelte";
  import Splitter from "./Splitter.svelte";
  import Crash from "./Crash.svelte";
  import { layout } from "../state/layout.svelte";

  let {
    repo,
    gitReady,
    fileTree,
    gitPane,
    scratchList,
    mavenView,
  }: {
    repo: string | null;
    /** Git 面板那个懒加载 chunk 到了没 */
    gitReady: boolean;
    fileTree: Snippet;
    gitPane: Snippet;
    scratchList: Snippet;
    /** Maven 工具窗（docs/IDEA.md 第 1 步）、结构（⌘7）、AI 助手：同一个宿主，按 sideView 挑 */
    mavenView: Snippet;
  } = $props();

  let windowWidth = $state(window.innerWidth);
</script>

<aside>
  <svelte:boundary>
    {#if layout.sideView === "scratch"}
      <!-- 草稿不看 root：没开项目也要能翻草稿（issue #40）。没项目的其他视图根本不会走到这儿（`layout.sideShown`） -->
      {@render scratchList()}
    {:else if layout.sideView === "git" && repo && gitReady}
      {@render gitPane()}
    {:else if layout.sideView === "git" && repo}
      <div class="no-root"><span class="spinner sm"></span> 正在载入 Git 面板…</div>
    {:else if layout.sideView === "maven" || layout.sideView === "structure" || layout.sideView === "agent" || layout.sideView === "gradle" || layout.sideView === "db"}
      {@render mavenView()}
    {:else}
      {@render fileTree()}
    {/if}
    {#snippet failed(err, reset)}
      <Crash error={err} scope="侧边栏" onReset={reset} />
    {/snippet}
  </svelte:boundary>
</aside>
<svelte:window bind:innerWidth={windowWidth} />
<Splitter label="调整侧边栏宽度" value={layout.sidebarWidth} min={140} max={Math.max(140, windowWidth - 360)} defaultValue={240}
  onchange={(v) => layout.sidebarWidth = v} ondrag={(v) => layout.resizing = v} />

<style>
  /*
   * 侧边栏是一座岛（2026-09-18，ui.md 二「岛按是什么分」）：和底部工具窗同一套描边、
   * 圆角、底色。上下各留一条 `--island-gap`：上面不贴标题栏，下面不贴状态栏 ——
   * 和主区右缝、下缝同一个数。头（38px）在岛里，同底部工具窗。
   */
  aside {
    overflow: hidden;
    margin: var(--island-gap) 0;
    background: var(--content-bg);
    border: var(--island-border);
    border-radius: var(--island-radius);
  }
  /* 不画右边线，理由同 FileTree 的 `.tree` —— 那条边界归 `.side-resizer` */
  .no-root {
    padding: 14px 12px;
    color: var(--text-faint);
    font-size: var(--fs-md);
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    height: 100%;
  }
</style>
