---
paths:
  - "src/**/*.svelte"
  - "src/**/*.ts"
---

# 前端的规矩

下面每一条都是**已经踩过**的坑，不是预防性的规矩。
界面长什么样见 [ui.md](ui.md)。

## `$state` 数组里的元素：拿到手的可能是原始对象

```js
const tab = { id, ... };
tabs = [...tabs, tab];   // tabs 是 $state
activeId = tab.id;       // 这里渲染了一次
await something();
tab.diffRaw = raw;       // ← 改得动数据，但**不产生信号**，界面不会重渲染
```

数组元素是**读取时**才被包成代理的，局部变量拿的是创建时那个原始对象。
异步流程尤其容易踩：`await` 回来时手上的引用早已不是响应式那份。

**规则：所有异步路径按 id 重新从数组里取**（`tabById(id)`）。
这个 bug 表现为「差异面板一直显示没有差异」，很难往响应式上想。

## 按住 Shift 时 `e.key` 是大写

`e.key === "g" && e.shiftKey` 永远不成立 —— 规范里 `key` 是修饰后的字符值。
⌘⇧G / ⌘⇧O / ⌘⇧F 都因为这个悄悄失效过。统一小写化：

```js
const k = e.key.length === 1 ? e.key.toLowerCase() : e.key;
```

## effect 的依赖集是**第一次跑**时读出来的

```js
$effect(() => {
  if (!host || view) return;      // ← 只有这两个是「守卫」
  view = new EditorView({ state: build(initial), parent: host });
  baseText = baseline ?? initial;  // ← path / baseline / initial / build() 里的
});                                //   showMinimap 全都进了依赖集
```

写这段的人想的是「只在挂载时做一次」，但 effect 不认这个意图 ——
它记下第一次实际读过的每一个信号。于是 `baseline` 一变（保存成功把磁盘那份
换成新的），cleanup 就跑，`view.destroy()`，**整个编辑器被重建**。

表现是：**⌘S 之后光标跳回文件开头、撤销栈整个清空**。实测光标 120 → 0，
`view` 换了实例，⌘Z 撤不回刚才那次修改。而旁边「保存成功」那条 effect 的
注释一直写着「不换 state，光标与撤销栈都保住」—— 它自己没错，是被连累的。

**要真的「只做一次」，就得把读 prop 的那几行套进 `untrack`**，只留下真正
该触发重建的那个（这里是 `host`）。

同一个 bug 有第二层：另一条 effect 会 `view.setState(build(text))`，
`view` 是同一个但 state 被换掉了 —— 光标和撤销栈照样没。**两处都要堵**，
只堵一处的现象和没堵一模一样。判据是「文本真变了才换 state」。

## 累计计数器当 prop + `{#key}` 重建 = 挂载当场触发一次

`savedTick` 是 App 上只增不减的计数。组件被 `{#key active.id}` 包着，
切标签就是全新实例，而这条 effect 判的是「非零」：

```js
$effect(() => {
  savedTick;
  if (!view || savedTick === 0) return;   // ← 这个会话里存过一次，它就永远非零
  onChange(false);                         // ← 于是每次挂载都当成「刚保存过」
});
```

结果：**只要保存过一次，带未保存改动的标签切走再切回来，圆点就没了**
（字还在，但 ⌘W 不再拦你）—— 和 M25 修的那个形状一模一样。
判据要跟**上次见到的值**比，并且在挂载时先对齐一次。

## CM6 的面板不在 `contentDOM` 里，所以键位到不了它

自研查找面板时踩的（2026-09-10）：面板 DOM 挂在 `.cm-panels` 上，而 `keymap`
是挂在 `view.contentDOM` 上的 —— 焦点一进查找框，⌘F、F3、⌘G、⌥⌘F **全都不响应**。
默认面板不是靠什么魔法，它自己在 keydown 里调了一句：

```js
runScopeHandlers(view, e, "search-panel")
```

`searchKeymap` 里那几条正是按 `scope: "editor search-panel"` 注册的。
自己加的绑定要想在面板里也管用，得标同一个作用域。

**同一个文件里还有一条**：`createPanel` 返回的 `Panel` 要在 `mount()` 里
自己 `focus()` + `select()`。CM6 不替你做 —— 默认面板也是自己做的。
少了这两句，⌘F 之后面板确实开了、看着完全正常，而**敲进去的字全落进正文**
（实测：按 ⌘F 打「Client」，第 8 行当场变成 `orderClieClientnt`，
标签上还多了个未保存的圆点）。

两条都是同一个形状：**「装了不生效」不报错**，只表现为「按了没反应」，
而人第一反应是自己按错了。

## CM6：不能在 `update()` 里读布局

会抛 `Reading the editor layout isn't allowed during an update`。
CM6 把一帧切成读、写两个阶段，正是为了避免读写交替触发强制重排。
用 `view.requestMeasure({ read, write })`。

滚动也别只靠 `ViewUpdate` —— 视口内的小幅滚动不一定触发 `viewportChanged`。
直接听 `view.scrollDOM` 的 `scroll` 事件。

## 重的东西一律按需加载

入口包只放两种模式都要的东西（ARCHITECTURE 的红线）。
CM6、xterm、Git 那套、67 个语言包全部 lazy。用 `src/lib/lazy/lazy.svelte.ts` 里的
`lazy()` / `lazyGroup()`，别再手写一遍 then/catch/finally 样板。

