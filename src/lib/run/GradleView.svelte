<script lang="ts">
  import HideSidebar from "../shell/HideSidebar.svelte";
  /**
   * Gradle 工具窗，照 IDEA 的（docs/TAKEOVER.md G24）：
   *
   * ```
   * shopdemo                  构建根（rootProject.name）
   *   Tasks                   根项目自己的任务，按组（build setup、help…）
   *   order-service           子项目
   *     Tasks
   *       application
   *         bootRun           双击跑
   * ```
   *
   * 模块读 settings 文件（`gradle_project`，当场画得出来）；任务要跑一次 `gradlew tasks --all`（`gradle_tasks`，热的不到 1 秒、冷 daemon 十几秒），
   * 打开时在后台跑、模块先画，结果按项目根记住（切走再回来不重跑），点「重新加载」才重跑 —— 同 Maven 工具窗算插件那条。
   * 跑什么都走任务（任务名「:模块 [任务]」）：运行窗、⌃R 重跑、⌘F2 停、同名先停旧的全是现成的。双击跑、单击选中（`clean` 不该被划过去顺手一点就跑）。
   * Spring Boot 应用不在这里，在 Services（同 Maven 窗）
   */
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import { gradleProject, gradleTasks } from "../ipc/tasks";
  import type { GradleProject, GradleTask } from "../ipc/commands";
  import { runTask } from "../state/runs-ops";
  import { runs } from "../state/runs.svelte";
  import type { IconName } from "../shell/Icon.svelte";

  let { root }: { root: string } = $props();

  let data = $state<GradleProject | null>(null);
  let error = $state("");
  let loading = $state(false);
  /** 每个构建根的任务：还在跑 tasks 是 "wait"，失败了是一句话；`truncated`：报告读满了上限（截断了要说出来，AGENTS.md） */
  let tasks = $state<Record<string, { list: GradleTask[]; truncated: boolean } | "wait" | { error: string }>>({});
  let open = $state(new Set<string>());
  let sel = $state("");
  let treeEl = $state<HTMLDivElement | null>(null);

  async function load(force: boolean) {
    loading = true;
    error = "";
    try {
      const d = await gradleProject(root);
      data = d;
      if (open.size === 0) open = new Set(d.roots.map((r) => `r:${r.rel}`));
      for (const r of d.roots) void loadTasks(r.rel, force);
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    } finally {
      loading = false;
    }
  }

  async function loadTasks(rel: string, force: boolean) {
    const key = `${root}\u0000${rel}`;
    const cached = tasksCache.get(key);
    if (cached && !force) {
      tasks[rel] = cached;
      return;
    }
    tasks[rel] = "wait";
    try {
      const t = await gradleTasks(root, rel);
      const v = { list: t.tasks, truncated: t.truncated };
      tasksCache.set(key, v);
      tasks[rel] = v;
    } catch (e) {
      tasks[rel] = { error: e instanceof Error ? e.message : String(e) };
    }
  }

  $effect(() => {
    void root;
    void load(false);
  });

  interface Row {
    key: string;
    depth: number;
    label: string;
    icon?: IconName;
    parent?: boolean;
    /** 双击跑的任务名（「:模块 [任务]」） */
    task?: string;
    info?: "wait" | "bad";
    note?: string;
  }

  const rows = $derived.by((): Row[] => {
    const out: Row[] = [];
    for (const r of data?.roots ?? []) {
      const rk = `r:${r.rel}`;
      out.push({ key: rk, depth: 0, label: r.name, icon: "gradle", parent: true, note: r.rel || undefined });
      if (!open.has(rk)) continue;
      const t = tasks[r.rel];
      // 根项目在最上面、它的子项目挂在下面（IDEA 的样子）
      for (const m of r.modules) {
        const isRoot = m.path === ":";
        const mk = `m:${r.rel}:${m.path}`;
        if (!isRoot) {
          out.push({ key: mk, depth: 1, label: m.path.slice(1).replaceAll(":", " › "), icon: "maven-project", parent: true });
          if (!open.has(mk)) continue;
        }
        const tk = `t:${r.rel}:${m.path}`;
        const base = isRoot ? 1 : 2;
        out.push({ key: tk, depth: base, label: "Tasks", icon: "lifecycle", parent: true });
        if (!open.has(tk)) continue;
        if (t === "wait" || t === undefined) {
          out.push({ key: `${tk}:wait`, depth: base + 1, label: "正在列任务（gradlew tasks --all）…", info: "wait" });
          continue;
        }
        if ("error" in t) {
          out.push({ key: `${tk}:bad`, depth: base + 1, label: `列不出任务：${t.error.split("\n")[0]}`, info: "bad" });
          continue;
        }
        if (t.truncated && isRoot) out.push({ key: `${tk}:cut`, depth: base + 1, label: "任务报告超过 256 KB，后面的没列（模块太多）", info: "bad" });
        const mine = t.list.filter((x) => x.project === m.path);
        const groups = [...new Set(mine.map((x) => x.group))].sort((a, b) => (a === "Other") === (b === "Other") ? a.localeCompare(b) : a === "Other" ? 1 : -1);
        for (const g of groups) {
          const gk = `${tk}:g:${g}`;
          out.push({ key: gk, depth: base + 1, label: g.toLowerCase(), icon: "folder", parent: true });
          if (!open.has(gk)) continue;
          for (const x of mine.filter((y) => y.group === g).sort((a, b) => a.name.localeCompare(b.name))) {
            out.push({ key: `${gk}:${x.name}`, depth: base + 2, label: x.name, icon: "maven-goal", task: `${m.path} [${x.name}]`, note: x.desc || undefined });
          }
        }
      }
    }
    return out;
  });

  function toggle(k: string) {
    const n = new Set(open);
    if (n.has(k)) n.delete(k);
    else n.add(k);
    open = n;
  }

  function status(task: string) {
    return runs.list.find((r) => r.name === task)?.status;
  }

  const selTask = $derived(rows.find((r) => r.key === sel)?.task ?? null);

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const i = rows.findIndex((r) => r.key === sel);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const j = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (rows[j]) sel = rows[j].key;
    } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      const r = rows[i];
      if (r?.parent && open.has(r.key) !== (e.key === "ArrowRight")) {
        e.preventDefault();
        toggle(r.key);
      }
    } else if (e.key === "Enter") {
      const r = rows[i];
      if (r?.task) {
        e.preventDefault();
        void runTask(r.task, root);
      } else if (r?.parent) toggle(r.key);
    }
  }
