<script lang="ts">
  /**
   * 内容区顶上的那几条确认横幅：外部改动冲突、大文件切编辑、错误说明、
   * 移除工作树、切分支被本地改动挡住、丢弃改动、关脏标签，以及远程操作的
   * 三条（分岔决策 / 推送确认 / 失败提示，那三条在 Git 那组懒加载的
   * `RemoteBars` 里，由 App 把加载到的组件传进来）。拉取 / 推送的**进度**不在这儿：
   * 2026-09-22 起后台任务的进度全归状态栏那一格（`state/progress.svelte.ts`），
   * 卡片只问问题和报错。
   *
   * 每一条读的都是各自 store 上的 `pending*`，按钮直接调 store 的方法。
   * 从 App.svelte 搬出来（issue #9 第 5b 步）—— 它们共用 `.confirm` 那套样式，
   * 分开搬就是样式写两份，所以等 git 那几条能出来了一起搬。
   *
   * ui.md 第十三条：不可逆的带 `danger`，选择题长得和普通确认一样，不是 err-banner。
   */
  // 只要类型：type import 不会把那个组件拽进入口包，它照旧跟着 Git 那组懒加载
  import type RemoteBars from "../git/RemoteBars.svelte";
  import { notify } from "../state/notify.svelte";
  import { layout } from "../state/layout.svelte";
  import { tabs } from "../state/tabs.svelte";
  import { docs } from "../state/docs.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { git } from "../state/git.svelte";
  import { branches } from "../state/branches.svelte";
  import { remote } from "../state/remote.svelte";
  import { overlay } from "../state/overlay.svelte";

  let stackEl = $state<HTMLElement | null>(null);

  /*
   * Esc = 最上面那张卡片的「取消 / 知道了」。macOS 的对话框 Esc 就是 Cancel；卡片虽然不是
   * 模态的，人看到一张问句的卡片，第一反应还是按 Esc 让它走。三条让位：别人先吃了 Esc 的
   * 不管（`defaultPrevented`：CM6 关查找面板、收多光标）；浮层开着的不管（它们自己的 Esc）；
   * 焦点在终端里的不管（Esc 是 vim 的键）。只按 `data-dismiss` 找 —— 那是「关掉这张、
   * 什么都不做」的按钮；有选择没取消的卡片（外部改过：保留我的 / 用磁盘上的）Esc 不替人选。
   */
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (overlay.quickOpen || overlay.outlineOpen || overlay.keysOpen || overlay.encOpen || overlay.gotoOpen || overlay.branchOpen) return;
    if (document.activeElement?.closest(".xterm")) return;
    const btn = stackEl?.querySelector<HTMLButtonElement>(".confirm [data-dismiss]");
    if (!btn) return;
    e.preventDefault();
    btn.click();
  }

  let {
    Bars,
  }: {
    /** `RemoteBars`，懒加载到了才有 */
    Bars: typeof RemoteBars | undefined;
  } = $props();
</script>

<!--
  M8：确认条不再是撑开内容区的通栏横条，而是**浮在内容岛顶上的卡片**。
  通栏横条有两个毛病：它把编辑器整体往下推一行（人正盯着的那行跳走了），
  贴着标签栏时两条 1px 边线叠在一起。卡片照浮层的规矩：不透明、投影抬起、不靠边线。
  `pointer-events: none` 的外层让卡片之外的编辑器照常可点。
-->
<svelte:window onkeydown={onKey} />

