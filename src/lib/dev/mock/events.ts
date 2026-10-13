/**
 * 桩：事件的登记表 —— 谁在听哪个名字。
 *
 * 真实现里「这个事件发给哪些回调」是 **Rust 那边**按名字（和目标窗口）路由的：`@tauri-apps/api` 的 `listen` 只是把回调交给
 * `transformCallback`、再 `invoke("plugin:event|listen", { event, handler })`，页面这一侧**不按名字过滤**（读过 2.11.1 的 `event.js`）。
 * 所以桩也得自己记着名字、按名字发。原来 `__mockMenu` 把菜单事件发给 `window._cb*` 的全部回调，注释写的是「每个监听器自己会对名字」——
 * 那是错的：`settings-changed` 的监听收到菜单 id，把 `settings.v` 换成了一个字符串，状态栏读 `settings.v.problems.length` 抛错
 * （2026-10-13 验对话框时撞见）；`_cb*` 里还有终端 / 日志数据通道的回调，也一并被喂了菜单 id。
 * `scratchbook`、`task-exit` 原来各自在 app.ts / tasks.ts 里记一份编号、还不认 `unlisten`（热更新、状态测试重复挂，旧回调照收），一起收到这里。
 */
import { type A, NOT_MINE } from "./data";

/** 监听编号 → 事件名。编号就是 `transformCallback` 给回调起的那个（`window._cb<编号>`），也当 eventId 还给前端（unlisten 时带回来） */
const LISTENERS = new Map<number, string>();

export function eventsCmd(cmd: string, a: A): unknown {
  switch (cmd) {
    case "plugin:event|listen": {
      const h = Number(a.handler);
      LISTENERS.set(h, String(a.event));
      return h;
    }
    case "plugin:event|unlisten":
      LISTENERS.delete(Number(a.eventId));
      return null;
    default:
      return NOT_MINE;
  }
}

/** 发一个事件给听着这个名字的回调（同 Rust 的 `emit`；桩里只有一个窗口，不分目标）。返回发给了几个 */
export function mockEmit(event: string, payload: unknown): number {
  let n = 0;
  for (const [h, name] of LISTENERS) {
    if (name !== event) continue;
    const f = (window as unknown as Record<string, unknown>)[`_cb${h}`];
    if (typeof f !== "function") continue;
    (f as (e: unknown) => void)({ event, id: h, payload });
    n++;
  }
  return n;
}