**CI 会卡这条线**：入口包超过 160 KB 直接失败，145 告警（2026-09-21 从 150/138 放宽，理由在 ci.yml 里）。本地想先看一眼：

```bash
pnpm build && ls -l dist/assets/$(grep -o 'assets/[^"]*\.js' dist/index.html | head -1 | cut -d/ -f2)
```

（**当前 145,786 B = 142.4 KiB（2026-09-23），告警线 145、红线 160**。09-21 晚是 142,131：09-22 那几批交互打磨涨了 3.6 KB（没逐项归因，是在事后对比 HEAD 构建时才发现的），09-23 Java 着色 / 成员跳转 / 堆栈链接只 +33 B —— 那些全在 Java 和日志的懒 chunk 里；CI 算的是 `size / 1024` 取整。这两条线是 2026-09-21 从 138/150 放宽的：清单上能搬去懒 chunk 的都搬完了（下面两段），剩下的是接线；入口包只占启动的 14%（JOURNAL 09-18）。**放宽不等于不数**：这个数字每轮照旧重量、照旧记在这儿，涨了要说得出是谁。
这个数字每轮都要重量一次 —— 它在 M20/M22/M24/M25 里从 126 一路涨到 157，
而 README 和这里各记了一个旧值，看着像互相矛盾。
崩溃屏 `Crash.svelte` 是刻意静态引入的 —— 需要它的时候，
正是模块加载可能已经不可信的时候。）

**光看总数不够，要知道是谁在里面。** 开 sourcemap build 一次，
按 mappings 把字节归到源文件上，比盯着一个总数有用得多 ——
2026-09-03 那次就是这么找到两块该出去的：

| | 归因字节 | 怎么处理 |
|---|---|---|
| `editor/langs.ts` | 23,419 | 一半是 500 行 `loadLang` 的 switch 和 67 个 `import()` 桩，**只有编辑器用，而编辑器是懒的**。拆成 `langs.ts`（识别，入口要）+ `langs-load.ts`（加载，跟着编辑器走）→ **−11.8 KB** |
| `@tauri-apps/api` 的 webview/window/dpi | 15,572 | 静态引一个 `getCurrentWebview` 就把一整串拽进来了，而它只服务一个拖放监听。改成动态 `import()` → **−17.3 KB** |

2026-09-07 又量了一次（138.5 → 123 KB），三块：

| | 实测省下 | 怎么处理 |
|---|---|---|
| `QuickSearch` + `Outline` + 它俩共用的 `fuzzy.ts` | **10,183 字节** | 都是按键才唤出的浮层。合成一个 `lazyGroup`（共用 fuzzy，分两次会多一个公共块），首屏画完 **300ms 后预拉** —— 这两个是天天按的，不能让第一次 ⌘P 等一次往返 |
| `editor/langs.ts` | **3,419 字节** | 状态栏那格语言名、⌘⇧O 支不支持，都得先有一个打开的文件才成立。改成 `active` 一有值就 `import()` |
| `dev/mock-ipc.ts` | **1,196 字节** | 见下面那条 —— 它**本来就不该在产物里** |

2026-09-15 #32 又量了一次（139,115 → 133,492 B）：

| | 实测省下 | 怎么处理 |
|---|---|---|
| `Confirms.svelte` | **5,069 字节** | 每条横幅都要先发生点什么才出现。懒加载 + 首屏后 300ms 预拉 + 任一 `pending*` 一亮兜底拉 —— 关脏标签的确认框晚一次往返出来，人会以为 ⌘W 没反应 |
| `editor/indent.ts` | **554 字节** | 归因说 2,186，实际 554 —— 又一次「归因高估数据密集的模块」。编辑器自己算，状态栏走 `lang.mod`（那个 chunk 本来就是有标签才拉的） |
| `ContextMenu`（从 Tabs 懒） | **−275（反而涨）** | TitleBar / Panel 还静态引着它，它根本没离开入口，只多了一层 lazy 壳。**先 grep 谁在引，再决定挪不挪** |

2026-09-17 #32 第二次（140,926 → 132,880 B）：

| | 实测省下 | 怎么处理 |
|---|---|---|
| `Panel.svelte`（底部工具窗） | **5,568 字节** + 3.1 KB CSS | 它自己只在「面板开着或有终端」时才渲染，和文件树同一个判据 → `lazy()`；恢复的会话面板开着就立刻拉，否则首屏后 300ms 预拉 |
| `ipc/commands.ts` 里只有懒模块用的 42 个封装 | **2,478 字节** | 搬去 `ipc/git.ts` / `log.ts` / `pty.ts` / `fs.ts` / `search.ts`。Rollup 把共享模块整个放进入口 chunk，懒 chunk 再从它导入 —— 封装不跟着调用方走就永远在入口里。DTO 全留在 `commands.ts`，`dto_sync.rs` 只看它 |

2026-09-21 第四次（141,865 → 136,948 B）。归因脚本按 sourcemap 的 mappings 把字节记到源文件、再按源码行分桶 —— 这次入口里已经没有 10 KB 级的整块了，剩下的都是 2–4 KB 的散块，每一刀都得挑「有触发条件才有用」的：

