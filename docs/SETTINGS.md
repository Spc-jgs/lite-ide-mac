# 配置文件（issue #44）—— 设计

> 状态：**做完了（2026-10-09）**。第 9 节四个问题 2026-10-08 按建议拍板；第 0 步实测推翻了两处设计（7.2 的理由、7.3 的首屏缓存），
> 7.4 的高亮按实测换了做法，详见第 8 节。各步的验证在第 10 节，过程在 JOURNAL 2026-10-08 / 09。
> 用户看的说明在 [USAGE.md](USAGE.md)「十二、设置」。
> 出发点见 [DIRECTION.md](DIRECTION.md) 第 6 节「配置文件」和 issue #44。
> 「跨窗口共享的东西只有 Rust 一个主人」这条原则来自 [MULTIWINDOW.md](MULTIWINDOW.md) 3.5。

## 0. 一句话

**`settings.json` 是你写的，应用只读不改；按钮和快捷键改的「界面状态」另存一份，由应用写。两份都归 Rust 管，变了广播给每个窗口。**

> 2026-10-22 改了：设置页（第 13 节）也写 `settings.json`，只改那一个值的字符、注释和次序都留着；按钮和快捷键照旧不碰它。下面第 3、9 节是当时的设计，留着看为什么。

issue 里有一条前提被实测推翻了（第 2 节）：工具路径（`JAVA_HOME` / Python / Node）**不该放进这一轮**，该跟着 #48 任务运行一起做。

## 1. 现状

| 东西 | 现在在哪 | 问题 |
|---|---|---|
| 缩略图开关、文件树「紧凑」「跟随」、Git 改动「分组」 | localStorage（`prefs.ts`），各窗口各读一次 | 多窗口之后**不同步**：A 里关了缩略图，B 要重开才变。不丢数据，只是不一致 |
| 编辑器字号（⌘= / ⌘- / ⌘0） | localStorage `editorFont`，App.svelte 自己管 | 同上；而且裸二进制和 `.app` 的 localStorage 不是同一份，两种启动方式字号不一样 |
| 代码字体 | 写死：`app.css` 的 `--code-font` + `Terminal.svelte` 的 `TERM_FONT` | 不能换（有人不要 JetBrains Mono） |
| 终端字号 | 写死 13 | 不能换 |
| 终端用哪个 shell | `$SHELL -l`，取不到退 `/bin/zsh`（`ptysvc::Session::spawn`） | 不能换（想用 fish / bash 的人） |

「最近切过的分支」也在 localStorage，但它是**历史记录**不是偏好，不归这次管。

## 2. issue 里的前提，实测后要改的

issue 写的是「从 Finder 启动的 .app 拿不到 shell 的环境变量，工具路径只能靠配置给」，所以第一批要放 `JAVA_HOME` / Python / Node。在你机器上量了一遍（2026-10-08）：

| 工具 | `.app` 拿到的 PATH（系统默认 `/usr/bin:/bin:/usr/sbin:/sbin`） | 登录 shell（`zsh -l`）里 |
|---|---|---|
| java / python3 | 找到的是 **macOS 自带的占位程序**（`/usr/bin/java`），版本不对 | temurin-17 / pyenv 的 shim |
| mvn / node / pnpm / claude | **找不到** | 都在 |

三个事实：

1. **终端本来就对。** 它起的是登录 shell，会读你的 `.zprofile` / `.zshrc`，所以 PATH、`JAVA_HOME` 和你平时一模一样。问题只出在**终端之外**起的子进程 —— 而现在还没有这种子进程，要等 #48 任务运行、#45 送进 `claude` 才有。
2. **在配置里写 `JAVA_HOME` 对你无效。** 你的 `.zshrc` 自己设了 `JAVA_HOME`（出现 2 次）。我们在起 shell 之前塞进去的值，会被 `.zshrc` 盖掉。issue 的验收第 4 条「配了 `JAVA_HOME` → 终端里 `echo` 出来是它」在你机器上不成立。
3. **不能在启动时把登录 shell 的 PATH 抄一份存着用。** node 走 fnm，它给每个 shell 会话临时建一个目录（`~/.local/state/fnm_multishells/55845_…/bin`），过一阵会被清掉。抄下来的 PATH 隔几个小时就失效，node 会莫名其妙找不到。

所以对的做法是：**终端之外的子进程也经过你的登录 shell 去跑**（`$SHELL -l -c '命令'`，量过一次 0.21 秒），工具在哪由你的 shell 配置说了算，不在我们这里再存一份会过期的副本。这件事的第一个用户是 #48，**放到 #48 的设计里做**。配置文件里要不要再留一个「覆盖用」的 `env`，等 #48 真有人用的时候再定 —— 现在加，就是一个没人读的设置项。

**这一轮的第一批因此改成：字体、字号、终端 shell**（第 5 节）。

## 3. 两份文件，两个主人

### 3.1 为什么不是一份

issue 原来的想法是把 5 个偏好都迁进 `settings.json`。问题在于：缩略图、紧凑、跟随、分组这几个是**按钮**改的，⌘= 是**快捷键**改的。按一下就得写回 `settings.json`，而那份文件是你手写的：

