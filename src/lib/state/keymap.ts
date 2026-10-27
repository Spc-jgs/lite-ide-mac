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
 * 带 accelerator 的菜单项和页面里的 keydown 抢的是同一个键。**谁先拿到**，实测过的只有一种情形（2026-10-08，JOURNAL「⌘S 那一条」）：
 * 焦点在编辑器里按 ⌘S，**网页先拿到**；网页 `preventDefault` 了，WKWebView 当它处理过，不再交给菜单栏，菜单事件一个都没有。
 * （原来这里写的是「AppKit 先把键吃掉，webview 收不到」—— 和实测相反，#53。网页没处理时是不是一定轮到菜单、焦点在终端 / 文件树里是不是同样，还没真按键测过。）
 * 不管谁先，同一个键位都不能两边都接：
 *
 * - 两边都接、网页不 `preventDefault` → 双触发（⌘W 一下关掉两个标签）
 * - 两边都接、网页 `preventDefault` → 焦点在网页里时菜单那边的动作永远不跑
 * - 两边都不接 → 按了没反应
 *
 * **两种都不报错。** 于是 `owner` 决定了两件事，缺一不可：
 *
 * | owner | 菜单项 | keydown 分支 |
 * |---|---|---|
 * | `menu` | 带 accelerator | **不许有** |
 * | `key`  | 只有标签，不带 accelerator | 有 |
 * | `cm6`  | **一个字都不许进菜单** | 没有（CM6 自己的 keymap） |
 * | `xterm` | 同 `cm6` | 没有（xterm 的 `attachCustomKeyEventHandler`） |
 *
 * `cm6` 和 `xterm` 是同一类：**控件自己吃键，按焦点分派**。所以 ⌘F 在表里出现两次
 * （`cm-find` / `term-find`）是对的 —— 焦点在哪个控件里哪个接，不会双触发；
 * `tests/keymap.test.ts` 里「同一个键位不许挂两条」对这一类放行。
 *
 * # 这张表是**默认**（docs/TAKEOVER.md R3）
 *
 * 你在设置里改过的存 `keymap.json`（只存改过的），生效 = 默认 ⊕ 覆盖（`accelOf` / `byId`）。菜单由 Rust 按生效的挂，
 * 编辑器里的按 id 挂（`editor/id-keys.ts`）—— 所以 `cm6` 那几行不再只是说明书，键真的从这里取。
 */

/**
 * `log`：日志视图自己吃的键（2026-10-23 加的）。它和 `cm6` / `xterm` 一样是「控件按焦点分派」：日志视图挂着的时候编辑器不在那个分组里、
 * 分屏时编辑器先吃到（`preventDefault`）日志视图就不管 —— 所以 F3 可以同时是编辑器的书签（照 IDEA）和日志的下一处命中，不算撞。
 * 不进菜单、改不了（写死在 LogPane 里），同 `key`
 */
