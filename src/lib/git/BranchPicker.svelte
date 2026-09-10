<script lang="ts">
  import type { GitBranch, GitWorktree } from "../ipc/commands";
  import { gitBranches, gitWorktrees } from "../ipc/commands";
  import Icon from "../shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import { showInFinder, copyText } from "../shell/pathactions";

  let {
    open = $bindable(false),
    anchor = null,
    repo,
    ahead = 0,
    behind = 0,
    onSwitch,
    onNewBranch,
    onOpenWorktree,
    onNewWorktree,
    onRemoveWorktree,
  }: {
    open?: boolean;
    /**
     * 挂件的位置（视口坐标，通常传它的 `left` / `bottom`）。
     *
     * **这个浮层是从分支挂件底下掉下来的，不是弹在屏幕中间的对话框。**
     * 居中弹窗那种做法有两处不对：一是它长得像「要你先处理完才能干别的」，
     * 而切分支只是随手看一眼；二是它离触发它的那个挂件十万八千里，
     * 眼睛要从左上角跳到屏幕中央再跳回来。IDEA 的分支挂件就挂在下面。
     *
     * 给 `null` 时退回居中。**正常路径上不会是 null** —— 三个入口
     * （挂件、Git 栏的分支行、菜单）都从同一个挂件元素上读位置；
     * 它是挂件还没挂上时的兜底，不是另一种用法。
     */
    anchor?: { x: number; y: number } | null;
    repo: string;
    /**
     * 当前分支与上游差多少。**这两个值在 `GitStatus` 上，不在 `GitBranch` 上**，
     * 所以只能由 App 传进来。切分支之前最想知道的就是「我现在在哪、差多少」。
     */
    ahead?: number;
    behind?: number;
    onSwitch: (name: string) => void;
    onNewBranch: (name: string) => void;
    onOpenWorktree: (path: string) => void;
    /** 目录名 + 分支名；分支不存在就新建 */
    onNewWorktree: (dir: string, branch: string) => void;
    onRemoveWorktree: (w: GitWorktree) => void;
  } = $props();

  let q = $state("");
  let branches = $state<GitBranch[]>([]);
  let trees = $state<GitWorktree[]>([]);
  let loading = $state(false);
  let err = $state("");
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  /** 新建工作树的两个输入 */
  let wtDir = $state("");
  let wtBranch = $state("");
  let mode = $state<"list" | "newWorktree">("list");
  let popEl = $state<HTMLElement | null>(null);

  /*
   * 位置钳进视口。抄 `ContextMenu` 那一套：**改的是 DOM 而不是 props** ——
   * 写回状态会让这个 effect 依赖自己写的值，一不小心就是 update 循环。
   *
   * 依赖里要带上 `items` 和 `mode`：过滤一改高度就变，钳完的 top 会过时，
   * 长列表筛成两行之后浮层底边会吊在半空。
   */
  $effect(() => {
    const e = popEl;
    const a = anchor;
    if (!e || !a) return;
    void [items.length, mode, loading];
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(a.x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${Math.max(pad, Math.min(a.y, window.innerHeight - r.height - pad))}px`;
  });

  // 每次打开都重新拉：分支和工作树在终端里随时会变，缓存只会骗人
  $effect(() => {
    if (!open) return;
    q = "";
    sel = 0;
    mode = "list";
    err = "";
    void (async () => {
      loading = true;
      try {
        [branches, trees] = await Promise.all([gitBranches(repo), gitWorktrees(repo)]);
      } catch (e) {
        err = String(e);
      } finally {
        loading = false;
      }
    })();
    // 等 DOM 出来再聚焦
    queueMicrotask(() => input?.focus());
  });

  /**
   * `kind` 决定**按下去做什么**，`group` 决定**排在哪一档**。
   * 拆开是因为当前分支要被钉到最上面单独成一组，但它按下去仍然是个分支。
   */
  type Group = "current" | "branch" | "worktree" | "remote" | "action";

  const GROUP_LABEL: Record<Group, string> = {
    current: "当前",
    branch: "本地分支",
    worktree: "工作树",
    remote: "远程分支",
    action: "",
  };

  /**
   * ↵ 对不同条目做的事不一样，脚栏要说清是哪一件。
   *
   * 分支 / 远程 / 工作树这三类现在**打开动作菜单**，不直接动手（见 `pick`）。
   * 「新建」那两条是动作不是对象，没有「对它做点别的」，↵ 就是执行。
   *
   * 远程分支那条曾经写的是「检出」，而它真正做的是 `switch --track origin/foo`，
   * **会多出一个本地分支**。那句话现在长在菜单项自己身上
   * （`检出为本地分支 foo`）—— 比放在脚栏好：它就在你要按的那一行上。
   */
  const ENTER_LABEL: Record<Item["kind"], string> = {
    branch: "操作…",
    remote: "操作…",
    worktree: "操作…",
    newBranch: "新建",
    newWorktree: "新建",
  };

  interface Item {
    kind: "branch" | "remote" | "worktree" | "newBranch" | "newWorktree";
    group: Group;
    label: string;
    hint: string;
    tag?: string;
    current?: boolean;
    branch?: GitBranch;
    tree?: GitWorktree;
  }

  /**
   * `/Users/shaopc/` 在每一行里都一样，而它吃掉的正是路径里唯一有区分度的那一截。
   * 这里不读环境变量：工作树都在同一个用户下，按第一段 `/Users/<name>/` 缩就够。
   */
  function shortPath(p: string) {
    const m = /^\/Users\/[^/]+\//.exec(p);
    return m ? `~/${p.slice(m[0].length)}` : p;
  }

  /** 当前分支名。给「从谁分出」和菜单里的措辞用 */
  let cur = $derived(branches.find((b) => b.isHead && !b.isRemote)?.name ?? "");

  let items = $derived.by(() => {
    const k = q.trim().toLowerCase();
    const hit = (s: string) => !k || s.toLowerCase().includes(k);
    const out: Item[] = [];

    const local = branches.filter((b) => !b.isRemote && hit(b.name));
    // 当前分支钉到最上面单独成一组：切分支之前最想确认的就是「我现在在哪」，
    // 混在按名字排的列表里就得先找一遍
    for (const b of local.filter((b) => b.isHead)) {
      out.push({
        kind: "branch",
        group: "current",
        label: b.name,
        hint: b.subject,
        tag: b.upstream ? `↗ ${b.upstream}` : "",
        current: true,
        branch: b,
      });
    }
    for (const b of local.filter((b) => !b.isHead)) {
      out.push({
        kind: "branch",
        group: "branch",
        label: b.name,
        hint: b.subject,
        tag: b.upstream ? `↗ ${b.upstream}` : "",
        branch: b,
      });
    }
    for (const w of trees.filter((w) => hit(w.branch || w.path))) {
      out.push({
        kind: "worktree",
        group: "worktree",
        label: w.branch || `(${w.sha})`,
        hint: shortPath(w.path),
        tag: w.locked ? "已锁定" : "",
        current: w.current,
        tree: w,
      });
    }
    for (const b of branches.filter((b) => b.isRemote && hit(b.name))) {
      out.push({ kind: "remote", group: "remote", label: b.name, hint: b.subject, branch: b });
    }
    // 输入了名字但没有同名分支 —— 直接给个「新建」出口，不用先切到别的界面
    if (k && !branches.some((b) => b.name.toLowerCase() === k)) {
      out.push({
        kind: "newBranch",
        group: "action",
        label: `新建分支「${q.trim()}」`,
        // 「从当前 HEAD 分出」是对的但没说是谁 —— base 就在手边，直接写出来
        hint: cur ? `从 ${cur} 分出` : "从当前 HEAD 分出",
      });
    }
    out.push({
      kind: "newWorktree",
      group: "action",
      label: "新建工作树…",
      hint: "把另一个分支检出到独立目录",
    });
    return out;
  });

  // 过滤变了就把选中项拉回顶部，否则会停在一个已经不存在的位置上
  $effect(() => {
    q;
    sel = 0;
  });

  /**
   * 每行的动作菜单。**它现在就是主动作** —— 单击 / ↵ 打开的是它。
   *
   * # 这条以前是反的，2026-09-10 按 IDEA 改过来
   *
   * 原来单击和 ↵ 直接切分支，注释里写的理由是「这个应用所有浮层都是
   * 打字 → ↵ → 完事，改成两次回车太重」。那个理由只算对了一半：
   *
   * **切分支不是打开一个文件。** 打开文件错了就再打开一个，切分支错了
   * 是几千个文件被检出、编辑器里所有标签重读、正在跑的构建对着一半新一半旧的
   * 工作区。这件事的撤销成本和「打开」根本不在一个量级上，而它原来只隔着
   * 一次手滑 —— 列表是 hover 就移动选中项的（`onmouseenter` 改 `sel`），
   * 鼠标划过去顺手一点就切了。
   *
   * IDEA 正是这么分的：点一个分支弹出它的动作,第一项才是 Checkout。
   * 我们的 `ContextMenu` 游标初值是 0、第一项就是「切换到 X」，所以
   * **↵↵ 仍然是一次切换**，键盘路径只多一个回车，换来的是鼠标路径不再有误切。
   *
   * 菜单里**只放今天已经有的能力**，一条新的 gitsvc 命令都没加。
   */
  let rowMenu = $state<{ x: number; y: number; item: Item } | null>(null);

  /**
   * 统一的开菜单入口 —— 单击、右键、⇧F10 走的都是它，**锚点也一样**。
   *
   * 不用鼠标坐标当锚点：主动作是单击之后，菜单会跟着指针在行内左右乱跑，
   * 而它讲的是「这一行」的事。钉在行的左下角，位置就永远和它说的对象对齐。
   */
  function openRowMenuAt(i: number) {
    const it = items[i];
    const el = rowEls[i];
    if (!it || !el || !hasMenu(it)) return;
    const r = el.getBoundingClientRect();
    rowMenu = { x: r.left + 24, y: r.bottom - 4, item: it };
  }

  function openRowMenu(e: MouseEvent, i: number) {
    e.preventDefault();
    e.stopPropagation();
    sel = i;
    openRowMenuAt(i);
  }

  let rowEls = $state<HTMLElement[]>([]);

  let rowMenuItems = $derived.by((): MenuItem[] => {
    const it = rowMenu?.item;
    if (!it) return [];
    const close = () => (open = false);
    switch (it.kind) {
      case "branch":
        return [
          ...(it.current
            ? []
            : [{ label: `切换到 ${it.label}`, run: () => { onSwitch(it.label); close(); } }]),
          {
            label: "在新工作树中打开…",
            run: () => {
              wtBranch = it.label;
              wtDir = "";
              mode = "newWorktree";
            },
          },
          { label: "复制分支名", sep: true, run: () => void copyText(it.label, "分支名") },
        ];
      case "remote": {
        const short = it.label.split("/").slice(1).join("/") || it.label;
        return [
          // 说清它要做什么：不是「切过去」，是**建一个本地分支**再切
          { label: `检出为本地分支 ${short}`, run: () => { onSwitch(it.label); close(); } },
          { label: "复制分支名", sep: true, run: () => void copyText(it.label, "分支名") },
        ];
      }
      case "worktree": {
        const w = it.tree!;
        const out: MenuItem[] = [];
        if (!it.current) out.push({ label: "打开", run: () => { onOpenWorktree(w.path); close(); } });
        out.push({ label: "在 Finder 中显示", run: () => void showInFinder(w.path) });
        out.push({ label: "复制路径", run: () => void copyText(w.path, "路径") });
        /*
         * 移除**会删掉那个目录**，所以它只出现在菜单里、带 danger、带省略号。
         * 原来它是常驻在行尾的一个 ✕，和「打开」只隔着几个像素 ——
         * 和 Git 栏「全部丢弃」当初撤进 ⋯ 是同一条判据：
         * 误点的代价不该只隔着一次手滑。
         */
        if (!it.current && !w.bare) {
          out.push({
            label: "移除工作树…",
            danger: true,
            sep: true,
            run: () => { onRemoveWorktree(w); close(); },
          });
        }
        return out;
      }
      default:
        return [];
    }
  });

  /**
   * 按下一行（单击或 ↵）。
   *
   * **对象类的三种（分支 / 远程 / 工作树）打开动作菜单，不直接动手**，
   * 理由见 `rowMenu` 那段。「新建」那两条是动作，按下去就执行 ——
   * 给一个只有一项的菜单去确认「新建」，那是纯粹的仪式。
   */
  function pick(it: Item, i: number) {
    switch (it.kind) {
      case "branch":
      case "remote":
      case "worktree":
        openRowMenuAt(i);
        break;
      case "newBranch":
        onNewBranch(q.trim());
        open = false;
        break;
      case "newWorktree":
        wtBranch = q.trim();
        wtDir = "";
        mode = "newWorktree";
        break;
    }
  }

  function onKey(e: KeyboardEvent) {
    /*
     * 行菜单开着的时候，键盘归它。
     *
     * 这个 handler 挂在 `<svelte:window>` 上，而 `ContextMenu` 的 Escape 只
     * `preventDefault` 不 `stopPropagation` —— 于是一次 Esc 会**同时**关掉菜单
     * 和整个浮层，人以为自己只是取消了菜单。↑↓ 同理：不挡的话列表和菜单
     * 会一起动。
     */
    if (rowMenu) {
      // 正常情况下这里根本轮不到（菜单拿着焦点，自己 stopPropagation 了）。
      // 但**焦点万一不在菜单上**，直接 return 会让 Esc 变成一个死键 ——
      // 菜单开着、按什么都没反应。兜底关掉它。
      if (e.key === "Escape") {
        e.preventDefault();
        rowMenu = null;
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (mode === "newWorktree") mode = "list";
      else open = false;
      return;
    }
    if (mode !== "list") return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = (sel + 1) % Math.max(1, items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = (sel - 1 + items.length) % Math.max(1, items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const it = items[sel];
      if (it) pick(it, sel);
    } else if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
      // 和文件树、标签栏同一对键位。现在和 ↵ 落到同一个地方，但键位留着 ——
      // 手记着 ⇧F10 的人不该发现它忽然没用了
      e.preventDefault();
      openRowMenuAt(sel);
    }
  }

  /** 哪些行有菜单。新建分支 / 新建工作树是动作不是对象，没有「对它做点别的」 */
  function hasMenu(it: Item) {
    return it.kind === "branch" || it.kind === "remote" || it.kind === "worktree";
  }

  function submitWorktree() {
    if (!wtDir.trim()) return;
    // 目标分支留空就用目录名当分支名 —— 十有八九就是想要的
    const dir = wtDir.trim();
    const br = wtBranch.trim() || dir.slice(dir.lastIndexOf("/") + 1);
    onNewWorktree(dir, br);
    open = false;
  }
</script>

<svelte:window onkeydown={open ? onKey : undefined} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <!--
    scrim **不压暗**：这是个从挂件底下掉出来的浮层，不是要你先处理完的对话框。
    压暗一层等于告诉人「后面那些现在都别碰」，而切分支只是随手看一眼。
    它留着只干一件事 —— 点外面关掉。
  -->
  <div class="scrim" onclick={() => (open = false)}></div>
  <div
    class="popup"
    class:anchored={!!anchor}
    bind:this={popEl}
    role="dialog"
    aria-label="分支与工作树"
  >
    {#if mode === "list"}
      <input
        bind:this={input}
        bind:value={q}
        placeholder="切换分支 / 打开工作树，输入新名字可直接新建"
        spellcheck="false"
      />
      <div class="results">
        {#if err}
          <div class="none err">{err}</div>
        {:else if loading}
          <div class="none">载入中…</div>
        {:else if items.length === 0}
          <div class="none">没有匹配</div>
        {:else}
          {#each items as it, i (it.kind + it.label)}
            {#if i === 0 || items[i - 1].group !== it.group}
              {#if it.group === "action"}
                <!--
                  「操作」和分支不同类：分支是「去哪儿」，它是「做什么」。
                  照 macOS 菜单的做法用一条分隔线隔开，不给它一个分组标题。
                -->
                <div class="divider"></div>
              {:else}
                <div class="sec">{GROUP_LABEL[it.group]}</div>
              {/if}
            {/if}
            <!--
              右键挂在整行上（不只是那个按钮）——「对着这一行右键」是人的直觉，
              而按钮只占行尾 22px。role="group" 是给这个 handler 交代身份用的，
              真正可聚焦的仍然是里面那个 button。
            -->
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <div
              class="rowwrap"
              class:on={i === sel}
              role="group"
              bind:this={rowEls[i]}
              oncontextmenu={(e) => openRowMenu(e, i)}
            >
              <button class="row" onclick={() => pick(it, i)} onmouseenter={() => (sel = i)}>
                <!--
                  四种条目四个图标。列表最长会有几十行，而**分组头一滚就看不见了** ——
                  「origin/dev 是远程的」不该靠记得自己滚过哪个标题。
                -->
                <span class="ic {it.kind}" class:cur={it.group === "current"}>
                  {#if it.kind === "worktree"}
                    <Icon name="files" size={14} />
                  {:else if it.kind === "newBranch" || it.kind === "newWorktree"}
                    <Icon name="plus" size={14} />
                  {:else if it.kind === "remote"}
                    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
                         stroke-width="1.25" aria-hidden="true">
                      <circle cx="8" cy="8" r="5.8" />
                      <path d="M2.4 8h11.2" stroke-linecap="round" />
                      <path d="M8 2.2c1.5 1.7 2.3 3.7 2.3 5.8S9.5 12.1 8 13.8C6.5 12.1 5.7 10.1 5.7 8S6.5 3.9 8 2.2z" />
                    </svg>
                  {:else if it.group === "current"}
                    <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor"
                         stroke-width="1.4" aria-hidden="true">
                      <circle cx="8" cy="8" r="3.1" />
                      <circle cx="8" cy="8" r="6.1" opacity="0.45" />
                    </svg>
                  {:else}
                    <Icon name="git" size={14} />
                  {/if}
                </span>
                <span class="lb" class:cur={it.current}>{it.label}</span>
                {#if it.tag}<span class="tg">{it.tag}</span>{/if}
                {#if it.group === "current" && behind}<span class="ab">↓{behind}</span>{/if}
                {#if it.group === "current" && ahead}<span class="ab">↑{ahead}</span>{/if}
                {#if it.kind === "worktree" && it.current}<span class="now">当前</span>{/if}
                <span class="ht">{it.hint}</span>
              </button>
              <!--
                行尾那个记号从「⋯ 按钮」变成了一个**不可点的 ›**。

                主动作已经是开菜单了，再放一个按钮做同一件事，是给同一个动作
                两个入口、两个 tab 停留点、两套 aria 名字。› 讲的是另一件事：
                「这一行按下去还有一层」—— 和 macOS 菜单里的子菜单箭头同一个意思。
              -->
              {#if hasMenu(it)}
                <span class="rm" aria-hidden="true">›</span>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
      <!-- 脚栏和随处搜索长一样：两个都是「⌘ 系浮层 + 过滤 + 列表 + 键盘驱动」 -->
      {#if rowMenu}
        <ContextMenu
          x={rowMenu.x}
          y={rowMenu.y}
          title={rowMenu.item.label}
          label="{rowMenu.item.label} 的操作"
          items={rowMenuItems}
          onclose={() => (rowMenu = null)}
        />
      {/if}

      <div class="foot">
        <span><kbd>↑↓</kbd> 选择</span>
        <span><kbd>↵</kbd> {items[sel] ? ENTER_LABEL[items[sel].kind] : "确认"}</span>
        <span class="gap"></span>
        <span><kbd>esc</kbd> 关闭</span>
      </div>
    {:else}
      <div class="form">
        <div class="ftitle">新建工作树</div>
        <p class="fdesc">
          工作树是同一个仓库的第二个检出目录 —— 可以在不打断当前改动的前提下，
          把另一个分支同时摊开在磁盘上。
        </p>
        <label>
          <span>目录</span>
          <input
            class="fi"
            bind:value={wtDir}
            placeholder="../lite-ide-hotfix 或绝对路径"
            spellcheck="false"
            onkeydown={(e) => e.key === "Enter" && submitWorktree()}
          />
        </label>
        <label>
          <span>分支</span>
          <input
            class="fi"
            bind:value={wtBranch}
            placeholder="留空则用目录名，分支不存在就新建"
            spellcheck="false"
            onkeydown={(e) => e.key === "Enter" && submitWorktree()}
          />
        </label>
        <div class="frow">
          <button onclick={() => (mode = "list")}>返回</button>
          <span class="gap"></span>
          <button class="primary" disabled={!wtDir.trim()} onclick={submitWorktree}>创建并打开</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 40; }
  .popup {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(620px, 88vw);
    max-height: 66vh;
    display: flex;
    flex-direction: column;
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop);
    z-index: 41;
    overflow: hidden;
  }
  /*
   * 挂在挂件底下时：不居中、窄一档（挨着窗口左上角，620px 会横穿大半个屏幕），
   * 高度也收一点 —— 从标题栏往下掉的浮层贴到屏幕底边就不像「掉下来」了。
   * left/top 由上面那个 effect 写进 style，这里只负责关掉居中那套。
   */
  .popup.anchored {
    top: 0;
    left: 0;
    transform: none;
    width: min(520px, calc(100vw - 16px));
    max-height: min(70vh, calc(100vh - 60px));
  }
  .popup > input {
    border: none;
    border-bottom: 1px solid var(--border-soft);
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 14px;
    padding: 11px 14px;
    outline: none;
  }
  .popup > input::placeholder { color: var(--text-faint); }

  .results { overflow-y: auto; padding: 4px 0; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; }
  .none.err { color: var(--lvl-error); font-family: var(--code-font); text-align: left; }
  .sec {
    /* 列表一长，分组头一滚就没了 —— 吸顶。图标那条是它的补充，不是替代 */
    position: sticky;
    top: 0;
    z-index: 1;
    background: var(--elevated);
    padding: 7px 14px 3px;
    font-size: 10.5px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
  }

  /* 当前项是内缩圆角块，和随处搜索、文件树、大纲同一套 */
  .rowwrap {
    display: flex;
    align-items: center;
    margin: 0 6px;
    border-radius: var(--r-md);
  }
  .rowwrap.on { background: var(--selected); }
  .divider { height: 1px; background: var(--border-soft); margin: 6px 14px; }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 4px 0 8px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .rowwrap.on .row { color: var(--text); }
  .lb { flex: none; max-width: 46%; overflow: hidden; text-overflow: ellipsis;
        font-family: var(--code-font); font-size: 12px; }
  .lb.cur { color: var(--accent); }
  .ht {
    flex: 1;
    min-width: 0;
    color: var(--text-faint);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .now, .tg {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: var(--r-md);
    background: var(--elevated-hi);
    color: var(--text-faint);
  }
  .now { color: var(--accent); }
  /*
   * 落后/领先只是**信息**，不做成按钮：这个项目没有 pull / push
   * （gitsvc 不起网络子进程），点了没有下文比不显示更糟。
   */
  .ab { flex: none; font-family: var(--code-font); font-size: 10.5px; color: var(--accent); }
  /*
   * 行尾的子菜单记号。这个格子换过两轮：
   *
   * 1. 常驻的 ✕（移除工作树）—— 一个**会删目录**的动作和「打开」只隔几个像素
   * 2. hover 才显形的 ⋯（更多操作）—— 那时主动作是「直接切」，菜单是副入口
   * 3. 现在：常驻的 ›
   *
   * 第三轮是跟着「按一行 = 打开它的动作菜单」一起改的。既然按下去必然有一层，
   * 这个记号就**不能等 hover 才出现** —— 它要在人按之前就说清「这里还有一层」，
   * 否则每一行看着都像会当场执行。macOS 菜单的子菜单箭头就是这个作用。
   *
   * 常驻但压得很暗（选中那行才提亮）：几十行列表里，一列整齐的 › 是背景纹理，
   * 不该和分支名抢注意力。
   */
  .rm {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    margin-right: 8px;
    color: var(--text-faint);
    font-size: 13px;
    line-height: 1;
    opacity: 0.45;
  }
  .rowwrap:hover .rm, .rowwrap.on .rm { opacity: 1; }

  /*
   * 四种条目四个形状。原来是四个 9px 的圆/方框，只靠边框颜色区分 ——
   * 在 12px 的行里那点色差根本读不出来，九行看着就是九个一样的圈。
   */
  .ic { flex: none; display: flex; color: var(--text-faint); }
  .ic.branch { color: var(--git-modified); }
  .ic.remote { color: var(--lvl-info); }
  .ic.worktree { color: var(--git-renamed); }
  .ic.cur { color: var(--git-added); }

  /* 与随处搜索的脚栏一模一样：两个都是「⌘ 系浮层 + 过滤 + 列表 + 键盘驱动」 */
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 14px;
    border-top: 1px solid var(--border-soft);
    background: var(--chrome-scrim);
    font-size: 10.5px;
    color: var(--text-faint);
    user-select: none;
  }
  .foot .gap { flex: 1; }
  kbd {
    font-family: var(--code-font);
    font-size: 10px;
    background: var(--hover);
    border-radius: 4px;
    padding: 1px 5px;
    margin-right: 3px;
  }

  .form { padding: 16px 18px 14px; display: flex; flex-direction: column; gap: 10px; }
  .ftitle { font-size: 14px; color: var(--text); }
  .fdesc { margin: 0; font-size: 11.5px; line-height: 1.6; color: var(--text-faint); }
  .form label { display: flex; align-items: center; gap: 10px; font-size: 12px; color: var(--text-dim); }
  .form label > span { flex: none; width: 34px; }
  .fi {
    flex: 1;
    background: var(--elevated-hi);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: 12px;
    padding: 5px 8px;
    outline: none;
  }
  .fi:focus { border-color: var(--accent); }
  .frow { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
  .frow .gap { flex: 1; }
  .frow button {
    padding: 4px 12px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: 12px;
    cursor: default;
  }
  .frow button:hover { background: var(--hover); color: var(--text); }
  .frow button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .frow button.primary:disabled { background: transparent; border-color: var(--border); color: var(--text-faint); }
</style>