| 写回的办法 | 代价 |
|---|---|
| 读进来、改一个值、整份重新序列化写回 | **你写的注释、空行、键的顺序全没了**。手写配置文件最讨厌的事 |
| 只改那一个值所在的那几个字符（VS Code 的做法） | 要一个能定位到「这个键的值在第几个字符」的 JSONC 解析器。自己写要两三百行加一堆边界情况；用库要加依赖 |
| 还有一个回环 | 应用写文件 → 自己的文件监听收到 → 重新加载 → 再广播一遍。要专门认出「这是我自己写的」 |

而且按钮一天能按几十次，你手写的文件被应用改来改去，`git diff` 自己的 dotfiles 时会很烦。

### 3.2 怎么分

| | `settings.json` | `ui-state.json` |
|---|---|---|
| 谁写 | **你**（应用只在它不存在时建一份模板，之后一个字节都不碰） | **应用**（你不用管它，也可以删） |
| 放什么 | 字体、基础字号、终端 shell —— 想好了就很少再改的 | 缩略图、紧凑、跟随、分组、⌘= 调出来的字号偏移 |
| 格式 | JSONC（能写注释、末尾逗号），见第 4 节 | 纯 JSON，应用自己读写 |
| 改了之后 | 文件监听发现 → 重读 → 广播 | 命令改 → 存盘 → 广播 |

两份都在 `~/Library/Application Support/com.liteide.app/`，**两种启动方式（裸二进制、`.app`）读的是同一份** —— 顺带修掉第 1 节「字号不一样」那条。

### 3.3 字号：基础值 + 偏移

- `settings.json` 里的 `editor.fontSize` 是**基础字号**（默认 13）。
- ⌘= / ⌘- 改的是 `ui-state.json` 里的**偏移**（+1、-2 …），实际字号 = 基础 + 偏移，夹在 9–28。
- ⌘0 把偏移清零 —— 回到「你在配置里写的那个字号」，而不是回到写死的 13。

这样「我平时用 14」写在配置里，「这会儿投屏放大一下」走快捷键，两件事不打架。

## 4. `settings.json` 的格式

### 4.1 JSONC，扁平的点号键

```jsonc
{
  // 代码字体。写具体的字体名，系统里没有就退回 JetBrains Mono
  "editor.fontFamily": "Fira Code",
  "editor.fontSize": 14,
  "terminal.shell": "/opt/homebrew/bin/fish",
}
```

- **能写注释、能有末尾逗号**：手写的配置文件没有注释就记不住为什么这么设。
- **扁平的点号键**（VS Code 的写法），不嵌套（Zed 的写法是 `"editor": { "fontSize": 14 }`）。扁平的好处：补全只有一层、报错能直接说「`editor.fontsize` 拼错了」、文档一张表就列完。
- **只写你改过的**。没写的就是默认值。模板里把所有键都列出来，但都注释掉，旁边写着默认值和一句说明：

```jsonc
{
  // 代码字体（编辑器和终端都用）。默认 "JetBrains Mono"
  // "editor.fontFamily": "JetBrains Mono",
  ...
}
```

### 4.2 解析：自己剥注释，不加依赖

`serde_json` 不认注释。做法是先过一遍：把注释和末尾逗号**换成等长的空格**，再交给 `serde_json`。换成空格而不是删掉，是为了让 `serde_json` 报的「第几行第几列」和你看到的文件对得上。

要处理的边界：字符串里的 `//`（`"http://..."` 不是注释）、字符串里的转义引号 `\"`、`/* */` 跨行。几十行代码，配一组测试。考虑过 `json5` / `jsonc-parser` 这两个库：前者支持的语法比我们要的多得多（单引号、十六进制数），后者是为「定位并改写某个值」设计的，我们不写回，用不上。

## 5. 第一批键

| 键 | 类型 | 默认 | 用在哪 | 改了何时生效 |
|---|---|---|---|---|
| `editor.fontFamily` | 字符串 | `"JetBrains Mono"` | 编辑器、差异、日志视图（`--code-font`） | 立刻 |
| `editor.fontSize` | 整数 9–28 | 13 | 同上，加上 ⌘= 的偏移 | 立刻 |
| `editor.tabLimit` | 整数 0–100 | 10（IDEA 的默认值） | 一组里最多几个标签页，超了关最久没看的；改过没保存的、固定的不关（docs/TAKEOVER.md I10）。0 = 不限 | 下一次开标签时（不回头收已经开着的） |
| `terminal.fontFamily` | 字符串 | 跟 `editor.fontFamily` | 终端（xterm 要具体字体名，见 `TERM_FONT` 那段注释） | 立刻（开着的终端也换） |
| `terminal.fontSize` | 整数 9–28 | 13 | 终端 | 立刻 |
| `terminal.shell` | 路径 | 空 = 用 `$SHELL` | 新开的终端 | **只对新开的终端**；开着的不动（换 shell 等于杀掉重开，会丢你正在跑的东西） |
| `java.jdtlsHome` | 路径 | 空 = 内置的那份（`.app` 里的） | Java 智能用的 jdtls（docs/IDEA.md 第 2 步） | 下次起 jdtls 时（这个项目的 jdtls 没在跑、又打开了 Java 文件；闲了 10 分钟会自己收掉） |
| `update.check` | 布尔 | true | 自动检查更新（docs/TAKEOVER.md MVP-8；设置对话框「更新」一类）：启动 30 秒后、之后每 12 小时去 GitHub Release 读 latest.json，有新的就后台下好、验签，状态栏出「重启更新」 | 下一次到点时（关掉之后菜单「检查更新…」照样能手动查） |

