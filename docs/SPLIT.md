# 分屏编辑器（issue #35）—— 设计

> 状态：设计稿，2026-09-21。实现分四步落地，每步一条提交，见末尾。
> 读之前先看 [ARCHITECTURE.md](ARCHITECTURE.md) 的前端状态层一节。

## 0. 一句话

内容区从「一个编辑器」变成「左右两组，各自有标签条和一个视图」。
**改动的核心只有一条：标签多了一个 `group` 字段，标签表多了一个「每组正在显示谁」。**
其余 110 处读 `tabs.active` 的代码一个字不改 —— 这是整个设计的支点。

## 1. 现状：为什么它不是「多渲染一个 Content」那么简单

```
tabs.list      : TabState[]        一维，顺序 = 标签条顺序
tabs.activeId  : number | null     唯一的活动标签
Content.svelte : {#key tabs.active.id} <Editor …/>   一个视图实例
docs.#live     : { path, get }     「此刻挂着编辑器的那一个」，只可能有一个
session.Session: { tabs, active }  快照里 active 是下标
```

四层都建立在「同一时刻只有一个视图」上：

| 层 | 依赖这条假设的地方 |
|---|---|
| 渲染 | `Content` 直接读 `tabs.active`（53 处）；`{#key active.id}` 切标签 = 销毁重建 |
| 文档 | `docs.#live` / `#viewProbe` / `#wordProbe` 各只有一份，按 path 认领 |
| 标签表 | `remove()` 让活动标签落到**列表里**的邻居；预览标签「同时最多一个」 |
| 会话 | `active` 是一维下标；`withoutTabs` 重算它 |
| 自检 | `tabsFaults`：「有标签但没有活动标签」「预览超过一个」「钉住的都在左边」都按整表算 |

## 2. 目标形态（照 IDEA / VS Code，取交集）

- 最多**两组**，左右排列，中间一条可拖的分隔线。不做上下、不做嵌套、不做三组以上 ——
  个人工作台，「左边看定义、右边改调用」覆盖了几乎全部用途；再多是给自己找活儿。
- 每组一条标签条、一个视图。标签条上的一切（预览斜体、圆点、图钉、右键菜单）**按组**成立。
- **焦点组**：有一个组是「当前」的。状态栏、面包屑、大纲、⌘S、⌘W、跳转历史、Git 面板的
  「当前文件」全都指焦点组正在显示的那个标签 —— 也就是 `tabs.active` **语义不变**。
- 点进哪一组的编辑器、点哪一组的标签条，哪一组就成为焦点组。非焦点组的编辑器不抢光标。
- 打开文件（树、⌘P、⌘B、拖进来、命令行）落在焦点组。
- **一个文件只能在一个组里**（issue 里的简化版）。在另一组里已经开着的文件被再次打开时，
  焦点跳到那一组的那个标签，不开第二份。同一文档双实例共享 doc 是后面的事 —— 它要动
  `docs` 的草稿回写、`{#key}` 的 stash 时序、CM6 的 `EditorState` 共享，另一轮。
- 一组关到空 → 这组自动收起，回到单栏（VS Code 默认行为；IDEA 也是）。不留空组。
- 分屏状态和分隔线位置进会话快照。

## 3. 状态层

### 3.1 `TabState` 加一个字段

```ts
/** 在哪一组。0 = 左（单栏时唯一的那组），1 = 右 */
group: 0 | 1;
```

用两个字面量而不是 `number`：上面说了不做三组，类型上把门关死比注释里写一句管用。

### 3.2 `Tabs` 类

```ts
class Tabs {
  list = $state<TabState[]>([]);          // 不变：全部标签，一维
  activeId = $state<number | null>(null); // 不变：焦点组正在显示的那个
  active = $derived(…)                    // 不变

  /** 每组正在显示的标签 id。长度 1 = 单栏，2 = 分屏。 */
  shown = $state<(number | null)[]>([null]);

  /** 焦点组 = active 所在的组；没有标签时 0 */
  activeGroup = $derived(this.active?.group ?? 0);
  split = $derived(this.shown.length === 2);
  inGroup(g)  // list.filter(t => t.group === g)，给标签条
  shownIn(g)  // byId(shown[g])
}
```

**`activeId` 和 `shown[activeGroup]` 永远相等** —— 这是新的核心不变量。
所有「切到某个标签」的写入收口到一个方法：

```ts
show(id: number) {
  const t = this.byId(id)!;
  this.shown[t.group] = id;   // 这一组显示它
  this.activeId = id;         // 它所在的组成为焦点组
}
```

