<script lang="ts">
  /**
   * 问题工具窗（⌘6，照 IDEA 的 Problems；docs/TAKEOVER.md G4 / I5）：两页 ——「当前文件」和「项目」。
   *
   * 两个来源，各自说清是哪来的（`state/problems.svelte.ts` 头上写着为什么只有开着的文件是实时的）：
   * - 语言服务器对**开着的文件**的实时检查（改一个字就更新）
   * - 上次 **⌘F9 构建**（Maven 编全项目）的编译错误 —— 没开的文件里的错只能从这儿知道
   *
   * 点一条跳过去（IDEA 的问题窗默认也是单击就跳）
   */
  import Icon from "../shell/Icon.svelte";
  import { problems } from "../state/problems.svelte";
  import { tabs } from "../state/tabs.svelte";
  import { runs } from "../state/runs.svelte";
  import { nav } from "../state/nav.svelte";
  import { runTask } from "../state/runs-ops";

  let { root }: { root: string | null } = $props();

  let page = $state<"file" | "project">("file");

  const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const rel = (p: string) => (root && p.startsWith(root + "/") ? p.slice(root.length + 1) : p);

  const activePath = $derived(tabs.active?.path ?? "");
  const current = $derived(problems.byPath[activePath] ?? null);
  const live = $derived(
    Object.values(problems.byPath)
      .filter((f) => !root || f.root === root)
      .sort((a, b) => (a.path === activePath ? -1 : b.path === activePath ? 1 : a.path.localeCompare(b.path))),
  );
  /** 上次 ⌘F9 那一格（同 IDEA 的 Build 窗，「构建项目」「重新构建项目」两个名字） */
  const build = $derived(
    [...runs.list].reverse().find((r) => (r.name === "构建项目" || r.name === "重新构建项目") && (!root || r.root === root)) ?? null,
  );
  const buildByFile = $derived.by(() => {
    const m = new Map<string, { line: number; col: number; error: boolean; message: string }[]>();
    for (const p of build?.problems?.items ?? []) {
      if (!m.has(p.path)) m.set(p.path, []);
      m.get(p.path)!.push(p);
    }
    return [...m];
  });
  const liveCount = $derived(live.reduce((n, f) => n + f.total, 0));
  const buildCount = $derived(build?.problems?.total ?? 0);
</script>

