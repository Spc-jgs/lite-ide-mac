/**
 * 「拿一个路径能干的事」—— 文件树和标签栏的右键菜单都要用，抽出来免得抄两份。
 *
 * 抄两份的具体代价：这两个函数里真正的内容都是**错误信息怎么说**
 * （见各自注释），而那是最容易在第二份里退化成「操作失败」的东西。
 */
import { revealInFinder } from "../ipc/commands";
import { notify } from "../state/notify.svelte";

/** 在 Finder 里显示。名字别叫 reveal —— 文件树里那个 reveal() 是「在树里定位」 */
export async function showInFinder(path: string) {
  try {
    await revealInFinder(path);
  } catch (e) {
    // 盘上没了是最常见的失败（切了分支、在终端里删了），一句话说得清 → fail
    notify.fail(String(e).replace(/^Error:\s*/, ""));
  }
}

/**
 * 复制到剪贴板。`what` 是「路径」「相对路径」这种说法，进提示语。
 */
export async function copyText(text: string, what: string) {
  try {
    await navigator.clipboard.writeText(text);
    notify.ok(`已复制${what}：${text}`);
  } catch {
    /*
     * 失败要说清是**剪贴板**不让写，不能只说"复制失败" —— 后者会让人
     * 以为是路径有问题，去查文件树。顺带把内容放进消息里，至少能手动选中。
     */
    notify.fail(`剪贴板不可用，${what}是：${text}`);
  }
}

/** 相对项目根的路径；不在根下面就原样返回 */
export function relTo(root: string, p: string): string {
  const base = root.endsWith("/") ? root : `${root}/`;
  return p.startsWith(base) ? p.slice(base.length) : p;
}
