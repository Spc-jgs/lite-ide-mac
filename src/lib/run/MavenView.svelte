<script lang="ts">
  /**
   * Maven 工具窗，照 IDEA 的（2026-10-15 重做，结构抄自 intellij-community 的 `MavenProjectsStructure`）：
   *
   * ```
   * Profiles                 勾上的跑目标、起服务都带 -P
   * shopdemo                 项目（子模块挂在把它列进 <modules> 的那个底下，不摊平）
   *   Lifecycle              IDEA 默认显示的 9 个阶段
   *   Plugins                有效 POM 里真用的插件 → 展开是它的目标（读插件 jar 里的描述）
   *   Dependencies           依赖树，带 scope
   *   order-service          子模块
   * ```
   *
   * 工具栏同 IDEA 的那一排：重新加载、执行 Maven 目标（自己敲）、运行选中的目标、离线、跳过测试、全部展开、全部收起。
   * Spring Boot 应用不在这里（IDEA 的 Maven 窗里也没有）—— 它们在 Services 工具窗。
   *
   * Plugins / Dependencies 要跑一次 mvn 才知道（`maven_resolve`，几秒到几十秒）：打开工具窗时在后台跑，别的节点不等它；
   * 结果按项目根记在模块里，切走再回来不重跑，点「重新加载」才重跑。
   *
   * 跑什么都走任务（`runs-ops.runTask`，任务名「artifactId [目标]」）：运行窗、⌃R 重跑、⌘F2 停、同名先停旧的全是现成的。
   * 双击跑、单击选中：`install`、`deploy` 不该被划过去顺手一点就跑起来（同 IDEA）。
   */
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import { mavenPrefsSet, mavenProject, mavenResolve } from "../ipc/tasks";
  import { readText, type MavenProject, type MavenResolved, type MavenModule, type MavenDep } from "../ipc/commands";
  import { runTask } from "../state/runs-ops";
  import { runs } from "../state/runs.svelte";
  import { notify } from "../state/notify.svelte";
  import { nav } from "../state/nav.svelte";
  import type { MenuItem } from "../shell/ContextMenu.svelte";
  import { cmenu } from "../shell/context-menu.svelte";
  import type { IconName } from "../shell/Icon.svelte";

  let { root }: { root: string } = $props();

  let data = $state<MavenProject | null>(null);
  let error = $state("");
  let loading = $state(false);
  /** 插件和依赖：按项目根记住（切走再回来不重跑 mvn），点重新加载才重跑 */
  let resolved = $state<MavenResolved | null>(null);
  let resolving = $state(false);
  /** 展开着的节点（key 见 `rows`） */
  let open = $state(new Set<string>(["P"]));
  let sel = $state("");
  let treeEl = $state<HTMLDivElement | null>(null);
  /** 「执行 Maven 目标」那个小输入框：在哪个模块上跑、敲了什么 */
  let exec = $state<{ module: string; text: string } | null>(null);
  let menu = $state<{ x: number; y: number; items: MenuItem[]; title: string } | null>(null);

  async function load(withResolve: boolean) {
    loading = true;
    error = "";
    try {
      const d = await mavenProject(root);
      data = d;
      // 第一次打开：展开每个 reactor 的根项目（IDEA 默认就是这样）
      if (open.size <= 1) {
        const next = new Set(open);
        for (const r of d.reactors) if (r.modules[0]) next.add(modKey(r.rel, r.modules[0].rel));
        open = next;
      }
    } catch (e) {
      error = String(e);
    } finally {
      loading = false;
    }
    if (withResolve || !resolvedCache.has(root)) void resolve();
  }

  async function resolve() {
    if (resolving || !data || data.reactors.length === 0) return;
    resolving = true;
    const r = root;
    try {
      const got = await mavenResolve(r);
      resolvedCache.set(r, got);
      if (r === root) resolved = got;
    } catch (e) {
      notify.fail(`无法解析插件和依赖：${e}`);
    } finally {
      resolving = false;
    }
  }

  $effect(() => {
    root;
    data = null;
    resolved = resolvedCache.get(root) ?? null;
    open = new Set(["P"]);
    void load(false);
  });

  // ── 树：摊成一行一行（键盘上下、全部展开 / 收起都好做） ──

  interface Row {
    key: string;
    depth: number;
    icon: IconName | null;
    label: string;
    /** 跟在名字后面的灰字（版本、scope、「默认开」） */
    note?: string;
    /** 有孩子：画箭头、能展开 */
    parent?: boolean;
    /** 双击跑的任务名 */
    task?: string;
    /** profile 行：前面一个勾 */
    profile?: { id: string; on: boolean };
    /** 说明行：还在算（带环）/ 出错 / 算完了是空的（不带环 —— 「空」画成转圈，人会以为还在算） */
    info?: "wait" | "bad" | "empty";
    /** 右键菜单 / 双击依赖用 */
    module?: { reactor: string; m: MavenModule };
    dep?: MavenDep;
  }

  const modKey = (reactor: string, rel: string) => `M\n${reactor}\n${rel}`;

  function resolvedOf(reactor: string, artifactId: string) {
    const r = resolved?.reactors.find((x) => x.rel === reactor);
    return { r, m: r?.modules.find((x) => x.artifactId === artifactId) };
  }

  let rows = $derived.by<Row[]>(() => {
    const d = data;
    if (!d) return [];
    const out: Row[] = [];
    if (d.profiles.length) {
      out.push({ key: "P", depth: 0, icon: "maven-profiles", label: "Profiles", parent: true });
      if (open.has("P"))
        for (const p of d.profiles)
          out.push({ key: `P\n${p.id}`, depth: 1, icon: null, label: p.id, note: p.activeByDefault ? "默认开" : undefined, profile: { id: p.id, on: p.on } });
    }
    for (const r of d.reactors) {
      const top = r.modules.filter((m) => m.parent === null);
      for (const m of top) addModule(out, r.rel, r.modules, m, 0);
    }
    return out;
  });

  function addModule(out: Row[], reactor: string, all: MavenModule[], m: MavenModule, depth: number) {
    const k = modKey(reactor, m.rel);
    out.push({ key: k, depth, icon: "maven-project", label: m.artifactId || m.rel || "(根)", parent: true, module: { reactor, m } });
    if (!open.has(k)) return;
    const d = depth + 1;
    // Lifecycle
    const lk = `${k}\nL`;
    out.push({ key: lk, depth: d, icon: "lifecycle", label: "Lifecycle", parent: true });
    if (open.has(lk)) for (const ph of data!.phases) out.push({ key: `${lk}\n${ph}`, depth: d + 1, icon: "phase", label: ph, task: `${m.artifactId} [${ph}]` });
    // Plugins / Dependencies：要 mvn 算
    const { r: rr, m: rm } = resolvedOf(reactor, m.artifactId);
    const pk = `${k}\nG`;
    out.push({ key: pk, depth: d, icon: "lifecycle", label: "Plugins", parent: true });
    if (open.has(pk)) {
      if (!rr) out.push(waitRow(`${pk}\n…`, d + 1));
      else if (rr.error) out.push({ key: `${pk}\n!`, depth: d + 1, icon: null, label: rr.error.split("\n")[0], info: "bad" });
      else if (!rm?.plugins.length) out.push({ key: `${pk}\n0`, depth: d + 1, icon: null, label: "没有插件（有效 POM 里这个模块的 <build><plugins> 是空的）", info: "empty" });
      else
        for (const p of rm?.plugins ?? []) {
          const plk = `${pk}\n${p.groupId}:${p.artifactId}`;
          out.push({ key: plk, depth: d + 1, icon: "maven-plugin", label: p.prefix, note: `${p.groupId}:${p.artifactId}:${p.version}`, parent: p.goals.length > 0 });
          if (open.has(plk))
            for (const g of p.goals) out.push({ key: `${plk}\n${g}`, depth: d + 2, icon: "maven-goal", label: `${p.prefix}:${g}`, task: `${m.artifactId} [${p.prefix}:${g}]` });
        }
    }
    const dk = `${k}\nD`;
    out.push({ key: dk, depth: d, icon: "library-folder", label: "Dependencies", parent: true });
    if (open.has(dk)) {
      if (!rr) out.push(waitRow(`${dk}\n…`, d + 1));
      else if (rr.error) out.push({ key: `${dk}\n!`, depth: d + 1, icon: null, label: rr.error.split("\n")[0], info: "bad" });
      else if (!rm?.deps.length) out.push({ key: `${dk}\n0`, depth: d + 1, icon: null, label: "没有依赖", info: "empty" });
      else addDeps(out, dk, rm?.deps ?? [], d + 1, { reactor, m });
    }
    for (const c of all.filter((x) => x.parent === m.rel)) addModule(out, reactor, all, c, d);
  }

  function waitRow(key: string, depth: number): Row {
    return { key, depth, icon: null, label: resolving ? "正在跑 mvn 算插件和依赖…" : "尚未解析（点击工具栏中的重新加载）", info: "wait" };
  }

  /** 依赖树：深度 0 是直接依赖，展开一个才露出它底下那一截（同 IDEA） */
  function addDeps(out: Row[], base: string, deps: MavenDep[], depth: number, module: Row["module"]) {
    let i = 0;
    const walk = (prefix: string, level: number, until: number, rowDepth: number) => {
      while (i < until) {
        const dep = deps[i];
        if (dep.depth < level) return;
        const end = deps.findIndex((x, j) => j > i && x.depth <= dep.depth);
        const stop = end < 0 ? until : Math.min(end, until);
        const key = `${prefix}\n${dep.groupId}:${dep.artifactId}`;
        out.push({
          key,
          depth: rowDepth,
          icon: "library",
          label: `${dep.groupId}:${dep.artifactId}:${dep.version}`,
          note: [dep.scope !== "compile" ? dep.scope : "", dep.optional ? "optional" : ""].filter(Boolean).join(" · ") || undefined,
          parent: stop > i + 1,
          dep,
          module,
        });
        i++;
        if (open.has(key)) walk(key, dep.depth + 1, stop, rowDepth + 1);
        else i = stop;
      }
    };
    walk(base, 0, deps.length, depth);
  }

  function toggle(k: string, force?: boolean) {
    const next = new Set(open);
    const on = force ?? !next.has(k);
    if (on) next.add(k);
    else next.delete(k);
    open = next;
  }

  /** 全部展开：项目、Lifecycle、Plugins（不展开每个插件的目标和整棵依赖树 —— IDEA 的也只展开到这一层，再往下几百行没法看） */
  function expandAll() {
    const next = new Set(open);
    next.add("P");
    for (const r of data?.reactors ?? [])
      for (const m of r.modules) {
        const k = modKey(r.rel, m.rel);
        next.add(k);
        next.add(`${k}\nL`);
        next.add(`${k}\nG`);
        next.add(`${k}\nD`);
      }
    open = next;
  }

  const status = (task: string) => runs.list.find((t) => t.root === root && t.name === task)?.status;

  function run(task: string) {
    void runTask(task, root);
  }

  /** 选中的那一行能不能跑（工具栏的 ▶） */
  let selTask = $derived(rows.find((r) => r.key === sel)?.task ?? null);

  async function setPrefs(patch: Partial<{ offline: boolean; skipTests: boolean; profiles: string[] }>) {
    const d = data;
    if (!d) return;
    const cur = { offline: d.offline, skipTests: d.skipTests, profiles: d.profiles.filter((p) => p.on).map((p) => p.id) };
    const next = { ...cur, ...patch };
    try {
      await mavenPrefsSet(root, next.offline, next.skipTests, next.profiles);
    } catch (e) {
      notify.fail(String(e));
      return;
    }
    const profilesChanged = patch.profiles !== undefined || patch.offline !== undefined;
    await load(false);
    // profile 会改插件和依赖；离线也要重算（刚才可能是上网算的）
    if (profilesChanged) void resolve();
  }

  function toggleProfile(id: string) {
    const on = data?.profiles.filter((p) => p.on).map((p) => p.id) ?? [];
    void setPrefs({ profiles: on.includes(id) ? on.filter((x) => x !== id) : [...on, id] });
  }

  /** 依赖双击：跳到声明它的 pom.xml 那一行（声明在 parent 的 dependencyManagement 里时只能跳到这个模块的 pom 开头） */
  async function openDep(row: Row) {
    if (!row.module || !row.dep) return;
    const { reactor, m } = row.module;
    const path = [root, reactor, m.rel, "pom.xml"].filter(Boolean).join("/");
    let line = 1;
    try {
      const t = (await readText(path)).content.split("\n");
      const i = t.findIndex((l) => l.includes(`<artifactId>${row.dep!.artifactId}</artifactId>`));
      if (i >= 0) line = i + 1;
    } catch {
      // 读不了就开在第一行
    }
    void nav.openAt(path, line, true);
  }

  function activate(row: Row) {
    if (row.task) run(row.task);
    else if (row.profile) toggleProfile(row.profile.id);
    else if (row.dep) void openDep(row);
    else if (row.parent) toggle(row.key);
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e) || exec) return;
    const i = rows.findIndex((r) => r.key === sel);
    const row = rows[i];
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const j = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      sel = rows[j]?.key ?? "";
      (treeEl?.querySelector(`[data-i="${j}"]`) as HTMLElement | null)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowRight" && row?.parent) {
      e.preventDefault();
      toggle(row.key, true);
    } else if (e.key === "ArrowLeft" && row) {
      e.preventDefault();
      if (row.parent && open.has(row.key)) toggle(row.key, false);
      else {
        // 收起的、叶子：跳到父节点（同 IDEA / 文件树）
        for (let j = i - 1; j >= 0; j--)
          if (rows[j].depth < row.depth) {
            sel = rows[j].key;
            break;
          }
      }
    } else if (e.key === "Enter" && row) {
      e.preventDefault();
      activate(row);
    }
  }

  // ── 执行 Maven 目标（IDEA 的 Run Anything 里那个 mvn） ──

  /** 在哪个模块上执行：选中的那一行属于哪个项目，没选中就是第一个 reactor 的根 */
  function moduleOfSel(): string {
    const i = rows.findIndex((r) => r.key === sel);
    for (let j = i; j >= 0; j--) if (rows[j].module && rows[j].depth <= rows[i].depth) return rows[j].module!.m.artifactId;
    return data?.reactors[0]?.modules[0]?.artifactId ?? "";
  }

  const SAFE = /^[A-Za-z0-9_\-.:=,@/+%]+$/;

  function runExec() {
    if (!exec) return;
    const text = exec.text.trim().replace(/^mvn\s+/, "");
    const toks = text.split(/\s+/).filter(Boolean);
    if (!toks.length) return;
    const bad = toks.find((t) => !SAFE.test(t));
    if (bad) {
      notify.hint(`「${bad}」包含 shell 特殊字符（引号、; $ | & 等），此处仅支持 Maven 目标和 -D 参数`);
      return;
    }
    const module = exec.module;
    exec = null;
    run(`${module} [${toks.join(" ")}]`);
  }

  function openMenu(e: MouseEvent, row: Row) {
    if (!row.module || row.dep) return;
    e.preventDefault();
    sel = row.key;
    const { reactor, m } = row.module;
    const pom = [root, reactor, m.rel, "pom.xml"].filter(Boolean).join("/");
    menu = {
      x: e.clientX,
      y: e.clientY,
      title: m.artifactId,
      items: [
        { label: "重新加载 Maven 项目", run: () => void load(true) },
        { label: "执行 Maven 目标…", run: () => (exec = { module: m.artifactId, text: "" }) },
        { label: "打开 pom.xml", sep: true, run: () => void nav.openAt(pom, 1, false) },
      ],
    };
    cmenu.load();
  }
