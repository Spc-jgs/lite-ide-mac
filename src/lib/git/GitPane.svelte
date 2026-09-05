<script lang="ts">
  import Icon from "../shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import type { GitEntry, GitStatus } from "../ipc/commands";

  let {
    status,
    busy,
    onOpenDiff,
    onStage,
    onUnstage,
    onDiscard,
    onCommit,
    onRefresh,
    onOpenBranches,
    onOpenLog,
    ahead = 0,
    behind = 0,
    onSync,
    syncing = null,
    onCancelSync = null,
  }: {
    status: GitStatus | null;
    busy: boolean;
    onOpenDiff: (e: GitEntry, staged: boolean) => void;
    onStage: (paths: string[]) => void;
    onUnstage: (paths: string[]) => void;
    /** 不可撤销，由上层弹确认条 */
    onDiscard: (entries: GitEntry[]) => void;
    onCommit: (message: string, amend: boolean) => void;
    onRefresh: () => void;
    /**
     * 打开分支面板。
     *
     * IDEA 里分支挂件**就是**分支操作的入口，而这里原来那行分支名只是块
     * 只读文字，真正的入口藏在标题栏右上角 —— 人在 Git 栏里想切分支，
     * 眼睛落的就是这一行。
     */
    onOpenBranches: () => void;
    /** 打开提交历史。工作区干净时那是唯一还能做的事，所以进了空态 */
    onOpenLog: () => void;
    ahead?: number;
    behind?: number;
    /**
     * 拉取 / 推送 / 抓取。**由上层做**，因为它们要开进度、要能取消、
     * 失败时要弹的东西（认证提示、被拒提示）横跨整个内容区。
     * 这里只负责把「现在该显示哪个动作」算出来。
     */
    onSync?: (what: "pull" | "push" | "fetch") => void;
    /** 正在跑的远程操作。null 表示没有 */
    syncing?: { what: "pull" | "push" | "fetch"; phase: string; percent: number | null } | null;
    /** 取消当前操作。push 进行中不给（状态不确定），由上层决定传不传 */
    onCancelSync?: (() => void) | null;
  } = $props();

  /*
   * 分支行右边那个胶囊现在显示什么。
   *
   * v0.5.0 里它是个**不可点的**胶囊，理由写在那时的设计里：
   * 「点了没有下文比不显示更糟」。M7 就是来兑现那句话的。
   *
   * 四态照 IDEA 的分支挂件：同步了不显示（「一切正常」不需要占位置）、
   * 只落后给拉取、只领先给推送、**两边都有就不给一键动作** ——
   * 那时要先决定合并还是变基，替人选一个是越权。
   */
  let syncState = $derived.by(() => {
    if (!status?.upstream) return null;
    if (behind && ahead) return { kind: "diverged" as const };
    if (behind) return { kind: "pull" as const };
    if (ahead) return { kind: "push" as const };
    return null;
  });

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
    if (e.untracked) return { ch: e.isDir ? "?" : "?", cls: "untracked" };
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

  /*
   * 「全部丢弃」从常驻位置撤进菜单。
   *
   * 它原来和「全部暂存」并排、同样大小、同样颜色，中间隔 2px ——
   * 而一个**不可撤销**，一个随手可逆。误点的代价不该只隔着 2px。
   * （菜单里它带 `danger`，颜色也就跟着分开了。）
   */
  let menu = $state<{ x: number; y: number } | null>(null);

  let menuItems = $derived.by((): MenuItem[] => [
    { label: "全部暂存", run: () => onStage(unstaged.map((e) => e.path)) },
    { label: "全部丢弃…", danger: true, sep: true, run: () => onDiscard(unstaged) },
  ]);

  function openMenu(e: MouseEvent) {
    e.preventDefault();
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    menu = { x: r.right - 8, y: r.bottom + 2 };
  }

  function doCommit() {
    if (!canCommit) return;
    onCommit(message, amend);
    message = "";
    amend = false;
    // 提交完就收回去 —— 刚提交完通常没有下一条要写
    composing = false;
  }
</script>

