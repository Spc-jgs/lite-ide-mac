import type { MenuItem } from "../shell/ContextMenu.svelte";
import { git } from "../state/git.svelte";
import { tabflow } from "../state/tabflow.svelte";
import { layout } from "../state/layout.svelte";
import { gitStage, gitUnstage } from "../ipc/commands";
import { logFilter } from "./log-filter.svelte";

/** 菜单绑定右键的文件，不读活动标签；文件树、编辑器、更改列表同一组动作。 */
export function fileGitMenu(path: string): MenuItem[] {
  const repo = git.repo;
  if (!repo || !path.startsWith(`${repo}/`)) return [];
  const rel = path.slice(repo.length + 1);
  const e = git.status?.entries.find((e) => e.path === rel);
  const writing = !!git.writing;
  return [
    { label: "查看工作区更改", sep: true, disabled: !e?.unstaged && !e?.conflicted, run: () => e && void (e.conflicted ? git.openMerge(e) : git.openDiff(e, false)) },
    { label: "查看已暂存更改", disabled: !e?.staged, run: () => e && void git.openDiff(e, true) },
    { label: "查看提交历史", disabled: !!e?.untracked, run: () => { logFilter.v = { path, lines: null }; layout.openGitTab("log"); } },
    { label: e?.conflicted ? "标记为已解决" : "暂存文件", sep: true, disabled: writing || (!e?.unstaged && !e?.conflicted), run: () => void git.run("暂存失败", () => gitStage(repo, [rel]), "暂存") },
    { label: "取消暂存", disabled: writing || !e?.staged, run: () => void git.run("取消暂存失败", () => gitUnstage(repo, [rel]), "取消暂存") },
    { label: "放弃文件更改…", danger: true, disabled: writing || !e, run: () => { if (e) git.pendingDiscard = [e]; } },
  ];
}

export const openWorkingFile = (path: string) => void tabflow.openPath(path, { preview: false });
