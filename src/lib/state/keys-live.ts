/**
 * 把你改过的键位（`keymap.json`，docs/TAKEOVER.md R3）放进 `keymap.ts`，改了（设置里、别的窗口、手改文件）再换上。
 *
 * 谁引谁就让它开始取：`shell/menu-actions.ts`（首屏后 300ms 预拉）和 `editor/id-keys.ts`（编辑器的懒块）。不放进入口：
 * 窗口出来那一刻用不着它 —— 菜单的键 Rust 建菜单时就挂好了；编辑器的键先按默认建、取回来再重配（`onOverrides`），差几十毫秒
 */
import { setOverrides } from "./keymap";
import { listenHere } from "../ipc/commands";
import { keymapOverrides } from "../ipc/keymap";

void keymapOverrides()
  .then(setOverrides)
  .catch(() => {});
void listenHere<Record<string, string>>("keymap-changed", (e) => setOverrides(e.payload)).catch(() => {});
