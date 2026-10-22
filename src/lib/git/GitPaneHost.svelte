<script lang="ts">
  /**
   * Git 改动面板的接线（2026-10-22 入口包瘦身，从 App.svelte 的 snippet 挪进来）：GitPane 本身照旧是只吃 props 的展示组件，
   * 数据和动作在这里从 store 接上。原来这二十来个回调写在 App 的模板里，跟着 App 进入口包；挪到这里就跟着 GitPane 一起懒加载
   */
  import GitPane from "./GitPane.svelte";
  import { git } from "../state/git.svelte";
  import { remote } from "../state/remote.svelte";
  import { layout } from "../state/layout.svelte";
  import { gitStage, gitUnstage } from "../ipc/commands";
</script>

<GitPane
  status={git.status}
  busy={git.busy}
  committing={git.writing === "提交"}
  committed={git.justDone === "提交"}
  onOpenDiff={(e, staged) => void (e.conflicted ? git.openMerge(e) : git.openDiff(e, staged))}
  onStage={(paths) => void git.run("暂存失败", () => gitStage(git.repo!, paths), "暂存")}
  onUnstage={(paths) => void git.run("取消暂存失败", () => gitUnstage(git.repo!, paths), "取消暂存")}
  onDiscard={(es) => (git.pendingDiscard = es)}
  onCommit={(m, amend, push) =>
    void git.commit(m, amend).then((ok) => {
      // 提交成功才推；推之前照常走推送确认条（列出要推的提交）
      if (ok && push) void remote.askPush();
    })}
  onRefresh={() => void git.refresh()}
  stashCount={git.stashes.length}
  onStash={() => void git.stashPush()}
  onUnstash={() => void git.stashPop()}
  onOpenLog={() => layout.openGitTab("log")}
  ahead={git.status?.ahead ?? 0}
  behind={git.status?.behind ?? 0}
/>