export type Owner = "menu" | "key" | "cm6" | "xterm" | "log";

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
  group: "导航" | "编辑" | "视图" | "Git" | "终端" | "构建" | "运行" | "文件" | "帮助";
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
  /**
   * 草稿本（docs/SCRATCH.md 第七节）：没有项目的那个窗口，全应用一处。⌘N 是「这个窗口里多一张」，这个是「去草稿本开一张白纸」。
   * ⇧⌘N：IDEA 里是「新建草稿文件」，Sublime / VS Code 里是「新建窗口」—— 两边的肌肉记忆都指向它。
   * 去哪个窗口由 Rust 定（`winctl::open_scratchbook`），菜单之外还有 Dock 右键和草稿列表顶上那一行
   */
  { id: "scratchbook", label: "在草稿本新建", accel: "⇧⌘N", group: "文件", owner: "menu" },
  /*
   * 2026-10-23 起不挂键（IDEA 的「Open…」在 macOS 默认键位里也没有键）：⌘O 让给了 IDEA 的找类（`quick-class`，docs/TAKEOVER.md I1）。
   * 没开项目的窗口里 ⌘O 照旧是打开文件夹 —— 那里没有类可找，用了一个多月的手感在最常用的场合还在。想要回原来的，设置 › 键位里改
   */
  { id: "open-folder", label: "打开文件夹…", group: "文件", owner: "menu" },
  /*
   * 回到「只有标签、没有项目」的轻窗口（issue #40 第三层）。开一个文件不再自动
   * 把父目录当项目之后，这是唯一能从「有项目」回到「没项目」的路。
   */
  { id: "close-project", label: "关闭项目", group: "文件", owner: "menu" },
  /*
   * 翻旧草稿走侧边栏的「草稿」视图（导轨上那个便签图标，issue #40）。
   * 这条只管「去 Finder 里整理」—— 草稿目录在 Finder 里默认看不见
   * （「资源库」是隐藏的）。它原来是「把草稿目录当项目根打开」，
   * 代价是切走当前项目，所以改了。
   */
  { id: "open-scratch-dir", label: "在 Finder 中显示草稿目录", group: "文件", owner: "menu" },
  /*
   * 侧边栏切到草稿。归菜单是为了浏览器里（`pnpm dev` 没有导轨之外的入口时）
   * 随处搜索的动作表里也有它；不挂键位 —— 导轨一下就到，没到要占一个组合键的程度。
   */
  { id: "toggle-scratch", label: "草稿列表", group: "视图", owner: "menu" },
  /*
   * 装 `lite` 命令（issue #40）。VS Code 的 Shell Command: Install 那种 ——
   * 脚本走 `open -a`，冷启动和已运行都对；软链进 /usr/local/bin 装不上时
   * 把那句 sudo 摆出来，不提权。
   */
  { id: "install-cli", label: "安装命令行工具…", group: "文件", owner: "menu" },
  /**
   * 例外：编辑器也按这个 id 挂了一份（Editor.svelte，带 `preventDefault`）—— 焦点在编辑器里时网页先拿到 ⌘S（见文件头），编辑器存它自己那份文本；
   * 别处（文件树、空白处）由菜单存当前标签。两条路存的是同一个标签，`preventDefault` 保证不会存两次
   */
  { id: "save", label: "保存", accel: "⌘S", group: "文件", owner: "menu" },
  /** 草稿「毕业」成真文件、普通文件另存一份。原生面板，默认目录是项目根 */
  { id: "save-as", label: "另存为…", accel: "⇧⌘S", group: "文件", owner: "menu" },
  { id: "close-tab", label: "关闭标签页", accel: "⌘W", group: "文件", owner: "menu" },
  { id: "close-all-tabs", label: "关闭所有标签页", group: "文件", owner: "menu" },
  /* IDEA 的 Reopen Closed Tab（同 Chrome / VS Code 的 ⇧⌘T）：一层层往回开你亲手关掉的标签 */
  { id: "reopen-tab", label: "重新打开已关闭的标签页", accel: "⇧⌘T", group: "文件", owner: "menu" },
  /** 本地历史（docs/IDEA.md 第 21 节，IDEA 的 Local History → Show History）。IDEA 默认没给它键位，这里也不给；编辑器右键里也有 */
  { id: "local-history", label: "本地历史…", group: "文件", owner: "menu" },
  /*
   * 「退出」原来是 predefined 的，不在这张表里。多窗口第 3 步换成了自己的一项：
   * predefined 那个走 AppKit 的 `terminate:`，前端来不及存现场（docs/MULTIWINDOW.md 3.6）。
   * 在应用菜单（最左边那个）里，归「文件」组只是为了速查表和随处搜索里有个位置。
   */
  { id: "quit", label: "退出 lite-ide", accel: "⌘Q", group: "文件", owner: "menu" },
  /*
   * 设置（issue #44）：在编辑器里打开 settings.json，不存在就先建一份带注释的模板。
   * 在应用菜单里（macOS 惯例：「关于」下面），⌘, 是 macOS 和 IDEA 共同的设置键。没有窗口时也能点（开一个窗口打开它）
   */
  { id: "open-settings", label: "设置…", accel: "⌘,", group: "文件", owner: "menu" },
  /* 自动更新（docs/TAKEOVER.md MVP-8）：应用菜单里、「设置…」下面，同 IDEA 的 Check for Updates…。手动查一次、说结果；设置里关了自动查也能用 */
  { id: "check-update", label: "检查更新…", group: "文件", owner: "menu" },

  // ── 导航 ──
  /*
   * 连按两下 ⇧ 是「两次抬起」的手势，不是任何 accelerator 能表达的。
   * 菜单项照放（那是它唯一的说明书），只是把手势写进标签文字里。
   */
  { id: "quick-all", label: "随处搜索", gesture: "连按两下 ⇧", group: "导航", owner: "key" },
  /*
   * 按文件名找：IDEA 的 Go to File 是 ⇧⌘O（2026-10-20 用户定的「照 IDEA 换」）。原来是 ⌘P（Sublime / VS Code 的习惯），
   * ⌘P 让给了 IDEA 的参数提示（`param-info`）。归菜单：⇧⌘O 在终端里没人用，挂进菜单不会抢走 shell 的键 ——
   * 原来 ⌘P 留在 keydown 就是因为终端里的 ⌘P 是 zsh 的「历史上一条」
   */
  { id: "quick-file", label: "查找文件…", accel: "⇧⌘O", group: "导航", owner: "menu" },
  /**
   * 找类：IDEA 的 Navigate › Class（⌘O，docs/TAKEOVER.md I1，2026-10-23 定的：用户把「等我拍板的」交给了我，键位跟 IDEA）。
   * 开的是 ⇧⇧ 那个面板的「类」一页（问 jdtls，驼峰缩写也认）。**没开项目时是打开文件夹**：那里没有类可找，原来 ⌘O 就是打开文件夹
   */
  { id: "quick-class", label: "查找类…", accel: "⌘O", group: "导航", owner: "menu" },
  /** IDEA 的 ⌘E：找文件那个面板空着的时候列的就是最近打开的，这条只是那个手势的名字。留 keydown：终端里的 ⌘E 是 shell 的「行尾」 */
  { id: "recent-files", label: "最近文件…", accel: "⌘E", group: "导航", owner: "key" },
  /* IDEA 的 Recent Locations：最近看过 / 跳过的地方，带那一行代码（⌘E 是最近的文件，这个是最近的位置） */
  { id: "recent-locations", label: "最近位置…", accel: "⇧⌘E", group: "导航", owner: "menu" },
  /*
   * 切标签（2026-09-23 之前一个键都没有）。归 keydown 不归菜单：⌃Tab 要「松开 ⌃ 才选定」，
   * 菜单项只有按下没有松开；⌘⇧[ ⌘⇧] 跟着它一起，省得一对键分两处接。终端里 xterm 已经放行。
   */
  { id: "tab-prev", label: "上一个标签页", accel: "⇧⌘[", group: "导航", owner: "key" },
  { id: "tab-next", label: "下一个标签页", accel: "⇧⌘]", group: "导航", owner: "key" },
  { id: "tab-recent", label: "切换到最近使用的标签页（按住 ⌃ 连续切换）", accel: "⌃Tab", group: "导航", owner: "key" },
  {
    id: "quick-content",
    label: "在项目中搜索…",
    accel: "⇧⌘F",
    group: "导航",
    owner: "menu",
  },
  /* 跨文件替换（#42）：IDEA 的 Replace in Path 就是 ⇧⌘R；编辑器里 ⌘R 是文件内替换（cm-replace），不冲突 */
  { id: "replace-content", label: "在项目中替换…", accel: "⇧⌘R", group: "导航", owner: "menu" },
  /** IDEA 的 File Structure 是 ⌘F12（⇧⌘O 让给了找文件，2026-10-20） */
  { id: "outline", label: "文件结构…", accel: "⌘F12", group: "导航", owner: "menu" },
  /*
   * 跳到声明。**只做「敢跳的」那两层**（本文件的符号表、import 推出来的文件），
   * 拿不准的一律不给下划线 —— 详见 `lib/editor/jump.ts` 的说明。
   *
   * `owner: "cm6"` 而不是 "menu"：它要读编辑器此刻的语法树和光标，
   * 而菜单栏的 accelerator 是不看焦点的，挂上去等于在日志视图里按 ⌘B
   * 也会触发一个够不着编辑器的命令。同 ⌘F 那两条的判据。
   */
  { id: "jump-decl", label: "转到声明（⌘ 单击）", accel: "⌘B", group: "导航", owner: "cm6" },
  /*
   * 查找用法（docs/IDEA.md 第 2 步，Java 文件、jdtls 起好之后）。IDEA 的 ⌥F7 开「Find」工具窗、⌥⌘F7 开挂在光标底下的列表；
   * 那个工具窗还没有，两个先都开列表 —— 按惯了 ⌥F7 的人按下去有东西，比没反应好。归 cm6：要读光标底下的词。
   * ⌥⌘F7 不写成 `alias`：别名的意思是「菜单一项只挂得下一个」（keymap.test.ts 卡着），这里不进菜单；它在 java-lsp.ts 的键位里
   */
  { id: "find-usages", label: "查找用法", accel: "⌥F7", group: "导航", owner: "cm6" },
  { id: "goto-impl", label: "转到实现（Java）", accel: "⌥⌘B", group: "导航", owner: "cm6" },
  { id: "goto-super", label: "转到父方法（Java）", accel: "⌘U", group: "导航", owner: "cm6" },
  { id: "quick-doc", label: "快速文档", accel: "F1", group: "导航", owner: "cm6" },
  // IDEA 的 Generate：macOS 键位是 ⌘N 和 ⌃↵ 两个，⌘N 在这里是「新建草稿」（菜单的），先只挂 ⌃↵
  { id: "generate", label: "生成…（Java：构造器、Getter / Setter、toString…）", accel: "⌃↵", group: "编辑", owner: "cm6" },
  // IDEA 的 Call Hierarchy / Type Hierarchy（macOS 键位）。⌃H 在 CM6 里是「往回删一个字」（emacs 那套），Java 文件里照 IDEA 盖掉
  { id: "call-hierarchy", label: "调用层次结构（Java）", accel: "⌃⌥H", group: "导航", owner: "cm6" },
  { id: "type-hierarchy", label: "类型层次结构（Java）", accel: "⌃H", group: "导航", owner: "cm6" },
  /** 改名（docs/IDEA.md 第 14 节，Java 文件、jdtls 起好之后）：IDEA 的 Refactor → Rename。归 cm6：要读光标底下的词 */
  { id: "rename", label: "重命名", accel: "⇧F6", group: "编辑", owner: "cm6" },
  /** 快速修复（docs/IDEA.md 第 16 节）：IDEA 的 Show Context Actions。归 cm6：要读光标在哪 */
  { id: "quick-fix", label: "快速修复", accel: "⌥↵", group: "编辑", owner: "cm6" },
  /** 跑光标所在的测试方法 / 类（docs/IDEA.md 第 17 节）：IDEA 的 Run context configuration。归 cm6：要读光标在哪 */
  /** `.http` 文件里是发光标所在的请求（docs/IDEA.md 第 19 节）：IDEA 里每个请求是一个临时运行配置，⌃⇧R 就是通用的「运行光标这儿的」 */
  /** 参数提示（Java），照 IDEA 是 ⌘P；敲 ( 或 , 时自己会弹。归 cm6：终端里的 ⌘P 照样交给 shell */
  { id: "param-info", label: "参数提示（Java）", accel: "⌘P", group: "导航", owner: "cm6" },
  { id: "run-test", label: "运行光标所在的测试（Java）/ 请求（.http）", accel: "⌃⇧R", group: "运行", owner: "cm6" },
  /** SQL 控制台（docs/DATABASE.md DB-2）：执行光标所在的那条，选中了就执行选中的那几条。IDEA 的 Execute 就是 ⌘↵。归 cm6：要读光标在哪 */
  { id: "db-run", label: "执行光标所在的语句（SQL 控制台）", accel: "⌘↵", group: "运行", owner: "cm6" },
  /** 调试光标所在的测试（docs/IDEA.md 第 18 节）：IDEA 的 Debug context configuration。归 cm6，理由同上 */
  { id: "debug-test", label: "调试光标所在的测试（Java）", accel: "⌃⇧D", group: "运行", owner: "cm6" },
  /*
   * IDEA 的 ⌘L。归菜单：它跳的是活动编辑器，但判「有没有编辑器」在 App 里
   * （没有就什么都不做），不需要读编辑器内部状态，和 ⌘B 那种 cm6 的不一样。
   */
  { id: "goto-line", label: "转到行…", accel: "⌘L", group: "导航", owner: "menu" },
  // ⌘[ / ⌘] 是 IDEA 的主键位，菜单一项只挂得下一个，别名走 App.svelte 的 keydown
  { id: "nav-back", label: "后退", accel: "⌥⌘←", alias: "⌘[", group: "导航", owner: "menu" },
  { id: "nav-fwd", label: "前进", accel: "⌥⌘→", alias: "⌘]", group: "导航", owner: "menu" },
  /*
   * 跳转够不着时的退路。**名字上就不叫跳转** —— 它是拿光标下那个词
   * 跑一次全局搜索，给的是候选不是答案。省掉的只是「选中、复制、⇧⌘F、粘贴」。
   */
  { id: "find-word", label: "在项目中查找此名称", group: "导航", owner: "menu" },

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
    label: "添加光标（在带下划线的标识符上为跳转）",
    accel: "⌘Click",
    group: "编辑",
    owner: "cm6",
  },
  { id: "cm-find", label: "在当前文件中查找", accel: "⌘F", group: "编辑", owner: "cm6" },
  { id: "cm-replace", label: "查找并替换", accel: "⌘R", group: "编辑", owner: "cm6" },
  /*
   * 查找面板开着时的上 / 下一处。CM6 的 `searchKeymap` 本来就有，**登记在这儿是为了让测试看得见**：
   * ⇧⌘G 原来是菜单上的「改动列表」—— 菜单先吃键，编辑器里「查找上一处」几周都是死的，
   * 而 CM6 自带的键不在这张表里，「同一个键位不许挂两条」那条测试看不见它（2026-09-23）。
   */
  { id: "cm-find-next", label: "查找下一处", accel: "⌘G", group: "编辑", owner: "cm6" },
  { id: "cm-find-prev", label: "查找上一处", accel: "⇧⌘G", group: "编辑", owner: "cm6" },
  /*
   * 编辑器键位照 IDEA（2026-09-23，理由和逐条对照在 `editor/idea-keys.ts` 头上）。
   * ⌘/ 也是同一个遭遇：菜单上挂着「快捷键速查」，编辑器里的注释一直按不出来。
   */
  { id: "cm-comment", label: "注释 / 取消注释", accel: "⌘/", group: "编辑", owner: "cm6" },
  { id: "cm-block-comment", label: "块注释", accel: "⌥⌘/", group: "编辑", owner: "cm6" },
  { id: "cm-duplicate", label: "复制行或选区", accel: "⌘D", group: "编辑", owner: "cm6" },
  { id: "cm-delete-line", label: "删除行", accel: "⌘⌫", group: "编辑", owner: "cm6" },
  { id: "cm-move-up", label: "上移行", accel: "⌥⇧↑", group: "编辑", owner: "cm6" },
  { id: "cm-move-down", label: "下移行", accel: "⌥⇧↓", group: "编辑", owner: "cm6" },
  { id: "cm-extend", label: "扩大选区", accel: "⌥↑", group: "编辑", owner: "cm6" },
  { id: "cm-shrink", label: "缩小选区", accel: "⌥↓", group: "编辑", owner: "cm6" },
  { id: "cm-next-occ", label: "选中下一个相同项", accel: "⌃G", group: "编辑", owner: "cm6" },
  { id: "cm-all-occ", label: "选中所有相同项", accel: "⌃⌘G", group: "编辑", owner: "cm6" },
  { id: "cm-case", label: "切换大小写", accel: "⇧⌘U", group: "编辑", owner: "cm6" },
  /** Java 文件（语言服务器起好之后）交给 jdtls 格式化整份 / 选中的几行（java-lsp.ts，docs/IDEA.md 第 15 节）；别的文件照旧只认 JSON */
  { id: "cm-format-json", label: "重新格式化代码（有语言服务器的文件；JSON 文件格式化 JSON）", accel: "⌥⌘L", group: "编辑", owner: "cm6" },
  /** IDEA 的 Optimize Imports（docs/TAKEOVER.md G22）：删没用的、补齐、按 IDEA 代码风格里的次序排、到阈值合成 `.*`。Java 文件里 jdtls 起好之后才有 */
  { id: "optimize-imports", label: "优化 import（Java）", accel: "⌃⌥O", group: "编辑", owner: "cm6" },
  /*
   * 重构（docs/TAKEOVER.md G12），键位照 IDEA 的 Refactor › Extract / Inline。选中一段按，抽完接着在新名字上开改名（⇧F6 那个框）。
   * ⌥⌘F 原来是查找面板里一条 VS Code 的「替换」别名（没登记、速查表不列），让给 IDEA 的抽字段
   */
  { id: "extract-variable", label: "引入变量（Java）", accel: "⌥⌘V", group: "编辑", owner: "cm6" },
  { id: "extract-constant", label: "引入常量（Java）", accel: "⌥⌘C", group: "编辑", owner: "cm6" },
  { id: "extract-field", label: "引入字段（Java）", accel: "⌥⌘F", group: "编辑", owner: "cm6" },
  { id: "extract-method", label: "提取方法（Java）", accel: "⌥⌘M", group: "编辑", owner: "cm6" },
  { id: "inline", label: "内联（Java）", accel: "⌥⌘N", group: "编辑", owner: "cm6" },
  // IDEA 的 Refactor › Move（F6）：整个类挪进另一个包，包声明和引用方的 import 跟着改；先列出要改哪些，点了才改
  { id: "move-class", label: "移动类（Java）", accel: "F6", group: "编辑", owner: "cm6" },
  // IDEA 的 Refactor › Change Signature（⌘F6）：光标在方法上，改名字 / 返回类型 / 可见性 / 参数（加、删、挪、改名、改类型），调用处跟着改；先列出要改哪些，点了才改
  { id: "change-signature", label: "更改签名（Java）", accel: "⌘F6", group: "编辑", owner: "cm6" },
  // IDEA 的 Refactor › Introduce Parameter（⌥⌘P）：选中方法体里的一个表达式，变成这个方法的参数，调用处把它传进来；做完接着在新参数名上改名
  { id: "introduce-parameter", label: "引入参数（Java）", accel: "⌥⌘P", group: "编辑", owner: "cm6" },
  { id: "encoding", label: "文件编码…", group: "编辑", owner: "menu" },
  /**
   * 跨文件替换、Java 改名、快速修复做完，胶囊上带一个「撤销」（2026-10-21 起，原来是一张卡片）；胶囊几秒就走，这里是常驻的那条路，
   * 应用重启过也撤得了（替换日志在盘上）。一直亮着：灰不灰要把替换的状态拽进入口包，没东西可撤时说一句为什么
   */
  { id: "undo-replace", label: "撤销上次的替换 / 重命名", group: "编辑", owner: "menu" },
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
  /*
   * 工具窗的编号键，照 IDEA（docs/TAKEOVER.md G7）：按一下打开、开着再按一下收起。
   * ⌘1 是上面那个侧边栏（IDEA 的 Project），⌘J 留着（VS Code 的手感，开合整个底部）；
   * IDEA 的 ⌘2 书签、⌘3 查找还没有对应的工具窗，等做了再占键（⌘7 结构 2026-10-22 做了） —— 按了没反应的键比没有更糟
   */
  { id: "tw-terminal", label: "终端", accel: "⌥F12", group: "视图", owner: "menu" },
  { id: "tw-run", label: "运行", accel: "⌘4", group: "视图", owner: "menu" },
  { id: "tw-debug", label: "调试", accel: "⌘5", group: "视图", owner: "menu" },
  { id: "tw-problems", label: "问题", accel: "⌘6", group: "视图", owner: "menu" },
  { id: "tw-structure", label: "结构", accel: "⌘7", group: "视图", owner: "menu" },
  { id: "tw-services", label: "Services", accel: "⌘8", group: "视图", owner: "menu" },
  { id: "tw-git", label: "Git", accel: "⌘9", group: "视图", owner: "menu" },
  // IDEA 的 TODO 工具窗（docs/TAKEOVER.md G18）：IDEA 在 macOS 上也没有默认键
  { id: "tw-todo", label: "TODO", group: "视图", owner: "menu" },
  // 通知记录（docs/TAKEOVER.md I11，IDEA 的 Notifications 工具窗）：IDEA 也没有默认键
  { id: "tw-notifications", label: "通知", group: "视图", owner: "menu" },
  // IDEA 的 Bookmarks 工具窗（⌘F3）
  { id: "tw-bookmarks", label: "书签", accel: "⌘F3", group: "视图", owner: "menu" },
  // IDEA 的 Coverage 工具窗（docs/TAKEOVER.md G15）：IDEA 也没有默认键
  { id: "tw-coverage", label: "覆盖率", group: "视图", owner: "menu" },
  { id: "toggle-minimap", label: "代码缩略图", group: "视图", owner: "menu" },
  /*
   * 按标签切软换行。默认由扩展名定（md / txt / log 换，代码不换，
   * `state/tab.ts` 的 `wrapsByDefault`），这条是「这一次看着不顺手」的手动开关。
   */
  { id: "toggle-wrap", label: "自动换行", group: "视图", owner: "menu" },
  /*
   * 编辑器字号。macOS 的通行写法（⌘= / ⌘- / ⌘0），Sublime、VS Code、浏览器全是它。
   * 只管编辑器：日志视图是虚拟滚动、行高写死，先不跟。
   */
  { id: "zoom-in", label: "放大字号", accel: "⌘=", group: "视图", owner: "menu" },
  { id: "zoom-out", label: "缩小字号", accel: "⌘-", group: "视图", owner: "menu" },
  { id: "zoom-reset", label: "实际大小", accel: "⌘0", group: "视图", owner: "menu" },
  /*
   * 分屏（issue #35，docs/SPLIT.md 第 5 节）。⌘\ 是 VS Code 的分屏；⌥Tab 是 IDEA 的
   * Goto Next Splitter；⌃⌘→ 是 VS Code 的 Move Editor into Next Group。CM6 占着 ⇧⌘\ 和 ⌥⌘\，
   * 裸 ⌘\ 没有。「向右分屏」和「移到另一组」在单栏时是同一件事（组 1 从这一下诞生），
   * 分屏时前者固定往右、后者往对面。
   */
  { id: "split-right", label: "向右分屏", accel: "⌘\\", group: "视图", owner: "menu" },
  { id: "move-to-other-group", label: "移动到另一组", accel: "⌃⌘→", group: "视图", owner: "menu" },
  { id: "focus-other-group", label: "切换到另一组", accel: "⌥Tab", group: "视图", owner: "menu" },
  { id: "unsplit", label: "合并分屏", group: "视图", owner: "menu" },

  // ── 终端 ──
  /*
   * ⌃⇧` 尤其该进菜单：焦点在 xterm 里时它今天能不能触发，
   * 取决于 xterm 有没有吞掉这个组合 —— 菜单 accelerator 不看焦点。
   */
  { id: "new-terminal", label: "新建终端", accel: "⌃⇧`", group: "终端", owner: "menu" },
  /** 焦点在终端里的 ⌘F（issue #34）。归 xterm，理由见文件头 owner 那张表 */
  { id: "term-find", label: "在终端中查找", accel: "⌘F", group: "终端", owner: "xterm" },
  { id: "close-terminal", label: "关闭当前终端", group: "终端", owner: "menu" },
  /* 拆分终端（#34 后半）：不占键 —— VS Code 的 ⌘\ 在这个应用里是编辑器的向右分屏，菜单的键不看焦点，挂上去两边会打架 */
  { id: "split-terminal", label: "向右拆分终端", group: "终端", owner: "menu" },
  /*
   * 把当前文件 / 选中的几行以 `@路径#L10-20` 写进终端里的 claude（#45），不回车。
   * ⌥⌘K 照 Claude Code 官方 JetBrains 插件的「插入文件引用」—— 在 IDEA 里装了插件的人手上已经是这个键；
   * IDEA 默认键位里 ⌥⌘K 没有占用，表里也没人用。归菜单：焦点在终端里时也该能按（选完代码切进终端再想起来）
   */
  { id: "send-to-terminal", label: "发送到终端", accel: "⌥⌘K", group: "终端", owner: "menu" },

  // ── 构建（docs/IDEA.md 第 22 节）：照 IDEA 的 Build 菜单。⌘F9 进菜单 —— 终端里没人用 ⌘F9，编辑器里它也没别的意思 ──
  { id: "build-project", label: "构建项目", accel: "⌘F9", group: "构建", owner: "menu" },
  /** IDEA 的 Rebuild Project 没有默认键位 */
  { id: "rebuild-project", label: "重新构建项目", group: "构建", owner: "menu" },

  // ── 运行（#48，docs/TASKS.md Q1：照 IDEA 的 macOS 键位）──
  /*
   * ⌃R 归 key，**不能进菜单**：终端里的 ⌃R 是 shell 的「搜历史」，菜单 accelerator 会被 AppKit 先吃掉，
   * 终端里就再也搜不了历史。App.svelte 的 keydown 接它，焦点在 xterm 里时放过（同 ⌘P 那条）
   */
  { id: "run-last", label: "重新运行上一个任务", accel: "⌃R", group: "运行", owner: "key" },
  { id: "run-pick", label: "运行任务…", accel: "⌃⌥R", group: "运行", owner: "menu" },
  /** 第一下软停（SIGINT 整组，5 秒后强杀），正在软停时再按一下立刻强杀 —— 同 IDEA 的停止按钮 */
  { id: "run-stop", label: "停止任务", accel: "⌘F2", group: "运行", owner: "menu" },
  { id: "tasks-file", label: "打开 tasks.json", group: "运行", owner: "menu" },
  /** 运行配置（docs/SETTINGS.md 13.6）：IDEA 的 Run › Edit Configurations…，默认也没有键位 */
  { id: "run-configs", label: "编辑配置…", group: "运行", owner: "menu" },
  /*
   * 调试（docs/IDEA.md 第 18 节），照 IDEA 的 macOS 键位。⌃D 归 key、不进菜单，理由同 ⌃R：终端里的 ⌃D 是「输入结束」。
   * 编辑器里 CM6 原来拿 ⌃D 删后一个字（macOS 的 Emacs 键位），已经从默认键位里摘掉（`idea-keys.ts` 的 `editorDefaults`）
   */
  { id: "debug-last", label: "调试上一个任务", accel: "⌃D", group: "运行", owner: "key" },
  /*
   * 单步和继续进菜单：F 键在编辑器里没别的用处。**没在调试 / 没停着时菜单项是灰的**（Rust 的调试会话设，`menu::sync_debug`）——
   * 灰掉的菜单项不吃键，终端里的 htop / mc 照样收得到 F7 F8 F9
   */
  { id: "debug-resume", label: "继续", accel: "F9", group: "运行", owner: "menu" },
  { id: "debug-over", label: "单步跳过", accel: "F8", group: "运行", owner: "menu" },
  { id: "debug-into", label: "单步进入", accel: "F7", group: "运行", owner: "menu" },
  { id: "debug-out", label: "单步跳出", accel: "⇧F8", group: "运行", owner: "menu" },
  { id: "debug-run-to", label: "运行到光标", accel: "⌥F9", group: "运行", owner: "menu" },
  { id: "debug-pause", label: "暂停程序", group: "运行", owner: "menu" },
  /** 热替换（docs/IDEA.md 第 26 节）：IDEA 的 Reload Changed Classes，默认也没有键位。构建（⌘F9）成功后自动做一次，这是手动那条路 */
  { id: "debug-reload", label: "重新加载已更改的类", group: "运行", owner: "menu" },
  /** 在光标那行下 / 取消断点（Java 文件）。归 cm6：要读光标在哪一行；点行号是同一件事（java-debug-gutter.ts） */
  { id: "toggle-breakpoint", label: "切换断点（Java）", accel: "⌘F8", group: "运行", owner: "cm6" },
  /**
   * 书签（docs/TAKEOVER.md G18，2026-10-23 定的键：照 IDEA 的 macOS 默认）：F3 在光标那行打 / 取消，⌘F3 看全部。
   * F3 原来在编辑器里还是「查找下一处」的一个别名（CM6 的默认表，⌘G 才是主键）—— 让给了书签；日志视图里的 F3 不受影响（`log` 那类）
   */
  { id: "bookmark-toggle", label: "切换书签", accel: "F3", group: "编辑", owner: "cm6" },
  // IDEA 的 View Breakpoints：全部断点、异常断点，改属性、停用、删除（docs/TAKEOVER.md G14）
  { id: "debug-breakpoints", label: "查看断点…", accel: "⇧⌘F8", group: "运行", owner: "menu" },

  /*
   * 日志模式里跳过滤命中。挂在 window 上（`LogPane.svelte`），
   * 因为翻日志时焦点可能在过滤框、也可能在列表上。
   *
   * **不进菜单**：它只在日志模式下有意义，而菜单项按模式增删会让
   * 菜单栏跳来跳去 —— 灰掉又占着位置。这类模式内的键位留给 keydown，
   * 由速查表负责说明。
   */
  { id: "log-next-hit", label: "日志：下一处命中", accel: "F3", group: "导航", owner: "log" },
  { id: "log-prev-hit", label: "日志：上一处命中", accel: "⇧F3", group: "导航", owner: "log" },

  // ── Git ──
  // ⌘K：IDEA 里就是打开提交窗口。原来是 ⇧⌘G，把编辑器的「查找上一处」抢没了（见 cm-find-prev）
  { id: "git-changes", label: "更改列表", accel: "⌘K", group: "Git", owner: "menu" },
  /*
   * 拉取 = fetch + 本地合并两步，不是 `git pull`。
   * 键位照 IDEA 的 macOS 默认表：⌘T 更新项目、⇧⌘K 推送（2026-10-26 改；原来写的是 ⇧⌘P / ⌥⌘P、注释说「照 IDEA」—— 不是，
   * IDEA 里 ⌥⌘P 是「引入参数」，让给了它，docs/TAKEOVER.md D11）
   */
  { id: "git-pull", label: "拉取", accel: "⌘T", group: "Git", owner: "menu" },
  { id: "git-push", label: "推送…", accel: "⇧⌘K", group: "Git", owner: "menu" },
  { id: "git-fetch", label: "获取", group: "Git", owner: "menu" },
  { id: "git-file-diff", label: "查看当前文件的更改", group: "Git", owner: "menu" },
  { id: "git-blame", label: "显示 / 隐藏注解（blame）", group: "Git", owner: "menu" },
  { id: "git-log", label: "提交历史", group: "Git", owner: "menu" },
  // IDEA 的 Show History / Show History for Selection（docs/TAKEOVER.md G16）：提交历史那个工具窗只列这个文件 / 这几行的，跟着改名往前找
  { id: "git-file-history", label: "当前文件的历史", group: "Git", owner: "menu" },
  { id: "git-selection-history", label: "所选行的历史", group: "Git", owner: "menu" },
  { id: "git-console", label: "Git 控制台", group: "Git", owner: "menu" },
  { id: "git-branches", label: "分支与工作树…", group: "Git", owner: "menu" },
  { id: "git-refresh", label: "刷新状态", group: "Git", owner: "menu" },

  // ── 帮助 ──
  // 原来挂着 ⌘/，把编辑器的注释抢没了（见 cm-comment）。IDEA 的速查也没有默认键；⇧⇧ 搜「快捷键」能到
  { id: "help-keys", label: "快捷键速查", group: "帮助", owner: "menu" },
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

