/**
 * 键位覆盖（docs/TAKEOVER.md R3）的命令包装。单独一个文件：只有 `state/keys-live.ts` 和设置对话框的键位页引，都是懒的 ——
 * 封装跟着调用方走（frontend.md：放进入口里的 `ipc/app.ts` 就跟着进了入口）
 */
import { invoke } from "@tauri-apps/api/core";

/** 你改过的键位：`{ 动作 id: 键位 }`，`""` = 不要键位。只有改过的 */
export const keymapOverrides = () => invoke<Record<string, string>>("keymap_overrides");

/** 整张覆盖表换掉、写 keymap.json；菜单当场换键，所有窗口收到 `keymap-changed` */
export const keymapSet = (overrides: Record<string, string>) => invoke<void>("keymap_set", { overrides });

/** 设置里开始 / 停止录键：录的时候菜单上的键全摘掉（AppKit 先吃菜单的键，页面收不到 ⌘S 这种），停了挂回去 */
export const keymapRecord = (on: boolean) => invoke<void>("keymap_record", { on });
