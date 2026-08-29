/**
 * 会话快照：重开应用时把上次的现场摆回来。
 *
 * 纯函数，零 import —— 这样 `tests/` 里能裸 node 直接跑（App.svelte 引了
 * 一大堆 IPC，测不动）。存取 localStorage 的那几行留在 App.svelte 里。
 *
 * # 存在哪、为什么可以存
 *
 * `localStorage`。WKWebView 把它落在 `~/Library/WebKit/com.liteide.app/`，
 * 正是 `scripts/uninstall.sh` 已经在删的三个目录之一 —— 所以这件事
 * 不给「卸载零残留」增加任何新义务。也正因如此没必要为它另建配置文件。
 *
 * # 存什么、不存什么
 *
 * 存：项目根、打开了哪些文件、光标停在第几行、几块面板的开合与尺寸。
 *
 * **不存文件内容**。于是也就不存「未保存的改动」—— 存了内容就等于
 * 在应用外面再放一份真相，下次打开时它和盘上的文件谁对？这个问题没有好答案，
 * 所以干脆不进这个坑：恢复出来的永远是**盘上当前的样子**。
 *
 * **不存日志模式的过滤条件**。它状态重（级别位、关键字、大小写、折叠），
 * 而且下次打开那个日志多半是为了看新的东西，把上次的过滤原样扣上来
 * 反而要先想一下「为什么只有这几行」。
 *
 * **不存终端**。pty 是活的子进程，跨进程恢复不了，假装能恢复只会更糟。
 */

/**
 * 快照格式版本。
 *
 * 字段含义变了就 +1，旧快照会被整份丢弃而不是将就着读 ——
 * 将就读出来的是一个半新半旧的界面，比干脆回到默认值难查得多。
 */
export const VERSION = 2;

/** localStorage 的键，与既有的 `lite-ide.minimap` 同前缀 */
export const KEY = "lite-ide.session";

/**
 * 最多记多少个标签。
 *
 * 不设上限的话，一次「全选打开」就能往 localStorage 里塞进几千条路径 ——
 * 而 localStorage 写满是会抛异常的，抛在启动路径上就是打不开。
 */
export const MAX_TABS = 40;

export interface TabSnap {
  path: string;
  /** 上次光标停在第几行（1-based）。日志模式记的是视图行。 */
  line?: number;
}

export interface Layout {
  sidebar: boolean;
  sidebarWidth: number;
  sideView: "files" | "git";
  panel: boolean;
  panelHeight: number;
  panelView: "term" | "log";
}

export interface Session {
  root: string | null;
  tabs: TabSnap[];
  /** 活动标签在 `tabs` 里的下标；越界或为空时由调用方回退到 0 */
  active: number;
  layout: Layout;
}

export const DEFAULT_LAYOUT: Layout = {
  sidebar: true,
  sidebarWidth: 240,
  sideView: "files",
  panel: false,
  panelHeight: 260,
  panelView: "term",
};

/**
 * 尺寸的合法区间。
 *
 * 存进去的值可能来自上一个版本、别的屏幕、或者手改过的 localStorage。
 * 一个 4000px 的侧边栏会把内容区挤没，而**界面上没有任何地方能把它拉回来** ——
 * 拖动手柄本身就在屏幕外。所以读回来一定要夹一遍。
 */
const SIDEBAR_MIN = 160;
const SIDEBAR_MAX = 640;
const PANEL_MIN = 80;
const PANEL_MAX = 900;

const clamp = (n: number, lo: number, hi: number) => Math.min(hi, Math.max(lo, Math.round(n)));

const isStr = (v: unknown): v is string => typeof v === "string" && v.length > 0;

/** 把任意值收成一个能用的布局，坏字段各自回退，不整份丢 */
export function toLayout(v: unknown): Layout {
  const o = (v ?? {}) as Record<string, unknown>;
  return {
    sidebar: typeof o.sidebar === "boolean" ? o.sidebar : DEFAULT_LAYOUT.sidebar,
    sidebarWidth:
      typeof o.sidebarWidth === "number" && Number.isFinite(o.sidebarWidth)
        ? clamp(o.sidebarWidth, SIDEBAR_MIN, SIDEBAR_MAX)
        : DEFAULT_LAYOUT.sidebarWidth,
    sideView: o.sideView === "git" ? "git" : "files",
    panel: typeof o.panel === "boolean" ? o.panel : DEFAULT_LAYOUT.panel,
    panelHeight:
      typeof o.panelHeight === "number" && Number.isFinite(o.panelHeight)
        ? clamp(o.panelHeight, PANEL_MIN, PANEL_MAX)
        : DEFAULT_LAYOUT.panelHeight,
    panelView: o.panelView === "log" ? "log" : "term",
  };
}

/**
 * 解析一份快照。**任何形式的坏数据都只返回 null，绝不抛。**
 *
 * 这段代码跑在启动路径上 —— 它抛一次，应用就打不开，而用户手里
 * 没有任何办法清掉那份坏数据（界面都出不来）。宁可当作没存过。
 */
export function parse(raw: string | null | undefined): Session | null {
  if (!isStr(raw)) return null;
  let o: Record<string, unknown>;
  try {
    const v: unknown = JSON.parse(raw);
    if (typeof v !== "object" || v === null || Array.isArray(v)) return null;
    o = v as Record<string, unknown>;
  } catch {
    return null;
  }
  // 版本不认就整份丢：将就着读会摆出一个半新半旧的界面
  if (o.v !== VERSION) return null;

  const rawTabs = Array.isArray(o.tabs) ? o.tabs : [];
  const tabs: TabSnap[] = [];
  const seen = new Set<string>();
  for (const t of rawTabs) {
    if (tabs.length >= MAX_TABS) break;
    const e = (t ?? {}) as Record<string, unknown>;
    if (!isStr(e.path) || seen.has(e.path)) continue;
    seen.add(e.path);
    const line =
      typeof e.line === "number" && Number.isFinite(e.line) && e.line >= 1
        ? Math.floor(e.line)
        : undefined;
    tabs.push(line === undefined ? { path: e.path } : { path: e.path, line });
  }

  const active =
    typeof o.active === "number" && Number.isFinite(o.active)
      ? clamp(o.active, 0, Math.max(0, tabs.length - 1))
      : 0;

  return {
    root: isStr(o.root) ? o.root : null,
    tabs,
    active,
    layout: toLayout(o.layout),
  };
}

/** 序列化。字段名压到一个字母是没必要的省，这里保持可读 —— 出问题时要能直接看懂。 */
export function serialize(s: Session): string {
  return JSON.stringify({
    v: VERSION,
    root: s.root,
    tabs: s.tabs.slice(0, MAX_TABS),
    active: clamp(s.active, 0, Math.max(0, Math.min(s.tabs.length, MAX_TABS) - 1)),
    layout: s.layout,
  });
}