现有的 `tabs.activeId = id` 写入点（App 的 `onSelect`、tabflow 的 `openPath`、persist 的恢复、
`remove` 的回退）全部改成 `show(id)`。`activeId` 保留为 `$state` 是因为读它的地方太多；
但它变成**只读约定** —— 直接赋值会绕过 `shown`，自检会在下一个转换点报出来。

其他新操作：

| 方法 | 做什么 | 边界 |
|---|---|---|
| `splitRight(id)` | 从单栏变双栏：`shown` 变两格，把 `id` 挪到组 1 并显示它，焦点到组 1 | 已经分屏时等于 `moveToGroup(id, 1)`。**`id` 所在组只有它一个时拒绝**（挪走之后原组空了会立刻收起，等于白做；IDEA / VS Code 这时是把同一个文件开两份，而这轮不做双实例）—— 菜单项灰掉，方法本身返回 false |
| `moveToGroup(id, g)` | 改 `t.group`，落在目标组末尾（钉住的落在钉住区末尾）；原组若正显示它，原组切到列表邻居 | 目标组不存在 → 等于 `splitRight` |
| `focusGroup(g)` | `activeId = shown[g]` | 点进非焦点组的编辑器 / 标签条时调 |
| `unsplit()` | 把组 1 的标签全部并回组 0（追加在末尾），`shown` 收成一格，焦点回组 0 正在显示的 | 菜单「合并分屏」 |

`remove(id)` 改动：活动标签回退**限于同组**（原来是整表的邻居）。回退之后这组空了 →
`shown` 去掉这一格、另一组的标签 `group` 归 0（如果空的是组 0，组 1 的标签全体改 0）、
焦点落到剩下那组正在显示的。

`add()` 不变签名，`group` 由调用方填 —— `openPath` 填 `activeGroup`。
`at`（顶掉预览标签落在同一格）是列表下标，跨组也成立：预览标签按组算（见 3.4）。

### 3.3 `docs`：「活着的编辑器」从一个变成一组

```ts
#live: Map<string, () => string>          // path → 读实时文本
#viewProbe / #wordProbe 同样改成 Map
```

认领规则原来是「新实例先挂、旧实例后卸，旧实例交回的 null 不能抹掉新实例」，用 path 判。
改成 Map 之后这条规则自然成立：`delete(path)` 只删自己那一份。
`liveText(t)` 查 `#live.get(t.path)`，签名不变。`tabs.livePath` → `livePaths: Set<string>`，
`audit` 传的 `liveId` 变成 `liveIds: Set<number>`。

**一个文件只在一个组里**在这里是硬前提：Map 按 path 键，两组同一个 path 会互相覆盖。
将来做「同一文件双实例」时这里要换成按标签 id 键 —— 那时 `TabState.path` 也不再唯一。

### 3.4 预览、钉住、拖放：按组

- 预览标签：**每组最多一个**。`tabs.preview` 改成 `previewIn(g)`；`openPath` 顶掉的是焦点组的预览。
- 钉住排最左：`setPinned` 的挪位限于同组。
- 关闭其他 / 右侧 / 全部：限于同组。「关闭所有标签」（菜单）仍是全部。

### 3.5 会话快照 —— **不升 VERSION**

issue 里写的是「这次真的不兼容，要升」。摸了 `session.ts` 之后我认为不必，
理由是这个文件自己定的规矩：*老值到新值有唯一且正确的对应，就不该整份丢掉*。

```ts
interface TabSnap {
  …
  /** 在右边那组。只存 1，不存 0 —— 老快照没有这个字段，「没有」= 左组 */
  group?: 1;
  /** 这一组正在显示的那个。每组恰好一个；老快照没有 → 左组的 shown 取 `active` */
  shown?: true;
}
interface Layout {
  …
  /** 分隔线位置，左组占的比例。可选，老快照没有 → 0.5 */
  splitRatio?: number;   // 夹在 0.2–0.8
}
```

`Session.active` 语义不变（焦点标签的下标）。解析时：
- 没有任何 `group: 1` → 单栏，`shown = [active]`。
- 有 → 双栏。每组的 `shown` 取该组第一个带 `shown: true` 的；没有就取该组第一个。
  `active` 指向的那个必须是它所在组的 shown —— 不是的话以 `active` 为准（它更重要：那是光标所在）。
- 右组一个标签都没恢复出来（文件都删了）→ 自动收成单栏，同运行时的规则。

`withoutTabs`（切项目时滤掉草稿）照旧按下标重算 `active`，`group` / `shown` 是标签自己的字段，
滤掉几行不影响。但要补一条：滤完某组的 shown 没了 → 该组第一个补上。

`serialize`：`group === 1` 时写 `group: 1`；`id === shown[t.group]` 时写 `shown: true`。

