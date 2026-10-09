/**
 * 「发送到终端」（#45，⌥⌘K / 编辑器右键）：把当前文件、或选中的那几行，以 `@路径#L10-20` 写进终端。
 *
 * AI 线的第一步（DIRECTION 第 7 节）：终端里本来就能跑 `claude`、输出里的「文件:行」也能 ⌘Click 跳回来，
 * 缺的是反方向 —— 想让它看某几行，得自己手打路径。这里补上这一半。
 *
 * 三条取舍：
 * - **只写入，不回车**：引用后面通常还要补一句「这里为什么空指针」，回车是人的事。
 * - **发给当前那个终端**（面板里亮着的那个，也就是最近用的那个）；一个都没有就开一个。
 *   新开的那个里还没有 claude —— 引用停在 shell 的提示符上，前面补 `claude "` 就能用；替人起 claude 不在这一步。
 * - **路径相对于终端前台进程的工作目录**（`pty_cwd`），不是项目根：claude 按它自己的目录解析 `@`，
 *   而人可能 `cd` 进子目录才起的它。不在那个目录底下就给绝对路径。
 */
import { tick } from "svelte";
import { tabs } from "../state/tabs.svelte";
import { terms } from "../state/terms.svelte";
import { layout } from "../state/layout.svelte";
import { project } from "../state/project.svelte";
import { docs } from "../state/docs.svelte";
import { notify } from "../state/notify.svelte";
import { mention, refPath } from "./claude-ref";
import { ready } from "./hooks";

export async function sendToTerminal(path = tabs.active?.path) {
  if (!path) return;
  // 先读选区：下面一开终端、焦点一挪，编辑器还在，但人可能已经在想下一件事了
  const spans = docs.selectedLines(path);
  let id = terms.activeId;
  if (id === null) id = terms.open(project.root ?? "~").id;
  // 偏好是 Git 窗时也要切过来，不然引用写进了一个看不见的终端
  layout.panelView = "term";
  layout.panel = true;
  const h = await ready(id);
  if (!h) {
    notify.fail("终端还没准备好，没发出去");
    return;
  }
  const started = terms.list.find((t) => t.id === id)?.cwd ?? null;
  h.paste(mention(refPath(path, (await h.cwd()) ?? started), spans));
  // 终端那格刚从隐藏变成显示，等这一拍画出来再给焦点（display:none 的元素拿不到焦点）
  await tick();
  h.focus();
}