`ui-state.json` 里：`editor.minimap`、`tree.compact`、`tree.follow`、`git.grouped`、`editor.fontSizeOffset`。

**不在第一批**：整个界面的缩放（见第 9 节 Q4）、浅色主题、工具路径（第 2 节）、按项目的配置（`.lite-ide/` 是 #48 的事）。

## 6. 读不对的时候

启动路径上不许抛（frontend.md「启动路径上的代码一律不许抛」那条），而且「静默用默认值」会让人以为自己的设置生效了。规则：

| 情况 | 怎么办 | 告诉你吗 |
|---|---|---|
| 文件不存在 | 全用默认值；**不建文件**（只在你点「设置…」时才建模板，不往你的目录里乱放东西） | 不说 |
| 启动时 JSON 坏了 | 全用默认值 | 状态栏：「设置文件第 12 行有错，这次用的默认值」，点了打开那个文件并跳到那一行 |
| **运行中**改坏了（热加载时） | **保留上一份好的**，不退回默认 —— 你改到一半按了 ⌘S，字体不该突然跳回默认再跳回来 | 同上 |
| 某个键类型不对（`"editor.fontSize": "14"`） | **只有这个键**用默认值，别的照常 | 列出是哪几个键 |
| 不认识的键（`editor.fontsize`，大小写拼错） | 忽略 | 提示「不认识的设置：`editor.fontsize`」—— 这是拼错时唯一的线索 |
| 数值越界（字号 200） | 夹到范围里 | 提示 |
| `terminal.shell` 指的文件不存在或不能执行 | 退回 `$SHELL` | 开终端时提示 |

`ui-state.json` 是应用自己写的，坏了就当没有，不提示（同 `windows.json` 的 `parse_saved`）。

## 7. 谁拥有、怎么下发、怎么热加载

### 7.1 Rust 是唯一的主人

- 解析、默认值、校验、夹值全在一个**不依赖 Tauri 的模块**里（`src-tauri/src/settings.rs`，或者进 `fsservice` 旁边一个小 crate），裸单测能测全 —— 同 `windows.rs` 的理由。
- **键的定义只有一份**：Rust 里每个键一条（名字、类型、默认值、范围、一句说明）。前端的补全、模板里的注释、校验，全从这一份生成 —— 不在前端再抄一份键名列表（那样迟早有一处漏改）。前端通过命令 `settings_schema()` 拿到它。
- 命令：`settings()` 取当前生效的值（已经合并了默认值和偏移，前端拿来直接用）、`set_ui_state(key, value)` 改界面状态、`open_settings()` 不存在就建模板并返回路径。
- 变了就 `app.emit("settings-changed", 全量)` **广播给所有窗口**（同 `recent-changed`）。全量而不是增量：一共十来个值，增量省下的字节不值得多一种出错的方式。

### 7.2 热加载：监听目录，不监听文件（第 0 步实测改写了理由）

**监听它所在的目录（不递归），按文件名过滤。** 理由是**文件可能还不存在**：第 6 节定了「不存在就不建」，而 `notify` 去监听一个
不存在的路径**直接报错**，之后文件被建出来也收不到任何事件（实测）。你在 Finder 里、或者用别的编辑器第一次把它建出来，我们得知道。

原稿写的理由是「编辑器保存会换 inode，直接盯文件第一次保存后就收不到」—— **在 macOS 上不成立**：FSEvents 按路径监听，不按 inode，
实测 lite-ide 自己的 `write_text`（tmp + rename，连存两次）、vim（换了 inode）、删掉再建，直接盯文件的那个监听全都照样收得到。
那条理由只在 inotify / kqueue 上成立。

实测出来的另外三条，实现时要照做：

- **不看事件类型，只看「这个文件名被碰过」，然后重读、和上次读到的文本比，一样就什么都不做。** FSEvents 会把这个路径过去的标记一起带上：
  原地覆写报出来的有「改名」；lite-ide 第二次 `write_text` 时 `settings.json` 只收到两条「改名」、**一条「内容变了」都没有** ——
  按类型筛「内容变了才重读」，这次保存就漏了。
