/**
 * 图片预览（docs/TAKEOVER.md G26）里能单测的那一半：缩放怎么算、信息栏怎么写。画的是 `ImagePreview.svelte`。
 */
import { invoke } from "@tauri-apps/api/core";

/**
 * 图片的原始字节（二进制回包；超过 32 MB 报错）。**不放 `ipc/fs.ts`**：那个模块原来只有文件树一个懒块用，
 * 多一个懒块共用它，Rollup 就把它拆成单独一块、入口里两张预加载表各多一个文件名（frontend.md「懒块之间新共用一个模块」）
 */
export const readImage = (path: string) => invoke<ArrayBuffer>("read_image", { path });

/** 缩放的档位：± 一下跳一档（IDEA 的图片编辑器也是按档跳，不是乘一个固定倍数 —— 乘法从 100% 出发回不到 100%） */
export const STEPS = [0.05, 0.1, 0.25, 0.5, 0.75, 1, 1.5, 2, 3, 4, 6, 8, 12, 16, 24, 32];
export const MIN = STEPS[0];
export const MAX = STEPS[STEPS.length - 1];

/** 四周留的空：图贴着岛边，看不出边界在哪 */
export const PAD = 24;

/**
 * 「适应」：大图缩到放得下，**小图不放大**（照 IDEA / macOS 预览）—— 16px 的图标放大到铺满一屏，
 * 看到的是一堆马赛克，而人想知道的是它「实际多大」。想放大就按 ＋
 */
export function fitScale(nw: number, nh: number, vw: number, vh: number): number {
  if (nw <= 0 || nh <= 0 || vw <= 0 || vh <= 0) return 1;
  return Math.max(MIN, Math.min(1, (vw - 2 * PAD) / nw, (vh - 2 * PAD) / nh));
}

/** 从当前比例往大 / 往小跳到下一档。当前比例落在两档之间（适应、捏合出来的）时，跳到那一侧最近的一档 */
export function step(cur: number, dir: 1 | -1): number {
  const eps = 1e-6;
  if (dir > 0) return STEPS.find((s) => s > cur + eps) ?? MAX;
  return [...STEPS].reverse().find((s) => s < cur - eps) ?? MIN;
}

/** 捏合 / ⌘ 滚轮：按滚动量连续缩放（不跳档），夹在上下限里 */
export function pinch(cur: number, deltaY: number): number {
  return Math.min(MAX, Math.max(MIN, cur * Math.exp(-deltaY * 0.01)));
}

export function percent(s: number): string {
  const p = s * 100;
  return `${p < 10 ? p.toFixed(1).replace(/\.0$/, "") : Math.round(p)}%`;
}

export function bytes(n: number): string {
  if (n < 1024) return `${n} B`;
  if (n < 1024 * 1024) return `${(n / 1024).toFixed(1).replace(/\.0$/, "")} KB`;
  return `${(n / 1024 / 1024).toFixed(1).replace(/\.0$/, "")} MB`;
}

/** MIME → 信息栏上那个类型名：`image/svg+xml` → SVG，`image/x-icon` → ICO */
export function kind(mime: string): string {
  const sub = mime.slice(mime.indexOf("/") + 1).replace(/^x-/, "").replace(/\+xml$/, "");
  return sub === "icon" ? "ICO" : sub.toUpperCase();
}