</script>

<div class="maven">
  <div class="head">
    <span class="title">Maven</span>
    <span class="gap"></span>
    <button class="ibtn" class:busy={loading || resolving} disabled={loading || resolving} onclick={() => void load(true)} title="重新加载所有 Maven 项目（重读 pom、重算插件和依赖）" aria-label="重新加载">
      <Icon name="refresh" />
    </button>
    <button class="ibtn" onclick={() => (exec = { module: moduleOfSel(), text: "" })} title="执行 Maven 目标…（作用于所选模块）" aria-label="执行 Maven 目标">
      <Icon name="run-anything" />
    </button>
    <button class="ibtn" disabled={!selTask} onclick={() => selTask && run(selTask)} title="运行选中的目标" aria-label="运行选中的目标">
      <Icon name="run" />
    </button>
    <button class="ibtn" class:on={data?.offline} onclick={() => void setPrefs({ offline: !data?.offline })} title={data?.offline ? "离线模式：开（-o，不连远程仓库）" : "离线模式：关"} aria-label="离线模式" aria-pressed={!!data?.offline}>
      <Icon name="offline" />
    </button>
    <button class="ibtn" class:on={data?.skipTests} onclick={() => void setPrefs({ skipTests: !data?.skipTests })} title={data?.skipTests ? "跳过测试：开（-DskipTests）" : "跳过测试：关"} aria-label="跳过测试" aria-pressed={!!data?.skipTests}>
      <Icon name="skip-tests" />
    </button>
    <button class="ibtn" onclick={expandAll} title="全部展开" aria-label="全部展开"><Icon name="expand" /></button>
    <button class="ibtn" onclick={() => (open = new Set())} title="全部收起" aria-label="全部收起"><Icon name="collapse" /></button>
  </div>
  {#if exec}
    <div class="exec">
      <span class="pre">mvn</span>
      <!-- svelte-ignore a11y_autofocus -->
      <input
        autofocus
        bind:value={exec.text}
        placeholder="clean install -DskipTests"
        aria-label="Maven 目标"
        onkeydown={(e) => {
          if (imeKey(e)) return;
          if (e.key === "Enter") {
            e.preventDefault();
            runExec();
          } else if (e.key === "Escape") {
            e.preventDefault();
            exec = null;
            treeEl?.focus();
          }
        }}
      />
      <span class="where" title="在这个模块上跑（-pl 它 -am）">{exec.module}</span>
    </div>
  {/if}
  <div class="tree" role="tree" aria-label="Maven 项目" tabindex="0" bind:this={treeEl} onkeydown={onKey}>
    {#if error}
      <div class="empty-msg bad">{error}</div>
    {:else if !data}
      <div class="empty-msg wait"><span class="spinner sm"></span> 正在读 pom…</div>
    {:else if data.reactors.length === 0}
      <div class="empty-msg">这个项目里没有 pom.xml（项目根和第一层子目录里都没找到）</div>
    {:else}
      {#each rows as row, i (row.key)}
        {@const st = row.task ? status(row.task) : undefined}
        <div
          class="row"
          class:on={sel === row.key}
          class:info={!!row.info}
          class:bad={row.info === "bad"}
          data-i={i}
          role="treeitem"
          aria-selected={sel === row.key}
          aria-expanded={row.parent ? open.has(row.key) : undefined}
          tabindex="-1"
          style:padding-left="{6 + row.depth * 19}px"
          onclick={() => {
            sel = row.key;
            treeEl?.focus();
            if (row.parent && !row.task && !row.dep) toggle(row.key);
          }}
          ondblclick={() => activate(row)}
          oncontextmenu={(e) => openMenu(e, row)}
          onkeydown={() => {}}
          title={row.task ? `双击运行：${row.task}` : row.info === "bad" ? (resolved?.reactors.find((r) => r.error)?.error ?? row.label) : (row.note ? `${row.label}  ${row.note}` : row.label)}
        >
          {#if row.parent}
            <span
              class="chev"
              class:open={open.has(row.key)}
              role="presentation"
              onclick={(e) => {
                e.stopPropagation();
                toggle(row.key);
              }}><Icon name="chevron-right" /></span
            >
          {:else}
            <span class="chev-gap"></span>
          {/if}
          {#if row.profile}
            <input type="checkbox" class="cb" checked={row.profile.on} tabindex="-1" onclick={(e) => { e.stopPropagation(); toggleProfile(row.profile!.id); }} aria-label="启用 profile {row.label}" />
          {:else if row.info === "wait"}
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

{#if menu && cmenu.comp}
  <cmenu.comp x={menu.x} y={menu.y} title={menu.title} label="Maven 项目" items={menu.items} onclose={() => (menu = null)} />
{/if}

<script lang="ts" module>
  /** 插件和依赖按项目根记住（组件切走就销毁，这份要活得比它长）。只在这次运行里：mvn 算出来的东西重启后可能已经不对了 */
  const resolvedCache = new Map<string, MavenResolved>();
</script>

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
  /* 执行 Maven 目标：头下面一条，不画框（同浮层顶上的搜索，ui.md 二之二） */
  .exec {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 4px 10px 6px;
    border-bottom: 1px solid var(--border-soft);
  }
  .exec .pre { color: var(--text-faint); font-family: var(--code-font); font-size: var(--fs-sm); }
  .exec input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-family: var(--code-font);
    font-size: var(--fs-sm);
  }
  .exec .where { flex: none; max-width: 40%; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
  .tree { flex: 1; overflow: auto; padding: 2px 6px 8px; outline: none; }
  /* 行：同文件树（24 高、内缩的圆角块当选中，ui.md 第一条） */
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
  .row.bad { color: var(--lvl-error); }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .chev-gap { flex: none; width: 16px; }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  .cb { flex: none; margin: 0 4px 0 1px; accent-color: var(--accent); }
  /* 名字和后面的灰字都可能很长（插件坐标、依赖版本）：窄的时候先挤灰字（名字不收缩、最多占四分之三），灰字吃剩下的 ——
     反过来的话插件那一行只剩一串 org.apache.maven.plugins:…，看不出是哪个插件（第一版就是这样） */
  .name { flex: 0 0 auto; max-width: 75%; overflow: hidden; text-overflow: ellipsis; }
  .note { flex: 1 1 0; min-width: 0; margin-left: 6px; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
  .st { flex: none; margin-left: auto; font-size: var(--fs-xs); }
  .st.running { color: var(--git-added); }
  .st.stopping { color: var(--lvl-warn); }
</style>
