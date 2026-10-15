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
  import { replace } from "../state/replace.svelte";
  import { project } from "../state/project.svelte";
  import { runs } from "../state/runs.svelte";
  import { keyVerdict, nudgesFor, takeFocus } from "./dialog-focus";
  import { nudgeDialog, setAsking } from "./modal";
  import Icon from "./Icon.svelte";
  import { cardOut, layerOut } from "./motion";

  /** 替换的动作跟着替换浮层懒加载；卡片上点了才拉（启动时那一问已经拉过了，多半是现成的） */
  const replaceOps = () => import("../state/replace-ops");

  let {
    Bars,
  }: {
    /** `RemoteBars`，懒加载到了才有 */
    Bars: typeof RemoteBars | undefined;
  } = $props();

  let stackEl = $state<HTMLElement | null>(null);
  let layerEl = $state<HTMLElement | null>(null);

  /*
   * 卡片的次序和上限（2026-10-13 盘点 A2）。原来没有上限：替换完成、上次任务没停干净、外部改过、报错可以同时出现，
   * 四张能把编辑区上半截全挡住。次序按种类不按到达先后 —— 出错的（红）、要你决定的（琥珀）、做完了能撤销的（中性）
   */
  const CARD_MAX = 2;
  const cards = $derived(
    [
      notify.banner && "banner",
      Bars && remote.err && "remote",
      tabs.active?.conflict && "conflict",
      replace.interrupted && replace.interrupted.root === project.root && "interrupted",
      runs.stale && runs.stale.root === project.root && "stale",
      replace.done && replace.done.root === project.root && "done",
    ].filter((x): x is string => !!x),
  );
  let moreOpen = $state(false);
  $effect(() => {
    if (cards.length <= CARD_MAX) moreOpen = false;
  });
  const showCard = (id: string) => moreOpen || cards.indexOf(id) < CARD_MAX;

  /** RemoteBars 两处共用的参数：对话框层里放「问」的那两种，卡片区里放「报」的那一种 */
  const barProps = $derived({
    diverge: remote.pendingDiverge,
    push: remote.pendingPush,
    err: remote.err,
    upstream: git.status?.upstream ?? "",
    ahead: git.status?.ahead ?? 0,
    onMerge: (mode: "merge" | "rebase", rem: boolean) => {
      if (rem) remote.lastMergeMode = mode;
      remote.pendingDiverge = null;
      void remote.pull(mode);
    },
    onPush: () => void remote.push(),
    onPull: () => {
      remote.err = null;
      void remote.pull();
    },
    onDismiss: (which: "diverge" | "push" | "err") => {
      if (which === "diverge") remote.pendingDiverge = null;
      else if (which === "push") remote.pendingPush = null;
      else remote.err = null;
    },
  });

  /** 此刻有没有要你先回答的对话框（有就压暗编辑岛） */
  const asking = $derived(
    !!(
      tabflow.pendingClose ||
      tabflow.pendingSwitch ||
      branches.pendingWtRemove ||
      branches.pendingBranchDelete ||
      branches.pendingCheckout ||
      git.pendingDiscard ||
      git.pendingRevertHunk ||
      (git.trustOpen && git.restricted) ||
      (Bars && (remote.pendingDiverge || remote.pendingPush))
    ),
  );

  /*
   * Esc = 最上面那张卡片的「取消 / 知道了」。macOS 的对话框 Esc 就是 Cancel；卡片虽然不是
   * 模态的，人看到一张问句的卡片，第一反应还是按 Esc 让它走。三条让位：别人先吃了 Esc 的
   * 不管（`defaultPrevented`：CM6 关查找面板、收多光标）；浮层开着的不管（它们自己的 Esc）；
   * 焦点在终端里的不管（Esc 是 vim 的键）。只按 `data-dismiss` 找 —— 那是「关掉这张、
   * 什么都不做」的按钮；有选择没取消的卡片（外部改过：保留我的 / 用磁盘上的）Esc 不替人选。
   */
  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape" || e.defaultPrevented) return;
    if (overlay.quickOpen || overlay.outlineOpen || overlay.keysOpen || overlay.encOpen || overlay.gotoOpen || overlay.branchOpen || overlay.replaceOpen) return;
    if (document.activeElement?.closest(".xterm")) return;
    // 对话框优先：它压着编辑区，人看到的是它；没有对话框才轮到顶上的卡片
    const btn =
      layerEl?.querySelector<HTMLButtonElement>(".dialog [data-dismiss]") ??
      stackEl?.querySelector<HTMLButtonElement>(".confirm [data-dismiss]");
    if (!btn) return;
    e.preventDefault();
    btn.click();
  }

  /*
   * 对话框开着时，别处的输入不算数（`shell/modal.ts` 头上那段）。挂在 window 的**捕获**阶段：比 App 的全局按键、
   * 组件自己的监听（Svelte 把 onclick / onkeydown 委托在根上，走冒泡）、CM6 都早 —— 在这儿 stopPropagation，它们就收不到。
   * 菜单那条路不经过这里（AppKit → Rust → `menu` 事件），在 `menu-actions.ts` 的 runMenu 门口另外拦
   */
  // 以 `asking` 为准，不看 DOM：答完那一拍起就不拦了，哪怕框还在淡出（modal.ts 的 setAsking 那段）
  const dialogEl = () => (asking ? (layerEl?.querySelector<HTMLElement>(".dialog") ?? null) : null);
  $effect.pre(() => setAsking(asking));
  /*
   * 淡出到一半又要问（关掉一个、120ms 内又弹一个）：Svelte 把正在退场的那层倒回来接着用，不新建 ——
   * 它身上还挂着 `leaving`（motion.ts 退场时打的），于是新的对话框不接鼠标、点不了，界面卡住。又在问了就摘掉。
   * 浏览器里复现过（2026-10-13）：点「取消」后 5ms 内再问一次，同一个元素被复用、`pointer-events: none`，框整个点不动；
   * 摘掉之后同一串操作「取消」照常。真窗口里窗口期是那 120ms，后台页帧不走时会一直停在半路
   */
  $effect(() => {
    if (asking) layerEl?.classList.remove("leaving");
  });

  function guardKey(e: KeyboardEvent) {
    const d = dialogEl();
    if (!d) return;
    const inDialog = d.contains(e.target as Node);
    const v = keyVerdict(e, inDialog);
    if (v === "pass") return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type !== "keydown") return;
    if (v === "cycle") cycleFocus(d, e.shiftKey);
    else if (nudgesFor(e) || !inDialog) nudgeDialog(d);
  }

  /** 点在对话框外面：挡掉、顶一下。只放过标题栏上拖窗口的那几块（Tauri 认的是被点的元素自己带 `data-tauri-drag-region`） */
  function guardPointer(e: MouseEvent) {
    const d = dialogEl();
    if (!d) return;
    const t = e.target as Element | null;
    if (t && (d.contains(t) || t.hasAttribute?.("data-tauri-drag-region"))) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type === "pointerdown") nudgeDialog(d);
  }

  /** Tab / ⇧Tab：在对话框里能拿焦点的东西之间转圈 */
  function cycleFocus(d: HTMLElement, back: boolean) {
    const all = [...d.querySelectorAll<HTMLElement>("button:not([disabled]), input:not([disabled]), [tabindex]:not([tabindex='-1'])")];
    if (all.length === 0) return;
    const i = all.indexOf(document.activeElement as HTMLElement);
    const n = all.length;
    all[i < 0 ? (back ? n - 1 : 0) : (i + (back ? -1 : 1) + n) % n].focus({ preventScroll: true });
  }

  $effect(() => {
    const keys = ["keydown", "keyup"] as const;
    const mice = ["pointerdown", "mousedown", "mouseup", "click", "dblclick", "auxclick", "contextmenu"] as const;
    for (const k of keys) window.addEventListener(k, guardKey, true);
    for (const m of mice) window.addEventListener(m, guardPointer, true);
    return () => {
      for (const k of keys) window.removeEventListener(k, guardKey, true);
      for (const m of mice) window.removeEventListener(m, guardPointer, true);
    };
  });

