/**
 * 对话框（`Confirms.svelte` 的 `.layer .dialog`，RemoteBars 问的那两种也在里面）开着时，别的输入一律不算数 ——
 * 「压暗」只是告诉人先回答这个，键盘和菜单走的是另外的路，原来照样生效：⇧⇧ / ⌘P 的浮层叠到压暗层上，
 * ⌃Tab 切到 B 而对话框问的还是 A（code review 2026-10-13）。macOS 的表单（sheet）挡的是整个窗口，这里照它：
 * 键盘、鼠标、菜单、「在草稿本新建」四个入口都先问一句 [`dialogUp`]，被挡下的那一下用 [`nudgeDialog`] 告诉人为什么没反应。
 * 哪个键放、哪个键挡（`keyVerdict`）只有 Confirms 用，放在跟着它懒加载的 `dialog-focus.ts` 里
 *
 * 判据看 DOM（画出来了 = 在问），不看各个 store 的 `pending*`：那张名单在 Confirms 里，搬进入口要把五个 store 一起拽进来；
 * 而「人眼前有没有一个对话框」本来就是这个问题的原意。这个文件在入口包里（App 和 menu-actions 用），只放这两个函数
 */

/** 此刻在问的那个对话框；没有就 null */
export function dialogUp(): HTMLElement | null {
  return document.querySelector<HTMLElement>(".layer .dialog");
}

/**
 * 被挡下的那一下：焦点回到对话框的默认按钮、对话框轻轻顶一下（`app.css` 的 `.dialog.nudge`）。
 * 不给反馈的话，被挡和「快捷键坏了」长得一模一样（frontend.md：「装了不生效」不报错，人以为自己按错了）
 */
export function nudgeDialog(d: HTMLElement | null = dialogUp()) {
  if (!d) return;
  const def = d.querySelector<HTMLElement>("[data-default]") ?? d.querySelector<HTMLElement>("[data-dismiss]");
  if (def && !d.contains(document.activeElement)) def.focus({ preventScroll: true });
  d.classList.remove("nudge");
  void d.offsetWidth; // 重新触发动画：连着被挡两下，第二下也要顶
  d.classList.add("nudge");
  // 到点摘掉：减弱动效时它是一圈描边、没有动画结束这回事，不摘就一直描着
  clearTimeout(nudgeTimer);
  nudgeTimer = setTimeout(() => d.classList.remove("nudge"), 400);
}
let nudgeTimer: ReturnType<typeof setTimeout> | undefined;