// ── 你改过的键位（docs/TAKEOVER.md R3）：这张表是默认，改过的存 `keymap.json`、只存改过的 ──

/** 你改过的：`{ 动作 id: 键位 }`，`""` = 不要键位。`state/keys-live.ts` 从 Rust 取来放进这里、改了再换 */
let over: Readonly<Record<string, string>> = {};
const subs = new Set<() => void>();

/** 换上新的覆盖表，告诉订了的（编辑器里那几张键位表要重配，`editor/id-keys.ts`） */
export function setOverrides(o: Record<string, string>): void {
  over = { ...o };
  for (const f of subs) f();
}

export function overrides(): Readonly<Record<string, string>> {
  return over;
}

/** 覆盖表一换就叫。返回退订 */
export function onOverrides(f: () => void): () => void {
  subs.add(f);
  return () => subs.delete(f);
}

/** 这一项生效的键位：改过用改的（`""` = 不要了 → undefined），没改用默认 */
export function accelOf(def: KeyDef, o: Readonly<Record<string, string>> = over): string | undefined {
  const a = o[def.id];
  return a === undefined ? def.accel : a || undefined;
}

/** 生效的那一项（`accel` 换成改过的）。菜单、速查表、随处搜索、右键菜单印出来的都从这里拿 */
function live(def: KeyDef): KeyDef {
  return def.id in over ? { ...def, accel: accelOf(def) } : def;
}