</script>

<!--
  两种长相（2026-10-12 设计稿①）：**要你先回答的**（你刚做的那个动作引起的选择题）是压暗编辑岛的对话框，在下面 `.layer` 里；
  **告诉你一件事的**和启动时冒出来的（替换做完能撤销、报错、外部改过、上次没停干净、替换中断）是顶上的卡片 ——
  Apple HIG：别为了告诉人一件事弹对话框，别在启动时弹。

  M8：卡片不是撑开内容区的通栏横条，而是**浮在内容岛顶上的卡片**。
  通栏横条有两个毛病：它把编辑器整体往下推一行（人正盯着的那行跳走了），
  贴着标签栏时两条 1px 边线叠在一起。卡片照浮层的规矩：不透明、投影抬起、不靠边线。
  `pointer-events: none` 的外层让卡片之外的编辑器照常可点。
-->
<svelte:window onkeydown={onKey} />

<!--
  要你先回答的那几种：对话框（app.css「对话框」那段）。标题是问句、一句后果、按钮照 macOS 排：
  会丢东西的（不是你点的那个动作本身）在最左、红字；「取消」挨着主按钮；主按钮最右。
-->
{#if asking}
  <div class="layer" bind:this={layerEl} out:layerOut>
    {#if tabflow.pendingClose}
      <!--
        「关闭全部」逐个问时，A 问完换 B 是同一拍里的事（doClose 置空、askNextClose 设下一个），这个块不重建、
        `takeFocus` 不会再跑 —— 把 id 传进去，换了人它在 update 里把焦点放回「保存」（code review 2026-10-13：
        原来焦点停在上一下按过的「不保存」上，接着按回车就把 B 也丢了）
      -->
      <div class="dialog" role="alertdialog" aria-label="要保存改动吗" use:takeFocus={tabflow.pendingClose.id}>
        <p class="title">要保存对 {tabflow.pendingClose.name} 的改动吗？</p>
        <p class="body">
          不保存的话，这次的改动会丢掉。
          <!-- 批量关闭时要说清后面还有几个，否则人不知道这个框还要弹几次 -->
          {#if tabflow.closeQueue.length}后面还有 {tabflow.closeQueue.length} 个没保存的标签。{/if}
        </p>
        <div class="actions">
          <button class="btn destructive" onclick={() => void tabflow.resolveClose("discard")}>不保存</button>
          <span class="gap"></span>
          <button class="btn" data-dismiss onclick={() => void tabflow.resolveClose("cancel")}>取消</button>
          <button class="btn primary" data-default onclick={() => void tabflow.resolveClose("save")}>保存</button>
        </div>
      </div>
    {:else if tabflow.pendingSwitch}
      <div class="dialog" role="alertdialog" aria-label="用编辑模式打开吗" use:takeFocus>
        <p class="title">用编辑模式打开 {tabflow.pendingSwitch.name}？</p>
        <p class="body">它有 {(tabflow.pendingSwitch.size / 1048576).toFixed(1)} MB，编辑模式会把全文读进内存，可能明显卡顿。</p>
        <div class="actions">
          <span class="gap"></span>
          <button class="btn" data-dismiss onclick={() => (tabflow.pendingSwitch = null)}>取消</button>
          <button class="btn primary" data-default onclick={() => tabflow.doSwitch(tabflow.pendingSwitch!, "edit")}>仍然编辑</button>
        </div>
      </div>
    {:else if branches.pendingWtRemove}
      {@const w = branches.pendingWtRemove}
      <div class="dialog" role="alertdialog" aria-label="移除工作树吗" use:takeFocus>
        <p class="title">移除工作树 {w.path.slice(w.path.lastIndexOf("/") + 1)}？</p>
        <p class="body"><b>{w.path}</b> 这个目录会被删掉，里面没提交的改动会一起没了。</p>
        <div class="actions">
          <button class="btn destructive" onclick={() => branches.removeWorktree(w, true)}>强制移除</button>
          <span class="gap"></span>
          <button class="btn" data-dismiss data-default onclick={() => (branches.pendingWtRemove = null)}>取消</button>
          <button class="btn danger" onclick={() => branches.removeWorktree(w, false)}>移除</button>
        </div>
      </div>
    {:else if branches.pendingBranchDelete}
      {@const d = branches.pendingBranchDelete}
      <div class="dialog" role="alertdialog" aria-label="删除分支吗" use:takeFocus>
        {#if d.notMerged}
          <p class="title">仍然删除分支 {d.name}？</p>
          <p class="body">它上面还有没合进别处的提交，git 拦下了。删了的话<b>那些提交会丢</b>。</p>
        {:else}
          <p class="title">删除分支 {d.name}？</p>
          <p class="body">这一步不可撤销。</p>
        {/if}
        <div class="actions">
          <span class="gap"></span>
          <button class="btn" data-dismiss data-default onclick={() => (branches.pendingBranchDelete = null)}>取消</button>
          <button class="btn danger" onclick={() => branches.deleteBranch(d.name, d.notMerged)}>{d.notMerged ? "仍然删除" : "删除"}</button>
        </div>
      </div>
    {:else if branches.pendingCheckout}
      {@const c = branches.pendingCheckout}
      <!--
        这不是错误，是一个选择题：git 拒绝切分支这件事本身没什么可报的，
        真正要说的是「这几个文件挡着，你打算怎么办」（ui.md 第十三条第三档）
      -->
      <div class="dialog wide" role="alertdialog" aria-label="切分支会覆盖改动" use:takeFocus>
        <p class="title">切到 {c.name} 会覆盖 {c.files.length} 个文件的改动</p>
        <p class="body">{c.files.slice(0, 3).join("、")}{c.files.length > 3 ? ` 等 ${c.files.length} 个` : ""}</p>
        <div class="actions">
          <!-- 越不可逆越靠左：丢东西的排最左，IDEA 的 Smart Checkout（收进 stash、切过去、再放回来）在它右边 -->
          <button class="btn destructive" onclick={() => void branches.discardThenCheckout()}>丢弃这些改动并切换</button>
          <button
            class="btn"
            onclick={() => void branches.stashThenCheckout()}
            title="改动收进 stash → 切过去 → 再取回来。取回时撞上冲突会留在改动列表里"
          >stash 再切换</button>
          <span class="gap"></span>
          <button class="btn" data-dismiss onclick={() => (branches.pendingCheckout = null)}>取消</button>
          <button
            class="btn primary"
            data-default
            onclick={() => {
              branches.pendingCheckout = null;
              layout.showSide("git");
            }}
          >去提交</button>
        </div>
      </div>
    {:else if git.pendingDiscard}
      {@const ds = git.pendingDiscard}
      <div class="dialog" role="alertdialog" aria-label="丢弃改动吗" use:takeFocus>
        <p class="title">丢弃 {ds.length === 1 ? ds[0].path : `${ds.length} 个文件`} 的改动？</p>
        <p class="body">改过的文件回到上次提交的样子，<b>这一步不可撤销</b>；未跟踪的文件移进废纸篓。</p>
        <div class="actions">
          <span class="gap"></span>
          <button class="btn" data-dismiss data-default onclick={() => (git.pendingDiscard = null)}>取消</button>
          <button class="btn danger" onclick={() => void git.discard(ds)}>丢弃</button>
        </div>
      </div>
    {:else if git.pendingRevertHunk}
      <!-- 撤销一块（issue #38）：和上面那条同一档 —— 动盘上的文件，不可撤销 -->
      <div class="dialog" role="alertdialog" aria-label="撤销这一块吗" use:takeFocus>
        <p class="title">撤销这一块改动？</p>
        <p class="body">工作区里这几行的改动会被丢掉，<b>这一步不可撤销</b>。</p>
        <div class="actions">
          <span class="gap"></span>
          <button class="btn" data-dismiss data-default onclick={() => (git.pendingRevertHunk = null)}>取消</button>
          <button class="btn danger" onclick={() => void git.revertHunk(git.pendingRevertHunk!)}>撤销这一块</button>
        </div>
      </div>
    {:else if git.trustOpen && git.restricted}
      {@const r = git.restricted}
      <!--
        仓库信任（issue #24）。列的是白名单外的键 —— 人看的就是「它想跑什么」，所以值一定要露出来，
        origin 只在被 include 进来时才显示（那时危险在另一个文件里）。默认按钮是「先不动 git」：回车不该替人信任
      -->
      <div class="dialog wide" role="alertdialog" aria-label="信任这个仓库吗" use:takeFocus>
        <p class="title">这个仓库会让 git 执行命令，Git 功能先没启用</p>
        <p class="body">它的 .git/config 里有 {r.suspects.length} 条这样的配置：</p>
        <pre class="code">{r.suspects
            .map((x) => `${x.key} = ${x.value}${x.origin && !x.origin.endsWith(".git/config") ? `   （${x.origin}）` : ""}`)
            .join("\n")}</pre>
        <p class="body">
          {#if r.hooks.length}另外 .git/hooks 里有 {r.hooks.join("、")}，提交 / 检出时会执行。{/if}
          信任 = 按这份内容记住这个仓库；config 再变会重新问。
        </p>
        <div class="actions">
          <span class="gap"></span>
          <button class="btn" data-dismiss data-default onclick={() => (git.trustOpen = false)}>先不动 git</button>
          <button class="btn primary" onclick={() => void git.trust()}>信任这个仓库</button>
        </div>
      </div>
    {:else if Bars}
      <!-- 分岔要选合并方式、推送前确认：你点了拉取 / 推送才有的选择题 -->
      <Bars part="ask" {...barProps} />
    {/if}
  </div>
{/if}

<div class="stack" class:below-tabs={tabs.list.length > 0} bind:this={stackEl}>
<!--
  次序（2026-10-13 盘点 A2）：出错的（红）在上 → 要你决定的（琥珀）→ 做完了、能撤销的（中性）。新来的不会把正在读的那张挤走：
  它按种类排进自己的位置，不是按到达先后。最多摆 CARD_MAX 张，其余折成「还有 N 条」
-->
{#if notify.banner && showCard("banner")}
  <div class="confirm bad tall err-banner" out:cardOut>
    <span class="btext">
      <b>{notify.banner.title}</b>
      <span class="bbody">{notify.banner.body}</span>
    </span>
    <button class="btn" data-dismiss onclick={() => notify.closeBanner()}>知道了</button>
  </div>
{/if}

{#if Bars && remote.err && showCard("remote")}
  <!-- 远程操作失败：告诉你结果（带下一步），是卡片 -->
  <Bars part="tell" {...barProps} />
{/if}

{#if tabs.active?.conflict && showCard("conflict")}
  <div class="confirm warn" out:cardOut>
    <span><b>{tabs.active.name}</b> 在编辑器外被改过，而你这边也有未保存的改动</span>
    <button class="btn" onclick={() => docs.resolveConflict(tabs.active!, "disk")}>用磁盘上的</button>
    <button class="btn primary" onclick={() => docs.resolveConflict(tabs.active!, "mine")}>保留我的</button>
  </div>
{/if}

{#if replace.interrupted && replace.interrupted.root === project.root && showCard("interrupted")}
  {@const p = replace.interrupted}
  <!--
    上次替换提交到一半应用没了（#42，docs/REPLACE.md 12.2）。琥珀色：要你决定，不是出错。
    默认动作（最右、primary）是退回改前：一个「改了一半」的仓库比哪边都糟。没有「知道了」—— 两条路必须选一条
  -->
  <div class="confirm warn" out:cardOut>
    <span>上次的替换中途中断了：<b>已改 {p.changed} 个文件</b>，共 {p.files} 个</span>
    <button class="btn" onclick={() => void replaceOps().then((m) => m.resolveInterrupted(false))}>保留现状</button>
    <button class="btn primary" onclick={() => void replaceOps().then((m) => m.resolveInterrupted(true))}>全部退回改前</button>
  </div>
{/if}

{#if runs.stale && runs.stale.root === project.root && showCard("stale")}
  {@const s = runs.stale}
  <!--
    上次应用没正常退出（崩了、被强杀），它起的任务还在自己的进程组里跑（#48，TASKS.md 第 5 节）—— IDEA 论坛里「IDE 崩了 JVM 还占着 8080」。
    琥珀：要你决定。默认动作（最右）是结束它们：它们是这个项目的任务、是我们起的，留着多半就是占着端口；「留着」= 从账上划掉、不再提醒
  -->
  <div class="confirm warn tall stale" out:cardOut>
    <div class="btext">
      <span>上次 lite-ide 没正常退出，它起的 <b>{s.list.length} 个任务</b>还在跑（可能占着端口）</span>
      <span class="rest">{s.list.map((l) => `${l.name}：${l.command}`).join("　·　")}</span>
    </div>
    <button class="btn" onclick={() => void import("../state/runs-ops").then((m) => m.resolveStale(false))}>留着</button>
    <button class="btn primary" onclick={() => void import("../state/runs-ops").then((m) => m.resolveStale(true))}>结束它们</button>
  </div>
{/if}

{#if replace.done && replace.done.root === project.root && showCard("done")}
  {@const d = replace.done}
  <!-- 撤销的退路（#42 第 6 节）：不加确认框，做完给这一张。下一次替换、或者点掉，它就没了；应用重启后还会再出一次 -->
  <div class="confirm" out:cardOut>
    <span>{d.hits ? `已替换 ${d.files} 个文件 ${d.hits} 处` : `上次的替换（${d.files} 个文件）还能撤销`}</span>
    <button class="btn" data-dismiss onclick={() => (replace.done = null)}>知道了</button>
    <button class="btn primary" onclick={() => void replaceOps().then((m) => m.undo())}>撤销</button>
  </div>
{/if}

{#if cards.length > CARD_MAX}
  <button class="more" onclick={() => (moreOpen = !moreOpen)} aria-expanded={moreOpen}>
    {moreOpen ? "收起" : `还有 ${cards.length - CARD_MAX} 条`}<Icon name={moreOpen ? "chevron-up" : "chevron-down"} />
  </button>
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
    pointer-events: none;
  }
  /* 间距写在每张自己身上、不用 gap：一张退场收起高度时（motion.ts 的 cardOut）连同间距一起收，下面那张平滑补上，不在最后跳 8px */
  .stack > :global(*) { margin-bottom: 8px; }
  /* 有标签栏时从它底下 8px 开始（标签栏 38px） */
  .stack.below-tabs { top: 46px; }
  /* 卡片的面（底、边、投影、淡入、warn / bad 两档色）在 app.css 的 `.confirm`；这里只剩正文排版 */
  .confirm .rest { color: var(--text-faint); font-size: var(--fs-sm); }
  /* 高卡片（`tall`）的正文是两行：一句话 + 一行灰的细节。原来按卡片名各列一遍（报错、上次的任务），第三张就漏了 */
  .confirm.tall .btext { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 3px; }
  /* 折起来的那几张：一颗小胶囊，不是又一张卡片（又一张就又挡一行代码） */
  .more {
    pointer-events: auto;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    padding: 3px 6px 3px 11px;
    border: none;
    border-radius: 99px;
    background: var(--elevated);
    box-shadow: var(--shadow-pop), var(--pop-ring);
    color: var(--text-muted);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
    animation: drop-in 120ms ease-out;
  }
  .more:hover { background: var(--elevated-hi); color: var(--text); }
  /*
   * 对话框那一层：盖住编辑岛、压暗，对话框在正中偏上（眼睛本来就在上半屏）。只盖编辑岛不盖侧边栏和状态栏：
   * 问的是这一块里的事。点暗处不关 —— 选择要按按钮做
   */
  .layer {
    position: absolute;
    inset: 0;
    z-index: 35;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: min(18vh, 160px);
    background: rgba(0, 0, 0, 0.38);
    border-radius: inherit;
    animation: fade-in 90ms ease-out;
  }
  /* 淡出中的那层不再接鼠标：点穿它落到编辑区上（模态守卫那边也不再把它算成「在问」） */
  .layer:global(.leaving), .layer:global(.leaving) .dialog { pointer-events: none; }
  /* 信任仓库那张列配置的：值要原样露出来，等宽、保住换行，太多时可以滚 */
  .dialog .code {
    margin: 2px 0;
    padding: 8px 10px;
    max-height: 9em;
    overflow: auto;
    border-radius: var(--r-sm);
    background: rgba(0, 0, 0, 0.22);
    font-family: var(--code-font);
    font-size: var(--fs-sm);
    line-height: 1.55;
    color: var(--text);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  .err-banner b { color: var(--lvl-error); }
  /* git 的说明本来就是分行排版的，保住换行；太长时可以滚 */
  .err-banner .bbody {
    white-space: pre-wrap;
    font-family: var(--code-font);
    font-size: var(--fs-sm);
    line-height: 1.55;
    color: var(--text-dim);
    max-height: 7.5em;
    overflow-y: auto;
  }
</style>