### 3.6 不变量（`invariant.ts`）

改：
- 「有标签但没有活动标签」→ 不变。
- 「预览超过一个」→ 每组各算。
- 「钉住的都在左边」→ 每组各算。

新增：
- `shown.length` 是 1 或 2；`split` 时每组至少一个标签（空组必须已被收起）。
- 每组的 `shown[g]` 指向一个 `group === g` 的标签。
- `activeId === shown[active.group]`。

`tabFaults` 不变 —— 单个标签的判据和它在哪组无关。

## 4. 渲染层

### 4.1 `App.svelte`

```svelte
<div class="editor-island">
  {#each tabs.shown as _, g (g)}
    <div class="group" class:focused={g === tabs.activeGroup} style:flex-basis=…>
      {#if tabs.inGroup(g).length > 0}
        <Tabs tabs={tabs.inGroup(g)} activeId={tabs.shown[g]} onSelect={(id) => tabs.show(id)} … />
      {/if}
      <Content tab={tabs.shownIn(g)} focused={g === tabs.activeGroup} … />
    </div>
    {#if g === 0 && tabs.split}<div class="splitter" …/>{/if}
  {/each}
  {#if confirms.comp}…{/if}   <!-- 横幅还是浮在整个岛上，不按组 -->
</div>
```

标签条的 `onSelect` 从「`activeId = id`」变成 `tabs.show(id)`（3.2）。
分隔线复用侧边栏 / 面板那套拖拽（`layout.resizing`），写 `layout.splitRatio`。

### 4.2 `Content.svelte`：从读全局到收 props

现在 53 处 `tabs.active` → 全部改成 prop `tab`。这是这一轮**行数最多**但最机械的改动，
每一处都是 `tabs.active` → `tab`、`tabs.active!.dirty = …` → `tab.dirty = …`。
`{#key tabs.active.id}` → `{#key tab.id}`。

多的一个 prop `focused`：
- `autofocus={focused && !tab.preview}` —— 非焦点组挂载时不抢光标。
- `docs.focusTick` 只对焦点组的编辑器生效（Editor 里已经是「挂载对齐、之后响应」，
  Content 传 `focusTick={focused ? docs.focusTick : 0}` 就够）。
- 空态卡片只在**单栏且没有标签**时出现；分屏时一组不可能为空（3.2）。

Content 上新增：`onfocusin` → `tabs.focusGroup(group)`。用 `focusin` 而不是 CM6 的
focus 事件：日志视图 / 差异视图也在组里，它们不是 CM6。

### 4.3 编辑器实例与 `docs` 的接线

Editor 的 `onLive / onView / onWordProbe / onStash` 全按 path 回调，3.3 的 Map 直接接得上。
`{#key tab.id}` 换代时的 stash 时序和单栏一模一样 —— 每组独立换代，互不干扰。

**Content 里那两段取 git 基线 / 注解的 effect**（`headText` / `blame`）现在读 `tabs.active`，
改成读 `tab`：两组各自取各自的基线。这里有一个之前没有的开销：`git.status` 一刷，
两个 effect 同时各发一次 `git_head_text` —— 和现在「切标签发一次」是一个量级，不用管。

### 4.4 状态栏、面包屑、大纲、Git 面板

一个字不改。它们读 `tabs.active`，而 `tabs.active` 就是焦点组正在显示的。

### 4.5 内存

第二个 CM6 实例的开销**没有单独量过** —— `app.log` 的预算行只有 `self=34MB editors=1` 这种
整机读数（ARCHITECTURE.md §预算）。预算是常驻 < 200 MB（JOURNAL 2026-09 放宽的那次），
两个编辑器不可能碰到它，但这轮结束要拿 `scripts/budget.sh` 比一次 `editors=1` 和 `editors=2`
的 `self`，把「一个编辑器多少」这个数记进 ARCHITECTURE。`writeBudgetLine` 数的是 `.cm-editor`
的 DOM 个数，分屏自动算成 2，不用改；但趋势表按「标签数」分组，分屏的读数会落进同一格 ——
下次看趋势时记得（写进 budget.sh 的注释）。

## 5. 命令与键位

`keymap.ts` 加四条（`menu.rs` 同步，`menu_sync.rs` 卡着）：

