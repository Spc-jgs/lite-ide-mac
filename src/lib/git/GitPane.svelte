<script lang="ts">
  import Icon from "../shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import type { GitEntry, GitStatus } from "../ipc/commands";
  import { groupByDir } from "./group";
  import { readPref, writePref } from "../state/prefs";

  let {
    status,
    busy,
    onOpenDiff,
    onStage,
    onUnstage,
    onDiscard,
    onCommit,
    onRefresh,
    stashCount = 0,
    onStash,
    onUnstash,
    onOpenLog,
    ahead = 0,
    behind = 0,
  }: {
    status: GitStatus | null;
    busy: boolean;
    onOpenDiff: (e: GitEntry, staged: boolean) => void;
    onStage: (paths: string[]) => void;
    onUnstage: (paths: string[]) => void;
    /** 不可撤销，由上层弹确认条 */
    onDiscard: (entries: GitEntry[]) => void;
    /** `push` = 提交完接着推（IDEA 的 Commit and Push…）：提交成功才推，推之前照常确认 */
    onCommit: (message: string, amend: boolean, push: boolean) => void;
    onRefresh: () => void;
    /**
     * stash（issue #33 ⑪）。入口在面板头的 ⋯ 里（和「全部丢弃」一样是
     * 「把改动从工作区拿走」那一档），取回的入口除了 ⋯ 还有干净时的空态。
     */
    stashCount?: number;
    onStash?: () => void;
    onUnstash?: () => void;
    /** 打开提交历史。工作区干净时那是唯一还能做的事，所以进了空态 */
    onOpenLog: () => void;
    /** 只给空态那句「和 origin/x 比落后 N 个」用；拉取 / 推送的入口在标题栏（M9） */
    ahead?: number;
    behind?: number;
  } = $props();

  let message = $state("");
  let amend = $state(false);
  /**
   * 提交框展不展开。
   *
   * **有改动就直接摊开** —— 侧边栏切到 Git 页的时候，人十有八九就是要提交，
   * 而原来它折成一行「写提交信息…」，要先点一下。IDEA 的提交工具窗也是
   * 直接摊开的。
   *
   * 干净时才收起：那时那个空输入框加按钮占着约 110px，还摆出一副
   * 「可以提交」的样子，而「工作区干净」这条真正的信息被顶到空白下面。
   */
  let composing = $state(false);


  let staged = $derived(status?.entries.filter((e) => e.staged) ?? []);
  let unstaged = $derived(status?.entries.filter((e) => e.unstaged) ?? []);
  let conflicts = $derived(status?.entries.filter((e) => e.conflicted) ?? []);
  /** 空仓库时 amend 无意义（没有上一条可改） */
  let canAmend = $derived(!!status && !status.unborn);
  /*
   * 有未解决的冲突时一律不许提交。git 自己也会拒绝，但等它报错太晚了 ——
   * 用户已经把提交信息敲完了。按钮直接灰掉，旁边说明原因。
   */
  let canCommit = $derived(
    conflicts.length === 0 && (staged.length > 0 || (amend && canAmend)),
  );

  /** 状态字符 → 显示用的单字母 + 语义类名 */
  function mark(e: GitEntry, side: "index" | "work"): { ch: string; cls: string } {
    if (e.conflicted) return { ch: "!", cls: "conflict" };
    if (e.untracked) return { ch: "?", cls: "untracked" };
    const c = side === "index" ? e.index : e.work;
    switch (c) {
      case "M": return { ch: "M", cls: "modified" };
      case "A": return { ch: "A", cls: "added" };
      case "D": return { ch: "D", cls: "deleted" };
      case "R": return { ch: "R", cls: "renamed" };
      case "C": return { ch: "C", cls: "renamed" };
      default: return { ch: "·", cls: "modified" };
    }
  }

  const baseName = (p: string) => {
    const t = p.endsWith("/") ? p.slice(0, -1) : p;
    return t.slice(t.lastIndexOf("/") + 1) + (p.endsWith("/") ? "/" : "");
  };
  const dirName = (p: string) => {
    const t = p.endsWith("/") ? p.slice(0, -1) : p;
    const i = t.lastIndexOf("/");
    return i < 0 ? "" : t.slice(0, i);
  };

  function onKey(e: KeyboardEvent) {
    // ⌘↵ 提交，与大多数 Git 客户端一致
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canCommit) {
      e.preventDefault();
      doCommit();
    }
  }

  /**
   * 按目录分组 / 平铺（issue #33 ⑮）。偏好存 localStorage，默认平铺 ——
   * 二十个以内平铺一眼扫得完，多了再切。折叠状态不存：那是这一刻的事。
   */
  let grouped = $state(readPref("git-grouped", false));
  let collapsed = $state(new Set<string>());
  function toggleGrouped() {
    grouped = !grouped;
    writePref("git-grouped", grouped);
  }
  function toggleDir(key: string) {
    const next = new Set(collapsed);
    if (next.has(key)) next.delete(key);
    else next.add(key);
    collapsed = next;
  }

  /**
   * 面板头的 ⋯（M9）：面板级的事 —— 分组方式、stash。原来它们挂在「改动」分组头上，
   * 而分组头在侧边栏 160px 时会折成两行；且只有已暂存时「改动」段不在，⋯ 还得
   * 挪到「已暂存」段上去 —— 同一个菜单在两个地方长，位置跟着数据跳。
   * 分组头上只留和**这一段**有关的：全部暂存 / 全部取消 / 全部丢弃。
   */
  let menu = $state<{ x: number; y: number } | null>(null);
  let menuItems = $derived.by((): MenuItem[] => [
    { label: grouped ? "平铺显示" : "按目录分组", run: toggleGrouped },
    // 收进 stash 和丢弃是同一档 ——「把改动从工作区拿走」，只是一个还能拿回来
    { label: "收进 stash", sep: true, run: () => onStash?.(), disabled: !status || status.entries.length === 0 },
    ...(stashCount > 0 ? [{ label: `取回 stash (${stashCount})`, run: () => onUnstash?.() }] : []),
  ]);
  function openMenu(e: MouseEvent) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: r.right - 8, y: r.bottom + 2 };
  }

  /**
   * 「改动」段的 ⋯ 只装「全部丢弃」。它原来和「全部暂存」并排、同样大小、同样颜色，
   * 中间隔 2px —— 而一个**不可撤销**，一个随手可逆。误点的代价不该只隔着 2px。
   */
  let wmenu = $state<{ x: number; y: number } | null>(null);
  let wmenuItems = $derived.by((): MenuItem[] => [
    { label: "全部丢弃…", danger: true, run: () => onDiscard(unstaged) },
  ]);
  function openWorkMenu(e: MouseEvent) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    wmenu = { x: r.right - 8, y: r.bottom + 2 };
  }

  function doCommit(push = false) {
    if (!canCommit) return;
    onCommit(message, amend, push);
    message = "";
    amend = false;
    // 提交完就收回去 —— 刚提交完通常没有下一条要写
    composing = false;
  }

  /**
   * 「提交」右边那个 ▾（issue #33 ⑩）：提交并推送。
   *
   * 照 IDEA 的提交按钮 —— 主动作是提交，下拉里是「提交并推送…」。不做成两个
   * 平级按钮：九成提交不需要马上推，而两个同色按钮并排会让人先想「哪个是哪个」。
   * 推之前照常走推送确认条（列出要推的提交），所以这里不是「不看一眼就推」。
   */
  let cmenu = $state<{ x: number; y: number } | null>(null);
  const cmenuItems: MenuItem[] = [{ label: "提交并推送…", run: () => doCommit(true) }];
  function openCommitMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    cmenu = { x: r.right - 8, y: r.bottom + 2 };
  }
