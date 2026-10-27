<script lang="ts">
  /**
   * 跑单测那一格左边的结果树（docs/IDEA.md 第 17 节），照 IDEA 的 Run 窗口测试视图：
   *
   * ```
   * ✗ 3 个通过 · 1 个没过     [重跑] [重跑失败的]
   * ▾ ✗ OrderTest        0.05s
   *     ✓ adds           11ms
   *     ✗ totals         3ms      ← 点一下：下面展开原因和堆栈；双击：跳到这个方法
   * ```
   *
   * 结果是 runs-ops 读 surefire 报告填进来的（`run.tests`）：跑的过程中每秒读一次、一个类跑完亮一个（G15），跑完再读一次最终的；
   * 跑完一条都没有就照实说（编译没过多半是这样）。
   */
  import Icon from "../shell/Icon.svelte";
  import type { RunTab } from "../state/runs.svelte";
  import type { TestCase } from "../ipc/commands";
  import { classFile, classLabel, failedSelector, parseTestTask } from "../editor/java-tests";
  import { runTask } from "../state/runs-ops";
  import { nav } from "../state/nav.svelte";
  import { readText } from "../ipc/commands";
  import { notify } from "../state/notify.svelte";

  let { run }: { run: RunTab } = $props();

  let sel = $state<TestCase | null>(null);
  let closed = $state(new Set<string>());

  let cases = $derived(run.tests?.cases ?? []);
  let groups = $derived.by(() => {
    const m = new Map<string, TestCase[]>();
    for (const c of cases) m.set(c.class, [...(m.get(c.class) ?? []), c]);
    return [...m];
  });
  const bad = (c: TestCase) => c.outcome === "failed" || c.outcome === "error";
  let nBad = $derived(cases.filter(bad).length);
  let nOk = $derived(cases.filter((c) => c.outcome === "passed").length);
  let nSkip = $derived(cases.filter((c) => c.outcome === "skipped").length);
  let running = $derived(run.status === "running" || run.status === "stopping");
  let retry = $derived(failedSelector(cases));
  let task = $derived(parseTestTask(run.name));

  const ms = (n: number) => (n >= 1000 ? `${(n / 1000).toFixed(2)}s` : `${n}ms`);
  const icon = (o: string) => (o === "passed" ? "check" : o === "skipped" ? "minus" : "error");

  function toggle(k: string) {
    const n = new Set(closed);
    if (n.has(k)) n.delete(k);
    else n.add(k);
    closed = n;
  }

  /** 跳到这个测试方法：`模块/src/test/java/包路径/类.java`，按「方法名(」找那一行。找不到文件（模块段写的是 artifactId 不是目录）就说一句 */
  async function jump(c: TestCase) {
    if (!task) return;
    const dir = task.module === "." ? run.root : `${run.root}/${task.module}`;
    // @Nested 的内部类在外层类的文件里
    const path = `${dir}/src/test/java/${classFile(c.class)}`;
    let text: string;
    try {
      text = (await readText(path)).content;
    } catch {
      notify.hint(`未找到 ${classFile(c.class).split("/").pop()}（如果模块目录名与 artifactId 不同，请从测试文件行号旁的 ▶ 运行）`);
      return;
    }
    const name = c.name.replace(/[(\[].*$/, "");
    const line = text.split("\n").findIndex((l) => new RegExp(`\\b${name}\\s*\\(`).test(l));
    void nav.openAt(path, line >= 0 ? line + 1 : 1, false);
  }

  // 重跑照原样：带覆盖率跑的那次重跑还带（G15）
  const again = (sel: string, cover = task?.cover ?? false) => task && void runTask(`${task.module} [${cover ? "cover" : "test"} ${sel}]`, run.root);
</script>

<div class="tt">
  <div class="bar">
    {#if running}
      <!-- 边跑边出（G15）：一个类跑完亮一个；已经有挂了的就先标红，不用等全跑完 -->
      <span class="sum" class:bad={nBad > 0}><span class="spinner sm"></span> 正在跑…{cases.length ? ` 已完成 ${cases.length} 个：${nOk} 个通过${nBad ? ` · ${nBad} 个失败` : ""}` : ""}</span>
    {:else if cases.length}
      <span class="sum" class:bad={nBad > 0}>
        <span class="ic"><Icon name={nBad ? "error" : "check"} /></span>
        {nOk} 个通过{nBad ? ` · ${nBad} 个失败` : ""}{nSkip ? ` · 跳过 ${nSkip}` : ""}{run.tests && run.tests.total > cases.length ? `（仅显示前 ${cases.length} / ${run.tests.total} 个）` : ""}
      </span>
    {:else}
      <span class="sum bad">没有测试结果</span>
    {/if}
    <span class="gap"></span>
    <button class="ibtn" disabled={running || !task} onclick={() => task && again(task.selector)} title="重新运行这些测试（⌃R）" aria-label="重新运行"><Icon name="rerun" /></button>
    <button class="ibtn" disabled={running || !retry} onclick={() => retry && again(retry)} title="仅重新运行失败的测试" aria-label="重新运行失败的测试"><Icon name="run" /></button>
    <button class="ibtn" disabled={running || !task} onclick={() => task && again(task.selector, true)} title="带覆盖率重新运行这些测试（JaCoCo）" aria-label="带覆盖率重新运行"><Icon name="run-coverage" /></button>
  </div>
  <div class="tree" role="tree" aria-label="测试结果">
    {#if !running && !cases.length}
      <div class="empty-msg">
        这次一个测试都没跑起来
        <div class="sub">可能是编译失败或未找到测试，请查看右侧的输出</div>
      </div>
    {/if}
    {#each groups as [cls, list] (cls)}
      {@const failed = list.some(bad)}
      <div class="row cls" role="treeitem" aria-selected="false" aria-expanded={!closed.has(cls)} tabindex="-1" onclick={() => toggle(cls)} onkeydown={() => {}}>
        <span class="chev" class:open={!closed.has(cls)}><Icon name="chevron-right" /></span>
        <span class="ic" class:ok={!failed}><Icon name={failed ? "error" : "check"} /></span>
        <span class="name">{classLabel(cls)}</span>
        <span class="ms">{ms(list.reduce((n, c) => n + c.ms, 0))}</span>
      </div>
      {#if !closed.has(cls)}
        {#each list as c, i (i)}
          <div
            class="row case"
            class:on={sel === c}
            role="treeitem"
            aria-selected={sel === c}
            tabindex="-1"
            onclick={() => (sel = sel === c ? null : c)}
            ondblclick={() => void jump(c)}
            onkeydown={() => {}}
            title="双击转到此方法"
          >
            <span class="ic" class:ok={c.outcome === "passed"} class:skip={c.outcome === "skipped"}><Icon name={icon(c.outcome)} /></span>
            <span class="name">{c.name}</span>
            <span class="ms">{ms(c.ms)}</span>
          </div>
          {#if sel === c && (c.message || c.detail)}
            <div class="detail">
              {#if c.message}<div class="msg">{c.message}</div>{/if}
              {#if c.detail}<pre>{c.detail}</pre>{/if}
            </div>
          {/if}
        {/each}
      {/if}
    {/each}
  </div>
</div>

<style>
  .tt { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  /* 岛内的工具栏：34 高（ui.md 一之二） */
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 2px; padding: 0 6px 0 10px; border-bottom: 1px solid var(--border-soft); }
  .bar .gap { flex: 1; }
  .sum { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-sm); color: var(--text-dim); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .sum.bad { color: var(--lvl-error); }
  .tree { flex: 1; overflow: auto; padding: 4px 6px 8px; }
  .row { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 8px 0 4px; border-radius: var(--r-sm); color: var(--text-dim); font-size: var(--fs-md); white-space: nowrap; cursor: default; }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .row.case { padding-left: 24px; }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  /* 通过：单色的勾染绿；跳过：灰；没过：error 图标本身是红的 */
  .ic.ok { color: var(--git-added); }
  .ic.skip { color: var(--text-faint); }
  .name { flex: 1 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .ms { flex: none; margin-left: 8px; color: var(--text-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; }
  .detail { margin: 2px 4px 6px 24px; padding: 6px 8px; border-radius: var(--r-sm); background: var(--field-bg); }
  .msg { color: var(--lvl-error); font-size: var(--fs-sm); margin-bottom: 4px; white-space: pre-wrap; }
  .detail pre { margin: 0; max-height: 220px; overflow: auto; font-family: var(--code-font); font-size: var(--fs-xs); color: var(--text-muted); white-space: pre-wrap; }
</style>
