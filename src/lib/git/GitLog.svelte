<script lang="ts">
  import { layout, laneColor } from "./graph";
  import type { GitEntry, GitLogEntry } from "../ipc/commands";
  import { gitLogEntries, gitCommitFiles } from "../ipc/git";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import { copyText } from "../shell/pathactions";

  let {
    repo,
    /** 当前编辑的文件（绝对路径），用于「只看这个文件的历史」 */
    filePath = "",
    onOpenCommitDiff,
    onCheckout,
  }: {
    repo: string;
    filePath?: string;
    onOpenCommitDiff: (sha: string, short: string, path: string) => void;
    /** 右键「检出到此提交」：游离检出。被本地改动挡住那一问由上层（branches）接 */
    onCheckout?: (sha: string) => void;
  } = $props();

  /**
   * 提交行的右键菜单（issue #33 ⑬）：复制哈希 / 复制提交信息 / 检出到此提交。
   * IDEA 那份还有 cherry-pick、和本地比较，先放这三条 —— 都是「看着历史顺手要做」的。
   */
  let cmenu = $state<{ x: number; y: number; c: GitLogEntry } | null>(null);
  let cmenuItems = $derived.by((): MenuItem[] => {
    const c = cmenu?.c;
    if (!c) return [];
    return [
      { label: "复制哈希", run: () => void copyText(c.sha, "哈希") },
      { label: "复制提交信息", run: () => void copyText(c.subject, "提交信息") },
      ...(onCheckout
        ? [{ label: `检出到此提交（游离）`, sep: true, run: () => onCheckout(c.sha) }]
        : []),
    ];
  });
  function openCmenu(e: MouseEvent, c: GitLogEntry) {
    e.preventDefault();
    picked = c; // 右键也要选中 —— 菜单作用在哪条上不能只靠人自己记
    cmenu = { x: e.clientX, y: e.clientY, c };
  }

  /** 一次拉多少条。再多就该做分页了，个人项目里 300 条足够翻很久 */
  const LIMIT = 300;

  let all = $state(true);
  let onlyFile = $state(false);
  let commits = $state<GitLogEntry[]>([]);
  let loading = $state(false);
  let err = $state("");
  let picked = $state<GitLogEntry | null>(null);
  let files = $state<GitEntry[]>([]);
  let filesLoading = $state(false);
  /** 关键字过滤，在已拉到的这批里筛，不再往 git 跑一趟 */
  let q = $state("");

  const ROW_H = 22;
  const LANE_W = 13;
  const DOT_R = 3.4;

  let rel = $derived(
    onlyFile && filePath.startsWith(`${repo}/`) ? filePath.slice(repo.length + 1) : "",
  );

  // 条件变了就重拉。repo / all / onlyFile 任一变化都要重来
  $effect(() => {
    const r = repo;
    const a = all;
    const p = rel;
    void (async () => {
      loading = true;
      err = "";
      try {
        commits = await gitLogEntries(r, LIMIT, a, p);
        picked = commits[0] ?? null;
      } catch (e) {
        err = String(e);
        commits = [];
      } finally {
        loading = false;
      }
    })();
  });

  // 选中的提交换了就拉它动过哪些文件
  $effect(() => {
    const c = picked;
    const r = repo;
    if (!c) {
      files = [];
      return;
    }
    void (async () => {
      filesLoading = true;
      try {
        files = await gitCommitFiles(r, c.sha);
      } catch {
        files = [];
      } finally {
        filesLoading = false;
      }
    })();
  });

  let shown = $derived.by(() => {
    const k = q.trim().toLowerCase();
    if (!k) return commits;
    return commits.filter(
      (c) =>
        c.subject.toLowerCase().includes(k) ||
        c.author.toLowerCase().includes(k) ||
        c.short.includes(k),
    );
  });

  /**
   * 泳道图只在**没有过滤**时画。
   * 过滤之后剩下的提交之间父子关系是断的，硬画出来的线是假的 ——
   * 一条看起来直连的线，中间可能隔着二十条被筛掉的提交。宁可不画。
   */
  let graph = $derived(q.trim() ? null : layout(commits));

  const MARK: Record<string, { ch: string; cls: string }> = {
    M: { ch: "M", cls: "modified" },
    A: { ch: "A", cls: "added" },
    D: { ch: "D", cls: "deleted" },
    R: { ch: "R", cls: "renamed" },
    C: { ch: "C", cls: "renamed" },
  };

  const baseName = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const dirName = (p: string) => {
    const i = p.lastIndexOf("/");
    return i < 0 ? "" : p.slice(0, i);
  };

  const x = (lane: number) => lane * LANE_W + LANE_W / 2;

  /**
   * 提交列表的键盘导航。浏览历史是「一条条往下看」的动作，
   * 每看一条都要摸鼠标是很累的。
   */
  let rowEls: HTMLButtonElement[] = [];

  function onRowKey(e: KeyboardEvent, i: number) {
    // 键盘也能开菜单（⇧F10 / ContextMenu），同文件树、标签栏
    if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
      e.preventDefault();
      const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
      picked = shown[i];
      cmenu = { x: r.left + 24, y: r.bottom + 2, c: shown[i] };
      return;
    }
    let to = -1;
    if (e.key === "ArrowDown") to = i + 1;
    else if (e.key === "ArrowUp") to = i - 1;
    else if (e.key === "Home") to = 0;
    else if (e.key === "End") to = shown.length - 1;
    else return;
    e.preventDefault();
    to = Math.min(Math.max(0, to), shown.length - 1);
    picked = shown[to] ?? null;
    rowEls[to]?.focus();
  }

  /** 在过滤框里按 ↓ 直接跳进列表，不用先摸一下鼠标 */
  function onQueryKey(e: KeyboardEvent) {
    if (e.key !== "ArrowDown" || shown.length === 0) return;
    e.preventDefault();
    picked = shown[0];
    rowEls[0]?.focus();
  }
