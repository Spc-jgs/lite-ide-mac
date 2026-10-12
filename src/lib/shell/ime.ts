/**
 * 这一下按键是不是给输入法的（组字、翻候选、确认候选字）—— 是的话别当成我们的键（#56 第 3 条：
 * 任务列表里拼音打「后端」，按回车确认候选字，同一下回车把选中的任务跑起来了）。
 *
 * 只看 `isComposing` 不够：WebKit（Safari、WKWebView）里**确认候选字的那一下回车**，`compositionend` 先到、`keydown` 后到，
 * 到 `keydown` 时 `isComposing` 已经是 false。那一下的 `keyCode` 是 229（「这个键输入法处理过了」），要靠它认。
 * `keyCode` 已经标了废弃，但这件事上没有替代品 —— `key` 在这时是 "Enter"，和一下真回车分不出来
 */
export function imeKey(e: Pick<KeyboardEvent, "isComposing" | "keyCode">): boolean {
  return e.isComposing || e.keyCode === 229;
}