</script>

<div class="gradle">
  <div class="head">
    <span class="title">Gradle</span>
    <span class="gap"></span>
    <button class="ibtn" class:busy={loading} disabled={loading} onclick={() => void load(true)} title="重新加载（重读 settings、重列任务）" aria-label="重新加载">
      <Icon name="refresh" />
    </button>
    <button class="ibtn" disabled={!selTask} onclick={() => selTask && void runTask(selTask, root)} title="运行选中的任务" aria-label="运行选中的任务">
      <Icon name="run" />
    </button>
    <button class="ibtn" onclick={() => (open = new Set(rows.filter((r) => r.parent).map((r) => r.key)))} title="展开一层" aria-label="展开一层"><Icon name="expand" /></button>
    <button class="ibtn" onclick={() => (open = new Set())} title="全部收起" aria-label="全部收起"><Icon name="collapse" /></button>
    <HideSidebar />
  </div>
  <div class="tree" role="tree" aria-label="Gradle 项目" tabindex="0" bind:this={treeEl} onkeydown={onKey}>
    {#if error}
      <div class="empty-msg bad">{error}</div>
    {:else if !data}
      <div class="empty-msg wait"><span class="spinner sm"></span> 正在读 settings…</div>
    {:else if data.roots.length === 0}
      <div class="empty-msg">此项目中没有 Gradle 构建文件<span class="sub">项目根和第一层子目录里都没找到 settings.gradle / build.gradle（有 pom.xml 的按 Maven 算）</span></div>
    {:else}
      {#each rows as row (row.key)}
        {@const st = row.task ? status(row.task) : undefined}
        <div
          class="row"
          class:on={sel === row.key}
          class:info={!!row.info}
          class:bad={row.info === "bad"}
          role="treeitem"
          aria-selected={sel === row.key}
          aria-expanded={row.parent ? open.has(row.key) : undefined}
          tabindex="-1"
          style:padding-left="{6 + row.depth * 19}px"
          onclick={() => {
            sel = row.key;
            treeEl?.focus();
            if (row.parent) toggle(row.key);
          }}
          ondblclick={() => row.task && void runTask(row.task, root)}
          onkeydown={() => {}}
          title={row.task ? `双击运行：${row.task}${row.note ? `\n${row.note}` : ""}` : row.label}
        >
          {#if row.parent}
            <span class="chev" class:open={open.has(row.key)}><Icon name="chevron-right" /></span>
          {:else}
            <span class="chev-gap"></span>
          {/if}
          {#if row.info === "wait"}
            <span class="spinner sm"></span>
          {:else if row.icon}
            <span class="ic"><Icon name={row.icon} /></span>
          {/if}
          <span class="name">{row.label}</span>
          {#if row.note}<span class="note">{row.note}</span>{/if}
          {#if st === "running"}<span class="st running">在跑</span>{:else if st === "stopping"}<span class="st stopping">正在停</span>{/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<script lang="ts" module>
  /** 任务按（项目根, 构建根）记住：组件切走就销毁，这份要活得比它长。只在这次运行里 */
  const tasksCache = new Map<string, { list: GradleTask[]; truncated: boolean }>();
</script>

<style>
  .gradle { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  /* 头、行的样子和 Maven 工具窗一模一样：同一个侧边栏里两个构建工具的窗长得不一样，人会以为是两种东西 */
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
  .tree { flex: 1; overflow: auto; padding: 2px 6px 8px; outline: none; }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding-right: 8px;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    white-space: nowrap;
    cursor: default;
  }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .tree:focus-visible .row.on { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row.info { color: var(--text-faint); font-size: var(--fs-sm); }
  .row.bad { color: var(--lvl-error); white-space: normal; height: auto; min-height: 24px; }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .chev-gap { flex: none; width: 16px; }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  .name { flex: 0 0 auto; max-width: 75%; overflow: hidden; text-overflow: ellipsis; }
  .note { flex: 1 1 0; min-width: 0; margin-left: 6px; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
  .st { flex: none; margin-left: auto; font-size: var(--fs-xs); }
  .st.running { color: var(--git-added); }
  .st.stopping { color: var(--lvl-warn); }
</style>
