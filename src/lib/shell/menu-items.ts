/**
 * 入口里那几个菜单有哪几项：标签的右键菜单、状态栏「缩进 · 换行符」那格（2026-10-21 入口包瘦身，从 `Tabs.svelte` / `StatusBar.svelte` 挪出来）。
 *
 * 标签栏和状态栏在入口包里，而这些菜单项要点了 / 右键了才用得上 —— 和菜单壳同一个判据：窗口出现之前一次都不会有。
 * 连带着 `pathactions`（复制路径、在 Finder 中显示）也离开了入口：入口里只有标签栏引它。
 * 加载句柄是 `context-menu.svelte.ts` 的 `menuItems`，两处共用一个 —— 入口里每一处 `import()` 都留一张依赖表（约 150 B）。
 * 调用方照旧用 `$derived` 调这里：菜单开着时标签变了（外面关掉一个），菜单项跟着变 —— 和挪出来之前一样。
 */
import type { MenuItem } from "./ContextMenu.svelte";
import type { Tab } from "./Tabs.svelte";
import { copyText, relTo, showInFinder } from "./pathactions";

export interface TabMenuCtx {
  tab: Tab;
  i: number;
  tabs: Tab[];
  root: string;
  onClose: (id: number, force?: boolean) => void;
  onPin?: (id: number, on: boolean) => void;
  onCloseMany?: (ids: number[]) => void;
  onMoveToOther?: (id: number) => void;
  moveLabel: string;
  onRevealInTree?: (path: string) => void;
}

/**
 * 差异/合并标签的 path 是 `git-diff:xxx` 这类**合成 key**，不是盘上的路径。
 * 拿它去 Finder 里显示或者复制，给出来的是一串没用的东西 ——
 * 所以那几项只对真实文件出现。判据就是「以 / 开头」。
 */
const isReal = (p: string) => p.startsWith("/");

export function tabMenuItems(c: TabMenuCtx): MenuItem[] {
  const { tab, i, tabs, root, onClose, onPin, onCloseMany, onMoveToOther, moveLabel, onRevealInTree } = c;
  /** 在当前项目根底下。草稿住在根外面，跟它有关的菜单项要跟着让开 */
  const inRoot = (p: string) => !!root && p.startsWith(root.endsWith("/") ? root : `${root}/`);
  // 钉住的也能从这儿关（`force`）：钉住防的是误关，不是不许关
  const out: MenuItem[] = [{ label: "关闭", run: () => onClose(tab.id, true) }];
  out.push({ label: tab.pinned ? "取消钉住" : "钉住", run: () => onPin?.(tab.id, !tab.pinned) });
  /*
   * 不适用的项**直接不出现**，而不是灰着放在那儿。
   *
   * 灰项要么让键盘游标停在一个按了没反应的条目上，要么就得写跳过逻辑 ——
   * 而「关闭其他」在只有一个标签时本来也没什么可解释的。
   */
  if (tabs.length > 1) {
    out.push({
      label: "关闭其他",
      run: () => onCloseMany?.(tabs.filter((t) => t.id !== tab.id).map((t) => t.id)),
    });
  }
  if (i < tabs.length - 1) {
    out.push({
      label: "关闭右侧的",
      run: () => onCloseMany?.(tabs.slice(i + 1).map((t) => t.id)),
    });
  }
  out.push({ label: "关闭全部", run: () => onCloseMany?.(tabs.map((t) => t.id)) });
  // 分屏（issue #35）：只有一个标签时不出现 —— 挪走它这一组就空了、立刻收起，等于白做。
  // 判据同上面那句：不适用的项直接不出现
  if (onMoveToOther && tabs.length > 1) {
    out.push({ label: moveLabel, sep: true, run: () => onMoveToOther(tab.id) });
  }

  if (isReal(tab.path)) {
    /*
     * 「定位」和「相对路径」只对**项目根底下**的文件成立。
     *
     * 草稿就住在根外面（`~/Library/Application Support/…/scratches`）：
     * 文件树里根本没有它那一行，定位必然落空；而 `relTo` 对根外的路径
     * 是原样返回绝对路径的 —— 一个叫「复制相对路径」的菜单项给你一串
     * `/Users/…/Application Support/…`，那是菜单在说谎。
     * 判据同上面那句：**不适用的项直接不出现**。
     */
    if (inRoot(tab.path)) {
      out.push({
        label: "在文件树中定位",
        sep: true,
        run: () => onRevealInTree?.(tab.path),
      });
    }
    out.push({
      label: "在 Finder 中显示",
      sep: !inRoot(tab.path),
      run: () => void showInFinder(tab.path),
    });
    out.push({ label: "复制路径", sep: true, run: () => void copyText(tab.path, "路径") });
    if (inRoot(tab.path)) {
      out.push({
        label: "复制相对路径",
        run: () => void copyText(relTo(root, tab.path), "相对路径"),
      });
    }
  }
  return out;
}

/**
 * 状态栏「缩进 · 换行符」：三档常用缩进 + LF / CRLF。文件猜出来的是 3 空格这种不在档里的，把它也列进去
 * 打上勾 —— 不然菜单里一个勾都没有，人会以为「现在」是什么都没设。
 * CR（老 Mac）同理不给选项：没人往这个方向改，但当前是 CR 时勾就空着，格子里的字说明了。
 */
export function fmtMenuItems(c: {
  indentNow: "tab" | number | null;
  eol: string | undefined;
  indentLabel: (ind: "tab" | number | null) => string;
  onSetIndent: (ind: "tab" | number) => void;
  onSetEol: (eol: "LF" | "CRLF") => void;
}): MenuItem[] {
  const spaces = [2, 4];
  if (typeof c.indentNow === "number" && !spaces.includes(c.indentNow)) spaces.push(c.indentNow);
  spaces.sort((a, b) => a - b);
  const opts: ("tab" | number)[] = [...spaces, "tab"];
  const items: MenuItem[] = opts.map((o) => ({
    label: c.indentLabel(o),
    checked: o === c.indentNow,
    run: () => c.onSetIndent(o),
  }));
  for (const [i, e] of (["LF", "CRLF"] as const).entries()) {
    items.push({ label: e, checked: c.eol === e, sep: i === 0, run: () => c.onSetEol(e) });
  }
  return items;
}
