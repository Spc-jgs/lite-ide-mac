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
  /*
   * 草稿是「看日志时顺手记两笔」的临时纸，落在
   * `~/Library/Application Support/com.liteide.app/scratches/`，
   * 一出生就有真实路径 —— 不是 VSCode 那种无路径的 Untitled，
   * 所以关闭时不需要问「存到哪儿」，保存、会话恢复、外部改动检测
   * 全都走普通文件那条路（见 App.svelte 的 `newScratch`）。
   *
   * **⌘N 给草稿，不给「在项目里新建文件」** —— 后者是低频动作，
   * 文件树右键已经有了；而草稿要的就是「想记就记」，中间不能隔一次找菜单。
   */
  { id: "new-scratch", label: "新建草稿", accel: "⌘N", group: "文件", owner: "menu" },
  { id: "open-folder", label: "打开文件夹…", accel: "⌘O", group: "文件", owner: "menu" },
  /*
   * 草稿目录在 Finder 里默认看不见（「资源库」是隐藏的），所以翻旧草稿
   * 只能从这儿进。它把草稿目录**当项目根打开** —— 文件树、⌘P、⇧⌘F
   * 立刻全都有，零新代码。代价是 Git 面板会空（那目录不是仓库）。
   */
  { id: "open-scratch-dir", label: "打开草稿目录", group: "文件", owner: "menu" },
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
  /*
   * 跳到声明。**只做「敢跳的」那两层**（本文件的符号表、import 推出来的文件），
   * 拿不准的一律不给下划线 —— 详见 `lib/editor/jump.ts` 的说明。
   *
   * `owner: "cm6"` 而不是 "menu"：它要读编辑器此刻的语法树和光标，
   * 而菜单栏的 accelerator 是不看焦点的，挂上去等于在日志视图里按 ⌘B
   * 也会触发一个够不着编辑器的命令。同 ⌘F 那两条的判据。
   */
  { id: "jump-decl", label: "跳到声明（⌘Click 同）", accel: "⌘B", group: "导航", owner: "cm6" },
  { id: "nav-back", label: "回到上一个位置", accel: "⌥⌘←", group: "导航", owner: "menu" },
  { id: "nav-fwd", label: "再回来", accel: "⌥⌘→", group: "导航", owner: "menu" },
  /*
   * 跳转够不着时的退路。**名字上就不叫跳转** —— 它是拿光标下那个词
   * 跑一次全局搜索，给的是候选不是答案。省掉的只是「选中、复制、⇧⌘F、粘贴」。
   */
  { id: "find-word", label: "在项目里找这个名字", group: "导航", owner: "menu" },

  // ── 编辑 ──
  /*
   * ⌘F 是 CM6 的 `searchKeymap` 给的，代码里一行没写。
   *
   * **⌥⌘F 以前不是。** 这段注释原来写的是「这两条都是 searchKeymap 给的」，
   * 那是错的 —— `searchKeymap` 一共七条绑定（`Mod-f` `F3` `Mod-g` `Escape`
   * `Mod-Shift-l` `Mod-Alt-g` `Mod-d`），没有 `Mod-Alt-f`。也就是说这张速查表
   * 挂了一个按下去什么都不会发生的键，而这张表正是用户唯一能发现有这个键的地方。
   * 2026-09-10 由 `editor/search-panel.ts` 真的接上了：打开面板并展开替换行。
   *
   * 这条教训比键本身值钱：**「这个键是框架自带的」是一句需要去查的断言，
   * 不是可以顺手写下的背景说明。**
   *
   * **⌘F 一个字都不许进菜单**：菜单会先把键吃掉，等于把编辑器的查找抢没了。
   */
  /*
   * ⌘Click 加光标是 CM6 在 macOS 上的默认行为，一直都有，只是从没写进速查表。
   * 现在**必须写**：⌘Click 同时也是跳转，而两者的分界线是「这个词有没有
   * 下划线」—— 不说清楚的话，人会以为跳转把加光标吃掉了。
   * （键盘那条 ⌥⌘↑ / ⌥⌘↓ 加光标不受影响，CM6 自带。）
   */
  {
    id: "cm-multi-cursor",
    label: "多光标：加一个光标（有下划线的词上则是跳转）",
    accel: "⌘Click",
    group: "编辑",
    owner: "cm6",
  },
  { id: "cm-find", label: "在当前文件里查找", accel: "⌘F", group: "编辑", owner: "cm6" },
  { id: "cm-replace", label: "查找并替换", accel: "⌥⌘F", group: "编辑", owner: "cm6" },
  { id: "encoding", label: "文件编码…", group: "编辑", owner: "menu" },
  { id: "toggle-mode", label: "切换编辑 / 日志模式", group: "编辑", owner: "menu" },

  // ── 视图 ──
  /*
   * **⌘B 原来是这儿的别名（VSCode 手感），2026-09-09 交给了「跳到声明」。**
   *
   * 两个都想要 ⌘B 的时候，判据是「谁没有主键位」：侧边栏有 ⌘1，
   * 而 IDEA 里 ⌘B 就是跳转的主键位，这个应用的手感一直是照着 IDEA 来的。
   * 拿掉一个已经存在的别名要在这张表里留痕 —— 它是速查表的唯一出处，
   * 而速查表是用户唯一能发现「⌘B 现在归谁」的地方。
   */
  {
    id: "toggle-sidebar",
    label: "侧边栏",
    accel: "⌘1",
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
  { id: "git-console", label: "Git 控制台", group: "Git", owner: "menu" },
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
  /*
   * **它自己就是日志查看器，所以这条几乎白送。**
   *
   * 应用的运行日志落在 `~/Library/Logs/com.liteide.app/app.log`，按这条会用
   * 日志模式（mmap + 级别过滤 + tail）打开它 —— 出了事回头查的时候，
   * 不用先去访达里翻一个藏在资源库下面的目录。
   */
  { id: "help-log", label: "打开应用日志", group: "帮助", owner: "menu" },
  /*
   * 日志的保留策略是「两份 × 2MB 封顶，不按时间删」（见 `crates/applog`）——
   * 它保证的是「不会长大」，不是「会自己消失」。**所以必须给一条手动清的路**：
   * 那个文件里有路径、有错误消息，用户要能随时把它抹掉，
   * 而不是去访达里翻 `~/Library/Logs/`。
   */
  { id: "help-log-clear", label: "清空应用日志", group: "帮助", owner: "menu" },
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
  /*
   * 不是单个字母的键各有各的写法。**这张表要和 `menu_sync.rs` 里那份
   * 一字不差** —— 那条测试正是拿两边的换算结果对比的，少一行就是
   * 「菜单上写着 ⌥⌘←，按下去没反应」，而它不报错。
   */
  const SPECIAL: Record<string, string> = {
    "`": "Backquote", // muda 认这个名字，不认裸的反引号加修饰键
    "←": "Left",
    "→": "Right",
    "↑": "Up",
    "↓": "Down",
  };
  const key = SPECIAL[rest] ?? rest.toUpperCase();
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