<div class="problems">
  <div class="bar" role="tablist" aria-label="问题">
    <button class="pg" class:on={page === "file"} role="tab" aria-selected={page === "file"} onclick={() => (page = "file")}>
      当前文件{#if current}<span class="n">{current.total}</span>{/if}
    </button>
    <button class="pg" class:on={page === "project"} role="tab" aria-selected={page === "project"} onclick={() => (page = "project")}>
      项目{#if liveCount + buildCount}<span class="n">{liveCount + buildCount}</span>{/if}
    </button>
  </div>

  <div class="body">
    {#if page === "file"}
      {#if !activePath}
        <div class="empty-msg">没有打开的文件</div>
      {:else if !activePath.endsWith(".java")}
        <div class="empty-msg">
          这个文件没有实时检查
          <div class="sub">现在只有 Java 文件有（语言服务器）；全项目的编译错误看「项目」那页</div>
        </div>
      {:else if !current}
        <div class="empty-msg ok-msg"><Icon name="check" /> {name(activePath)} 没有错误和警告</div>
      {:else}
        {@render list(current.path, current.items.map((d) => ({ line: d.line + 1, col: d.character + 1, error: d.severity === 1, message: d.message })), current.total, false)}
      {/if}
    {:else}
      <div class="sec">开着的文件 <span class="src">语言服务器，实时</span></div>
      {#if live.length === 0}
        <div class="none">开着的文件里没有错误和警告</div>
      {:else}
        {#each live as f (f.path)}
          {@render list(f.path, f.items.map((d) => ({ line: d.line + 1, col: d.character + 1, error: d.severity === 1, message: d.message })), f.total, true)}
        {/each}
      {/if}

      <div class="sec">
        上次构建 <span class="src">⌘F9，Maven 编全项目{build?.status === "running" ? " · 正在构建…" : ""}</span>
        <span class="gap"></span>
        {#if root}
          <button class="btn sm" disabled={build?.status === "running"} onclick={() => void runTask("构建项目", root)}>构建项目</button>
        {/if}
      </div>
      {#if !build}
        <div class="none">尚未构建。未打开文件中的编译错误需构建后才能显示（⌘F9）</div>
      {:else if build.status === "running"}
        <div class="none"><span class="spinner sm"></span> 正在构建</div>
      {:else if !buildByFile.length}
        <div class="none">{build.status === "failed" ? "构建失败，但未识别到编译错误，请查看运行窗口中的输出（⌘4）" : "上次构建没有编译错误"}</div>
      {:else}
        {#each buildByFile as [path, items] (path)}
          {@render list(path, items, items.length, true)}
        {/each}
        {#if (build.problems?.total ?? 0) > (build.problems?.items.length ?? 0)}
          <div class="none">只列了前 {build.problems?.items.length} 条，一共 {build.problems?.total} 条</div>
        {/if}
      {/if}
    {/if}
  </div>
</div>

{#snippet list(path: string, items: { line: number; col: number; error: boolean; message: string }[], total: number, header: boolean)}
  {#if header}
    <div class="file" title={path}><span class="fn">{name(path)}</span><span class="dir">{rel(path)}</span><span class="n">{total}</span></div>
  {/if}
  {#each [...items].sort((a, b) => Number(b.error) - Number(a.error) || a.line - b.line) as p, i (i)}
    <button class="row" class:nested={header} onclick={() => void nav.openAt(path, p.line, false, p.col)} title="跳到 {name(path)}:{p.line}">
      <span class="sev" class:error={p.error} aria-label={p.error ? "错误" : "警告"}></span>
      <span class="msg">{p.message}</span>
      <span class="loc">{p.line}:{p.col}</span>
    </button>
  {/each}
  {#if total > items.length}<div class="none" class:nested={header}>还有 {total - items.length} 条没列出来（只取了前 {items.length} 条）</div>{/if}
{/snippet}

<style>
  .problems { display: flex; flex-direction: column; height: 100%; background: var(--content-bg); }
  .bar { flex: none; display: flex; gap: 2px; padding: 4px 8px; border-bottom: 1px solid var(--border-soft); }
  .pg {
    display: flex;
    align-items: center;
    gap: 6px;
    height: 24px;
    padding: 0 10px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--fs-sm);
    cursor: pointer;
  }
  .pg:hover { background: var(--hover); }
  .pg.on { background: var(--selected); color: var(--text); }
  .n { color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .body { flex: 1; min-height: 0; overflow: auto; padding: 4px 0 8px; }
  .sec {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 12px 4px;
    font-size: var(--fs-sm);
    font-weight: 600;
    color: var(--text);
  }
  .sec .src { font-weight: 400; color: var(--text-faint); }
  .gap { flex: 1; }
  .none { padding: 4px 12px; font-size: var(--fs-sm); color: var(--text-muted); }
  .none.nested { padding-left: 28px; }
  .file {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 12px 2px;
    font-size: var(--fs-sm);
    overflow: hidden;
    white-space: nowrap;
  }
  .fn { color: var(--text); }
  .dir { min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    min-height: 24px;
    padding: 2px 12px;
    border: none;
    background: none;
    color: var(--text-dim);
    font: inherit;
    font-size: var(--fs-sm);
    text-align: left;
    cursor: pointer;
  }
  .row.nested { padding-left: 28px; }
  .row:hover { background: var(--hover); }
  .row:focus-visible { outline: none; background: var(--selected); }
  .sev { flex: none; width: 8px; height: 8px; border-radius: 99px; background: var(--lvl-warn); }
  .sev.error { background: var(--lvl-error); }
  .msg { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text); }
  .loc { flex: none; color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .ok-msg { display: flex; align-items: center; gap: 6px; }
</style>
