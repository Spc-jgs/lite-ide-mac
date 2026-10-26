<script lang="ts">
  /**
   * 覆盖率工具窗（docs/TAKEOVER.md G15，IDEA 的 Coverage）：最近一次带覆盖率跑出来的报告，每个源文件一行 —— 行覆盖率、一条进度、跑到 / 总行数；
   * 没跑到多的排上面（最该补测试的）。点一行开那个文件，行号旁看哪几行没跑到。「清掉」把编辑器里的颜色一起收了（IDEA 的 Hide Coverage）
   */
  import { clearCoverage, filePct, lastCoverage, onCoverage, totalPct } from "../state/coverage";
  import { project } from "../state/project.svelte";
  import { nav } from "../state/nav.svelte";

  let tick = $state(0);
  $effect(() => onCoverage(() => tick++));
  const rep = $derived((void tick, lastCoverage()));
  const sum = $derived(rep ? totalPct(rep.files) : null);
  const rel = (p: string) => (project.root && p.startsWith(`${project.root}/`) ? p.slice(project.root.length + 1) : p);
  const base = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const pctText = (x: number) => `${Math.floor(x * 100)}%`;
  /** 第一处没跑到的行：点一行跳到那儿（全跑到了就去第一行） */
  const firstMiss = (f: { missed: number[]; partial: number[] }) => Math.min(...f.missed, ...f.partial, Number.MAX_SAFE_INTEGER);
</script>

<div class="cov">
  <div class="bar">
    {#if rep && sum}
      <span class="sum">行覆盖率 <b>{pctText(sum.pct)}</b>（{sum.hit} / {sum.all} 行）· {rep.name}{rep.total > rep.files.length ? ` · 只列了前 ${rep.files.length} / ${rep.total} 个文件` : ""}</span>
    {:else}
      <span class="sum">没有覆盖率报告</span>
    {/if}
    <span class="gap"></span>
    <button class="btn sm quiet" disabled={!rep} onclick={clearCoverage} title="编辑器里行号旁的颜色一起收掉">清掉</button>
  </div>
  <div class="list" role="list">
    {#if !rep}
      <div class="empty-msg">还没带覆盖率跑过<span class="sub">点测试旁边的 ▶ 选「带覆盖率运行」，或者运行窗里测试结果上面那个按钮 —— 跑完这里列每个文件，编辑器行号旁绿的跑到了、红的没跑到</span></div>
    {:else if !rep.files.length}
      <div class="empty-msg">报告里一个源文件都没有<span class="sub">测试跑到的代码都不在这个项目的 src/main/java 里？</span></div>
    {:else}
      {#each rep.files as f (f.path)}
        {@const p = filePct(f)}
        {@const miss = firstMiss(f)}
        <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
        <div class="row" role="listitem" title="{rel(f.path)}：点了跳到第一处没跑到的行" onclick={() => void nav.openAt(f.path, miss === Number.MAX_SAFE_INTEGER ? 1 : miss, false)} onkeydown={() => {}} tabindex="-1">
          <span class="name">{base(f.path)}</span>
          <span class="dir">{rel(f.path).slice(0, -base(f.path).length).replace(/\/$/, "")}</span>
          <span class="gap"></span>
          <span class="meter" aria-hidden="true"><span class="fill" class:low={p < 0.5} style:width={pctText(p)}></span></span>
          <span class="pct" class:low={p < 0.5}>{pctText(p)}</span>
          <span class="n">{f.covered.length + f.partial.length} / {f.covered.length + f.partial.length + f.missed.length}</span>
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .cov { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  .bar { flex: none; display: flex; align-items: center; gap: 8px; height: 34px; padding: 0 8px 0 12px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); color: var(--text-faint); }
  .sum b { color: var(--text); font-weight: 600; }
  .gap { flex: 1; }
  .list { flex: 1; min-height: 0; overflow: auto; padding: 4px 6px; }
  .row { display: flex; align-items: center; gap: 8px; height: 24px; padding: 0 8px; border-radius: var(--r-sm); cursor: default; font-size: var(--fs-md); }
  .row:hover { background: var(--hover); }
  .name { flex: none; color: var(--text); }
  .dir { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-sm); color: var(--text-faint); }
  .meter { flex: none; width: 80px; height: 4px; border-radius: 99px; background: color-mix(in srgb, var(--lvl-error) 45%, transparent); overflow: hidden; }
  .fill { display: block; height: 100%; background: #5fb865; }
  .pct { flex: none; width: 38px; text-align: right; font-variant-numeric: tabular-nums; color: var(--text-dim); }
  .pct.low { color: var(--lvl-error); }
  .n { flex: none; width: 72px; text-align: right; font-size: var(--fs-sm); color: var(--text-faint); font-variant-numeric: tabular-nums; }
</style>
