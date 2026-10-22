<script lang="ts">
  /**
   * 草稿列表的接线（2026-10-22 入口包瘦身，从 App.svelte 的 snippet 挪进来，理由同 GitPaneHost）：列表本身照旧吃 props
   */
  import ScratchList from "./ScratchList.svelte";
  import { tabs } from "../state/tabs.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { worktree } from "../state/worktree.svelte";
  import { notify } from "../state/notify.svelte";
  import { revealInFinder } from "../ipc/commands";
</script>

<ScratchList
  activePath={tabs.active?.path ?? ""}
  onOpen={(p, keep) => void tabflow.openPath(p, { preview: !keep })}
  onNew={() => void tabflow.newScratch()}
  onTrash={(p) => void tabflow.trashScratch(p)}
  onRename={(p, stem) => worktree.renameScratch(p, stem)}
  onSaveAs={(p) => void tabflow.openPath(p).then(() => worktree.saveAs())}
  onReveal={(p) => void revealInFinder(p).catch((e) => notify.fail(String(e)))}
/>