- **防抖。** vim 存一次，`settings.json` 收到 11 条，夹着 `.swp`、`settings.json~`、`4913`（vim 探测目录可写的临时文件）。
- **按文件名精确比**，不按前缀：vim 的 `settings.json~`、lite-ide 的 `.settings.json.lite-ide-tmp` 都带着这个名字。同目录的 `windows.json`
  （`save_now` 走 `.tmp` + rename）、`trust.json`、`ui-state.json` 被写时目录监听都会收到，过滤之后 0 条命中（实测）；`scratches/` 子目录不递归，0 条。

现成的 `fsservice::watch` 是给项目目录用的：递归、回调里只说「git 变了 / 文件变了」，不说是哪个文件，不合用。在 `fsservice` 里加一个
`watch_file(path, on_change)`：监听父目录、只认这个文件名、300ms 防抖。VS Code 的保存方式没测到（机器上没有它的命令行），
用「原地覆写」代替测了（Sublime 的默认）。

### 7.3 前端

- 新的 `state/settings.svelte.ts`：持有当前值，`listenHere("settings-changed")` 收到就替换。
- **首屏：挂载之前 `await settings()`，不做缓存**（第 0 步实测改写）。原稿是在 localStorage 里留一份「只用于首屏」的缓存、先画再换 ——
  前提是「一次 IPC 往返太贵」。实测真 `.app` 上挂载之前的一次往返 **< 1ms**（WebKit 的计时精度就是 1ms，量到的全是 0 或 1），
  单窗口冷启动和三个窗口一起恢复都一样；整个启动到挂载约 410ms。为了省这 1ms 去维护一份「缓存和真值可能不一致」的状态，不值。
  前提是 Rust 在 `setup` 里就把设置读进内存，`settings()` 只读内存（同 `windows.json` 在 `setup` 里读）。
- 字体：改 `--code-font`；终端那边改 `term.options.fontFamily / fontSize`（xterm 支持运行时改）。
- 这个 store 要进入口包（首屏要用），补全和 lint 跟着编辑器懒加载，不进入口。

### 7.4 「设置…」

- 菜单「lite-ide → 设置…」⌘,（macOS 的惯例位置和键位）。先改 `keymap.ts`，`menu_sync.rs` 会卡漂移。
- 点了：Rust 不存在就建模板（第 4.1 节），返回路径，前端在当前窗口开一个标签。
- **高亮不用 `@codemirror/lang-json`**（第 0 步实测）：lezer 的 JSON 语法不认注释，注释里的 `"JetBrains Mono"` 被当成键名，
  **注释掉的整行默认值和真正生效的设置上一样的颜色** —— 模板恰恰是「全部键都注释着列出来」，这样比不高亮还误导。
  改用 `@codemirror/legacy-modes` 的 JavaScript `json` 模式（已经是依赖、已经按需加载），再给它一张 `tokenTable: { property: tags.propertyName }`：
  注释、键名、字符串、数字、`true` / `null` 各是各的颜色（实测）。括号匹配这种模式没有语法树信息，CM6 会退回按字符扫，第 4 步真机看一眼。
- 编辑这个文件时额外挂两样（只对这一个文件）：
  - **补全**：键名、取值提示，来自 `settings_schema()`。自己写一个 CM6 completion source，不引 `codemirror-json-schema`（新依赖，而且它为通用 JSON Schema 设计，我们只有十来个扁平的键）。
  - **报错标在行上**：Rust 解析出的错误（行、列、哪个键）作为 CM6 的诊断画出来，和状态栏那句是同一份结果。

## 8. 第 0 步：实测结果（2026-10-08）

| 要验的 | 结果 | 改了设计的哪里 |
|---|---|---|
| 监听父目录 + 按文件名过滤，各种保存方式都收得到；同目录别的文件被写时不触发 | **都收得到**（`write_text` 连存两次、原地覆写、vim、删掉再建）；同目录 `windows.json` / `trust.json` / `ui-state.json` 被写 0 命中；子目录 0 条。**意外**：直接盯文件也全收得到（FSEvents 按路径），原稿的理由不成立；真正要监听目录的理由是「文件可能还不存在」—— 监听不存在的路径 `notify` 直接报错。另外事件类型不可信、vim 一次 11 条 | 7.2 改写理由，加「不看类型、比内容、防抖、按名字精确比」三条 |
| `@codemirror/lang-json` 打开带注释的文件 | **不能用**：注释里的字符串被认成键名，注释掉的默认值和生效的设置一个颜色。有错误恢复，后面的真设置还认得对 | 7.4 改用 legacy-modes 的 `json` 模式 + `tokenTable`，实测注释 / 键名 / 值各是各的颜色 |
| 首屏缓存那套会不会闪 | 前提就不成立：挂载之前一次 IPC 往返 **< 1ms**（单窗口、三窗口恢复都一样），直接等它回来再画 | 7.3 删掉首屏缓存，改成挂载前 `await settings()` |

实测用的东西：一个独立的小程序直接调 `fsservice::write_text` 和 `notify`（就是正式代码会用的那两个），lezer 解析器在 Node 里跑，
IPC 往返在临时身份的 `.app` 上往 `main.ts` 临时加了一个探针、量完撤掉。