</script>

<div class="log">
  <div class="left">
    <div class="tools">
      <input
        class="q"
        bind:value={q}
        onkeydown={onQueryKey}
        placeholder="过滤标题 / 作者 / sha"
        spellcheck="false"
      />
      <!-- title 是给窄窗准备的：栏只有 190px 时字会被省略到只剩复选框 -->
      <label class="chk" title="全部分支"><input type="checkbox" bind:checked={all} /> <span>全部分支</span></label>
      <label class="chk" class:off={!filePath} title="只看当前文件">
        <input type="checkbox" bind:checked={onlyFile} disabled={!filePath} /> <span>只看当前文件</span>
      </label>
      <span class="gap"></span>
      <span class="cnt">
        {#if loading}载入中…{:else}{shown.length}{q.trim() ? ` / ${commits.length}` : ""} 条{/if}
      </span>
    </div>

    <div class="rows">
      {#if err}
        <div class="msg err">{err}</div>
      {:else if !loading && commits.length === 0}
        <div class="msg">还没有提交</div>
      {:else}
        {#each shown as c, i (c.sha)}
          {@const g = graph?.rows[i]}
          <button
            bind:this={rowEls[i]}
            class="crow"
            class:on={picked?.sha === c.sha}
            onclick={() => (picked = c)}
            oncontextmenu={(e) => openCmenu(e, c)}
            onkeydown={(e) => onRowKey(e, i)}
            title={c.subject}
          >
            {#if graph && g}
              <svg
                class="g"
                width={graph.width * LANE_W}
                height={ROW_H}
                viewBox="0 0 {graph.width * LANE_W} {ROW_H}"
                aria-hidden="true"
              >
                <!-- 直穿本行、与本提交无关的泳道 -->
                {#each g.through as j}
                  <line x1={x(j)} y1="0" x2={x(j)} y2={ROW_H}
                        stroke={laneColor(j)} stroke-width="1.5" />
                {/each}
                <!-- 从上方汇入本提交的分支 -->
                {#each g.ins as j}
                  <path d="M{x(j)} 0 C{x(j)} {ROW_H / 2} {x(g.lane)} {ROW_H / 2} {x(g.lane)} {ROW_H / 2}"
                        fill="none" stroke={laneColor(j)} stroke-width="1.5" />
                {/each}
                <!-- 从本提交往下走的线 -->
                {#each g.outs as k}
                  {#if k === g.lane}
                    <line x1={x(g.lane)} y1={ROW_H / 2} x2={x(g.lane)} y2={ROW_H}
                          stroke={laneColor(k)} stroke-width="1.5" />
                  {:else}
                    <path d="M{x(g.lane)} {ROW_H / 2} C{x(k)} {ROW_H / 2} {x(k)} {ROW_H / 2} {x(k)} {ROW_H}"
                          fill="none" stroke={laneColor(k)} stroke-width="1.5" />
                  {/if}
                {/each}
                <circle cx={x(g.lane)} cy={ROW_H / 2} r={DOT_R}
                        fill="var(--content-bg)" stroke={laneColor(g.lane)} stroke-width="2" />
              </svg>
            {/if}
            <span class="subject">
              {#each c.refs as r}
                <span class="ref" class:head={r === "HEAD"} class:remote={r.includes("/")}>{r}</span>
              {/each}
              {c.subject}
            </span>
            <span class="who">{c.author}</span>
            <span class="when">{c.when}</span>
          </button>
        {/each}
      {/if}
    </div>
  </div>

  <div class="right">
    {#if picked}
      <div class="dtools">
        <span class="sha">{picked.short}</span>
        <span class="gap"></span>
        {#if !filesLoading}<span>{files.length} 个文件</span>{/if}
      </div>
      <div class="detail">
        <div class="dsubject">{picked.subject}</div>
        <div class="dmeta">
          <span>{picked.author}</span>
          <span class="dim">{picked.email}</span>
          <span class="dim">{picked.date} · {picked.when}</span>
        </div>
        {#if picked.parents.length > 1}
          <div class="dmeta"><span class="tagx">合并提交 · {picked.parents.length} 个父</span></div>
        {/if}
      </div>
      <div class="dfiles">
        {#if filesLoading}
          <div class="msg">载入中…</div>
        {:else if files.length === 0}
          <div class="msg">这次提交没有文件变化</div>
        {:else}
          {#each files as f (f.path)}
            <button
              class="drow"
              onclick={() => onOpenCommitDiff(picked!.sha, picked!.short, f.path)}
              title={f.orig ? `${f.orig} → ${f.path}` : f.path}
            >
              <span class="m {MARK[f.index]?.cls ?? 'modified'}">{MARK[f.index]?.ch ?? "M"}</span>
              <span class="fname">{baseName(f.path)}</span>
              <span class="fdir">{dirName(f.path)}</span>
            </button>
          {/each}
        {/if}
      </div>
    {:else}
      <div class="msg">选一条提交看详情</div>
    {/if}
  </div>
</div>

{#if cmenu}
  <ContextMenu x={cmenu.x} y={cmenu.y} label="{cmenu.c.short} 的操作" items={cmenuItems} onclose={() => (cmenu = null)} />
{/if}

<style>
  .log {
    display: grid;
    /* 左边列表吃掉主要宽度，右边详情固定一块 —— 与 IDEA 的日志窗同构 */
    grid-template-columns: 1fr 320px;
    height: 100%;
    overflow: hidden;
    background: var(--editor-bg);
  }
  .left { display: flex; flex-direction: column; overflow: hidden; }
  /*
   * 详情栏（M8）：不再压一层 --hover 的白 —— 岛里再抬一块浅色板，看着像另一个面板。
   * 和左边靠一条 5% 的线分开就够；第一行是和左边过滤条等高的头（sha · N 个文件），
   * 两栏的第一行在同一条水平线上。
   */
  .right {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-left: 1px solid var(--border-soft);
  }
  .dtools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    box-sizing: border-box;
    border-bottom: 1px solid var(--border-soft);
    font-size: 11px;
    color: var(--text-dim);
    user-select: none;
  }
  .dtools .sha { font-family: var(--code-font); color: var(--accent); }
  .dtools .gap { flex: 1; }

  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px; /* M8：工具栏统一 34，和右边详情栏的头齐平 */
    box-sizing: border-box;
    padding: 0 8px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border-soft); /* M8 */
    font-size: 11px;
    color: var(--text-dim);
    user-select: none;
  }
  .tools .gap { flex: 1; }
  /*
   * 窄窗（800px）时两个复选框曾经竖着叠成三行，把 34px 的条撑到 60。
   * 整行不许折；空间不够时按「输入框收缩 → 复选框的字省略 → 计数让位」的次序让。
   * 最小窗宽 720 时日志栏只有 ~190px，那时能保住的只有输入框和「全部分支」。
   */
  .tools { overflow: hidden; }
  .tools > * { flex: none; white-space: nowrap; }
  .tools .chk { flex: 0 1 auto; min-width: 0; }
  .tools .chk > span { overflow: hidden; text-overflow: ellipsis; }
  .q {
    flex: 0 1 200px;
    min-width: 80px;
    width: 200px;
    background: var(--elevated-hi);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 11.5px;
    padding: 2px 7px;
  }
  .q:focus { outline: none; border-color: var(--accent); }
  .chk { display: flex; align-items: center; gap: 4px; }
  .chk.off { opacity: 0.4; }
  .chk input { margin: 0; accent-color: var(--accent); }
  .cnt { font-family: var(--code-font); font-size: 10.5px; color: var(--text-faint); }

  .rows { flex: 1; overflow: auto; }
  /* 内缩的圆角块，和文件树 / 标签栏同一套（ui.md 第一条）—— 原来是通栏色条 */
  .crow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: calc(100% - 8px);
    margin: 0 4px;
    height: 22px;
    padding: 0 10px 0 4px;
    border-radius: var(--r-sm);
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .crow:hover { background: var(--hover); }
  .crow.on { background: var(--selected); color: var(--text); }
  .crow:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .g { flex: none; display: block; }
  .subject { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .who { flex: none; width: 110px; color: var(--text-faint); font-size: 11.5px;
         overflow: hidden; text-overflow: ellipsis; }
  .when { flex: none; width: 96px; color: var(--text-faint); font-size: 11px; text-align: right; }

  .ref {
    display: inline-block;
    margin-right: 5px;
    padding: 0 5px;
    border-radius: var(--r-sm); /* M8 */
    font-size: 10px;
    font-family: var(--code-font);
    background: var(--selected);
    color: var(--text-dim);
    border: 1px solid var(--border);
  }
  .ref.head { color: var(--accent); border-color: var(--accent); }
  .ref.remote { color: var(--git-untracked); }

  .detail {
    flex: none;
    padding: 10px;
    border-bottom: 1px solid var(--border-soft);
  }
  .dsubject { color: var(--text); font-size: 13px; line-height: 1.45; margin-bottom: 7px; }
  .dmeta {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
    font-size: 11px;
    color: var(--text-dim);
  }
  .dmeta .dim { color: var(--text-faint); }
  .tagx {
    font-size: 10px;
    color: var(--lvl-warn);
    border: 1px solid var(--lvl-warn);
    border-radius: var(--r-sm);
    padding: 0 4px;
    opacity: 0.85;
  }

  .dfiles { flex: 1; overflow: auto; }
  .drow {
    display: flex;
    align-items: center;
    gap: 6px;
    width: calc(100% - 8px);
    margin: 0 4px;
    height: 22px;
    padding: 0 8px 0 6px;
    border-radius: var(--r-sm);
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .drow:hover { background: var(--hover); }
  .drow:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .fname { flex: none; max-width: 60%; overflow: hidden; text-overflow: ellipsis; }
  .fdir {
    flex: 1;
    min-width: 0;
    color: var(--text-faint);
    font-size: 10.5px;
    overflow: hidden;
    text-overflow: ellipsis;
    /* 路径太长砍前面 —— 结尾的目录名才有辨识度 */
    direction: rtl;
    text-align: left;
  }
  .m {
    flex: none;
    width: 11px;
    text-align: center;
    font-family: var(--code-font);
    font-size: 10.5px;
    font-weight: 600;
  }
  .m.modified { color: var(--git-modified); }
  .m.added { color: var(--git-added); }
  .m.deleted { color: var(--git-deleted); }
  .m.renamed { color: var(--git-renamed); }

  .msg { padding: 16px 12px; color: var(--text-faint); font-size: 12px; text-align: center; }
  .msg.err { color: var(--lvl-error); font-family: var(--code-font); text-align: left; }
</style>
