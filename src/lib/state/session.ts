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
 * 存：项目根、打开了哪些文件、光标停在第几行、几块面板的开合与尺寸，
 * 以及**未保存的草稿**。
 *
 * # 草稿：这条原来是「不存」，2026-09-03 改了
 *
 * 原话是「存了内容就等于在应用外面再放一份真相，下次打开时它和盘上的文件
 * 谁对？这个问题没有好答案」。那个问题现在有答案了，而且答案一直就在应用里：
 *
 * **草稿不是「另一份真相」，它就是一次没保存的编辑** —— 和应用开着的时候
 * 一模一样。运行中同一个文件本来就同时有两份：`content`（盘上那份，dirty 的
 * 基线）和 `draft`（编辑器里的）。跨一次重启没有让这件事变得更难，
 * 只是让判据要连 `stamp` 一起存：
 *
 * - stamp 和盘上现在的一致 → 文件没动过，草稿原样恢复，dirty 照旧算出来
 * - stamp 对不上 → 我们不在的时候有人改了盘上那份。这就是**应用运行中
 *   早就有的冲突**（`conflict`），交给用户选「用磁盘上的 / 保留我的」
 * - 草稿恰好和盘上现在一样 → 那就不是草稿了，直接丢掉
 *
 * 三种情况都不需要我们替谁做主。而不存的代价是实打实的：**没手动保存过
 * 就退出，改动直接没**，连一句提示都没有 —— 而会话恢复对外宣称的是
 * 「回到上次的现场」。
 *
 * 草稿有上限（见 `MAX_DRAFT_CHARS`）：localStorage 写满是会抛的，
 * 而这段代码在启动路径上。超限的草稿宁可不存，也不能把整份快照拖下水。
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
export const VERSION = 3;

/** localStorage 的键，与既有的 `lite-ide.minimap` 同前缀 */
export const KEY = "lite-ide.session";

/**
 * 最多记多少个标签。
 *
 * 不设上限的话，一次「全选打开」就能往 localStorage 里塞进几千条路径 ——
 * 而 localStorage 写满是会抛异常的，抛在启动路径上就是打不开。
 */
export const MAX_TABS = 40;

/**
 * 单份草稿的字符上限，以及所有草稿加起来的上限。
 *
 * localStorage 的配额通常是 5MB 左右，**写满会抛**，而写快照这件事
 * 就在启动路径旁边。这两道闸的意思是：草稿是锦上添花，
 * 撑爆配额把「上次开了哪些文件」一起赔进去就是本末倒置。
 *
 * 按字符数而不是字节数卡：localStorage 本来就按 UTF-16 存，
 * 而且 `s.length` 不用先编码一遍。50 万字符的源文件已经很不寻常
 * （真到那个量级的多半会被判成日志模式）。
 */
export const MAX_DRAFT_CHARS = 500_000;
export const MAX_DRAFTS_CHARS = 2_000_000;

/** 文件指纹。判断「我们不在的时候盘上那份有没有被人改过」 */
export interface Stamp {
  mtimeMs: number;
  size: number;
}

export interface TabSnap {
  path: string;
  /** 上次光标停在第几行（1-based）。日志模式记的是视图行。 */
  line?: number;
  /** 未保存的草稿。只有编辑模式、且确实和盘上那份不同才有 */
  draft?: string;
  /** 草稿是基于哪一份盘上内容改出来的。恢复时拿它和现在的比 */
  stamp?: Stamp;
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
  /**
   * 最近打开过的项目根，最新的排最前。
   *
   * 记的是**项目根不是文件**：这个应用的会话恢复本来就以 root 为单位，
   * 开回一个项目上次的标签会跟着回来，比记住散落的文件有用得多。
   */
  recent: string[];
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
/**
 * 最近打开的上限。
 *
 * 8 是照 macOS 自己的「最近使用的项目」来的。再多子菜单会长过一屏，
 * 而「最近」的价值恰恰在于不用找。**Rust 侧 `menu.rs` 的 RECENT_MAX
 * 是同一个数**，那边只是防御性地再截一次。
 */
export const RECENT_MAX = 8;

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
    const snap: TabSnap = { path: e.path };
    if (line !== undefined) snap.line = line;
    /*
     * 草稿：类型不对、超长、或者是空串就当没有。
     *
     * 空串**不是**「没有草稿」的同义词 —— 用户可能真的把文件清空了 ——
     * 但那种改动值不值得跨重启记住，和「一份坏数据把启动搞挂」比起来
     * 不值一提。这里按不存处理，恢复出来就是盘上那份。
     */
    if (isStr(e.draft) && e.draft.length <= MAX_DRAFT_CHARS) {
      snap.draft = e.draft;
      const st = (e.stamp ?? {}) as Record<string, unknown>;
      if (
        typeof st.mtimeMs === "number" && Number.isFinite(st.mtimeMs) &&
        typeof st.size === "number" && Number.isFinite(st.size)
      ) {
        snap.stamp = { mtimeMs: st.mtimeMs, size: st.size };
      }
      // 没有 stamp 的草稿照样收下，只是恢复时一律按「盘上可能变过」处理
    }
    tabs.push(snap);
  }

  const active =
    typeof o.active === "number" && Number.isFinite(o.active)
      ? clamp(o.active, 0, Math.max(0, tabs.length - 1))
      : 0;

  /*
   * 最近列表按和别的字段同一条来：**任何形式的坏数据都只当它不存在**。
   * 不是数组就当空的，元素不是字符串就跳过，重复的去掉，最后截到上限。
   * 这段在启动路径上，抛一次应用就打不开（见文件头那段）。
   */
  const recent: string[] = [];
  if (Array.isArray(o.recent)) {
    for (const r of o.recent) {
      if (!isStr(r) || r === "" || recent.includes(r)) continue;
      recent.push(r);
      if (recent.length >= RECENT_MAX) break;
    }
  }

  return {
    root: isStr(o.root) ? o.root : null,
    tabs,
    active,
    layout: toLayout(o.layout),
    recent,
  };
}

/**
 * 序列化。字段名压到一个字母是没必要的省，这里保持可读 —— 出问题时要能直接看懂。
 *
 * `withDrafts = false` 是**退一步的那一档**：localStorage 写满会抛，
 * 而多半就是草稿撑的。那时候宁可把草稿全丢掉重存一次，
 * 也不能连「上次开了哪些文件、光标在哪」一起赔进去。
 */
export function serialize(s: Session, withDrafts = true): string {
  let budget = MAX_DRAFTS_CHARS;
  const tabs = s.tabs.slice(0, MAX_TABS).map((t) => {
    const { draft, stamp, ...rest } = t;
    if (!withDrafts || draft === undefined) return rest;
    // 单份超限、或者总额度用完了，就只丢这一份草稿，标签本身照存
    if (draft.length > MAX_DRAFT_CHARS || draft.length > budget) return rest;
    budget -= draft.length;
    return stamp === undefined ? { ...rest, draft } : { ...rest, draft, stamp };
  });
  return JSON.stringify({
    v: VERSION,
    root: s.root,
    tabs,
    active: clamp(s.active, 0, Math.max(0, Math.min(s.tabs.length, MAX_TABS) - 1)),
    layout: s.layout,
    /*
     * 写出去也截一次：内存里那份被谁 push 多了，不该顺着写进磁盘。
     *
     * `?? []` 不是多余的防御：这个函数和 parse 一样贴着启动/保存路径，
     * 调用方少给一个字段就该退成空列表，不能抛 —— 抛一次这一轮的现场就没了。
     */
    recent: (s.recent ?? []).slice(0, RECENT_MAX),
  });
}
