<script lang="ts">
  import Icon from "../shell/Icon.svelte";
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
  } = $props();

  let message = $state("");
  let amend = $state(false);
  /**
   * 提交框展不展开。
   *
   * 没东西可提交时，那个空输入框加按钮占着约 110px，还摆出一副「可以提交」
   * 的样子；而「工作区干净」这条真正的信息被顶到一片空白的下面。
   * 有暂存内容、或者用户主动点开、或者已经写了一半时才展开。
   */
  let composing = $state(false);
  let box = $state<HTMLTextAreaElement | null>(null);

  function beginCompose() {
    composing = true;
    // 等 textarea 真的出现在 DOM 里再聚焦
    queueMicrotask(() => box?.focus());
  }

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
  <div class="head">
    <span class="title">GIT</span>
    <span class="gap"></span>
    <button class="act" onclick={onRefresh} title="刷新状态" aria-label="刷新" class:spin={busy}>
      <Icon name="refresh" size={13} />
    </button>
  </div>

  {#if !status}
    <div class="hint">不是 Git 仓库</div>
  {:else}
    <div class="branch" title={status.upstream ? `跟踪 ${status.upstream}` : "没有设置上游分支"}>
      <span class="bicon"><Icon name="git" size={12} /></span>
      <span class="bname">{status.branch || "（无分支）"}</span>
      {#if status.detached}<span class="tagx">游离</span>{/if}
      {#if status.unborn}<span class="tagx">尚无提交</span>{/if}
      {#if status.ahead}<span class="ab">↑{status.ahead}</span>{/if}
      {#if status.behind}<span class="ab">↓{status.behind}</span>{/if}
    </div>

    {#if !(composing || staged.length > 0 || message.trim() !== "")}
      <button class="collapsed" onclick={beginCompose} title="写提交信息">
        {#if conflicts.length > 0}
          <span class="conflict-mark"><Icon name="warn" size={12} /></span>
          <span class="warn">先解决 {conflicts.length} 处冲突</span>
        {:else if status.entries.length === 0}
          <Icon name="check" size={12} />
          <span>工作区干净</span>
        {:else}
          <Icon name="plus" size={12} />
          <span>写提交信息…</span>
        {/if}
      </button>
    {:else}
    <div class="commit">
      <textarea
        bind:this={box}
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
          <button class="mini" onclick={() => onDiscard(unstaged)}>全部丢弃</button>
          <button class="mini" onclick={() => onStage(unstaged.map((e) => e.path))}>全部暂存</button>
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

      {#if status.entries.length === 0}
        <div class="hint clean">工作区干净</div>
      {/if}
      {#if status.truncated}
        <div class="hint">改动过多，只列出了前 5000 条</div>
      {/if}
    </div>
  {/if}
</div>

<style>
  .git {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    flex: none;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 11px;
    letter-spacing: 0.06em;
    color: var(--text-dim);
    user-select: none;
  }
  .head .gap, .sec .gap, .crow .gap { flex: 1; }
  .act {
    flex: none;
    display: grid;
    place-content: center;
    width: 22px;
    height: 22px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-faint);
    cursor: default;
  }
  .act:hover { background: var(--panel-bg-2); color: var(--text); }
  .act.spin { color: var(--accent); }

  .branch {
    flex: none;
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 7px 10px;
    font-size: 12px;
    color: var(--text);
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
  }
  .bicon { flex: none; color: var(--text-faint); }
  .bname {
    font-family: var(--code-font);
    font-size: 11.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .ab { font-family: var(--code-font); font-size: 11px; color: var(--accent); flex: none; }
  .tagx {
    flex: none;
    font-size: 10px;
    color: var(--lvl-warn);
    border: 1px solid var(--lvl-warn);
    border-radius: 3px;
    padding: 0 4px;
    opacity: 0.8;
  }

  .commit { flex: none; padding: 8px 10px; border-bottom: 1px solid var(--border-soft); }
  .collapsed {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    padding: 7px 10px;
    background: transparent;
    border: none;
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11.5px;
    text-align: left;
    cursor: default;
  }
  .collapsed:hover { background: var(--panel-bg-2); color: var(--text-dim); }
  .collapsed .warn { color: var(--lvl-warn); }
  .commit textarea {
    width: 100%;
    resize: vertical;
    background: var(--editor-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 12px;
    line-height: 1.5;
    padding: 6px 8px;
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
    border-radius: 4px;
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
    background: var(--panel-bg-2);
    border-radius: 8px;
    padding: 0 5px;
    font-size: 10px;
  }
  .mini {
    background: transparent;
    border: none;
    color: var(--text-faint);
    font-size: 10.5px;
    padding: 1px 5px;
    border-radius: 3px;
    cursor: default;
    text-transform: none;
    letter-spacing: 0;
  }
  .mini:hover { background: var(--panel-bg-2); color: var(--text); }

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
    border-radius: 3px;
  }
  .rowact:hover { background: var(--panel-bg); color: var(--text); }
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
  .hint.clean { text-align: center; }
</style>
