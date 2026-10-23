<script lang="ts">
  /**
   * 差异标签、合并标签的接线（2026-10-23 入口包瘦身，从 Content.svelte 挪进来，同 `GitPaneHost` 那次）：
   * DiffView / MergeView 照旧只吃 props；十来个 prop 和回调原来写在 Content 的模板里、跟着它进入口包，
   * 挪到这里就跟着 Git 那组懒块走。Content 那边只剩「给我这个标签」一句
   */
  import DiffView from "./DiffView.svelte";
  import MergeView from "./MergeView.svelte";
  import type { TabState } from "../state/tab";
  import { git } from "../state/git.svelte";

  let { tab }: { tab: TabState } = $props();
</script>

{#if tab.mode === "merge"}
  <MergeView
    text={tab.mergeText ?? ""}
    path={tab.rel ?? tab.name}
    onResolve={(c, r) => void git.resolveMerge(tab, c, r)}
  />
{:else}
  <DiffView
    raw={tab.diffRaw ?? ""}
    capped={!!tab.diffCapped}
    path={tab.rel ?? tab.name}
    staged={!!tab.diffStaged}
    commit={tab.diffShort ?? ""}
    toLocal={!!tab.diffToLocal}
    untracked={!!tab.diffUntracked}
    onToggleStaged={() => void git.toggleDiffSide(tab.id)}
    onApplyHunk={(patch, unstage) => void git.applyHunk(patch, unstage)}
    onRevertHunk={(patch) => (git.pendingRevertHunk = patch)}
    loadNew={() => git.diffNewText(tab.id)}
  />
{/if}
