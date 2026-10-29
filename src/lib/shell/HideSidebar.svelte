<script lang="ts">
  import Icon from "./Icon.svelte";
  import { layout } from "../state/layout.svelte";
</script>

<!-- 隐藏只改布局；不关闭工具窗的会话、连接或任务，和底部工具窗的 — 同义。 -->
<button class="ibtn" onclick={() => (layout.sidebar = false)} title="隐藏侧栏" aria-label="隐藏侧栏">
  <Icon name="minus" />
</button>

<style>
  .ibtn { margin-left: auto; }
</style>