| | 实测省下 | 怎么处理 |
|---|---|---|
| `runMenu` 那张 70 个 case 的表 + 帮助菜单四个动作 | **2,819 字节** | 搬去 `shell/menu-actions.ts`，App 里 `import()`，首屏后 300ms 预拉。它走的本来就是 AppKit → IPC 事件 → 前端这条异步路，多一次本地 chunk 往返察觉不到。App 独有的 `$state`（缩略图、字号、分支挂件锚点）不搬，`MenuCtx` 回调过去。四个只有它用的 IPC 封装搬去 `ipc/app.ts`（同 #32 那条：封装跟着调用方走） |
| `ContextMenu.svelte`（标题栏 / 标签栏 / 状态栏三处静态引） | **2,098 字节** | 三处**一起**改懒，句柄只有 `shell/context-menu.svelte.ts` 一份；打开时 `load()`，`{#if menu && cmenu.comp}` 等到就画；App 首屏后预拉。2026-09-15 只改标签栏那次反而涨 275，就是没一起改 |
| Content 里取 git 基线 / 注解那两段 → 塞进编辑器 chunk 的 `EditorHost` 壳，用 `{...rest}` 透传 | **−268（反而涨）** | 壳本身省了 1.1 KB，但 `{...rest}` 让 Svelte 运行时的 `spread_props` 那串进了入口（`props.js` 690 → 1,895）。**运行时是入口和所有懒 chunk 共享的模块，懒 chunk 新用到的运行时函数照样落在入口里** —— 判断一刀值不值，要连编译产物用到了哪些运行时函数一起算。撤了 |

2026-09-21 第五次，分屏（issue #35）做完（142,586 → 140,098 B）。分屏本身 +5.5 KB —— 设计稿估的
1.5–2.5 少算了一倍多：`tabs.svelte.ts` 一个文件 +2 KB，App / Content / session / persist / invariant
各 +0.5–1.3。**估入口包增量要按「改了几个入口文件」而不是「加了几个功能点」算。**

| | 实测省下 | 怎么处理 |
|---|---|---|
| `tabflow.svelte.ts` 里开文件夹 / 最近 / 关项目 / 切模式 / 批量关 / 废纸篓 | **1,735 字节** | 拆去 `tabflow-ops.ts`，照 `git-ops.ts` 那套一行转发。留下的是首屏和 ⌘W 要的：`openPath` / `launchScratch` / `newScratch` / `requestClose` / `doClose` |
| `docs.svelte.ts` 里冲突裁决 / 换编码重开 / 改编码与换行符 | **565 字节** | 同上，`docs-ops.ts`。`setEol` 因此从同步变成 async —— 调用方本来就是 `void` 的，只有状态测试要补 `await` |
| 归因里 `StatusBar` 6.3 KB 看着像候选 | **0** | 按 10 行一桶看，1.4 KB 记在 `</script>` 那一行 —— 那是模板字符串，全是首屏要画的格子。**大 ≠ 可搬** |

还剩什么能砍：Svelte 运行时 38.6 KB 里 `boundary.js` 2.9 KB 是崩溃隔离的代价，不动；
`App.svelte` 12.8 KB 全是接线；`tabflow` 剩 3.3 KB、`docs` 剩 3.7 KB 都是启动路径。
**再压线就该动红线本身了** —— JOURNAL 2026-09-18 那段量过：入口包只占启动的 14%，
红线的意义是防退化不是继续压（2026-09-21 据此放宽到 145/160）。

判据是同一条：**入口包是首屏之前必须解析执行完的那一段**。
问一句「这东西在窗口出现之前有用吗」，没用就该出去。

**归因会高估数据密集的模块。** 这次归因说 `langs.ts` 占 11.0 KB，
真把它挪走只少了 3.4 KB，而 `keymap.ts` 从 2.4 KB「涨」到 9.6 KB ——
`langs.ts` 几乎全是表，语句少、mapping 稀，一段 mapping 会把后面邻居的字节
一起吞进来。**归因用来排序候选，实际收益要靠「挪走前后各量一次」定。**

## 别靠 tree-shaking 保证「开发桩不进产物」

`main.ts` 原来是静态 `import` 桩 + `if (import.meta.env.DEV)` 条件调用，
注释还写着「整个模块被 tree-shake 掉，产物里一个字节都不剩」。**那是错的**：
2026-09-07 在生产入口包里 grep 到了 `m13/git` —— 桩里那张假提交图是模块级的
`[...].map(...)`，**`.map()` 是方法调用，打包器证明不了它没有副作用**，
于是连同整个数据字面量一起留下了 1,195 字节。

现在是 `await import()`：`import.meta.env.DEV` 在生产构建里是常量假，
整个 `if` 块连同那句 import **在语法层面**就被消除，不再依赖 tree-shaking
能推到哪一步。CI 里另有一步「开发桩不许进产物」拿桩里的假数据串当哨兵 ——
那只是烟雾报警，真正的保证是那个死分支。

顺带一条哨兵的教训：第一版哨兵里混了两条**函数体内**的字符串，实测它们
不会漏（函数被 shake 掉了，留下的只有模块级数据）—— 那种哨兵是一条
永远不响的报警。哨兵要取自**真的漏出来过的那部分**。

## 窗口是半透明的，所以「填个底色」不再是安全操作