/**
 * 能不能改：菜单的、编辑器的能。不能的三种，理由各一条 ——
 * 归 keydown 的（⌃R、⌘E、⌃Tab…）各有各的特殊处理（终端里让给 shell、按住 ⌃ 连按），写死在 `App.svelte` 里；
 * 终端的 ⌘F 是 xterm 自己的；连按两下 ⇧ 是手势，表达不成一个键
 */
export function editable(def: KeyDef): boolean {
  // 「⌘Click」这种鼠标手势写在 accel 里，不是一个键
  return (def.owner === "menu" || def.owner === "cm6") && !def.gesture && (!def.accel || splitAccel(def.accel) !== null);
}

/** 系统和「编辑」菜单（predefined，走 AppKit 的选择器）占着的键：改到这上面，要么被系统吃掉，要么把复制粘贴弄没了 */
export const RESERVED: Readonly<Record<string, string>> = {
  "⌘Z": "撤销", "⇧⌘Z": "重做", "⌘X": "剪切", "⌘C": "复制", "⌘V": "粘贴", "⌘A": "全选",
  "⇧⌘W": "关闭窗口", "⌘M": "最小化", "⌘H": "隐藏 lite-ide", "⌥⌘H": "隐藏其他", "⌃⌘F": "进入全屏",
  "⌘Space": "输入法 / 聚焦搜索", "⌃Space": "切换输入法",
};

