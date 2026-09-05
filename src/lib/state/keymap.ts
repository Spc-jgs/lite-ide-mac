/**
 * 键位表 —— 全应用**唯一**的出处。
 *
 * # 为什么要有它
 *
 * 加菜单栏之前，键位散在三个地方：`App.svelte` 的 keydown 分支、
 * 随处搜索那张 `actions` 表里的 `hint` 字符串、空态卡片里手写的一份。
 * 菜单栏会是**第四处**。
 *
 * 手抄四份的结果是可预见的：改一个键位漏掉三处。而
 * **显示错的快捷键比不显示更糟** —— v0.5.0 里那个把 `⌘1` 显示成 `1⌘`
 * 的 bug 就是先例（源码是对的，渲染错了，不报错也不崩溃）。
 *
 * # `owner` 这一列是判据，不是注释
 *
 * macOS 的菜单项一旦带上 accelerator，AppKit 会**先把键吃掉**，
 * webview 收不到。所以同一个键位不能两边都接：
 *
 * - 两边都接 → 双触发（⌘W 一下关掉两个标签）
 * - 两边都不接 → 按了没反应
 *
 * **两种都不报错。** 于是 `owner` 决定了两件事，缺一不可：
 *
 * | owner | 菜单项 | keydown 分支 |
 * |---|---|---|
 * | `menu` | 带 accelerator | **不许有** |
 * | `key`  | 只有标签，不带 accelerator | 有 |
 * | `cm6`  | **一个字都不许进菜单** | 没有（CM6 自己的 keymap） |
 */

export type Owner = "menu" | "key" | "cm6";

export interface KeyDef {
  /** 与菜单项 id、随处搜索的 action id 是同一个 */
  id: string;
  /** 菜单和速查表里显示的文案 */
  label: string;
  /**
   * 显示用的键位，macOS 写法。修饰键次序**必须**是 ⌃⌥⇧⌘
   * （Apple 的规范次序，`accelOrderIsApple` 卡着）。
   * 不是键盘能敲出来的手势（连按两下 ⇧）就留空，改用 `gesture`。
   */
  accel?: string;
  /** 表达不成 accelerator 的手势，直接写进标签里给人看 */
  gesture?: string;
  /** 别名。菜单一项只能挂一个 accelerator，别名只在速查表里出现 */
  alias?: string;
  /**
   * 速查表的分组。
   *
   * **不等于菜单里的位置** —— 菜单是按「做这件事时你在想什么」分的，
   * 速查表是按「这个键管哪一摊」分的，两者不必一致
   * （比如「文件编码…」在菜单的「文件」下，在速查表里归「编辑」）。
   */
  group: "导航" | "编辑" | "视图" | "Git" | "终端" | "文件" | "帮助";
  owner: Owner;
}

/**
 * 全部键位。
 *
 * **顺序即速查表里的显示顺序**（按 group 稳定分组），所以改动这张表
 * 就是在改界面 —— 不要按字母排序之类的理由重排。
 */