**顺带看到、这一轮不管的**：`tsconfig.json` 这类常带注释的 `.json` 现在也是用 `lang-json` 打开的，同样错色。等这一轮的 `json` 模式落地后可以顺手判一下要不要给它们也换上。

## 9. 拍板记录（2026-10-08，四条都按建议）

| | 问题 | 定的 |
|---|---|---|
| **Q1** | 一份文件（应用也写 `settings.json`）还是两份（第 3 节） | **两份**。不碰你手写的文件，也不用自己写「定位改写」的解析器。（2026-10-22 推翻了一半：设置页也写它，见 13.3） |
| **Q2** | 工具路径（`JAVA_HOME` / Python / Node）这一轮做不做 | **不做，挪到 #48**，理由在第 2 节：对你无效（`.zshrc` 会盖掉），而且对的做法是经过登录 shell 跑，第一个用户是 #48 |
| **Q3** | JSONC + 扁平点号键 | **是**（第 4 节） |
| **Q4** | 整个界面的缩放（标题栏、侧边栏、状态栏一起放大）这一轮做不做 | **不做**。WebView 能整体缩放，但窗口左上角的红绿灯是原生的、不跟着缩，标题栏的高度和它要重新对；而且现在没人要。先只做编辑器和终端的字号 |

## 10. 分步落地

| 步 | 做什么 | 怎么验 |
|---|---|---|
| 0 ✅ | 第 8 节三件事实测 | 结论已写回（第 8 节）：推翻两处、换掉一处 |
| 1 ✅ | Rust：剥注释 + 键定义 + 合并默认值 + 校验（纯函数，裸单测）；`ui-state.json` 的解析 / 序列化（读写盘在第 2 步） | `src-tauri/src/settings.rs`，20 条测试；17 处各自改坏都红（第一版有一处没抓到，见 JOURNAL） |
| 2 ✅ | Rust：`watch_file`（fsservice）、`settingsctl.rs`（读盘、监听、存 `ui-state.json`、广播）、`commands/settings.rs`（`settings` / `settings_schema` / `set_ui_state` / `step_font` / `adopt_ui_state` / `open_settings`）；DTO 和 TS 类型、桩 `mock/settings.ts` | `watch_file` 4 条、Store 5 条、模板 1 条，各自改坏都红（有一处绿：是多余的代码，删了，见 JOURNAL）；临时身份 `.app` 8/8 |
| 3 ✅ | 前端：`settings.svelte.ts`（挂载前 `await settings.init()`）+ `settings-view.ts`（纯函数）、字体字号接上（CSS 变量 + 终端 `term.options`，**不重建终端**）、5 个旧偏好改走 Rust；**迁移**（下面） | 纯函数 10 条 + 状态测试 11 条，4 处改坏都红；浏览器里改设置终端不重建（去掉 `untrack` 就重建，验过红）；临时身份 `.app` 9/9（用真实数据副本升级、两窗口字号同步、⌘0 回到配置里的字号、写坏保持）；入口包 +1,072 B |
| 4 ✅ | 「设置…」⌘,（应用菜单；没有窗口时开一个窗口打开它）、模板（`create_new`）、`settings-file.ts`（legacy `json` 模式 + `tokenTable`、键名补全、问题标在行上，懒加载）、状态栏「设置有 N 处问题」、`terminal.shell`（读设置时判一次报问题，起终端时再判一次） | 补全 / 标注 8 条 + Rust 1 条，4 处改坏都红；浏览器里：注释整行一个颜色、括号匹配正常、补全插入不多引号、标注和状态栏提示；临时身份 `.app` 10/10（不发全局按键）；入口包 +832 B |
| 5 ✅ | 文档：USAGE「十二、设置」（顺手改对了字号那句：⌘0 回到配置里的字号）、ARCHITECTURE 的「属于谁」表 / 文件树 / 零污染那条、DIRECTION 标完成、AGENTS 的索引表 | —— |

**迁移**：升级后第一次启动，`ui-state.json` 不存在而 localStorage 里有旧的 5 个值 → 前端交给 Rust（同 `adoptRecent`：Rust 那边已经有了就不收）。旧的 `editorFont` 换算成偏移（旧值 − 13）。**旧键不删**：回退到旧版本时偏好还在。`settings.json` 不因为迁移而被建出来（第 6 节：只在你点「设置…」时建）。

## 11. 验收（真 `.app` 上，按第 2 节改过）

1. 删掉 `settings.json` 启动 → 一切默认，**文件没有被建出来**；点「设置…」→ 建出模板并打开
2. 改 `editor.fontFamily` 存盘 → 不重启，所有窗口的编辑器和终端都换了字体
3. 写一个语法错误存盘 → 字体**保持上一份**，状态栏说第几行错了，编辑器里那一行标红；应用照常；重启也照常起来（用默认值）
4. 写一个拼错的键 → 提示不认识它
5. ⌘= 两下 → 两个窗口的字号都 +2；⌘0 → 回到 `settings.json` 里写的基础字号
6. A 窗口关掉缩略图 → B 窗口跟着关
7. 配 `terminal.shell` 为 `/bin/bash` → 新开的终端是 bash，开着的那个不动；配一个不存在的路径 → 退回 `$SHELL` 并提示
8. 升级迁移：拿你真实数据的一份副本当「升级前」（临时身份的 `.app`），旧的缩略图 / 字号设置都还在

