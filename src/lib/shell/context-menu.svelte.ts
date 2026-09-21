/**
 * 右键菜单壳的懒加载句柄 —— 入口里三个用它的地方（标题栏项目挂件、标签栏、状态栏
 * 「缩进 · 换行符」那格）共用这一份（入口包瘦身，2026-09-21）。
 *
 * 判据同别处：菜单要先右键 / 先点一下才出现，窗口出现之前一次都不会有。
 * 2026-09-15 那次只把标签栏那处改懒，入口包反而**涨了 275 字节** —— 标题栏和面板
 * 还静态引着它，它根本没离开入口，只多了一层壳。所以这次三处一起改，并且
 * 句柄只有这一份：三份 `lazy()` 就是三份加载状态，而它们等的是同一个 chunk。
 *
 * 打开时才拉、拉到才画：`menu` 状态先置上，`{#if menu && cmenu.comp}` 等 chunk 一到
 * 就画出来。本地 chunk 几毫秒，人看不出「晚一拍」；App 首屏后 300ms 还预拉一次。
 * 文件树 / 面板 / Git 面板那些懒模块也用它，Rollup 会把它切成它们共享的小块 ——
 * 开着项目时它跟着文件树一起就到了。
 */
import { lazy } from "../lazy/lazy.svelte";
import type ContextMenu from "./ContextMenu.svelte";

export const cmenu = lazy<typeof ContextMenu>(() => import("./ContextMenu.svelte"), "右键菜单");
