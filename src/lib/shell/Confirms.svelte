<script lang="ts">
  /**
   * 内容区顶上的那几条确认横幅：外部改动冲突、大文件切编辑、错误说明、
   * 移除工作树、切分支被本地改动挡住、丢弃改动、关脏标签，以及远程操作的
   * 三条（分岔决策 / 推送确认 / 失败提示，那三条在 Git 那组懒加载的
   * `RemoteBars` 里，由 App 把加载到的组件传进来）。
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
<div class="stack" class:below-tabs={tabs.list.length > 0}>
{#if tabs.active?.conflict}
  <div class="confirm conflict">
    <span><b>{tabs.active.name}</b> 在编辑器外被改过，而你这边也有未保存的改动</span>
    <button class="btn primary" onclick={() => docs.resolveConflict(tabs.active!, "mine")}>保留我的</button>
    <button class="btn" onclick={() => docs.resolveConflict(tabs.active!, "disk")}>用磁盘上的</button>
  </div>
{/if}

{#if tabflow.pendingSwitch}
  <div class="confirm">
    <span>
      <b>{tabflow.pendingSwitch.name}</b> 有 {(tabflow.pendingSwitch.size / 1048576).toFixed(1)}MB，
      编辑模式会把全文读进内存，可能明显卡顿
    </span>
    <button class="btn primary" onclick={() => tabflow.doSwitch(tabflow.pendingSwitch!, "edit")}>仍然编辑</button>
    <button class="btn" onclick={() => (tabflow.pendingSwitch = null)}>取消</button>
  </div>
{/if}

{#if notify.banner}
  <div class="confirm err-banner">
    <span class="btext">
      <b>{notify.banner.title}</b>
      <span class="bbody">{notify.banner.body}</span>
    </span>
    <button class="btn" onclick={() => notify.closeBanner()}>知道了</button>
  </div>
{/if}

{#if branches.pendingWtRemove}
  <div class="confirm danger">
    <span>
      要移除工作树 <b>{branches.pendingWtRemove.path}</b> 吗？
      <b>那个目录会被删掉</b>，里面未提交的改动会一起没
    </span>
    <button class="btn danger" onclick={() => branches.removeWorktree(branches.pendingWtRemove!, false)}>移除</button>
    <button class="btn danger" onclick={() => branches.removeWorktree(branches.pendingWtRemove!, true)}>强制移除</button>
    <button class="btn" onclick={() => (branches.pendingWtRemove = null)}>取消</button>
  </div>
{/if}

{#if branches.pendingBranchDelete}
  {@const d = branches.pendingBranchDelete}
  <div class="confirm danger">
    <span>
      {#if d.notMerged}
        <b>{d.name}</b> 上还有没合进别处的提交，git 拦下了。仍然删除的话<b>那些提交会丢</b>
      {:else}
        要删除分支 <b>{d.name}</b> 吗？<b>这一步不可撤销</b>
      {/if}
    </span>
    <button class="btn danger" onclick={() => branches.deleteBranch(d.name, d.notMerged)}>{d.notMerged ? "仍然删除" : "删除"}</button>
    <button class="btn" onclick={() => (branches.pendingBranchDelete = null)}>取消</button>
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
    <button
      class="btn primary"
      onclick={() => {
        branches.pendingCheckout = null;
        layout.showSide("git");
      }}
    >去提交</button>
    <!-- IDEA 的 Smart Checkout：收进 stash、切过去、再放回来。不丢东西的那条路排在丢东西的前面 -->
    <button
      class="btn"
      onclick={() => void branches.stashThenCheckout()}
      title="改动收进 stash → 切过去 → 再取回来。取回时撞上冲突会留在改动列表里"
    >stash 再切换</button>
    <button class="btn danger" onclick={() => void branches.discardThenCheckout()}>丢弃这些改动并切换</button>
    <button class="btn" onclick={() => (branches.pendingCheckout = null)}>取消</button>
  </div>
{/if}

{#if git.pendingDiscard}
  <div class="confirm danger">
    <span>
      要丢弃
      {#if git.pendingDiscard.length === 1}
        <b>{git.pendingDiscard[0].path}</b>
      {:else}
        <b>{git.pendingDiscard.length} 个文件</b>
      {/if}
      的改动吗？未跟踪的文件会被直接删除，<b>这一步不可撤销</b>
    </span>
    <button class="btn danger" onclick={() => void git.discard(git.pendingDiscard!)}>丢弃</button>
    <button class="btn" onclick={() => (git.pendingDiscard = null)}>取消</button>
  </div>
{/if}

{#if Bars && (remote.syncing || remote.pendingDiverge || remote.pendingPush || remote.err)}
  <Bars
    progress={remote.syncing ? { what: remote.syncing.what, phase: remote.syncing.phase, percent: remote.syncing.percent } : null}
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
    onCancel={remote.syncing && remote.syncing.what !== "push" ? () => remote.cancel() : null}
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
    <button class="btn primary" onclick={() => void tabflow.resolveClose("save")}>保存并关闭</button>
    <button class="btn" onclick={() => void tabflow.resolveClose("discard")}>丢弃改动</button>
    <button class="btn" onclick={() => void tabflow.resolveClose("cancel")}>取消</button>
  </div>
{/if}
</div>

<style>
  .stack {
    position: absolute;
    left: 0;
    right: var(--island-gap);
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
  .confirm {
    pointer-events: auto;
    display: flex;
    align-items: center;
    gap: 10px;
    max-width: min(760px, calc(100% - 32px));
    padding: 8px 10px 8px 14px;
    background: var(--elevated);
    border: var(--island-border);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop);
    font-size: 12px;
  }
  .confirm b { color: var(--text); font-weight: 600; }
  .confirm .rest { color: var(--text-faint); font-size: 11.5px; }
  .confirm .btn { flex: none; }
  /* 带色的三种：色罩叠在 --elevated 上，卡片仍然不透明（浮层不许透）；边线跟着色走 */
  .confirm.conflict {
    background: linear-gradient(rgba(214, 174, 88, 0.12), rgba(214, 174, 88, 0.12)), var(--elevated);
    border-color: rgba(214, 174, 88, 0.35);
  }
  /* 不可撤销的操作用红色描边，别让它长得跟普通确认一样 */
  .confirm.danger {
    background: linear-gradient(rgba(247, 84, 100, 0.10), rgba(247, 84, 100, 0.10)), var(--elevated);
    border-color: rgba(247, 84, 100, 0.35);
  }
  .confirm.err-banner {
    align-items: flex-start;
    width: min(760px, calc(100% - 32px));
    background: linear-gradient(rgba(247, 84, 100, 0.10), rgba(247, 84, 100, 0.10)), var(--elevated);
    border-color: rgba(247, 84, 100, 0.35);
  }
  .err-banner .btext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
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
