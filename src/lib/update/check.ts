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
    if (u.phase === "ready") notify.ok(`新版 ${u.version} 已下好：点状态栏的「重启更新」换上`, 6000);
    else if (u.phase === "latest") notify.ok(`已经是最新的（${u.current}）`);
    else if (u.phase === "downloading" || u.phase === "checking") notify.hint("正在查 / 下载，好了状态栏会出「重启更新」");
    else notify.fail(u.error || "没查成");
  } catch (e) {
    notify.fail(`没查成：${String(e)}`);
  } finally {
    t.end();
  }
}