export const KEYS: KeyDef[] = [
  // ── 文件 ──
  { id: "open-folder", label: "打开文件夹…", accel: "⌘O", group: "文件", owner: "menu" },
  { id: "save", label: "保存", accel: "⌘S", group: "文件", owner: "menu" },
  { id: "close-tab", label: "关闭标签", accel: "⌘W", group: "文件", owner: "menu" },
  { id: "close-all-tabs", label: "关闭所有标签", group: "文件", owner: "menu" },

  // ── 导航 ──
  /*
   * 连按两下 ⇧ 是「两次抬起」的手势，不是任何 accelerator 能表达的。
   * 菜单项照放（那是它唯一的说明书），只是把手势写进标签文字里。
   */
  { id: "quick-all", label: "随处搜索", gesture: "连按两下 ⇧", group: "导航", owner: "key" },
  /*
   * ⌘P **故意留在 keydown**。
   *
   * 进菜单的话，焦点在终端里按 ⌘P 会被菜单抢走 —— 而终端里的 ⌘P
   * 更可能是想给 shell 的（zsh 的历史上一条）。菜单项只写标签、不挂 accelerator。
   */
  { id: "quick-file", label: "找文件…", accel: "⌘P", group: "导航", owner: "key" },
  {
    id: "quick-content",
    label: "在项目中搜索…",
    accel: "⇧⌘F",
    group: "导航",
    owner: "menu",
  },
  { id: "outline", label: "文件结构…", accel: "⇧⌘O", group: "导航", owner: "menu" },

  // ── 编辑 ──
  /*
   * 这两条代码里一行都没写 —— 是 CM6 的 `searchKeymap` 给的。
   * 正因为如此，它们在别处一个字都不会出现，而对用的人来说
   * 「谁实现的」不重要，「按了有没有用」才重要。
   *
   * **⌘F 一个字都不许进菜单**：菜单会先把键吃掉，等于把编辑器的查找抢没了。
   */
  { id: "cm-find", label: "在当前文件里查找", accel: "⌘F", group: "编辑", owner: "cm6" },
  { id: "cm-replace", label: "查找并替换", accel: "⌥⌘F", group: "编辑", owner: "cm6" },
  { id: "encoding", label: "文件编码…", group: "编辑", owner: "menu" },
  { id: "toggle-mode", label: "切换编辑 / 日志模式", group: "编辑", owner: "menu" },

  // ── 视图 ──
  /*
   * ⌘B 是 ⌘1 的别名（VSCode 手感，很多人手指记的是它）。
   * macOS 菜单一项只能挂一个 accelerator —— 主键位进菜单，
   * 别名留给 keydown，速查表是唯一能把两个都说清的地方。
   */
  {
    id: "toggle-sidebar",
    label: "侧边栏",
    accel: "⌘1",
    alias: "⌘B",
    group: "视图",
    owner: "menu",
  },
  { id: "toggle-panel", label: "终端面板", accel: "⌘J", group: "视图", owner: "menu" },
  { id: "toggle-minimap", label: "代码缩略图", group: "视图", owner: "menu" },

  // ── 终端 ──
  /*
   * ⌃⇧` 尤其该进菜单：焦点在 xterm 里时它今天能不能触发，
   * 取决于 xterm 有没有吞掉这个组合 —— 菜单 accelerator 不看焦点。
   */
  { id: "new-terminal", label: "新建终端", accel: "⌃⇧`", group: "终端", owner: "menu" },
  { id: "close-terminal", label: "关闭当前终端", group: "终端", owner: "menu" },

  /*
   * 日志模式里跳过滤命中。挂在 window 上（`LogPane.svelte`），
   * 因为翻日志时焦点可能在过滤框、也可能在列表上。
   *
   * **不进菜单**：它只在日志模式下有意义，而菜单项按模式增删会让
   * 菜单栏跳来跳去 —— 灰掉又占着位置。这类模式内的键位留给 keydown，
   * 由速查表负责说明。
   */
  { id: "log-next-hit", label: "日志：下一处命中", accel: "F3", group: "导航", owner: "key" },
  { id: "log-prev-hit", label: "日志：上一处命中", accel: "⇧F3", group: "导航", owner: "key" },

  // ── Git ──
  { id: "git-changes", label: "改动列表", accel: "⇧⌘G", group: "Git", owner: "menu" },
  /*
   * 拉取 = fetch + 本地合并两步，不是 `git pull`。
   * 键位照 IDEA：⇧⌘P 更新项目、⌥⌘P 推送。
   */
  { id: "git-pull", label: "拉取", accel: "⇧⌘P", group: "Git", owner: "menu" },
  { id: "git-push", label: "推送…", accel: "⌥⌘P", group: "Git", owner: "menu" },
  { id: "git-fetch", label: "抓取远程", group: "Git", owner: "menu" },
  { id: "git-file-diff", label: "查看当前文件的改动", group: "Git", owner: "menu" },
  { id: "git-log", label: "提交历史", group: "Git", owner: "menu" },
  { id: "git-branches", label: "分支与工作树…", group: "Git", owner: "menu" },
  { id: "git-refresh", label: "刷新状态", group: "Git", owner: "menu" },

  // ── 帮助 ──
  { id: "help-keys", label: "快捷键速查", accel: "⌘/", group: "帮助", owner: "menu" },
  /*
   * 没有键位。登记在这儿不是为了速查表（`shortcuts()` 会把它滤掉），
   * 是因为**菜单里的每一项都必须在这张表里有登记** ——
   * `menu_sync.rs` 反向也查：菜单里冒出一条表里没有的就红。
   * 那道检查正是这一条被加进来的原因。
   */
  { id: "help-repo", label: "项目主页", group: "帮助", owner: "menu" },
];

