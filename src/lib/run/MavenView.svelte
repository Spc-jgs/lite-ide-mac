<script lang="ts">
  /**
   * Maven 工具窗（docs/IDEA.md 第 1 步）：照 IDEA 的那个 —— 模块树，每个模块底下是认出来的 Spring Boot 应用和生命周期目标，**双击跑**。
   *
   * 跑什么都走任务那套（`runs-ops.runTask`）：Spring Boot 应用就是任务列表里那个同名任务，生命周期目标是「artifactId [目标]」
   * （IDEA 运行窗标签的样子，Rust 的 `taskdefs::maven_goal` 认）—— 运行窗、⌃R 重跑、⌘F2 停、同名先停旧的、输出进日志视图全是现成的。
   * 双击而不是单击：`install`、`clean` 不该被划过去顺手一点就跑起来（同 IDEA；单击只是选中）。
   */
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import { mavenProject } from "../ipc/tasks";
  import type { MavenProject, MavenModule } from "../ipc/commands";
  import { runTask } from "../state/runs-ops";
  import { runs } from "../state/runs.svelte";

  let { root }: { root: string } = $props();

  let data = $state<MavenProject | null>(null);
  let error = $state("");
  let loading = $state(false);
  /** 展开着的模块（reactor + 模块路径）。默认展开有 Spring Boot 应用的那几个 —— 打开工具窗多半是为了起服务 */
  let open = $state(new Set<string>());
  /** 选中的那一行（单击选中、双击跑） */
  let sel = $state("");

  const key = (reactor: string, m: MavenModule) => `${reactor}\n${m.rel}`;

  async function load() {
    loading = true;
    error = "";
    try {
      const d = await mavenProject(root);
      data = d;
      const next = new Set(open);
      for (const r of d.reactors) for (const m of r.modules) if (m.apps.length) next.add(key(r.rel, m));
      open = next;
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
  }

  $effect(() => {
    root;
    data = null;
    open = new Set();
    void load();
  });

  function toggle(k: string) {
    const next = new Set(open);
    if (next.has(k)) next.delete(k);
    else next.add(k);
    open = next;
  }

  /** 这一层缩进几格：模块路径有几段（`services/order` 是 2）；根模块 0 */
  const depth = (m: MavenModule) => (m.rel ? m.rel.split("/").length : 0);

  const status = (task: string) => runs.list.find((t) => t.root === root && t.name === task)?.status;

  /** 双击 / 回车：跑 */
  function run(task: string) {
    void runTask(task, root);
  }

  function onKey(e: KeyboardEvent, task: string | null, k?: string) {
    if (imeKey(e)) return;
    if (e.key === "Enter" && task) {
      e.preventDefault();
      run(task);
    } else if ((e.key === "ArrowRight" || e.key === "ArrowLeft") && k) {
      e.preventDefault();
      if (open.has(k) !== (e.key === "ArrowRight")) toggle(k);
    }
  }

  /** 目标的说法：IDEA 的 Lifecycle 里就是目标名本身；依赖树换成人话 */
  const goalLabel = (g: string) => (g === "dependency:tree" ? "依赖树" : g);
</script>

<div class="maven">
  <div class="head">
    <span class="title">Maven</span>
    <span class="gap"></span>
    <button class="ibtn" class:busy={loading} disabled={loading} onclick={() => void load()} title="重新读 pom（加了模块、改了入口类之后）" aria-label="重新读 pom">
      <Icon name="refresh" />
    </button>
  </div>
  <div class="tree" role="tree" aria-label="Maven 模块">
    {#if error}
      <div class="empty-msg bad">{error}</div>
    {:else if !data}
      <div class="empty-msg wait"><span class="spinner sm"></span> 正在读 pom…</div>
    {:else if data.reactors.length === 0}
      <div class="empty-msg">这个项目里没有 pom.xml（项目根和第一层子目录里都没找到）</div>
    {:else}
      {#each data.problems as p (p)}<div class="empty-msg bad">{p}</div>{/each}
      {#each data.reactors as r (r.rel)}
        {#if data.reactors.length > 1}<div class="reactor">{r.rel}/</div>{/if}
        {#each r.modules as m (m.rel)}
          {@const k = key(r.rel, m)}
          {@const d = depth(m)}
          <button
            class="row mod"
            class:on={sel === k}
            role="treeitem"
            aria-selected={sel === k}
            aria-expanded={open.has(k)}
            style:padding-left="{6 + d * 19}px"
            onclick={() => {
              sel = k;
              toggle(k);
            }}
            onkeydown={(e) => onKey(e, null, k)}
            title={m.rel ? `${r.rel ? `${r.rel}/` : ""}${m.rel}/pom.xml` : `${r.rel ? `${r.rel}/` : ""}pom.xml`}
          >
            <span class="chev" class:open={open.has(k)}><Icon name="chevron-right" /></span>
            <span class="ic"><Icon name="maven" /></span>
            <span class="name">{m.artifactId || m.rel || "(根)"}</span>
            {#if d === 0 && r.modules.length > 1}<span class="sub">根 · 对整个项目</span>{/if}
          </button>
          {#if open.has(k)}
            {#each m.apps as a (a.task)}
              {@const st = status(a.task)}
              {@const ak = `${k}\napp\n${a.task}`}
              <button
                class="row leaf"
                class:on={sel === ak}
                role="treeitem"
                aria-selected={sel === ak}
                style:padding-left="{6 + (d + 1) * 19 + 16}px"
                onclick={() => (sel = ak)}
                ondblclick={() => run(a.task)}
                onkeydown={(e) => onKey(e, a.task)}
                title={`${a.mainClass}\n双击运行（和 ⌃⌥R 里选它是同一个任务）`}
              >
                <span class="ic"><Icon name="run" /></span>
                <span class="name">{a.task}</span>
                {#if st === "running"}<span class="st running">在跑</span>{:else if st === "stopping"}<span class="st stopping">正在停</span>{/if}
              </button>
            {/each}
            {#each data.goals as g (g)}
              {@const task = `${m.artifactId} [${g}]`}
              {@const gk = `${k}\ngoal\n${g}`}
              {@const st = status(task)}
              <button
                class="row leaf goal"
                class:on={sel === gk}
                class:sep={g === "dependency:tree"}
                role="treeitem"
                aria-selected={sel === gk}
                style:padding-left="{6 + (d + 1) * 19 + 16}px"
                onclick={() => (sel = gk)}
                ondblclick={() => run(task)}
                onkeydown={(e) => onKey(e, task)}
                title={`双击运行：mvn ${g}${m.rel ? (g === "clean" ? `（只清 ${m.rel}）` : `（${m.rel} 和它依赖的模块）`) : d === 0 && r.modules.length > 1 ? "（整个项目）" : ""}`}
              >
                <span class="name">{goalLabel(g)}</span>
                {#if st === "running"}<span class="st running">在跑</span>{/if}
              </button>
            {/each}
          {/if}
        {/each}
      {/each}
      <div class="hint">双击运行 · 输出在底部的运行工具窗</div>
    {/if}
  </div>
</div>

<style>
  .maven { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  /* 头和文件树、草稿的一模一样（38 高、大写、灰）：同一个侧边栏里几个视图的头长得不一样，人会以为是几种东西 */
  .head {
    flex: none;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    font-size: var(--fs-sm);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .gap { flex: 1; }
  .tree { flex: 1; overflow: auto; padding: 2px 6px 8px; }
  .reactor { padding: 8px 6px 2px; color: var(--text-faint); font-size: var(--fs-xs); }
  /* 行：同文件树（22–24 高、内缩的圆角块当选中，ui.md 第一条） */
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    height: 24px;
    padding-right: 8px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    text-align: left;
    white-space: nowrap;
    cursor: default;
  }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row.mod { color: var(--text); }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  .name { min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .sub { flex: none; margin-left: 6px; color: var(--text-faint); font-size: var(--fs-xs); }
  /* 生命周期目标：比应用低一档（它们是工具，应用是你要起的东西） */
  .goal { color: var(--text-dim); font-size: var(--fs-sm); }
  .goal.sep { margin-top: 2px; }
  .st { flex: none; margin-left: auto; font-size: var(--fs-xs); }
  .st.running { color: var(--git-added); }
  .st.stopping { color: var(--lvl-warn); }
  .hint { padding: 10px 8px 0; color: var(--text-faint); font-size: var(--fs-xs); }
</style>