/**
 * 按覆盖表算出来，同一个键挂了两件事的：`{ 键位: [动作 id…] }`。规矩同 `tests/keymap.test.ts` 那条 ——
 * 编辑器和终端各吃各的（按焦点分派），两条都是 cm6 / xterm 且不同 owner 时不算撞；cm6 和 cm6 撞了算（同一个编辑器里）。
 * 别名（⌘[）也占着键
 */
export function conflicts(o: Readonly<Record<string, string>> = over): Record<string, string[]> {
  const by = new Map<string, KeyDef[]>();
  const put = (a: string, k: KeyDef) => by.set(a, [...(by.get(a) ?? []), k]);
  for (const k of KEYS) {
    const a = accelOf(k, o);
    if (a) put(a, k);
    if (k.alias) put(k.alias, k);
  }
  const out: Record<string, string[]> = {};
  for (const [a, ks] of by) {
    if (ks.length < 2) continue;
    const widget = ks.every((k) => k.owner === "cm6" || k.owner === "xterm" || k.owner === "log");
    const sameWidget = widget && new Set(ks.map((k) => k.owner)).size < ks.length;
    if (!widget || sameWidget) out[a] = ks.map((k) => k.id);
  }
  return out;
}

/**
 * 主键的写法：显示用的 ↔ Tauri（muda）的 ↔ CM6 的 ↔ `KeyboardEvent.code`。**和 `src-tauri/src/keymap.rs` 的 `key_name` 是同一张表**
 * （那边读的是 muda 0.19 认的名字）。字母数字、F1–F20、标点不在表里：字母数字两边都是大写原样，标点原样
 */
