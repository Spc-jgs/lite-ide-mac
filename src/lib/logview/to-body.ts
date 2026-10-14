/**
 * Svelte action：把节点挂到 `body` 上，挪完再给里面第一个输入框焦点、全选。
 *
 * 为什么要挪：日志面板（`LogPane` 的 `.pane`）有 `container-type: inline-size`，它顺带给了布局包含 ——
 * **`position: fixed` 的后代从此以它为参照、不再以窗口为参照**，按屏幕坐标摆的浮层会落到别处、被裁掉（frontend.md 2026-10-14 那节）。
 *
 * 为什么焦点在这儿给：`append` 是摘下来再插回去，已经拿到的焦点当场丢；子元素的 action 又先于父元素跑 ——
 * 在输入框自己的 action 里给焦点，紧接着就被这一下挪丢了，敲的字落空。
 */
export function toBody(node: HTMLElement) {
  document.body.append(node);
  const input = node.querySelector<HTMLInputElement>("input");
  input?.focus({ preventScroll: true });
  input?.select();
  return { destroy: () => node.remove() };
}