## 12. 这一轮明确不做

- 设置界面（上一轮讨论定的：先只有文件）
- 工具路径、`env`（第 2 节，挪到 #48）
- 整个界面的缩放、浅色主题（第 9 节 Q4）
- 按项目的设置（`.lite-ide/settings.json`）
- 应用改写 `settings.json`（第 3 节）

## 13. 设置页（2026-10-21，用户：「maven、jdk、项目、项目启动都没有设置页，你自己去排查」）

### 13.1 盘点：写死或靠猜、而且没有别的办法改的

| 块 | 原来 | 问题 |
|---|---|---|
| Spring 服务的启动 | `mvn … compile && exec java -cp … 主类`，写死 | 加不了 VM 参数、程序参数、环境变量，选不了 Spring profile。唯一的退路是 `tasks.json` 里手拼同名任务 —— 那样它就不算「认出来的」，**调试、热替换、Services 分组全失效**（`spring_main` 对文件里的同名任务返回空） |
| 项目 JDK | 按 pom 的版本自动挑最接近的（`taskdefs::jdk_prefix`） | 挑错了改不了，也看不到用的是哪个 |
| JDK 列表 | 扫三处（系统、`~/Library/Java`、sdkman） | Homebrew / asdf / jenv / mise 装的找不到，也不能手动加；jdtls 那边另一套找法（`java_home -X`），两边看到的不一样 |
| 用哪个 Maven | 永远是 PATH 里的 `mvn` | 项目自带 `mvnw`（Spring Initializr 生成的都有）也不用它；指定不了安装目录 |
| Maven 配置 | `~/.m2/settings.xml` 写死 | 公司项目常用单独的 settings.xml / 本地仓库；加不了 `-T 4`、`-U`、`MAVEN_OPTS` |
| jdtls 内存 | `-Xmx1G` 写死 | 大项目导入慢、内存不够 |
| 原有 6 项 | 只能手写 JSON | 没有界面 |

这一轮不做：编辑器默认缩进、普通文件自动保存、项目默认编码、单测的 VM 参数。

### 13.2 调研：三家怎么做的

| | 形态 | 存在哪 |
|---|---|---|
| IntelliJ IDEA | ⌘, 模态对话框：左分类树 + 搜索、右表单、OK / 取消 / 应用；项目级的设置带「项目」图标 | IDE 级在配置目录的 XML；项目级在 `.idea/`；**运行配置默认在 `.idea/workspace.xml`（只有你自己的）**，勾「Store as project file」才写进项目（2020.1 起建议 `.run/*.run.xml`） |
| Zed（2025 秋重做） | **独立窗口**：项目窗口围着一个项目转，设置分用户 / 项目两层，做成标签会把两种范围搅在一起 | `settings.json`（「文件永远在，想完全自己控制就去改它」） |
| VS Code | 图形的设置编辑器，User / Workspace 两个页签，改过的项左边一条色条 | **就是 `settings.json`**：设置编辑器是它的图形前端 |

### 13.3 拍板（2026-10-21）

| | 定的 | 理由 |
|---|---|---|
| 形态 | **IDEA 式对话框**（⌘,）+ 运行配置**另一个对话框**（Services 的「编辑配置…」） | 用户的习惯是 IDEA；改 JDK / Maven / jdtls 要重启语言服务器，「改一批、点应用」比改一项生效一项稳；真模态的对话框现成（`.dialog` + `modal.ts`），独立窗口要在多窗口那套里加一种窗口 |
| 全局存哪 | **`settings.json`，你和设置页都能写** —— 推翻第 9 节 Q1 | VS Code、Zed 都是「设置页 + 一份 JSON，两边都能改」，是现在的通用做法。Q1 当时怕的是按钮一天按几十次改你的文件 —— 那些照旧去 `ui-state.json`；设置页是你主动改的，写回去才是你要的。只改那一个值的字符，注释和次序都留着（13.5） |
| 按项目存哪 | **跟 IDEA：默认私有**（应用数据目录，相当于 `workspace.xml`），不往项目里写 | VM 参数里常有本机路径、密码；不会不小心提交上去 |
| 共享 | 第二步：「存成项目文件」写成 IDEA 的 `.run/<名字>.run.xml` | 同事用 IDEA 提交的 Spring Boot 配置能直接读到，反过来也一样 |

### 13.4 放在哪一层