const KEY_NAMES: { show: string; tauri: string; cm: string; code: string }[] = [
  { show: "←", tauri: "Left", cm: "ArrowLeft", code: "ArrowLeft" },
  { show: "→", tauri: "Right", cm: "ArrowRight", code: "ArrowRight" },
  { show: "↑", tauri: "Up", cm: "ArrowUp", code: "ArrowUp" },
  { show: "↓", tauri: "Down", cm: "ArrowDown", code: "ArrowDown" },
  { show: "↵", tauri: "Enter", cm: "Enter", code: "Enter" },
  { show: "⌫", tauri: "Backspace", cm: "Backspace", code: "Backspace" },
  { show: "⌦", tauri: "Delete", cm: "Delete", code: "Delete" },
  { show: "Tab", tauri: "Tab", cm: "Tab", code: "Tab" },
  { show: "Space", tauri: "Space", cm: "Space", code: "Space" },
  { show: "Esc", tauri: "Escape", cm: "Escape", code: "Escape" },
  { show: "`", tauri: "Backquote", cm: "`", code: "Backquote" },
];
const PUNCT: Record<string, string> = { Minus: "-", Equal: "=", BracketLeft: "[", BracketRight: "]", Backslash: "\\", Semicolon: ";", Quote: "'", Comma: ",", Period: ".", Slash: "/" };