</script>

<!--
  已暂存 / 改动 两段的行是同一个形状，只差按钮：`side` = 看哪一栏的状态（index / work）。
  分组模式下目录头一行、文件只写名字（目录已经在头上了）；平铺时名字后跟灰色目录。
-->
{#snippet fileRow(e: GitEntry, side: "index" | "work")}
  <div class="frow-wrap" class:in-group={grouped}>
    <button class="frow" onclick={() => onOpenDiff(e, side === "index")} title={e.path}>
      <span class="m {mark(e, side).cls}">{mark(e, side).ch}</span>
      <span class="fname" class:gone={(side === "index" ? e.index : e.work) === "D"}>{baseName(e.path)}</span>
      {#if !grouped}<span class="fdir">{dirName(e.path)}</span>{/if}
    </button>
    {#if side === "index"}
      <button class="rowact" onclick={() => onUnstage([e.path])} title="取消暂存" aria-label="取消暂存">−</button>
    {:else}
      <button class="rowact" onclick={() => onDiscard([e])} title="丢弃改动" aria-label="丢弃改动">↺</button>
      <button class="rowact" onclick={() => onStage([e.path])} title="暂存" aria-label="暂存">＋</button>
    {/if}
  </div>
{/snippet}

{#snippet fileList(list: GitEntry[], side: "index" | "work")}
  {#if grouped}
    {#each groupByDir(list) as g (g.dir)}
      {@const key = `${side}:${g.dir}`}
      {#if g.dir !== ""}
        <button
          class="gdir"
          class:closed={collapsed.has(key)}
          onclick={() => toggleDir(key)}
          title={g.dir}
          aria-expanded={!collapsed.has(key)}
        >
          <span class="gcaret"><Icon name="chevron-right" size={10} /></span>
          <span class="gname">{g.dir}</span>
          <span class="gcnt">{g.items.length}</span>
        </button>
      {/if}
      {#if g.dir === "" || !collapsed.has(key)}
        {#each g.items as e (e.path)}
          {@render fileRow(e, side)}
        {/each}
      {/if}
    {/each}
  {:else}
    {#each list as e (e.path)}
      {@render fileRow(e, side)}
    {/each}
  {/if}
{/snippet}


<div class="git">
  {#if !status}
    <div class="hint">不是 Git 仓库</div>
  {:else}
    <!--
      面板头（M9）：和文件树的头同一形状 —— 标题 + 计数 + 两个 24px 工具按钮，38 高。
      原来这一行是分支名 + 同步胶囊，和标题栏的分支挂件是**同一个数据、同一个点击**，
      一屏印两遍（ui.md 第十一条：标题栏管「哪个分支」，只有它管）。
      同步胶囊搬去了标题栏（`TitleBar` 的 `SyncPill`），进度卡片跟着走 `RemoteBars`。
    -->
    <div class="head">
      <span class="title">改动</span>
      {#if status.entries.length > 0}<span class="hcnt">{status.entries.length}</span>{/if}
      {#if status.detached}<span class="tagx">游离</span>{/if}
      {#if status.unborn}<span class="tagx">尚无提交</span>{/if}
      <span class="gap"></span>
      <button class="hb" onclick={onRefresh} title="刷新状态" aria-label="刷新" class:spin={busy}>
        <Icon name="refresh" size={14} />
      </button>
      <button class="hb" onclick={openMenu} title="更多操作" aria-label="更多操作">
        <Icon name="more-v" size={14} />
      </button>
    </div>

    {#if status.entries.length === 0 && !composing && message.trim() === ""}
      <!--
        空态只出现一次。原来「工作区干净」印了两遍 —— 一遍在折叠的提交按钮里、
        一遍在列表的空态里 —— 而下面还剩三百多像素什么都没有。
        干净时唯一还能做的事是看历史，那就把它放出来。
      -->
      <div class="empty">
        <span class="emark"><Icon name="check" size={17} /></span>
        <span class="etitle">工作区干净</span>
        {#if behind || ahead}
          <span class="esub">
            和 <span class="mono">{status.upstream}</span> 比{#if behind}落后 {behind} 个{/if}{#if behind && ahead}、{/if}{#if ahead}领先 {ahead} 个{/if}提交
          </span>
        {:else if status.upstream}
          <span class="esub">和 <span class="mono">{status.upstream}</span> 一致</span>
        {/if}
        <button class="btn" onclick={onOpenLog}>看提交历史</button>
        {#if stashCount > 0}
          <button class="btn" onclick={() => onUnstash?.()}>取回 stash ({stashCount})</button>
        {/if}
      </div>
    {:else}
    <div class="commit">
      <textarea
        bind:value={message}
        onkeydown={onKey}
        placeholder={amend ? "改写上一条提交信息…" : "提交信息（⌘↵ 提交）"}
        rows="2"
      ></textarea>
      <!--
        冲突提示单独占一行，不挤在按钮那一排里。
        挤在一起时（侧边栏默认 240px）三样东西都放不下，于是每一样都
        在字中间断行 ——「改写上一/条」「先解决 1 处冲/突」「提交/(3)」。
        而且这句话正是那一刻最要紧的信息，值得一整行。
      -->
      {#if conflicts.length > 0}
        <p class="blocked">先解决 {conflicts.length} 处冲突</p>
      {/if}
      <div class="crow">
        <label class="amend" class:off={!canAmend}>
          <input type="checkbox" bind:checked={amend} disabled={!canAmend} />
          改写上一条
        </label>
        <span class="gap"></span>
        <div class="btn-split">
          <button class="btn primary" disabled={!canCommit} onclick={() => doCommit()}>
            提交 {staged.length > 0 ? `(${staged.length})` : ""}
          </button>
          <button
            class="btn primary"
            disabled={!canCommit}
            onclick={openCommitMenu}
            title="提交并推送…"
            aria-label="更多提交方式"
          ><Icon name="chevron-down" size={10} /></button>
        </div>
      </div>
    </div>
    {/if}

    <div class="list">
      {#if conflicts.length > 0}
        <div class="sec">
          <span class="sname">冲突中</span>
          <span class="cnt">{conflicts.length}</span>
        </div>
        {#each conflicts as e (e.path)}
          <div class="frow-wrap">
            <button class="frow" onclick={() => onOpenDiff(e, false)} title="{e.path}（点开解决冲突）">
              <span class="m conflict">!</span>
              <span class="fname">{baseName(e.path)}</span>
              <span class="fdir">{dirName(e.path)}</span>
            </button>
          </div>
        {/each}
      {/if}

      {#if staged.length > 0}
        <div class="sec">
          <span class="sname">已暂存</span>
          <span class="cnt">{staged.length}</span>
          <span class="gap"></span>
          <button class="btn sm quiet" onclick={() => onUnstage(staged.map((e) => e.path))}>全部取消</button>
        </div>
        {@render fileList(staged, "index")}
      {/if}

      {#if unstaged.length > 0}
        <div class="sec">
          <span class="sname">未暂存</span>
          <span class="cnt">{unstaged.length}</span>
          <span class="gap"></span>
          <button class="btn sm quiet" onclick={() => onStage(unstaged.map((e) => e.path))}>全部暂存</button>
          <button class="btn sm quiet more" onclick={openWorkMenu} title="更多操作" aria-label="更多操作">⋯</button>
        </div>
        {@render fileList(unstaged, "work")}
      {/if}

      <!--
        这里原来还有一句「工作区干净」。它和折叠起来的提交按钮里那句
        一模一样，同屏印了两遍 —— 空态现在只由上面那块 .empty 负责。
      -->
      <!--
        条数写实际值，不写死 5000。`truncated` 现在有两个来源：条目数撞上
        `MAX_ENTRIES`（5000），或者 `git status` 的 stdout 撞上字节上限
        （2026-09-07 给它设的闸）。后者截在哪儿看运气，写死 5000 就是
        一句和眼前列表对不上的话。
      -->
      {#if status.truncated}
        <div class="hint">改动过多，只列出了前 {status.entries.length} 条</div>
      {/if}
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="改动面板的操作" items={menuItems} onclose={() => (menu = null)} />
{/if}
{#if wmenu}
  <ContextMenu x={wmenu.x} y={wmenu.y} label="改动的操作" items={wmenuItems} onclose={() => (wmenu = null)} />
{/if}
{#if cmenu}
  <ContextMenu x={cmenu.x} y={cmenu.y} label="提交方式" items={cmenuItems} onclose={() => (cmenu = null)} />
{/if}

<style>
  .git {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    /* 不画右边线（M8）：右边是岛的圆角边 */
    overflow: hidden;
  }
  .head .gap, .sec .gap, .crow .gap { flex: 1; }
  /* 面板头：和文件树的 `.head` 同一套（38 高、11px 大写标题、24px 工具按钮） */
  .head {
    flex: none;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 0 4px 0 10px;
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .hcnt {
    font-family: var(--code-font);
    font-size: 10px;
    letter-spacing: 0;
    background: var(--selected);
    border-radius: var(--r-sm);
    padding: 0 5px;
    color: var(--text-dim);
  }
  .head .hb {
    flex: none;
    display: grid;
    place-content: center;
    width: 24px;
    height: 24px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    cursor: default;
  }
  .head .hb:hover { background: var(--hover); color: var(--text); }
  .head .hb:active { background: var(--pressed); }
  .head .hb:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .head .hb.spin { color: var(--accent); }
  .tagx {
    flex: none;
    font-size: 10px;
    letter-spacing: 0;
    text-transform: none;
    color: var(--lvl-warn);
    border: 1px solid var(--lvl-warn);
    border-radius: var(--r-sm);
    padding: 0 4px;
    opacity: 0.8;
  }

  .commit { flex: none; padding: 8px 10px; border-bottom: 1px solid var(--border-soft); }
  /*
   * 空态。原来「工作区干净」印了两遍（折叠的提交按钮里一遍、列表空态里一遍），
   * 下面还剩三百多像素空白。现在只印一遍，并且把那片空白用起来：
   * 说清「干净」之后紧接着的那件事是什么。
   */
  .empty {
    flex: 1;
    min-height: 0;
    display: grid;
    place-content: center;
    justify-items: center;
    gap: 9px;
    padding: 0 20px;
    text-align: center;
    user-select: none;
  }
  .emark {
    display: grid;
    place-content: center;
    width: 34px;
    height: 34px;
    border-radius: 50%;
    background: var(--hover);
    color: var(--git-added);
  }
  .etitle { color: var(--text-dim); font-size: 12.5px; }
  .esub { color: var(--text-faint); font-size: 11px; line-height: 1.6; }
  .esub .mono { font-family: var(--code-font); }
  .commit textarea {
    width: 100%;
    resize: vertical;
    /*
     * 抬起面：底比面板亮，边是 15% 的白。这两样加上大圆角就是那个
     * "一块摞上去的板"的观感 —— 输入框在 ChatGPT 里正是这么处理的
     * （#212121 + #ffffff26 + 28px 圆角）。
     */
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 12px;
    line-height: 1.5;
    padding: 8px 10px;
  }
  .commit textarea:focus { outline: none; border-color: var(--accent); }
  .commit textarea::placeholder { color: var(--text-faint); }
  .crow { display: flex; align-items: center; gap: 8px; margin-top: 6px; }
  .amend {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 11px;
    color: var(--text-faint);
    user-select: none;
    /* 侧边栏能拖到 160px，不锁住就会断成「改写上一/条」 */
    white-space: nowrap;
  }
  .amend.off { opacity: 0.4; }
  .amend input { margin: 0; accent-color: var(--accent); }
  /* 冲突三角走 currentColor，颜色由这层给 —— 图标自己不带颜色 */
  .conflict-mark { display: flex; color: var(--lvl-error); }
  .blocked {
    margin: 6px 0 0;
    font-size: 11px;
    color: var(--lvl-warn);
  }

  /* 横向 6px 是给行的圆角块留的余地，和文件树同一套 */
  .list { flex: 1; overflow-y: auto; padding: 0 6px 8px; }
  .sec {
    /*
     * 吸顶。改动最多有 5000 条，滚到一半就不知道现在看的是
     * 「已暂存」还是「改动」了。
     * 底色必须跟着外壳走：外壳是透光的，这里填不透明色会在滚动时
     * 拖出一条实心带。
     */
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--panel-bg);
    backdrop-filter: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 8px 2px 4px 6px;
    font-size: 10.5px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
    /*
     * 侧边栏能拖到 160px，那时「改动 4 全部丢弃 全部暂存」放不下。
     * 允许整行折，但每一块自己不许断 —— 否则会断成「改/动」「全部/丢弃」，
     * 一个两字的词竖着排下来根本认不出。折行之后是：
     *
     *     改动  4
     *       全部丢弃  全部暂存
     */
    flex-wrap: wrap;
  }
  .sec > * { white-space: nowrap; }
  .sec .cnt {
    font-family: var(--code-font);
    background: var(--selected);
    border-radius: var(--r-sm);
    padding: 0 5px;
    font-size: 10px;
  }
  /* 分组头里的按钮再小一号，字色不抢眼；⋯ 那个只装「全部丢弃」 */
  .sec .btn.sm { height: 18px; padding: 0 6px; font-size: 10.5px; letter-spacing: 0; text-transform: none; }
  .sec .btn.more { padding: 0 5px; font-size: 12px; }

  /* 行操作按钮平时不占视觉，hover 才浮出来 —— 列表安静，动作随手可及 */
  /* 悬停是内缩圆角块，和文件树同一套 —— 两边挨着，做法不一样一眼看得出来 */
  .frow-wrap { display: flex; align-items: center; border-radius: var(--r-sm); }
  /* 分组时文件行往里缩一格，让目录头看起来是它们的父 */
  .frow-wrap.in-group { margin-left: 12px; }
  .gdir {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    min-width: 0;
    height: 22px;
    padding: 0 4px;
    border: none;
    background: none;
    color: var(--text-dim);
    font-size: 12px;
    text-align: left;
    border-radius: var(--r-sm);
    cursor: pointer;
  }
  .gdir:hover { background: var(--hover); }
  .gdir:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .gcaret { display: inline-flex; color: var(--text-faint); transition: transform 0.12s; transform: rotate(90deg); }
  .gdir.closed .gcaret { transform: none; }
  .gname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--code-font); }
  .gcnt { margin-left: auto; color: var(--text-faint); font-size: 11px; }
  .frow-wrap:hover { background: var(--hover); }
  .frow-wrap .rowact { opacity: 0; }
  .frow-wrap:hover .rowact { opacity: 1; }
  .rowact {
    flex: none;
    width: 20px;
    height: 22px;
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 12px;
    cursor: default;
    border-radius: var(--r-sm);
  }
  .rowact:hover { background: var(--hover); color: var(--text); }
  .rowact:focus-visible { opacity: 1; outline: 1px solid var(--accent); }

  .frow {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 4px 0 6px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .frow:disabled { cursor: default; }
  .frow:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .fname { flex: none; overflow: hidden; text-overflow: ellipsis; max-width: 60%; }
  .fname.gone { text-decoration: line-through; opacity: 0.65; }
  .fdir {
    flex: 1;
    min-width: 0;
    color: var(--text-faint);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
    /* 路径太长时砍前面而不是后面 —— 结尾的目录名才是有辨识度的那截 */
    direction: rtl;
    text-align: left;
  }
  .m {
    flex: none;
    width: 12px;
    text-align: center;
    font-family: var(--code-font);
    font-size: 11px;
    font-weight: 600;
  }
  .m.modified { color: var(--git-modified); }
  .m.added { color: var(--git-added); }
  .m.deleted { color: var(--git-deleted); }
  .m.untracked { color: var(--git-untracked); }
  .m.renamed { color: var(--git-renamed); }
  .m.conflict { color: var(--git-conflict); }

  .hint { padding: 14px 12px; color: var(--text-faint); font-size: 12px; }
</style>