/**
 * 速查表要显示的那些 —— 有键位或有手势的。
 *
 * 没有键位的菜单项（「项目主页」「刷新状态」…）也在 `KEYS` 里，
 * 但它们不属于一张**快捷键**速查表。
 */
export function shortcuts(): KeyDef[] {
  return KEYS.filter((k) => k.accel || k.gesture);
}

/** 按 id 取。找不到返回 undefined —— 调用方自己决定要不要兜底 */
export function byId(id: string): KeyDef | undefined {
  return KEYS.find((k) => k.id === id);
}

/**
 * 显示用的键位 → Tauri（muda）的 accelerator 字符串。
 *
 * **不存两份。** 存两份就会漂移，而漂移的表现是「菜单上写着 ⌘S，
 * 按下去没反应」—— 又是一个不报错的 bug。
 *
 * 修饰键映射：⌃ Ctrl · ⌥ Alt · ⇧ Shift · ⌘ CmdOrCtrl。
 * 用 `CmdOrCtrl` 而不是 `Cmd`：这个应用只发 macOS，两者等价，
 * 但万一哪天在别的平台上编译，前者不会变成一个按不出来的键位。
 */
export function toTauriAccel(accel: string | undefined): string | undefined {
  if (!accel) return undefined;
  const mods: string[] = [];
  let rest = accel;
  // 按 Apple 的次序剥，剥完剩下的就是主键
  if (rest.startsWith("⌃")) (mods.push("Ctrl"), (rest = rest.slice(1)));
  if (rest.startsWith("⌥")) (mods.push("Alt"), (rest = rest.slice(1)));
  if (rest.startsWith("⇧")) (mods.push("Shift"), (rest = rest.slice(1)));
  if (rest.startsWith("⌘")) (mods.push("CmdOrCtrl"), (rest = rest.slice(1)));
  if (rest.length === 0) return undefined;
  // muda 认 "Backquote"，不认裸的反引号加修饰键那种写法
  const key = rest === "`" ? "Backquote" : rest.toUpperCase();
  return [...mods, key].join("+");
}

/** 修饰键的 Apple 规范次序。速查表和菜单都按它显示 */
const APPLE_ORDER = ["⌃", "⌥", "⇧", "⌘"] as const;

/**
 * 这个 accel 的修饰键次序对不对。
 *
 * 单拎出来是为了能测 —— 老的 `actions` 表里写的是 `⌘⇧G`，
 * 而 Apple 的次序是 `⇧⌘G`。这类错误肉眼扫不出来。
 */
export function accelOrderIsApple(accel: string): boolean {
  let seen = -1;
  for (const ch of accel) {
    const i = APPLE_ORDER.indexOf(ch as (typeof APPLE_ORDER)[number]);
    if (i === -1) break; // 到主键了，修饰键部分结束
    if (i <= seen) return false;
    seen = i;
  }
  return true;
}