| 设置 | 范围 | 存在 |
|---|---|---|
| 字体、字号、终端 shell、jdtls 路径（原有） | 全局 | `settings.json` |
| jdtls 内存 `java.jdtlsMemory`（MB） | 全局 | `settings.json` |
| 手动添加的 JDK `java.jdks`（目录列表） | 全局（IDEA 的 SDK 表也是全局的） | `settings.json` |
| Maven：用不用 `mvnw`、安装目录、settings.xml、本地仓库、额外参数、`MAVEN_OPTS` | 全局 | `settings.json`。**和 IDEA 不一样**：IDEA 是项目级的，再配一份「新项目的默认」。一台机器一般一套 Maven，先做全局，真有项目要不一样时再加按项目覆盖 |
| 离线、跳过测试、勾的 profile | 此项目（原样，Maven 工具窗里那三个开关） | `maven-prefs.json`（原样） |
| 项目 JDK（空 = 按 pom 自动） | 此项目 | `project-settings.json`（应用数据，按项目根） |
| 运行配置：每个 Spring 服务的 Spring profile、VM 参数、程序参数、环境变量、JDK | 此项目 | 同上 |

### 13.5 写 `settings.json`：只改那一个值

设置页「应用」时给 Rust 一批「键 → 新值 / 恢复默认」。Rust 在原文上逐个改：

- **已经写了这个键**：只换值那一段字符；恢复默认就整行删掉（连同它后面的逗号）
- **没写、但模板里有它那行注释**（`// "editor.fontSize": 13,`）：插在那行注释后面，和它的说明挨着
- **都没有**：插在最后一个 `}` 前面，末尾带逗号（JSONC 认）
- **文件有语法错误**：不写，对话框里说「settings.json 第 N 行有错，先修好它」—— 在一份读不懂的文本上动刀，会把它改得更坏
- 写完照旧走文件监听重读、广播，和你手改是同一条路；校验也是同一个函数

### 13.6 运行配置怎么进命令

认出来的服务照旧由我们拼命令，配置只往里加东西，所以调试、热替换照常：

```text
{JDK 那段} mvn … compile … && cd 模块 && {export 环境变量;} exec java {VM 参数} {-Dspring.profiles.active=…} -cp … 主类 {程序参数}
```

- VM 参数、程序参数是你写的一串，**照 shell 的规矩原样放进去**（要带空格就自己加引号）。IDEA 有自己的一套拆分规则，我们的命令本来就是 shell 跑的，再发明一套只会两边对不上
- 环境变量逐个 `export K='v'`：键要像个变量名，值按 shell 引起来
- JDK：运行配置里选的 > 项目的 > 按 pom 自动

### 13.7 分步

进度（2026-10-22）：1–3 做完；4 改了方向（13.8）：做的是「从 IDEA 的 workspace.xml 导入」，不是 `.run/*.run.xml`。

| 步 | 做什么 | 怎么验 |
|---|---|---|
| 1 | Rust：新键（`Kind` 加布尔、字符串列表）、`settings.json` 的定点改写、`project-settings.json`、运行配置进命令、Maven 怎么调（mvnw / 目录 / -s / 本地仓库 / 额外参数 / MAVEN_OPTS，所有 mvn 命令一处拼）、JDK 多找几处 + 手动加的、jdtls 的 runtimes 和内存跟着走 | 单测，各自改坏验红 |
| 2 | 前端：设置对话框、运行配置对话框、⌘, 改开对话框（对话框里留「打开 settings.json」）、Services「编辑配置…」、桩 | 浏览器桩 |
| 3 | 真 `.app`：设置页改了写回去、注释还在；运行配置的 profile / VM 参数真到了 JVM、调试照常；项目有 mvnw 时用它 | 验收脚本 |
| 4 | ~~`.run/*.run.xml`：读 IDEA 的 Spring Boot 配置、「存成项目文件」~~ → 从 `.idea/workspace.xml` 导入（13.8） | 字段照 IDEA 源码 / 字节码核对；你机器上三份真文件过一遍；验收脚本 |

### 13.8 从 IDEA 带运行配置过来

**为什么改了方向。** 动手前去看了你机器上 IDEA 存的真文件（只看结构、不看值）：三个项目都**没有** `.run/`，
11 份 Spring Boot 运行配置全在私有的 `.idea/workspace.xml` 里，而且全是 `nameIsGenerated="true"` —— IDEA 认出主类自动建的，
只有主类和模块，profile、VM 参数、环境变量一个都没配。`.run/*.run.xml` 对你是空的；你选了「改成读 workspace.xml」。
按现在的数据它也导入不出东西（这些我们扫 pom 本来就认得），它管的是「在 IDEA 里配过」的那种项目。`.run/` 和 `.idea/runConfigurations/`
是同一种 `<configuration>`，加进来是几行的事，等真有人提交了再做。

**读什么、怎么转**（`ideacfg.rs`；字段名、默认值、怎么进命令照 IDEA 2026.1 核对：平台部分读 intellij-community 的源码，
Spring Boot 插件不开源、读装着的那份 IDEA 里的字节码）：

