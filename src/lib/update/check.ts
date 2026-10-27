/**
 * 菜单「检查更新…」（docs/TAKEOVER.md MVP-8）：手动查一次、把结果说出来。自动查（启动 30 秒后、每 12 小时）是静悄悄的，
 * 只有下好了才在状态栏出「重启更新」；手动查的人在等一句话 —— 没有新版、下好了、为什么没查成，都说
 */
import { notify } from "../state/notify.svelte";
import { progress } from "../state/progress.svelte";
import { updateCheck } from "../ipc/update";

export async function checkUpdate() {
  const t = progress.start("检查更新");
  try {
    const u = await updateCheck();
    if (u.phase === "ready") notify.ok(`新版本 ${u.version} 已就绪，可在状态栏中重启以完成更新`, 6000);
    else if (u.phase === "latest") notify.ok(`已是最新版本（${u.current}）`);
    else if (u.phase === "downloading" || u.phase === "checking") notify.hint("正在检查或下载更新，完成后将在状态栏中提示");
    else notify.fail(u.error ? `检查更新失败：${u.error}` : "检查更新失败");
  } catch (e) {
    notify.fail(`检查更新失败：${String(e)}`);
  } finally {
    t.end();
  }
}
