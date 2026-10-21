<script lang="ts">
  /**
   * 构建（⌘F9，docs/IDEA.md 第 22 节）那一格左边的错误树，照 IDEA 的 Build 窗：
   *
   * ```
   * ✗ 2 个错误 · 1 个警告                    [重新构建]
   * ▾ OrderApplication.java   order-service/src/main/java/…
   *     ✗ 找不到符号            :13         ← 点一下：展开补充（符号、位置）；双击：跳到那一行那一列
   * ```
   *
   * 错误是 runs-ops 在构建结束后从这次的输出里认出来的（`run.problems`，`javac.rs`）。构建失败了却一条都没认出来 ——
   * 不是编译错误（依赖没下下来、pom 写错），照实说、原因在右边的输出里。
   */
  import Icon from "../shell/Icon.svelte";
  import type { RunTab } from "../state/runs.svelte";
  import type { BuildProblem } from "../ipc/commands";
  import { runTask } from "../state/runs-ops";
  import { nav } from "../state/nav.svelte";

  let { run }: { run: RunTab } = $props();

  let sel = $state<BuildProblem | null>(null);
  let closed = $state(new Set<string>());

  let items = $derived(run.problems?.items ?? []);
  let groups = $derived.by(() => {
    const m = new Map<string, BuildProblem[]>();
    for (const p of items) m.set(p.path, [...(m.get(p.path) ?? []), p]);
    // 有错误的文件排前面，同一档按路径
    return [...m].sort(([a, x], [b, y]) => Number(y.some((p) => p.error)) - Number(x.some((p) => p.error)) || a.localeCompare(b));
  });
  let nErr = $derived(items.filter((p) => p.error).length);
  let nWarn = $derived(items.length - nErr);
  let running = $derived(run.status === "running" || run.status === "stopping");

  const file = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const dir = (p: string) => {
    const rel = p.startsWith(`${run.root}/`) ? p.slice(run.root.length + 1) : p;
    return rel.slice(0, Math.max(0, rel.lastIndexOf("/")));
  };
  const first = (m: string) => m.split("\n")[0];

  function toggle(k: string) {
    const n = new Set(closed);
    if (n.has(k)) n.delete(k);
    else n.add(k);
    closed = n;
  }
</script>

<div class="bt">
  <div class="bar">
    {#if running}
      <span class="sum"><span class="spinner sm"></span> 正在构建…</span>
    {:else if items.length}
      <span class="sum" class:bad={nErr > 0}>
        <span class="ic"><Icon name={nErr ? "error" : "warn"} /></span>
        {nErr ? `${nErr} 个错误` : ""}{nErr && nWarn ? " · " : ""}{nWarn ? `${nWarn} 个警告` : ""}{run.problems && run.problems.total > items.length ? `（只列了前 ${items.length} / ${run.problems.total}）` : ""}
      </span>
    {:else if run.status === "done"}
      <span class="sum ok"><span class="ic"><Icon name="check" /></span> 构建成功</span>
    {:else if run.status === "stopped"}
      <span class="sum">已停止</span>
    {:else}
      <span class="sum bad">构建失败</span>
    {/if}
    <span class="gap"></span>
    <button class="ibtn" disabled={running} onclick={() => void runTask(run.name, run.root)} title="再构建一次（⌃R）" aria-label="再构建一次"><Icon name="rerun" /></button>
  </div>
  <div class="tree" role="tree" aria-label="构建结果">
    {#if !running && !items.length && run.status === "failed"}
      <div class="empty-msg bad">
        构建失败了，但不是编译错误
        <div class="sub">多半是依赖没下下来、pom 写错、插件报错 —— 原因在右边的输出里</div>
      </div>
    {:else if !running && !items.length && run.status === "done"}
      <div class="empty-msg">
        没有错误
        <div class="sub">主代码和测试代码都编过了（mvn test-compile）</div>
      </div>
    {/if}
    {#each groups as [path, list] (path)}
      {@const bad = list.some((p) => p.error)}
      <div class="row file" role="treeitem" aria-selected="false" aria-expanded={!closed.has(path)} tabindex="-1" onclick={() => toggle(path)} onkeydown={() => {}} title={path}>
        <span class="chev" class:open={!closed.has(path)}><Icon name="chevron-right" /></span>
        <span class="ic"><Icon name={bad ? "error" : "warn"} /></span>
        <span class="name">{file(path)}</span>
        <span class="where">{dir(path)}</span>
      </div>
      {#if !closed.has(path)}
        {#each list as p, i (i)}
          <div
            class="row item"
            class:on={sel === p}
            role="treeitem"
            aria-selected={sel === p}
            tabindex="-1"
            onclick={() => (sel = sel === p ? null : p)}
            ondblclick={() => void nav.openAt(p.path, p.line, false, p.col)}
            onkeydown={() => {}}
            title="双击跳到这一行"
          >
            <span class="ic"><Icon name={p.error ? "error" : "warn"} /></span>
            <span class="name">{first(p.message)}</span>
            <span class="where">:{p.line}</span>
          </div>
          {#if sel === p && p.message.includes("\n")}
            <pre class="detail">{p.message.split("\n").slice(1).join("\n")}</pre>
          {/if}
        {/each}
      {/if}
    {/each}
  </div>
</div>

<style>
  .bt { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  /* 岛内的工具栏：34 高（ui.md 一之二），同 TestTree */
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 2px; padding: 0 6px 0 10px; border-bottom: 1px solid var(--border-soft); }
  .bar .gap { flex: 1; }
  .sum { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-sm); color: var(--text-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sum.bad { color: var(--lvl-error); }
  .sum.ok .ic { color: var(--git-added); }
  .tree { flex: 1; overflow: auto; padding: 4px 6px 8px; }
  .row { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 8px 0 4px; border-radius: var(--r-sm); color: var(--text-dim); font-size: var(--fs-md); white-space: nowrap; cursor: default; }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .row.item { padding-left: 24px; }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .where { flex: none; margin-left: auto; padding-left: 8px; color: var(--text-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; }
  .row.file .where { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  /* javac 的补充（符号、位置）：原样的编译器输出，等宽（ui.md 八之二） */
  .detail { margin: 2px 8px 6px 46px; padding: 6px 8px; border-radius: var(--r-sm); background: var(--hover); color: var(--text-dim); font-family: var(--code-font); font-size: var(--fs-sm); white-space: pre-wrap; }
</style>