2026-09-04 起，窗口后面挂着一块 `NSVisualEffectView`（`Sidebar` 材质、
`BehindWindow` 混合），由 WindowServer 把桌面模糊后透上来。这不是配色，
是**材质**，webview 里的 `backdrop-filter` 顶不上 —— 它只能模糊页面自己的内容，
桌面在 webview 之外，写多少 blur 都一动不动。

于是 CSS 里每一处 `background:` 都要先回答一个问题：**这块面在第几层？**

| 层 | 谁 | 用什么 |
|---|---|---|
| 外壳 | 标题栏 · 竖条 · 侧边栏 · 标签栏 · 面板头 · 工具条 · 状态栏 | `--panel-bg`（transparent）/ `--chrome-scrim` |
| 内容 | 编辑器 · 日志 · 差异 · 终端 | `--content-bg`（透 6%） |
| 交互态 | hover / selected / pressed | `--hover` / `--selected` / `--pressed`，**一律白叠加** |
| 浮层 | 菜单 · 弹窗 · 输入框 · 徽章 | `--elevated` / `--elevated-hi`，**不透明** + `--shadow-pop` |

四条会咬人的：

1. **不透明色不能当 hover 用。** 原来共 43 处引用，其中 **29 处**写的是
   `:hover { background: var(--panel-bg-2) }`（`#212121` 实色）——
   在玻璃上那是**凿一个洞**，一块生硬的矩形浮在表面上。
   `--panel-bg-2` 因此被拆掉了（hover 归 `--hover`，浮层归 `--elevated`），
   **不留别名** —— 留一个 0 引用的别名就是下一个 `--surface-3`
   （那个 token 定义了从来没人用，2026-09-04 一起删了）。

2. **浮层必须不透明。** 这条最反直觉：菜单做成半透明的话，
   桌面在 webview 之外，`backdrop-filter` 模糊不到它 —— 壁纸会**清晰地**
   穿过菜单，字直接糊掉。「实心卡片摞在玻璃上」才是对的观感，
   抬起靠 `--shadow-pop` 不靠透明。

3. **要「挡住底下滚过去的东西」的地方用 `--content-solid`，不是 `--content-bg`。**
   CM6 的行号栏、差异视图那根吸在左边的行号列，背景不是装饰是遮挡。
   DiffView 里那段注释早就写着「背景必须不透明」，而它依赖的正是
   `--editor-bg` 曾经是实色 —— 内容层一透，这个依赖就断了，
   表现是横向滚动时正文从行号底下透出来。

4. **内容层只画一次。** 两层半透明叠起来，6% 透光被压成 0.36%，
   于是编辑器比旁边的日志视图明显"更实"，两块内容区的色调对不上。
   编辑器的底画在 `Editor.svelte` 的 `.editor` 上，CM6 主题里的 `BG`
   已经改成 `transparent`。

配套的三处，动了要一起动：

- `tauri.conf.json` 的 `transparent: true` 和 `Cargo.toml` 的
  `tauri = { features = ["macos-private-api"] }` **必须成对**。
  只写配置不加 feature，`build.rs` 直接把构建拦下来
  （报的是「features 与 allowlist 不匹配」，不是「窗口不透明」，
  第一次撞上会往错的方向查）。
- `tauri.conf.json` **不能再有 `backgroundColor`**。它原来是 `#1E1F22`
  （上上版的 IDEA 灰），透明窗口下就是糊在玻璃前面的一层实色。
- `html, body` 必须是 `transparent`。body 有一点不透明的底，
  NSVisualEffectView 就整块被盖住 —— 表现是「vibrancy 好像没生效」，
  而 Rust 侧一切正常，**从那头是查不出来的**。

## CSP 写在两个地方，改一处要改两处

`src-tauri/tauri.conf.json` 的 `app.security.csp` 和 `vite.config.ts` 的
`server.headers`。后者是为了让 `pnpm dev` 也跑在同一条 CSP 下 —— 理由和
mock-ipc 一样：**CSP 挡下东西不会报错**，只表现为「某处不好使了」，
要是只有打包后的壳带 CSP，这类问题得等 45 秒的 Tauri 构建才撞得上。

两条不能动的：

- `style-src` 必须留 `'unsafe-inline'` —— CM6 和 xterm 都在运行时往 head 里
  插 `<style>`，去掉这条编辑器和终端直接白给。
- `main.ts` 里那个 `securitypolicyviolation` 监听不能删，它是唯一的线索来源。

改完 CSP 必须实测：起 `pnpm dev`，**开一个文件（CM6）+ 开终端（xterm）**，
看控制台有没有违规。只看首屏渲染正常是不够的，那两个组件是懒加载的。

## 异步 effect 里 `await` 之后要检查自己是不是已经被清理了

```js
$effect(() => {
  let tick = null;
  const timer = setTimeout(async () => {
    await something();          // ← cleanup 可能正好在这中间跑
    tick = setInterval(...);    // ← 装出一个再也没人清的轮询
  }, 180);
  return () => { clearTimeout(timer); if (tick) clearInterval(tick); };
});
```

cleanup 只能清掉它**当时看得见**的东西。`await` 回来时那次 cleanup 早过去了，
`tick` 是在它身后才被赋值的。加一个 `let dead = false`，cleanup 里置位，
每个 await 之后先判它。

