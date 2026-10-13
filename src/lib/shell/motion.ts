/**
 * 退场动效（2026-10-13 盘点 C1）。只给「压在内容上、一消失底下就跳」的两种：
 * - 对话框的压暗层：一闪而没刺眼 → 120ms 淡出；
 * - 卡片：一张没了，下面那张瞬移上来像抖了一下 → 140ms 收起高度 + 淡出，下面的跟着平滑补上。
 * 菜单、浮层不加 —— 它们一关人接着就要操作，等动画就是卡（macOS 原生菜单也是瞬没）。
 *
 * 用 Svelte 的 `out:`：量过，过渡的运行时本来就在入口包里，多一处用法 +50 B。
 * Svelte 的过渡不看系统的「减弱动态效果」，这里自己看（app.css 的 CSS 动画由那个 media 块管）。
 */
import { fade, slide, type TransitionConfig } from "svelte/transition";

const reduced = () => typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;

export function layerOut(node: Element): TransitionConfig {
  // 淡出中的那层不再接鼠标（Confirms 里 `.layer.leaving`）：点穿它落到编辑区上
  node.classList.add("leaving");
  return fade(node, { duration: reduced() ? 0 : 120 });
}

export function cardOut(node: Element): TransitionConfig {
  const s = slide(node, { duration: reduced() ? 0 : 140 });
  return { ...s, css: (t, u) => `${s.css?.(t, u) ?? ""};opacity:${t}` };
}
