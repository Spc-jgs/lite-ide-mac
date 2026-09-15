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
  import Crash from "./Crash.svelte";
  import { layout } from "../state/layout.svelte";

  let {
    root,
    repo,
    gitReady,
    fileTree,
    gitPane,
    scratchList,
  }: {
    root: string | null;
    repo: string | null;
    /** Git 面板那个懒加载 chunk 到了没 */
    gitReady: boolean;
    fileTree: Snippet;
    gitPane: Snippet;
    scratchList: Snippet;
  } = $props();

  /** 侧边栏横向拖拽。上限留出编辑区的活路，不让它被挤没 */
  function startResize(e: PointerEvent) {
    e.preventDefault();
    layout.resizing = true;
    const startX = e.clientX;
    const startW = layout.sidebarWidth;
    const move = (ev: PointerEvent) => {
      layout.sidebarWidth = Math.max(
        140,
        Math.min(window.innerWidth - 360, startW + (ev.clientX - startX)),
      );
    };
    const up = () => {
      layout.resizing = false;
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
  }
</script>

<aside>
  <svelte:boundary>
    {#if layout.sideView === "scratch"}
      <!-- 草稿排在 `!root` 前面：没开项目也要能翻草稿（issue #40） -->
      {@render scratchList()}
    {:else if !root}
      <div class="no-root">把文件夹拖进来</div>
    {:else if layout.sideView === "git" && repo && gitReady}
      {@render gitPane()}
    {:else if layout.sideView === "git" && repo}
      <div class="no-root">正在载入 Git 面板…</div>
    {:else}
      {@render fileTree()}
    {/if}
    {#snippet failed(err, reset)}
      <Crash error={err} scope="侧边栏" onReset={reset} />
    {/snippet}
  </svelte:boundary>
</aside>
<div
  class="side-resizer"
  role="separator"
  aria-label="调整侧边栏宽度"
  aria-orientation="vertical"
  onpointerdown={startResize}
></div>

<style>
  aside { overflow: hidden; }
  /* 不画右边线，理由同 FileTree 的 `.tree` —— 那条边界归 `.side-resizer` */
  .no-root {
    padding: 14px 12px;
    color: var(--text-faint);
    font-size: 12px;
    background: var(--panel-bg);
    height: 100%;
  }
  /*
   * 拖拽条：**热区和画出来的线要分开。**
   *
   * 原来是 `background: var(--border)` —— 热区多宽，亮条就多宽，
   * 于是界面正中间横着一条 4px 的白条（876px 高，玻璃上更扎眼）。
   * 但 4px 是好按的下限，不能为了好看把热区缩掉。
   *
   * 所以底留空，只用一个居中的 1px 伪元素画线。悬停时线变 accent，
   * 按住时才把整条 4px 点亮 —— 那时人已经在拖了，反馈越实越好。
   */
  .side-resizer {
    position: relative;
    background: transparent;
    cursor: col-resize;
  }
  .side-resizer::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 1.5px;
    width: 1px;
    background: var(--border);
    transition: background 0.1s;
  }
  .side-resizer:hover::after { background: var(--accent); }
  .side-resizer:active { background: var(--accent); }
  @media (prefers-reduced-motion: reduce) { .side-resizer::after { transition: none; } }
</style>