（`LogPane` 的过滤轮询踩过：在 1GB 文件上连打十个字 = 十个 80ms 的轮询一起烧 IPC。）

## `{#key}` 重建组件之前，组件里的状态要先交回来

编辑器是 `{#key active.id}` 包着的 —— 切标签就**销毁重建**，而它只往外报
一个 `onChange(dirty: boolean)`，实时文本从来没回写过。于是：

> 在 A 里打几个字 → 切到 B → 切回 A：**字没了，标签上那个「有未保存改动」的
> 圆点也一起没了**。人完全不会察觉自己丢了东西。

（圆点也没的原因更要命：重建时新实例拿 `initial` 当 dirty 基线，而 `initial`
就是那份没更新过的旧内容，一比相等，于是报「不脏」。）

两条：

1. **销毁前把内部状态交出去。** `Editor` 在 mount effect 的 cleanup 里调
   `onStash(path, text)`。
2. **交出去的时候要带上自己的身份。** cleanup 跑在切标签**之后**，
   这时候读 `path` 这个 prop 拿到的是**新标签**的值 —— 组件必须在挂载时
   快照一份 `curPath`，交的是它自己那份。写成 `active` 就是把 A 的内容
   盖到 B 头上。

还有一条连带的：**「草稿」和「磁盘那份」必须分成两个字段**
（`tab.draft` / `tab.content`）。合成一个的话基线会被草稿顶掉，
dirty 就再也算不出来了。保存成功、选「用磁盘上的」时都要把草稿清掉。

## 「只藏不卸载」要一路贯彻到最外层

底部面板那块踩过：切「终端 ↔ Git 日志」时用的是 `class:hidden`，
注释也写清了「组件一销毁 Session 就 drop，shell 直接被 kill」——
但**外面还包着一层 `{#if panel}`**，⌘J 一收起，正在跑的 gradle build 当场就没。

判据很简单：**这个组件的销毁有没有副作用？** 有的话（起了子进程、占了句柄、
连着流），它的每一层可见性条件都得是 `class:hidden`，不能是 `{#if}`。
只做对里面那一层，等于没做。

## CM6 的扩展可能「装了不生效」

`rectangularSelection()` / `crosshairCursor()` 都在扩展列表里，但少了
`EditorState.allowMultipleSelections.of(true)`，CM6 会把每次事务的选区
`asSingle()` 压成一个 —— 矩形选择、⌥ 点加光标、「选中所有匹配」全部无效。

**扩展进了列表不等于功能可用。** 这类 bug 不报错不崩溃，只是「按了没反应」，
而人第一反应是自己按错了快捷键。加完 CM6 扩展要实际按一下，
或者在页面里 dispatch 一个构造好的 state 验证。

## 启动路径上的代码一律不许抛

会话恢复读的是 localStorage 里一份**上次**写的数据 —— 版本可能是旧的、
可能被手改过、可能只写了一半。这段代码抛一次，应用就打不开，
而用户手里没有任何办法清掉那份坏数据（界面都出不来）。

`session.parse` 因此对**任何**输入都只返回 `null`，绝不抛；
尺寸类字段读回来一定要夹（一个 4000px 的侧边栏会把内容区挤没，
而拖动手柄本身就在屏幕外，拉不回来）。

## 恢复现场时，先记状态再开东西

会话恢复踩过：位置记在 `openPath()` **之后**，结果整个不生效 ——
`openPath` 一把标签加进去 `activeId` 就变了，兑现位置的 effect 当场就跑，
而那时要读的 Map 还是空的。等写进去时 `activeId` 已经不会再变，
effect 也就不会再跑第二次。

**凡是「A 触发 B 去读 C」的结构，C 必须在 A 之前就位。**

## 同一件事写在三个地方，早晚有一处少一行

「磁盘那份成了准」这件事发生在三处：保存成功、外部改动后重读、冲突时选
「用磁盘上的」。每处都要做同样的三件事（换 content、**清 draft**、清 dirty），
而重读那一处漏了清 draft —— 另外两处不但做了，还各写了一条注释说明为什么必须做。

漏掉的后果：切走再切回来，陈草稿把刚读回来的内容顶掉，还被算成「有未保存改动」。

现在只有 `state/doc.ts` 的 `settled()` 一个出口，三处 `Object.assign(tab, settled(x))`。
**测这种函数要贴到一个已经带着旧值的对象上测**：它返回的是新对象，
「没有 draft 这个键」和 `draft: undefined` 读出来都是 `undefined`，
只看返回值的话，把那一行删掉测试照样绿（第一版测试就是这么写的，验红时才发现）。

## 会话快照存草稿，判据是 stamp

`session.ts` 原来明写「不存草稿」，理由是「下次打开时它和盘上的文件谁对？
这个问题没有好答案」。2026-09-03 推翻了 —— 但推翻老决定要给出新答案，
不能只说「用户要」：

**草稿不是「另一份真相」，它就是一次没保存的编辑**，和应用开着的时候一样。
运行中本来就同时有 `content`（盘上那份）和 `draft`（编辑器里的），
跨重启只是要连 `stamp` 一起存：stamp 一致就原样恢复；对不上就走
`conflict`（运行中早就有的那套「用磁盘上的 / 保留我的」）；
草稿和盘上现在一样就丢掉。当初「没有好答案」，是因为那时还没有
`draft`/`content` 分离 —— 那是 M25 才有的。

