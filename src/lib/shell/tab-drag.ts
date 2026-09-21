/**
 * 标签拖拽（2026-09-21）：组内排序，拖到另一条标签条就是挪组（分屏欠的最后一块）。
 *
 * **不走 HTML5 的 dragstart / drop。** 在 Tauri 的 WKWebView 里那套事件不完整：wry 为了接住
 * 从 Finder 拖进来的文件接管了 NSView 的拖拽入口，页面内部的 `draggable` 按下去拖过去，
 * `drop` 从来不来（文件树 2026-09-15 用 CGEvent 验过）。所以和文件树一样自己用 pointer 事件做。
 *
 * **按需加载**：`Tabs.svelte` 在入口包里，这里的落点计算、幽灵标签、插入线加起来两千多字节，
 * 而它们只在人真的拖起一个标签之后才有用 —— Tabs 里只留「按下、挪过 5px」那几行，
 * 过了阈值才 `import()` 这里。第一次拖多等一次本地 chunk 往返，人看不出。
 *
 * 落点由 DOM 决定，不由状态决定：`elementFromPoint` 找到鼠标底下那条标签条（`.tabs[data-group]`，
 * 两个组各一条）和那条标签，按标签中线判断插左边还是右边；`index` 是「目标组里除了被拖的
 * 这个以外」的序号 —— 和 `tabs.moveTo` 的约定一致，同组往后拖不用再减一。
 */
import type { Group } from "../state/tab";

export interface DragSpec {
  id: number;
  label: string;
  /** 能不能落到这一组（组里只有它一个时不能挪走，见 `tabs.moveTo`） */
  canDrop: (g: Group) => boolean;
}

interface Target {
  g: Group;
  index: number;
  /** 插入线的位置 */
  x: number;
  top: number;
  height: number;
}

/**
 * 从「已经过了阈值的那一下 pointermove」开始接管，到 pointerup 结束。
 * `onDrop` 只在落在一条能接的标签条上时叫。
 */
export function dragTab(e: PointerEvent, spec: DragSpec, onDrop: (g: Group, index: number) => void): void {
  const ghost = document.createElement("div");
  ghost.textContent = spec.label;
  ghost.setAttribute("aria-hidden", "true");
  Object.assign(ghost.style, {
    position: "fixed",
    zIndex: "60",
    padding: "2px 8px",
    fontSize: "12px",
    fontFamily: "var(--ui-font)",
    color: "var(--text)",
    background: "var(--elevated)",
    border: "1px solid var(--border)",
    borderRadius: "var(--r-sm)",
    boxShadow: "var(--shadow-pop)",
    pointerEvents: "none",
    whiteSpace: "nowrap",
  } satisfies Partial<CSSStyleDeclaration>);
  // 插入线：2px 的 accent 竖线，和岛间拖拽热区亮起来是同一种颜色
  const line = document.createElement("div");
  line.setAttribute("aria-hidden", "true");
  Object.assign(line.style, {
    position: "fixed",
    zIndex: "59",
    width: "2px",
    background: "var(--accent)",
    borderRadius: "1px",
    pointerEvents: "none",
    display: "none",
  } satisfies Partial<CSSStyleDeclaration>);
  document.body.append(ghost, line);

  let target: Target | null = null;

  function place(ev: PointerEvent) {
    ghost.style.left = `${ev.clientX + 12}px`;
    ghost.style.top = `${ev.clientY + 12}px`;
    target = targetAt(ev.clientX, ev.clientY);
    if (target) {
      line.style.display = "block";
      line.style.left = `${target.x}px`;
      line.style.top = `${target.top}px`;
      line.style.height = `${target.height}px`;
    } else {
      line.style.display = "none";
    }
  }

  const targetAt = (x: number, y: number) => targetUnder(x, y, spec);

  function up() {
    window.removeEventListener("pointermove", place);
    window.removeEventListener("pointerup", up);
    window.removeEventListener("pointercancel", up);
    ghost.remove();
    line.remove();
    /*
     * 松手那一下浏览器还会发一个 click，落在鼠标底下的那个标签上 —— 不拦的话
     * 等于「拖完顺手点了目标标签」，焦点跳到别人身上。只吃紧接着的这一次。
     */
    const eat = (ce: MouseEvent) => {
      ce.stopPropagation();
      ce.preventDefault();
    };
    window.addEventListener("click", eat, { capture: true, once: true });
    setTimeout(() => window.removeEventListener("click", eat, { capture: true }), 0);
    if (target) onDrop(target.g, target.index);
  }

  place(e);
  window.addEventListener("pointermove", place);
  window.addEventListener("pointerup", up);
  window.addEventListener("pointercancel", up);
}

/**
 * 鼠标已经松开、模块才到（第一次拖时 `import()` 的那一次往返比一下快拖慢）：
 * 不画任何东西，直接按松手的位置算落点。
 */
export function dropAt(x: number, y: number, spec: DragSpec, onDrop: (g: Group, index: number) => void): void {
  const t = targetUnder(x, y, spec);
  if (t) onDrop(t.g, t.index);
}

function targetUnder(x: number, y: number, spec: DragSpec): Target | null {
  const bar = document.elementFromPoint(x, y)?.closest<HTMLElement>(".tabs[data-group]");
  if (!bar) return null;
  const g = Number(bar.dataset.group) as Group;
  if (!spec.canDrop(g)) return null;
  // 目标组里除自己以外的标签，按屏上的顺序
  const tabs = [...bar.querySelectorAll<HTMLElement>(".tab[data-id]")].filter((el) => Number(el.dataset.id) !== spec.id);
  const barRect = bar.getBoundingClientRect();
  const top = barRect.top + 5;
  const height = barRect.height - 10;
  for (let i = 0; i < tabs.length; i++) {
    const r = tabs[i].getBoundingClientRect();
    if (x < r.left + r.width / 2) return { g, index: i, x: r.left - 2, top, height };
  }
  const last = tabs[tabs.length - 1]?.getBoundingClientRect();
  return { g, index: tabs.length, x: last ? last.right + 1 : barRect.left + 6, top, height };
}