/** `⌥⌘L` → `{ mods: ["⌥", "⌘"], key: "L" }`。修饰键次序不对、主键不认识、不是功能键又没带 ⌃ ⌥ ⌘（裸字母、⇧ + 字母进菜单就打不了字）→ null */
export function splitAccel(accel: string): { mods: string[]; key: string } | null {
  const mods: string[] = [];
  let rest = accel;
  for (const m of APPLE_ORDER) {
    if (rest.startsWith(m)) {
      mods.push(m);
      rest = rest.slice(1);
    }
  }
  const fkey = /^F([1-9]|1[0-9]|20)$/.test(rest);
  const known = fkey || /^[A-Z0-9]$/.test(rest) || Object.values(PUNCT).includes(rest) || KEY_NAMES.some((n) => n.show === rest);
  // 不是功能键就要带 ⌃ ⌥ ⌘ 之一：只带 ⇧ 的字母就是打一个大写字母
  if (!known || (!fkey && !mods.some((m) => m !== "⇧"))) return null;
  return { mods, key: rest };
}

/** CM6 keymap 的写法：`⌥⌘L` → `Alt-Mod-l`、`⌃⇧R` → `Ctrl-Shift-r`、`⌘⌫` → `Mod-Backspace`。认不了 → null */
export function toCmKey(accel: string | undefined): string | null {
  const s = accel ? splitAccel(accel) : null;
  if (!s) return null;
  const MOD: Record<string, string> = { "⌃": "Ctrl", "⌥": "Alt", "⇧": "Shift", "⌘": "Mod" };
  const named = KEY_NAMES.find((n) => n.show === s.key)?.cm;
  // 字母写小写：带 Shift 时 CM6 拿「Shift + 原键」去配（现有的 `Ctrl-Shift-r`、`Shift-Mod-u` 都是这么写的）
  const key = named ?? (s.key.length === 1 ? s.key.toLowerCase() : s.key);
  return [...s.mods.map((m) => MOD[m]), key].join("-");
}

