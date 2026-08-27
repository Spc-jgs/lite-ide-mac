<script lang="ts">
  import { layout, laneColor } from "./graph";
  import type { GitEntry, GitLogEntry } from "../ipc/commands";
  import { gitLogEntries, gitCommitFiles } from "../ipc/commands";

  let {
    repo,
    /** 当前编辑的文件（绝对路径），用于「只看这个文件的历史」 */
    filePath = "",
    onOpenCommitDiff,
  }: {
    repo: string;
    filePath?: string;
    onOpenCommitDiff: (sha: string, short: string, path: string) => void;
  } = $props();

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
      <label class="chk"><input type="checkbox" bind:checked={all} /> 全部分支</label>
      <label class="chk" class:off={!filePath}>
        <input type="checkbox" bind:checked={onlyFile} disabled={!filePath} /> 只看当前文件
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
                        fill="var(--panel-bg)" stroke={laneColor(g.lane)} stroke-width="2" />
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
      <div class="detail">
        <div class="dsubject">{picked.subject}</div>
        <div class="dmeta">
          <span class="sha">{picked.short}</span>
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
          <div class="dhead">{files.length} 个文件</div>
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
  .right {
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-left: 1px solid var(--border);
    background: var(--panel-bg);
  }

  .tools {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 4px 8px;
    background: var(--panel-bg);
    border-bottom: 1px solid var(--border);
    font-size: 11px;
    color: var(--text-dim);
    user-select: none;
  }
  .tools .gap { flex: 1; }
  .q {
    width: 200px;
    background: var(--editor-bg);
    border: 1px solid var(--border);
    border-radius: 3px;
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
  .crow {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 22px;
    padding: 0 10px 0 4px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .crow:hover { background: var(--panel-bg-2); }
  .crow.on { background: var(--accent-sel); color: var(--text); }
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
    border-radius: 8px;
    font-size: 10px;
    font-family: var(--code-font);
    background: var(--panel-bg-2);
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
  .dmeta .sha { font-family: var(--code-font); color: var(--accent); }
  .tagx {
    font-size: 10px;
    color: var(--lvl-warn);
    border: 1px solid var(--lvl-warn);
    border-radius: 3px;
    padding: 0 4px;
    opacity: 0.85;
  }

  .dfiles { flex: 1; overflow: auto; }
  .dhead {
    padding: 7px 10px 4px;
    font-size: 10.5px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
  }
  .drow {
    display: flex;
    align-items: center;
    gap: 6px;
    width: 100%;
    height: 21px;
    padding: 0 8px 0 10px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .drow:hover { background: var(--panel-bg-2); }
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