<div class="git">
  {#if !status}
    <div class="hint">不是 Git 仓库</div>
  {:else}
    <!--
      分支行同时是标题行和入口。原来上面还压着一条只写着「GIT」的标题栏 ——
      30px，说的是侧边栏图标已经说过的事。刷新钮挪到这一行右端：
      它本来就是「这行数据新不新」的位置。
    -->
    <div class="branch">
      <button
        class="bbtn"
        onclick={onOpenBranches}
        title={status.upstream ? `跟踪 ${status.upstream} —— 点击切换分支 / 工作树` : "没有设置上游分支 —— 点击切换分支 / 工作树"}
      >
        <span class="bicon"><Icon name="git" size={13} /></span>
        <span class="bname">{status.branch || "（无分支）"}</span>
        <span class="caret" aria-hidden="true"><Icon name="chevron-down" size={10} /></span>
      </button>
      {#if status.detached}<span class="tagx">游离</span>{/if}
      {#if status.unborn}<span class="tagx">尚无提交</span>{/if}
      <span class="gap"></span>
      <!--
        改动条数和分支是同一类信息（「这个仓库现在什么状态」），
        放在一起就省掉了分组头里那个重复的计数。
      -->
      {#if syncing}
        <!-- 进行中：胶囊让位给「取消」，进度条长在下面一行 -->
        {#if onCancelSync}
          <button class="sync cancel" onclick={onCancelSync}>取消</button>
        {/if}
      {:else if syncState?.kind === "pull"}
        <!-- 胶囊带上动词才知道点了会发生什么 -->
        <button class="sync" onclick={() => onSync?.("pull")} title="抓取远程并合并到本地">
          ↓{behind} 拉取
        </button>
      {:else if syncState?.kind === "push"}
        <!-- 推送用 accent —— 它是唯一会改到别人东西的那个 -->
        <button class="sync push" onclick={() => onSync?.("push")} title="把本地提交推到远程">
          ↑{ahead} 推送
        </button>
      {:else if syncState?.kind === "diverged"}
        <button class="sync diverged" onclick={() => onSync?.("pull")} title="本地和远程分岔了，要先决定怎么合">
          ↑{ahead} ↓{behind} 已分岔
        </button>
      {:else if status.entries.length > 0}
        <span class="chgs">{status.entries.length} 处改动</span>
      {/if}
      <button class="act" onclick={onRefresh} title="刷新状态" aria-label="刷新" class:spin={busy}>
        <Icon name="refresh" size={13} />
      </button>
    </div>

    {#if syncing}
      <!--
        进度长在分支行下面，**不弹模态** —— 拉取的时候人还想接着看代码。
        百分比可能是 null（git 的措辞不是稳定接口），那时只显示一行文字。
      -->
      <div class="prog">
        <div class="pline">
          <span class="ptext">{syncing.phase}</span>
          {#if syncing.percent !== null}<span class="ppct">{syncing.percent}%</span>{/if}
        </div>
        <div class="pbar" class:indet={syncing.percent === null}>
          {#if syncing.percent !== null}
            <div class="pfill" style:width="{syncing.percent}%"></div>
          {/if}
        </div>
      </div>
    {/if}

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
        <button class="ebtn" onclick={onOpenLog}>看提交历史</button>
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
        <button class="primary" disabled={!canCommit} onclick={doCommit}>
          提交 {staged.length > 0 ? `(${staged.length})` : ""}
        </button>
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
          <button class="mini" onclick={() => onUnstage(staged.map((e) => e.path))}>全部取消</button>
        </div>
        {#each staged as e (e.path)}
          <div class="frow-wrap">
            <button class="frow" onclick={() => onOpenDiff(e, true)} title={e.path}>
              <span class="m {mark(e, 'index').cls}">{mark(e, "index").ch}</span>
              <span class="fname" class:gone={e.index === "D"}>{baseName(e.path)}</span>
              <span class="fdir">{dirName(e.path)}</span>
            </button>
            <button class="rowact" onclick={() => onUnstage([e.path])} title="取消暂存" aria-label="取消暂存">−</button>
          </div>
        {/each}
      {/if}

      {#if unstaged.length > 0}
        <div class="sec">
          <span class="sname">改动</span>
          <span class="cnt">{unstaged.length}</span>
          <span class="gap"></span>
          <button class="mini" onclick={() => onStage(unstaged.map((e) => e.path))}>全部暂存</button>
          <button class="mini more" onclick={openMenu} title="更多操作" aria-label="更多操作">⋯</button>
        </div>
        {#each unstaged as e (e.path)}
          <div class="frow-wrap">
            <button
              class="frow"
              onclick={() => onOpenDiff(e, false)}
              title={e.isDir ? `${e.path}（整个目录未跟踪）` : e.path}
              disabled={e.isDir}
            >
              <span class="m {mark(e, 'work').cls}">{mark(e, "work").ch}</span>
              <span class="fname" class:gone={e.work === "D"}>{baseName(e.path)}</span>
              <span class="fdir">{dirName(e.path)}</span>
            </button>
            <button class="rowact" onclick={() => onDiscard([e])} title="丢弃改动" aria-label="丢弃改动">↺</button>
            <button class="rowact" onclick={() => onStage([e.path])} title="暂存" aria-label="暂存">＋</button>
          </div>
        {/each}
      {/if}

      <!--
        这里原来还有一句「工作区干净」。它和折叠起来的提交按钮里那句
        一模一样，同屏印了两遍 —— 空态现在只由上面那块 .empty 负责。
      -->
      {#if status.truncated}
        <div class="hint">改动过多，只列出了前 5000 条</div>
      {/if}
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    label="改动的操作"
    items={menuItems}
    onclose={() => (menu = null)}
  />
{/if}

<style>
  .git {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .branch .gap, .sec .gap, .crow .gap { flex: 1; }
  .act {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    cursor: default;
  }
  .act:hover { background: var(--hover); color: var(--text); }

  /*
   * 分支行右端的同步胶囊。
   *
   * 尺寸和 `.chgs` 一致 —— 它们占的是同一个位置，切换时那一行不该跳。
   */
  .sync {
    flex: none;
    font-family: var(--code-font);
    font-size: 10.5px;
    padding: 1px 7px;
    border-radius: var(--r-sm);
    background: var(--selected);
    border: none;
    color: var(--text);
    cursor: default;
    white-space: nowrap;
  }
  .sync:hover { background: var(--pressed); }
  /* 推送是唯一会改到别人东西的动作，给它 accent */
  .sync.push { background: rgba(91, 141, 239, 0.22); color: #8fb4f5; }
  .sync.push:hover { background: rgba(91, 141, 239, 0.32); }
  /* 分岔要先做决定，用警告色但不是错误色 —— 它不是坏事 */
  .sync.diverged { background: rgba(214, 174, 88, 0.18); color: var(--lvl-warn); }
  .sync.diverged:hover { background: rgba(214, 174, 88, 0.28); }
  .sync.cancel { background: var(--hover); color: var(--text-dim); }
  .sync:focus-visible { outline: 1px solid var(--accent); outline-offset: 1px; }

  /* 进度：长在分支行下面，不弹模态 —— 拉取时人还想接着看代码 */
  .prog {
    flex: none;
    padding: 7px 10px 8px;
    border-bottom: 1px solid var(--border-soft);
  }
  .pline { display: flex; align-items: baseline; gap: 8px; margin-bottom: 5px; }
  .ptext {
    flex: 1;
    min-width: 0;
    font-size: 11.5px;
    color: var(--text-dim);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ppct { flex: none; font-family: var(--code-font); font-size: 11px; color: var(--text-faint); }
  .pbar { height: 3px; border-radius: 2px; background: var(--hover); overflow: hidden; }
  .pfill { height: 100%; background: var(--accent); transition: width 0.12s linear; }
  /*
   * 认不出百分比时走这条：一条来回跑的条，只说「还在动」。
   * **不能显示成 0%** —— 那是在报一个我们并不知道的数。
   */
  .pbar.indet::after {
    content: "";
    display: block;
    width: 34%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(300%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .pbar.indet::after { animation: none; width: 100%; opacity: 0.4; }
    .pfill { transition: none; }
  }
  .act.spin { color: var(--accent); }

  /*
   * 分支行既是标题行也是入口。上面原来还压着一条只写「GIT」的标题栏 ——
   * 30px，说的是侧边栏图标已经说过的事。240px 宽的侧边栏里，
   * 纵向每一格都值钱。
   */
  .branch {
    flex: none;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 34px;
    padding: 0 4px 0 6px;
    font-size: 12px;
    color: var(--text);
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
  }
  .bbtn {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    max-width: 100%;
    padding: 3px 6px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: inherit;
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .bbtn:hover { background: var(--hover); }
  .bbtn:active { background: var(--pressed); }
  .bbtn:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .bicon { flex: none; display: flex; color: var(--text-faint); }
  .bbtn:hover .bicon { color: var(--text-dim); }
  .caret { flex: none; display: flex; color: var(--text-faint); }
  .bname {
    font-family: var(--code-font);
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  /*
   * 改动条数和分支同属「这个仓库现在什么状态」，放一行。
   * 做成不可点的胶囊 —— 这个项目没有 pull / push（gitsvc 不起网络子进程），
   * 点了没有下文比不显示更糟。
   */
  .chgs {
    flex: none;
    font-family: var(--code-font);
    font-size: 10.5px;
    color: var(--text-dim);
    background: var(--selected);
    border-radius: var(--r-sm);
    padding: 1px 7px;
  }
  .tagx {
    flex: none;
    font-size: 10px;
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
  .ebtn {
    margin-top: 2px;
    padding: 4px 12px;
    background: var(--hover);
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 11.5px;
    cursor: default;
  }
  .ebtn:hover { background: var(--selected); color: var(--text); }
  .ebtn:focus-visible { outline: 1px solid var(--accent); outline-offset: 1px; }
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
  .primary {
    background: var(--accent);
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    color: #fff;
    font-size: 11.5px;
    padding: 3px 12px;
    cursor: default;
    white-space: nowrap;
  }
  .primary:disabled { background: transparent; border-color: var(--border); color: var(--text-faint); }
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
    border-radius: var(--r-md);
    padding: 0 5px;
    font-size: 10px;
  }
  .mini {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10.5px;
    padding: 1px 6px;
    border-radius: var(--r-sm);
    cursor: default;
    text-transform: none;
    letter-spacing: 0;
  }
  .mini:hover { background: var(--hover); color: var(--text); }
  /* 「⋯」里装的是不可撤销的那些（全部丢弃）—— 见 menuItems 上面那段 */
  .mini.more { padding: 1px 5px; font-size: 12px; line-height: 1; }

  /* 行操作按钮平时不占视觉，hover 才浮出来 —— 列表安静，动作随手可及 */
  /* 悬停是内缩圆角块，和文件树同一套 —— 两边挨着，做法不一样一眼看得出来 */
  .frow-wrap { display: flex; align-items: center; border-radius: var(--r-md); }
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
