/** 自动更新（docs/TAKEOVER.md MVP-8）的命令包装：只有菜单「检查更新…」和状态栏点了「重启更新」用，跟着它们懒加载 */
import { invoke } from "@tauri-apps/api/core";
import type { UpdateInfo } from "./commands";

/** 查一次：有新的就在后台下好、验过签名（过程走 `update-status`） */
export const updateCheck = () => invoke<UpdateInfo>("update_check");

/** 换成下好的新版、存好现场退出、退出后起新的。换不了（没写权限、包不对）回错误，不退 */
export const updateApply = () => invoke<void>("update_apply");
