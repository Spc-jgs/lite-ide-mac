/**
 * 打开交互式变基对话框（docs/TAKEOVER.md G25）。单独一个文件：提交历史（懒块）点了右键菜单那一项才 `import()` 它，
 * 不跟着 `settings/open.ts`（那里静态引着设置对话框那几个，提交历史引它就全拽进来了）
 */
import { openModal } from "../settings/modal-host";
import RebaseDialog from "./RebaseDialog.svelte";

let open = false;

export function openRebase(repo: string, from: string) {
  if (open) return;
  open = true;
  openModal(RebaseDialog, { repo, from }, () => (open = false));
}