| IDEA 里 | XML | 我们这边 |
|---|---|---|
| Active profiles | `ACTIVE_PROFILES` | Spring profile |
| VM options | `VM_PARAMETERS` | VM 参数（**重新拆、重新包引号**，见下） |
| Override configuration properties | `<additionalParameters><param>`，勾上的（`enabled` 默认关、没写就是没勾） | 进 VM 参数：`-D名=值`，值空的是 `-D名` |
| Enable debug output / Hide banner | `DEBUG_MODE` / `HIDE_BANNER` | `-Ddebug` / `-Dspring.main.banner-mode=OFF` |
| Program arguments | `PROGRAM_PARAMETERS` | 程序参数（同 VM 参数） |
| Environment variables | `<envs><env name value/>` | 环境变量；名字放不进 shell 的不带、说一句 |
| JRE | `ALTERNATIVE_JRE_PATH` + `_ENABLED` | 是路径、是 JDK 就带；是 IDEA 自己 JDK 表里的名字（`corretto-17`）认不出、说一句、跟项目 |
| 社区版的 Application 配置 | `type="Application"`、`MAIN_CLASS_NAME` | 同上，按主类对上 Spring 服务 |

不带、也不说的：`ENABLE_LAUNCH_OPTIMIZATION`（**默认开**，IDEA 每次起都带 `-XX:TieredStopAtLevel=1` —— 它自己的启动加速，不是你配的）、
JMX、缩短命令行、URL 路径这些 IDEA 自己的行为。带不过来、要说一句的：别的工作目录、关了「带上系统环境变量」、`.env` 文件、`$名字$` 路径变量。

**参数串不能原样搬。** IDEA 拆参数只认双引号（`\"` 转义、单引号是普通字符），再把 `${名字}` 换成环境变量；我们的 VM 参数一栏是 shell 的写法。
原样搬的话，`-Dmsg='a b'` 在 IDEA 里是一个带单引号的值、到 shell 里引号没了；`-Dcost=$5` 到 shell 里 `$5` 被吃掉。
所以照 IDEA 的规则拆成一个个参数，再一个个套单引号，`${名字}` 留在外面给 shell 展开（IDEA 先查自己的路径变量、不分大小写，这两处对不齐）。

**路径要换回来。** IDEA 存盘时把值里的项目目录、家目录换成 `$PROJECT_DIR$` / `$USER_HOME$`，读的时候换回去（`$PROJECT_DIR$` 是 `.idea` 所在的目录 ——
项目根和每个 reactor 目录下的 `.idea` 都读，IDEA 的项目根不一定是你打开的那一层）。

**点一下是填进表单，不是存。** 和手改一样要点「确定」；整份换掉不合并（IDEA 里没配的就是空的）。对上服务按主类，
好几个服务同一个主类时再按模块名分。workspace.xml 超过 16 MB 不读、说一句（它常年攒着别的组件，RunManager 只是一段）。

### 13.9 编辑器里两样 Java 智能的开关（2026-10-22，docs/TAKEOVER.md G11）

`editor.highlightUsages`（同名高亮）、`editor.inlayHints`（参数名提示），默认都开（IDEA 的默认）。键归 `editor.` 不归 `java.`：
一是它们是编辑器里看得见的东西（IDEA 在 Editor 下面），以后别的语言服务器也用；二是 `java.` 开头的改了会提醒「要重启 jdtls」，这两样不用 ——
关掉当场收、打开马上问一次（前端订阅 `settings-changed`，不重建编辑器）。值要进 `SettingsDto`（前端要读），所以入口里的默认值表多两个字段（+45 B）。
桩原来「应用」之后不广播（真实现靠文件监听重读再广播），设置页里切的开关在浏览器里没反应 —— 补上了。

### 13.10 键位（2026-10-23，docs/TAKEOVER.md R3）

**不进 `settings.json`，单独一份 `keymap.json`**（同目录）：`settings::DEFS` 是「一项一个值、只改那一个值的字符」，键位是一张「动作 → 键」的表，
塞进去就得给一百多个动作各开一个设置项。VS Code 的 `keybindings.json`、IDEA 的 keymap XML 也都和设置分开。整份是我们写的（没有人的注释要留），只存改过的。

- **谁是默认**：`src/lib/state/keymap.ts` 照旧是唯一出处。生效 = 默认 ⊕ 覆盖。Rust 读覆盖、建菜单时挂生效的键（`keymap::menu_accel`），前端的编辑器键按 id 挂（`editor/id-keys.ts`）
- **改了当场生效、不重建菜单**：muda 的菜单项能换 accelerator（`menu::apply_keys`）；编辑器的每张键位表包在 compartment 里，覆盖一换逐个 reconfigure。重建菜单要换掉 `manage` 进去的 `MenuHandles`（只能放一次）、还要重灌灰态和「最近打开」
- **录键时摘掉菜单的键**：菜单挂着的键 AppKit 先吃掉、页面收不到 —— 不摘的话 ⌘S 这种「有人用着的」永远录不进来，而「把一个键从 A 挪给 B」是最常见的改法
- **撞键前端判**：全表（连同编辑器自己吃的键）只有 `keymap.ts` 有。Rust 只管「读得懂」和「归 keydown 的那几项不挂进菜单」（`keymap::KEY_OWNED_IN_MENU`，`menu_sync.rs` 卡着和 keymap.ts 一致）