<div class="stack" class:below-tabs={tabs.list.length > 0} bind:this={stackEl}>
{#if tabs.active?.conflict}
  <div class="confirm warn">
    <span><b>{tabs.active.name}</b> 在编辑器外被改过，而你这边也有未保存的改动</span>
    <button class="btn" onclick={() => docs.resolveConflict(tabs.active!, "disk")}>用磁盘上的</button>
    <button class="btn primary" onclick={() => docs.resolveConflict(tabs.active!, "mine")}>保留我的</button>
  </div>
{/if}

{#if git.trustOpen && git.restricted}
  {@const r = git.restricted}
  <!--
    仓库信任（issue #24）。琥珀色同「外部改过」那张：是要你决定，不是出错。
    列的是白名单外的键 —— 人看的就是「它想跑什么」，所以值一定要露出来，
    origin 只在被 include 进来时才显示（那时危险在另一个文件里）。
  -->
  <div class="confirm warn tall trust">
    <span class="btext">
      <b>这个仓库的 .git/config 里有 {r.suspects.length} 条会让 git 执行命令的配置，Git 功能先没启用</b>
      <span class="bbody">{r.suspects
          .map((x) => `${x.key} = ${x.value}${x.origin && !x.origin.endsWith(".git/config") ? `   （${x.origin}）` : ""}`)
          .join("\n")}</span>
      <span class="rest">
        {#if r.hooks.length}另外 .git/hooks 里有 {r.hooks.join("、")}，提交 / 检出时会执行。{/if}
        信任 = 按这份内容记住这个仓库；config 再变会重新问。
      </span>
    </span>
    <button class="btn" data-dismiss onclick={() => (git.trustOpen = false)}>先不动 git</button>
    <button class="btn primary" onclick={() => void git.trust()}>信任这个仓库</button>
  </div>
{/if}

{#if tabflow.pendingSwitch}
  <div class="confirm">
    <span>
      <b>{tabflow.pendingSwitch.name}</b> 有 {(tabflow.pendingSwitch.size / 1048576).toFixed(1)}MB，
      编辑模式会把全文读进内存，可能明显卡顿
    </span>
    <button class="btn" data-dismiss onclick={() => (tabflow.pendingSwitch = null)}>取消</button>
    <button class="btn primary" onclick={() => tabflow.doSwitch(tabflow.pendingSwitch!, "edit")}>仍然编辑</button>
  </div>
{/if}

{#if notify.banner}
  <div class="confirm bad tall err-banner">
    <span class="btext">
      <b>{notify.banner.title}</b>
      <span class="bbody">{notify.banner.body}</span>
    </span>
    <button class="btn" data-dismiss onclick={() => notify.closeBanner()}>知道了</button>
  </div>
{/if}

{#if branches.pendingWtRemove}
  <div class="confirm bad">
    <span>
      要移除工作树 <b>{branches.pendingWtRemove.path}</b> 吗？
      <b>那个目录会被删掉</b>，里面未提交的改动会一起没
    </span>
    <button class="btn danger" onclick={() => branches.removeWorktree(branches.pendingWtRemove!, true)}>强制移除</button>
    <button class="btn" data-dismiss onclick={() => (branches.pendingWtRemove = null)}>取消</button>
    <button class="btn danger" onclick={() => branches.removeWorktree(branches.pendingWtRemove!, false)}>移除</button>
  </div>
{/if}

{#if branches.pendingBranchDelete}
  {@const d = branches.pendingBranchDelete}
  <div class="confirm bad">
    <span>
      {#if d.notMerged}
        <b>{d.name}</b> 上还有没合进别处的提交，git 拦下了。仍然删除的话<b>那些提交会丢</b>
      {:else}
        要删除分支 <b>{d.name}</b> 吗？<b>这一步不可撤销</b>
      {/if}
    </span>
    <button class="btn" data-dismiss onclick={() => (branches.pendingBranchDelete = null)}>取消</button>
    <button class="btn danger" onclick={() => branches.deleteBranch(d.name, d.notMerged)}>{d.notMerged ? "仍然删除" : "删除"}</button>
  </div>
{/if}

{#if branches.pendingCheckout}
  <!--
    这不是错误横幅，是一个选择题 —— 所以它长得和「丢弃改动」「关闭脏标签」
    一样，不是 err-banner。git 拒绝切分支这件事本身没什么可报的，
    真正要说的是「这几个文件挡着，你打算怎么办」。
  -->
  <div class="confirm">
    <span>
      切到 <b>{branches.pendingCheckout.name}</b> 会覆盖
      <b>{branches.pendingCheckout.files.length} 个文件</b>的改动：
      <span class="rest">{branches.pendingCheckout.files.slice(0, 3).join("、")}{branches.pendingCheckout.files.length > 3 ? " …" : ""}</span>
    </span>
    <!-- 越不可逆越靠左：丢东西的排最左，IDEA 的 Smart Checkout（收进 stash、切过去、再放回来）在它右边 -->
    <button class="btn danger" onclick={() => void branches.discardThenCheckout()}>丢弃这些改动并切换</button>
    <button
      class="btn"
      onclick={() => void branches.stashThenCheckout()}
      title="改动收进 stash → 切过去 → 再取回来。取回时撞上冲突会留在改动列表里"
    >stash 再切换</button>
    <button class="btn" data-dismiss onclick={() => (branches.pendingCheckout = null)}>取消</button>
    <button
      class="btn primary"
      onclick={() => {
        branches.pendingCheckout = null;
        layout.showSide("git");
      }}
    >去提交</button>
  </div>
{/if}

{#if git.pendingDiscard}
  <div class="confirm bad">
    <span>
      要丢弃
      {#if git.pendingDiscard.length === 1}
        <b>{git.pendingDiscard[0].path}</b>
      {:else}
        <b>{git.pendingDiscard.length} 个文件</b>
      {/if}
      的改动吗？未跟踪的文件会被直接删除，<b>这一步不可撤销</b>
    </span>
    <button class="btn" data-dismiss onclick={() => (git.pendingDiscard = null)}>取消</button>
    <button class="btn danger" onclick={() => void git.discard(git.pendingDiscard!)}>丢弃</button>
  </div>
{/if}

{#if git.pendingRevertHunk}
  <!-- 撤销一块（issue #38）：和上面那条同一档 —— 动盘上的文件，不可撤销 -->
  <div class="confirm bad">
    <span>要撤销这一块吗？工作区里这几行的改动会被丢掉，<b>这一步不可撤销</b></span>
    <button class="btn" data-dismiss onclick={() => (git.pendingRevertHunk = null)}>取消</button>
    <button class="btn danger" onclick={() => void git.revertHunk(git.pendingRevertHunk!)}>撤销这一块</button>
  </div>
{/if}

{#if Bars && (remote.pendingDiverge || remote.pendingPush || remote.err)}
  <Bars
    diverge={remote.pendingDiverge}
    push={remote.pendingPush}
    err={remote.err}
    upstream={git.status?.upstream ?? ""}
    ahead={git.status?.ahead ?? 0}
    onMerge={(mode, rem) => {
      if (rem) remote.lastMergeMode = mode;
      remote.pendingDiverge = null;
      void remote.pull(mode);
    }}
    onPush={() => void remote.push()}
    onPull={() => {
      remote.err = null;
      void remote.pull();
    }}
    onDismiss={(which) => {
      if (which === "diverge") remote.pendingDiverge = null;
      else if (which === "push") remote.pendingPush = null;
      else remote.err = null;
    }}
  />
{/if}

{#if tabflow.pendingClose}
  <div class="confirm">
    <span><b>{tabflow.pendingClose.name}</b> 有未保存的改动</span>
    {#if tabflow.closeQueue.length}
      <!-- 批量关闭时要说清后面还有几个，否则人不知道这个框还要弹几次 -->
      <span class="rest">（后面还有 {tabflow.closeQueue.length} 个）</span>
    {/if}
    <button class="btn" onclick={() => void tabflow.resolveClose("discard")}>丢弃改动</button>
    <button class="btn" data-dismiss onclick={() => void tabflow.resolveClose("cancel")}>取消</button>
    <button class="btn primary" onclick={() => void tabflow.resolveClose("save")}>保存并关闭</button>
  </div>
{/if}
</div>

<style>
  /* 浮在编辑器岛里（App 的 `.editor-island` 是定位父级），左右贴岛边 —— 原来父级是 `.main`，右边要扣掉岛的右缝 */
  .stack {
    position: absolute;
    left: 0;
    right: 0;
    top: 8px;
    z-index: 30;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    pointer-events: none;
  }
  /* 有标签栏时从它底下 8px 开始（标签栏 38px） */
  .stack.below-tabs { top: 46px; }
  /* 卡片的面（底、边、投影、淡入、warn / bad 两档色）在 app.css 的 `.confirm`；这里只剩正文排版 */
  .confirm .rest { color: var(--text-faint); font-size: 11.5px; }
  .err-banner .btext, .trust .btext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  .confirm.trust, .confirm.err-banner { width: min(760px, calc(100% - 32px)); }
  .trust .bbody { white-space: pre-wrap; font-family: var(--code-font); font-size: 11.5px; line-height: 1.55; color: var(--text); }
  .err-banner b { color: var(--lvl-error); }
  /* git 的说明本来就是分行排版的，保住换行；太长时可以滚 */
  .err-banner .bbody {
    white-space: pre-wrap;
    font-family: var(--code-font);
    font-size: 11.5px;
    line-height: 1.55;
    color: var(--text-dim);
    max-height: 7.5em;
    overflow-y: auto;
  }
</style>