两条实现上的硬要求：

1. **上限 + 退路。** localStorage 写满会抛，而这段代码在启动路径旁边。
   超限只丢那一份草稿；真写不下时**退一步不带草稿再存一次** ——
   不能让新功能把「上次开了哪些文件」这个老保证一起赔进去。
2. **光靠响应式 effect 存不下来。** 它订阅的是布局/标签/项目根，
   **打字不动其中任何一个**。要另外挂一个「有脏标签时定时落盘」，
   否则「改了半天没切标签也没退出」一次都不会存。

## 状态消息走 `notify`

别直接写 `error = …` 再自己 `setTimeout` 清除。
`src/lib/state/notify.svelte.ts` 三个通道：`ok` / `fail` / `block`（多行，不自动消失）。
各通道自己管定时器 —— 手写会出现「后一条消息被前一条的定时器抹掉」。

## 盘上的东西被外部改了，两件事要一起做

切分支、丢弃改动、动工作树、从终端切回来 —— 都要
`workingTreeChanged()`：重读已打开文件 **且** 重列已展开目录。
少做哪一件都会留下一个说谎的界面。

## 桩必须和真实现严格对齐

`src/lib/dev/mock-ipc.ts` 是浏览器里调 UI 用的（`pnpm dev`，热更新毫秒级，
比等 Tauri 重编译快得多）。它喂的数据结构**必须**和 Rust 侧 DTO 一致 ——
分叉之后它就失去了全部价值，还会骗人。改 DTO 记得同步改桩。

**「严格对齐」包括排序这类不起眼的行为。** 桩的 `list_dir` 原来按写死的顺序返回，
而 Rust 侧是「目录在前、同名不区分大小写」—— 于是新建出来的文件在浏览器里
永远吊在列表最后，而真实现会把它排到该在的位置。「新建完滚过去」那段交互
就是在这种地方白验的。

**桩比真实现快，所以「验红」这一步在它上面会失效。**

会话恢复那个「一个文件一个文件地闪」的 bug 就是这么差点被放过去的：
插好计数器一量，修复前后**都是 1 次**。不是修好了，是 mock-ipc 的
`read_text` 立即返回，5 个文件在同一帧里开完，Svelte 只渲染最后一次 ——
**那个 bug 在桩上根本不发生**。

给桩的 `probe_path` / `read_text` / `file_stamp` 临时加 25ms 延迟复现真实节奏，
撤掉修复立刻变成 5 次。验**时序类**的东西（渲染次数、竞态、防抖、加载顺序）时，
先问一句：这个 bug 在毫秒级返回的桩上还成立吗。

**真实现有「进行中」的操作，桩要能停在中间一会儿**（2026-09-22 立的）。

上面那条说的是时序 bug 在桩上不发生；这条是反过来的：**围着中间态做的 UI 在桩上
不出现**。凡是真实现要跑一阵的（提交跑钩子、列一个几万条目的目录、废纸篓走 Finder、
过滤扫 1GB、推送），围着它建的东西 —— 按钮的 busy 态、文件树的等待环、过滤条上的
计数 + 环、「慢操作才说话」那条 300ms 的线、「一次只允许一个写操作」的守卫 ——
在 0ms 返回的桩上**一次都验不到**，而且没有任何报错告诉你没验到。

四个先例，都是 UI 做完了才发现桩上看不见，回头补的：`git_commit` 睡 1.2s、
`list_dir` 的 `node_modules` 睡 900ms、`trash_entry` 睡 800ms、`log_filter_stat`
问四次才 `complete`（命中数按比例长上去，看得出是在扫）。

规矩：**给桩加一条会跑一阵的命令时，先问「真实现在这儿会等多久」；会等的就让桩也等**，
时长取「够跨过 UI 的延迟线（150 / 300ms）、又不至于让调 UI 变难受」，注释里写明是
故意的、验的是哪个状态。挑桩里那一条数据当慢的（`node_modules`），别全部变慢。

生产构建里 `import.meta.env.DEV` 为假，整个模块会被 tree-shake 掉。

## 自动化浏览器里「验不了」的那几样，要认出来

它是个**后台标签页**（`document.hidden === true`），于是：

| 你想验的 | 实际发生的 |
|---|---|
| `scrollTo({behavior:"smooth"})` | **完全不动**。`scrollTop` 一直是 0，看着像功能坏了 |
| `await requestAnimationFrame` | **永远不回调**，整个 `javascript_tool` 调用挂到 45s 超时 |
| `e.code` | **永远是空串** —— 每一个键都是，包括裸的 `a` |
| `e.keyCode` | **永远是 0**，于是按钮的原生激活（`↵`）不触发 |
| `↵` 的 `e.key` | 也是空串 |

滚动那条一次坑掉半小时：位置算得对、`scrollTo` 也调了（monkey-patch
`Element.prototype.scrollTo` 能看到参数），就是不动。
**验滚动要验「传给 `scrollTo` 的目标值」，不要验 `scrollTop`**；
等待一律用 `setTimeout`，不要用 rAF。
（2026-09-05 差异视图那轮又用上了一次：跳转目标算得准，`scrollTop` 全程是 0。）