| id | label | accel | 组 | 灰掉的条件 |
|---|---|---|---|---|
| `split-right` | 向右分屏 | `⌘\` | 视图 | 焦点组不到两个标签 |
| `unsplit` | 合并分屏 | — | 视图 | 没分屏 |
| `focus-other-group` | 切到另一组 | `⌥Tab` | 视图 | 没分屏 |
| `move-to-other-group` | 移到另一组 | `⌃⌘→` | 视图 | 焦点组不到两个标签（没分屏时它就是「向右分屏」；分屏时移走最后一个 = 合并，也没意义） |

键位取舍：`⌘\` 是 VS Code 的分屏，macOS 上没被系统占；`⌥Tab` 是 IDEA 的
Goto Next Splitter；`⌃⌘→` 是 VS Code 的 Move Editor into Next Group。
现有 `⌥⌘←/→` 是导航前进后退，不冲突。CM6 这边（grep 了 `@codemirror/commands` 的 dist）：
`Shift-Mod-\`（跳到配对括号）和 `Mod-Alt-\`（缩进选区）是占着的，**裸 `Mod-\` 没有**，
`Alt-Tab` 没有。所以分屏不能用 ⇧⌘\ 那种变体，就是 ⌘\。

标签右键菜单加一项：「向右分屏打开」（单栏时）/「移到另一组」（分屏时）。只有一个标签的组里**不出现**而不是灰掉 —— `Tabs.svelte` 里的规矩是「不适用的项直接不出现」（「关闭其他」也是这么做的），菜单栏那条才灰。
文件树 / ⌘P 不加「在右侧打开」—— IDEA 也没有，想开在右边就先切过去再开。

`syncMenuState` 现在传四个 bool；要多传 `split`，Rust 侧 `menu.rs` 灰掉对应项。
**改 IPC 签名 → `mock-ipc.ts` 同步。**

## 6. 桩与验证

- `mock-ipc.ts`：无新命令；`sync_menu_state` 多一个参数。
- 状态层测试（`tests/state-tabflow.state.test.ts` 新增一节）：
  分屏 → 开文件落焦点组 → 关到空自动收起 → 移动标签 → 焦点跟随；
  `remove` 回退限同组；预览按组。
- `tests/session.test.ts`：老快照（无 `group`）解析成单栏；双栏来回；右组全丢 → 收成单栏；
  `withoutTabs` 滤掉某组的 shown 后补位。
- `tests/invariant.test.ts`：新增三条不变量各一个红例。
- 每条新测试按 AGENTS.md 那条：改坏被测代码跑一次确认红。
- 浏览器：`pnpm dev` 里 ⌘\ 分屏、点两边切焦点看状态栏跟不跟、⌘W 关到空收起、刷新恢复。
- `pnpm app:bundle` 收尾。入口包：新代码都在入口（tabs / Content / App）。**实测 ③ 做完 +5.5 KB**（136,948 → 142,472，估的 1.5–2.5 KB 少算了一倍多：tabs.svelte.ts 一个文件就 +2 KB，App / Content / session / persist / invariant 各 +0.5–1.3），已经过了 138 KiB 告警线 —— ④ 之后这轮内加一步 ⑤ 瘦身，把 `tabflow` / `docs` 里「有动作才跑」的方法拆去 `*-ops.ts`（frontend.md 2026-09-21 那段）。

## 7. 分步落地（四条提交）

| 步 | 内容 | 产出可验证的东西 |
|---|---|---|
| ① 状态层 | `TabState.group`、`Tabs.shown / show / splitRight / moveToGroup / unsplit / remove 收组`、`docs` 的 Map、不变量、状态测试 | 测试全绿；界面**看不出任何变化**（只有一组） |
| ② 会话 | `TabSnap.group / shown`、`Layout.splitRatio`、`persist` 快照与恢复、session 测试 | 测试全绿；老快照照常恢复 |
| ③ 渲染 | **先出设计图**（分屏后的岛：两条标签条、分隔线、焦点组怎么标、非焦点组的标签条怎么弱化），验收过再写代码。然后 `Content` 改 props、App 的 `{#each shown}`、分隔线、`focusin`、`Tabs` 按组、右键菜单两项 | 设计图；浏览器里能分屏能合并能拖 |
| ④ 命令 | keymap 四条、`menu.rs`、`syncMenuState` 多一参、桩、灰掉规则 | ⌘\ / ⌥Tab 在 .app 里可用；`menu_sync` 绿 |

①② 先做的原因：它们能在没有任何界面的情况下被测试卡住，而且做完之后界面照旧 ——
如果 ③ 做到一半要停，仓库仍是能发版的状态。

## 8. 明确不做（这一轮）

- 同一文件在两组里各开一份（要共享 doc）
- 上下分屏、三组、嵌套
- 拖标签到另一组（拖放本来就没做；右键「移到另一侧」够用）
- 分屏时的空态（不存在：空组自动收起）
- 差异 / 合并视图的「左右各一份」—— 它们本来就是一个标签，可以整个搬到另一组