/**
 * 录键：按下的这一下 → 显示写法（`⌥⌘L`）。主键按 `e.code` 认（物理键位）—— ⌥ 在 macOS 上会把 `e.key` 变成别的字（⌥L = ¬），
 * 按 key 认就录成了 `⌥⌘¬`。只按了修饰键 → null（还在等主键）
 */
export function accelFromEvent(e: Pick<KeyboardEvent, "code" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string | null {
  const c = e.code;
  let key: string | undefined;
  if (/^Key[A-Z]$/.test(c)) key = c.slice(3);
  else if (/^Digit[0-9]$/.test(c)) key = c.slice(5);
  else if (/^F([1-9]|1[0-9]|20)$/.test(c)) key = c;
  else key = PUNCT[c] ?? KEY_NAMES.find((n) => n.code === c)?.show;
  if (!key) return null;
  const mods = [e.ctrlKey && "⌃", e.altKey && "⌥", e.shiftKey && "⇧", e.metaKey && "⌘"].filter(Boolean).join("");
  return mods + key;
}

/**
 * 速查表要显示的那些 —— 有键位或有手势的。
 *
 * 没有键位的菜单项（「项目主页」「刷新状态」…）也在 `KEYS` 里，
 * 但它们不属于一张**快捷键**速查表。
 */
export function shortcuts(): KeyDef[] {
  return KEYS.map(live).filter((k) => k.accel || k.gesture);
}

/** 按 id 取（键位是生效的那个）。找不到返回 undefined —— 调用方自己决定要不要兜底 */
export function byId(id: string): KeyDef | undefined {
  const k = KEYS.find((k) => k.id === id);
  return k && live(k);
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
  // `` ` `` 写成 Backquote：muda 认这个名字，不认裸的反引号加修饰键；↵ ⌫ 这些是你改键位时才会出现的（R3）
  const key = KEY_NAMES.find((n) => n.show === rest)?.tauri ?? rest.toUpperCase();
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