### `.cm-cursor` 的个数**不能**用来数光标（2026-09-09）

验「⌘Click 还加不加光标」时踩的：数 `document.querySelectorAll('.cm-cursor')`
得到 1，据此写了一句「多光标没生效」——**是错的**。改用「点两下之后打两个字，
看正文里出现几次」，答案是 2，两个光标一直都在。

光标那几个 DOM 元素跟焦点和闪烁状态挂钩，自动化里读到的数不作数。
**要数光标就让它们各打一个字**，那是唯一骗不了人的证据。

（差点因此把一个好用的手势判成坏的，还顺手写进了注释 —— 那比测错更糟。）

### 面板藏在后面时，`setTimeout` 被钳到 1 秒（2026-09-10）

验 git 写守卫时踩的：桩的 fetch 有一串 90ms 的进度延时，量出来跑了十几秒，
于是「操作做完之后那句提示还挂着」看起来像一个泄漏 —— 实际上操作压根没做完。
把面板亮到前台（`tabs_select`）重测，19 秒变回正常，结论整个反过来。

浏览器对**不可见**的标签页有节流：`setTimeout` / `setInterval` 的最小间隔
被钳到 1000ms。所以：

- **凡是要量时长的，先把面板亮到前台。** 后台量出来的数一律不作数。
- 轮询等待时别按「循环次数 × 间隔」估总时长 —— 250ms × 40 在后台是 40 秒，
  会直接把 `javascript_tool` 跑到 45 秒超时。

### 动画的 `currentTime` 不走，`animation-delay` 永远走不完（2026-09-21）

验按钮的 busy 态（150ms 之后正文隐形、环出现）时踩的：截图里按钮一直是正文，
`getAnimations()` 显示三个动画 `running` 但 `currentTime` 停在 0，空闲时 900ms 只有
两次 rAF。面板不驱动时渲染被节流，动画的起始时间要到第一次渲染机会才定下来，
之后也不按时间推进。**截图只能证明静态的样子**，任何「N 毫秒之后变成什么」都验不了。

证据从 Web Animations API 拿：`for (const a of el.getAnimations({subtree:true})) a.currentTime = 200`
手动拨过去，再读计算样式（`-webkit-text-fill-color`、`::after` 的 `opacity`、
`offsetWidth` 前后对比）。要截时间态的图，先把全页动画都拨到终点再截。

**`e.code` 是空串这条比看上去要紧。** 凡是靠 `e.code` 判的键位，
在这上面**一次都不会触发** —— 比如差异视图的 `⌥Z`
（`e.altKey && e.code === "KeyZ"`，因为 macOS 上 ⌥z 的 `e.key` 是「Ω」）。
不是功能坏了，是这里验不了。要验就去点按钮，或者在真 `.app` 上按。

## 修饰键**现在传得下去了**（2026-09-05 复测，原来写的是传不下去）

