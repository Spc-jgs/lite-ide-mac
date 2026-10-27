/**
 * 状态栏「重启更新」点下去（docs/TAKEOVER.md MVP-8）。重启照 ⌘Q 的路走：每个窗口存好现场（未保存的照常自动保存、会话照常恢复），
 * 所以一下就做，不再弹框问 —— 同 IDEA 更新通知上的「Restart」。
 * 例外：有任务 / 服务在跑，重启会把它们停掉（Spring 服务停了要重新起），先说一句、再点一次才做
 */
import { notify } from "../state/notify.svelte";
import { runs } from "../state/runs.svelte";
import { updateApply } from "../ipc/update";

export function restartToUpdate(version: string) {
  const n = runs.alive() + runs.alive(true);
  if (n > 0) {
    notify.fail(`${n} 个任务正在运行，重启将终止这些任务`, 8000, { label: "仍要重启", run: () => void go(version) });
    return;
  }
  void go(version);
}

async function go(version: string) {
  try {
    await updateApply();
    notify.ok(`正在安装 ${version}，即将重启…`, 6000);
  } catch (e) {
    notify.fail(`安装更新失败：${String(e)}`);
  }
}
