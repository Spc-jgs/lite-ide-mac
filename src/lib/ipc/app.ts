/**
 * 应用层面的命令包装 —— **只有「帮助 / 应用」菜单那几项用的**（入口包瘦身，2026-09-21）。
 *
 * 判据同 `git.ts` / `log.ts`：包装函数跟着调用方走。这四条的调用方只有
 * `shell/menu-actions.ts`，而那个模块是懒的 —— 留在 `commands.ts` 的话，Rollup 会把
 * 整个 `commands.ts` 放进入口 chunk，懒模块再从它导入，等于入口包替一个一年按不了
 * 几次的「安装命令行工具…」付钱。DTO（`CliInstall`）还在 `commands.ts`，`dto_sync.rs` 只看那儿。
 */
import { invoke } from "@tauri-apps/api/core";
import type { CliInstall } from "./commands";

/** 装 `lite` 命令。开发构建（不在 .app 里）会 reject */
export const installCli = () => invoke<CliInstall>("install_cli");

/** 日志文件的路径 —— 拿它开一个标签，用这个应用自己的日志引擎看 */
export const appLogPath = () => invoke<string>("app_log_path");

/** 清空应用日志（两份都清）。判据在 Rust 侧，前端只是按一下 */
export const clearAppLog = () => invoke<void>("clear_app_log");

/** 交给系统默认浏览器打开。Rust 侧只放行 https —— 见那边的注释 */
export const openExternal = (url: string) => invoke<void>("open_external", { url });
