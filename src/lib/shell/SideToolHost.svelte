<script lang="ts">
  /**
   * 侧边栏里按需拉的工具窗（Maven、结构、AI 助手）共用的宿主：App（入口包）里只有这一个懒加载句柄，宿主自己按 `sideView` 再拉具体那个。
   * 原来 App 直接懒加载 MavenView；加结构窗（G17）时没再在入口里开第二个句柄 —— 入口里每多一处 `import()` 就多一张依赖表（frontend.md）
   */
  import { layout } from "../state/layout.svelte";
  import { lazy } from "../lazy/lazy.svelte";

  let { root }: { root: string } = $props();

  const maven = lazy(() => import("../run/MavenView.svelte"), "Maven 工具窗");
  const structure = lazy(() => import("../search/StructureView.svelte"), "结构");
  const agent = lazy(() => import("../agent/AgentPane.svelte"), "AI 助手");
  const gradle = lazy(() => import("../run/GradleView.svelte"), "Gradle 工具窗");
  const db = lazy(() => import("../db/DatabaseView.svelte"), "数据库工具窗");
  const cur = $derived(layout.sideView === "structure" ? structure : layout.sideView === "agent" ? agent : layout.sideView === "gradle" ? gradle : layout.sideView === "db" ? db : maven);
  $effect(() => cur.load());
</script>

{#if cur.error}
  <div class="empty-msg bad">{cur.error}</div>
{:else if cur.comp}
  <cur.comp {root} />
{/if}