这条以前写的是「修饰键传不下去，到手时 `shiftKey === false`」，
[issue #2](https://github.com/Spc-jgs/lite-ide-mac/issues/2) 那轮的记录。
2026-09-05 用 `computer` 的 key 动作连按六次、在 window 上捕获 keydown 实测：

| 按下 | 到手 |
|---|---|
| `shift+F10` | `key:"F10"` `shiftKey:true` |
| `cmd+s` | `key:"s"` `metaKey:true` |
| `ctrl+shift+\`` | ``key:"`"`` `ctrlKey:true` `shiftKey:true` |
| `alt+z` | `key:"z"` `altKey:true` |

四个修饰键全都到位。**工具改好了，这条是过时不是错**——
但同一次量到的 `e.code` 为空、`keyCode` 为 0 仍然成立（见上表），
所以 `⌥Z` 那类依旧验不了：`alt+z` 到手的 `e.key` 是 `"z"` 而不是真机上的 `"Ω"`，
`e.code` 又是空的，两条路都对不上。

在它上面验不了的东西，别写成「验过了」——
但也别把「以前验不了」当成永远验不了，**这一条就是复测之后翻过来的**。

### ⚠ 2026-09-07：`shift+F10` 在浮层开着的时候一个事件都不来

上面那张表是在**没有浮层、焦点在页面上**时量的。2026-09-07 验分支浮层的
`⇧F10` 右键菜单时，同样用 `computer` 的 key 动作，**window 上的捕获监听器
一条 keydown 都没收到**（`seen: []`）—— 不是 `shiftKey` 丢了，是事件根本没到页面。

差别是当时分支浮层开着、焦点在它的输入框里。是浏览器吃了、系统吃了、
还是别的，没查下去。

**结论不是「上面那条错了」，是「它有前提」**：那张表只在裸页面上成立。
要验浮层里的键盘路径，用 `dispatchEvent` 构造事件走代码路径，
然后老实说「真键按下去这儿验不了」。

---

## 浮层 `focus()` 要带 `preventScroll`，不然它会把自己关掉（2026-09-20）

提交历史的右键菜单在浏览器里**一次都开不出来**：菜单挂上了，50ms 后没了，控制台干净。
`ContextMenu` 开完就 `e.focus()` 好接 Esc 和方向键 —— 而 `focus()` 默认会把祖先滚到
「让这个元素可见」。菜单是 `position: fixed`，但 DOM 上长在触发它的容器里（底部面板），
浏览器照样给面板来了一下 scroll；`ContextMenu` 自己那个捕获阶段的 scroll 监听把这一下
当成「内容滚走了」，刚开就关。同一份 `ContextMenu` 在标签栏上开得出来（同一会话验的），
只是因为那个容器碰巧没得滚 —— 所以这条不是「提交历史的菜单坏了」，是壳本身的坑。

判据：**任何自己监听 scroll 来关掉的浮层，拿焦点时都要 `focus({ preventScroll: true })`**。
这类 bug 只表现为「点了没反应」，要在 dispatch 之后分别在微任务和 50ms 各查一次 DOM 才看得出
「挂上又没了」和「压根没挂上」是两回事。

## 页面内部的拖拽不能用 HTML5 dnd（2026-09-15）

文件树拖拽移动那轮踩的：`draggable` + `dragstart` / `dragover` / `drop` 在 `pnpm dev`
的浏览器里全对，真 `.app` 里目标行有 hover 高亮、`drop` 一次都不来。wry 为了接住
从 Finder 拖进来的文件接管了 NSView 的拖拽入口，页面内部的拖拽事件链在 WKWebView 里
就断了。**用 pointer 事件自己做**（按下记住、挪过阈值算开始、`elementFromPoint` 找落点），
FileTree 里那套可以照抄。这类东西「桩上全对」不作数，必须上真 `.app` 验 ——
用 CGEvent 模拟鼠标（scratchpad 里那个 `drag.swift`）能验。


## 外壳网格的 `1fr` 一律写 `minmax(0, 1fr)`，内容岛加 `contain: strict`（2026-09-17）

`1fr` 是 `minmax(auto, 1fr)`：轨道下限是内容的 min-content，网格每次布局都要先把那一格
的子树排一遍来问「你多高」。编辑器里每敲一个键 `.cm-line` 就变脏，这一问跟着来一次 ——
`sample` WebContent 进程，按键时间的 75% 在 `GridTrackSizingAlgorithm →
logicalHeightForGridItem → 一路 layout 到底`；粘了 2000 行日志的草稿里每键 9–18ms 全是它，
而 JS 侧（CM6 `dispatch`）只有 2ms。**JS 里量不到的时间去 `sample` 原生进程**，
不用 sudo，栈是明文的。

## 量按键延迟别用 System Events 的 `keystroke`

它是 AX 客户端，每键多出几十毫秒排队，一串 45 个字尾部能拖到 800ms —— 看着像编辑器慢，
其实是驱动工具慢（同一份空草稿：`keystroke` p50 55ms，CGEvent 21ms）。
用 `CGEventPost`：十几行 Swift，`CGEvent(keyboardEventSource:virtualKey:keyDown:)` +
`keyboardSetUnicodeString` + `post(tap: .cghidEventTap)`，每键之间 `sleep` 12ms。
smoke.sh 只关心对错不关心时长，它用 `keystroke` 没问题。

## 驱动真 `.app` 的两个坑（2026-09-17）

- **别用 `tell application "lite-ide" to activate`。** 它按名字找的是 bundle 里那份 `.app`，
  你跑的是 `target/release/lite-ide` 裸二进制的话，它会**再起一个实例**（app.log 里多一行
  「启动」，两个实例抢同一份 localStorage，快照被空实例盖掉）。要前台就
  `tell application "System Events" to set frontmost of process "lite-ide" to true`。
- **裸二进制和 `.app` 的 localStorage 不是同一份。** WKWebView 按进程身份分目录：
  `~/Library/WebKit/lite-ide/…` 和 `~/Library/WebKit/com.liteide.app/…`。从盘上读会话快照
  （`localstorage.sqlite3` 的 `ItemTable`，值是 UTF-16LE）要看对目录，不然读到的是
  几小时前另一份的。

## 状态层（`*.svelte.ts`）能在裸 node 里测，改流程要带一条（2026-09-18）

`tabflow` / `docs` / `persist` / `worktree` 原来零自动化测试 —— 09-03 两个丢数据 bug、
M25 切标签丢未保存、09-09 `stashed` 回滚全出在这层，靠间歇红的 smoke 兜。理由是「`$state`
不是 JS，裸 node 跑不了」。其实 svelte 自己就带编译器：`tests/runes/hooks.mjs` 用
`compileModule` 把 `.svelte.ts` 编成引用 `svelte/internal/client` 的普通 JS（那正是浏览器里
跑的东西，只是不经 vite），`register.mjs` 垫上 `window` / `localStorage`，IPC 走 `mock-ipc.ts`
（和 `pnpm dev` 同一份桩）。没有框架，多的只是一个 loader。

`tests/state-tabflow.state.test.ts` 是样板：编辑器是一个 20 行的替身，和 `Editor.svelte` 讲
同一套契约（`docs.onEditorLive` 交出「读此刻文本」、切走 `stashDraft` 交回草稿），测的是
状态层拿到这些之后做对没有。把 `liveText` 改回 09-03 的写法（无视活着的编辑器），
「保存并关闭写的是编辑器此刻的文本」那条当场红。

规矩：**改了 tabflow / docs / worktree 里的流程，先想这条流程的「丢数据形状」是什么，
在那个文件里加一条。** 桩里能表达的状态都能测（另存为的保存面板用 `globalThis.prompt` 顶）。
