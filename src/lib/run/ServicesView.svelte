<script lang="ts">
  /**
   * Services 工具窗（docs/IDEA.md 第 12 节），照 IDEA 的 Services → Spring Boot：
   *
   * ```
   * Spring Boot                 ┃ 选中那个服务的输出（运行窗那一格同一个东西：级别着色、过滤、跟随、堆栈跳源码）
   *   Running                   ┃
   *     OrderServiceApplication :8080
   *   Not Started               ┃
   *     AdminApplication        ┃
   * ```
   *
   * 分组和次序抄 IDEA 的 `RunDashboardRunConfigurationStatus`：Running → Failed → Finished → Not Started，空的组不画。
   * 「跑着」按运行记录判（`runs.list` 里 `service` 的那几条，退出事件 runs-ops 已经在听）；有哪些服务、在听哪个端口只有 Rust 知道：
   * 认服务（`services_list`，要翻 Java 源码）在打开时、有服务换了状态时问；端口（`task_ports`，一次 lsof）在「跑着、还没端口」时隔 1.5 秒问一次 ——
   * Spring Boot 起来要几秒，一分钟还没端口就不问了（不是 web 服务，或者起不来；那时候日志比端口有用）。
   *
   * 工具栏的动作作用在选中的那一行上：选中服务 → 启动（跑着就是重跑）/ 停；选中分组或「Spring Boot」→ 整组启动 / 整组停（同 IDEA）。
   * 双击服务启动，单击只选中。
   */
  import { untrack } from "svelte";
  import { logFocus } from "./log-focus.svelte";
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import RunView from "./RunView.svelte";
  import { servicesList, taskHeap, taskPorts } from "../ipc/tasks";
  import type { Service, TaskHeap } from "../ipc/commands";
  import { runTask, stopRun, openRunLog, freePortAndRerun, dismissHolder, focusService } from "../state/runs-ops";
  import { debugTask } from "../debug/debug-ops";
  import { runs, type RunTab } from "../state/runs.svelte";
  import { nav } from "../state/nav.svelte";
  import type { MenuItem } from "../shell/ContextMenu.svelte";
  import { cmenu } from "../shell/context-menu.svelte";
  import type { IconName } from "../shell/Icon.svelte";

  let { root }: { root: string } = $props();

  let list = $state<Service[] | null>(null);
  let error = $state("");
  /** 端口按运行 id 记（重跑换了 id，旧的自然作废） */
  let ports = $state<Record<number, number[]>>({});
  let closed = $state(new Set<string>());
  let treeEl = $state<HTMLElement | null>(null);
  let menu = $state<{ x: number; y: number; title: string; items: MenuItem[] } | null>(null);

  type Group = "running" | "failed" | "finished" | "idle";
  const GROUPS: { id: Group; label: string; icon: IconName | null }[] = [
    { id: "running", label: "Running", icon: "run" },
    { id: "failed", label: "Failed", icon: "error" },
    { id: "finished", label: "Finished", icon: "stop" },
    { id: "idle", label: "Not Started", icon: null },
  ];

  /** 这个服务最近的那次运行（同名的重跑是原地换掉的，最多一条） */
  const runOf = (s: Service): RunTab | undefined => runs.list.find((r) => r.service && r.root === root && r.name === s.task);
  const alive = (r: RunTab | undefined) => r?.status === "running" || r?.status === "stopping";
  function groupOf(r: RunTab | undefined): Group {
    if (!r) return "idle";
    if (alive(r)) return "running";
    return r.status === "failed" ? "failed" : "finished";
  }
  /** 类名的最后一段：IDEA 的 Spring Boot 运行配置就叫这个 */
  const short = (s: Service) => s.mainClass.slice(s.mainClass.lastIndexOf(".") + 1);

  type Row = { key: string; depth: number; label: string; icon: IconName | null; note?: string; port?: string; svc?: Service; members?: Service[]; parent: boolean };

  let rows = $derived.by<Row[]>(() => {
    if (!list || list.length === 0) return [];
    const out: Row[] = [{ key: "boot", depth: 0, label: "Spring Boot", icon: "services", members: list, parent: true }];
    if (closed.has("boot")) return out;
    for (const g of GROUPS) {
      const members = list.filter((s) => groupOf(runOf(s)) === g.id);
      if (!members.length) continue;
      const key = `g:${g.id}`;
      out.push({ key, depth: 1, label: g.label, icon: g.icon, members, parent: true });
      if (closed.has(key)) continue;
      for (const s of members) {
        const r = runOf(s);
        const p = r ? ports[r.id] : undefined;
        out.push({
          key: `s:${s.task}`,
          depth: 2,
          label: short(s),
          icon: null,
          note: s.module,
          port: alive(r) && p?.length ? p.map((x) => `:${x}`).join(" ") : undefined,
          svc: s,
          parent: false,
        });
      }
    }
    return out;
  });

  let sel = $state<string | null>(null);
  // 从响应头的链路 ID 跳过来的（#77，`log-focus`）：选中那个服务，它的日志视图自己按那个 ID 过滤
  $effect(() => {
    const f = logFocus.req;
    if (!f) return;
    untrack(() => {
      const r = runs.list.find((x) => x.id === f.id && x.root === root && x.service);
      if (r) {
        closed = new Set([...closed].filter((key) => key !== "boot" && key !== "g:running"));
        sel = `s:${r.name}`;
      }
    });
  });
  let selRow = $derived(rows.find((r) => r.key === sel) ?? null);
  /** 选中的服务（选中分组时为 null）和它那次运行：右边画它的输出 */
  let selSvc = $derived(selRow?.svc ?? null);
  let selRun = $derived(selSvc ? runOf(selSvc) : undefined);
  /** 工具栏作用在谁身上：选中服务就是它，选中分组就是组里的全部 */
  let targets = $derived<Service[]>(selRow?.svc ? [selRow.svc] : (selRow?.members ?? []));
  let canStop = $derived(targets.some((s) => alive(runOf(s))));
  let canStart = $derived(targets.length > 0 && (selRow?.svc ? true : targets.some((s) => !alive(runOf(s)))));

  // ⌘F2 在 Services 开着时停选中的这个（runs-ops.stopTarget）
  $effect(() => {
    focusService(selRun?.id ?? null);
    return () => focusService(null);
  });

  async function refresh() {
    const r = root;
    try {
      const l = await servicesList(r);
      if (r !== root) return;
      list = l;
      error = "";
      runs.services = l.length;
      const got: Record<number, number[]> = { ...ports };
      for (const s of l) {
        const run = runOf(s);
        if (run && s.ports.length) got[run.id] = s.ports;
      }
      ports = got;
    } catch (e) {
      error = e instanceof Error ? e.message : String(e);
    }
  }

  // 打开时、换项目时、有服务换了状态时（起来了、退了、重跑换了 id）重新认一遍：tasks.json 里同名的、改了 pom 的都以 Rust 那次为准
  let sig = $derived(runs.list.filter((r) => r.service && r.root === root).map((r) => `${r.id}${r.status}`).join());
  $effect(() => {
    void root;
    void sig;
    void refresh();
  });

  // 刚跑起来的服务自动选中（从 Maven 窗、任务列表、⌃R 起的都算）：人要看的就是它在说什么
  let seen = new Set<number>();
  $effect(() => {
    for (const r of runs.list) {
      if (!r.service || r.root !== root || seen.has(r.id)) continue;
      seen.add(r.id);
      if (r.status === "running") sel = `s:${r.name}`;
    }
  });

  // 端口：跑着、还没端口的，隔 1.5 秒问一次，最多问 40 次（一分钟）。每问完一轮 `ports` 换了，这个 effect 重跑、排下一次
  let polls = new Map<number, number>();
  $effect(() => {
    if (!list) return;
    const waiting = list.map(runOf).filter((r): r is RunTab => !!r && r.status === "running" && !ports[r.id]?.length && (polls.get(r.id) ?? 0) < 40);
    if (!waiting.length) return;
    const t = setTimeout(async () => {
      const got: Record<number, number[]> = { ...ports };
      for (const r of waiting) {
        polls.set(r.id, (polls.get(r.id) ?? 0) + 1);
        got[r.id] = await taskPorts(r.id).catch(() => []);
      }
      ports = got;
    }, 1500);
    return () => clearTimeout(t);
  });

  /*
   * 堆（docs/IDEA.md 第 27 节）：跑着的服务每 2 秒问一次，每个记最近 60 个点（两分钟）—— 全都问、只画选中的那个：换着选时曲线是现成的。
   * Rust 读的是 JVM 自己写的性能计数，不起子进程，问得起。
   * 只在这个工具窗开着时问 —— 它挂着就是开着。停了的那次运行不再问，它的点跟着丢掉
   */
  const HEAP_KEEP = 60;
  let heaps = $state<Record<number, TaskHeap[]>>({});
  $effect(() => {
    if (!list) return;
    const ids = list.map(runOf).filter((r): r is RunTab => !!r && r.status === "running").map((r) => r.id);
    if (!ids.length) return;
    const t = setInterval(async () => {
      const got = await taskHeap(ids).catch(() => [] as TaskHeap[]);
      const next: Record<number, TaskHeap[]> = {};
      for (const h of got) next[h.id] = [...(heaps[h.id] ?? []), h].slice(-HEAP_KEEP);
      heaps = next;
    }, 2000);
    return () => clearInterval(t);
  });
  const mb = (n: number) => `${Math.round(n / 1048576)} MB`;
  const heapTip = (h: TaskHeap) =>
    `堆：已用 ${mb(h.used)}，已分配 ${mb(h.committed)}，上限 ${mb(h.max)}\nGC ${h.gcCount} 次，共 ${h.gcMs} ms\n曲线为最近两分钟的用量（每 2 秒采样一次）`;
  /** 小曲线的点：最新的在最右，高度按这段时间里最大的「向系统要了」算 —— 用量贴着它就是快满了 */
  function spark(hs: TaskHeap[]): string {
    const top = Math.max(...hs.map((h) => h.committed), 1);
    return hs.map((h, i) => `${HEAP_KEEP - (hs.length - 1 - i)},${(12 - (h.used / top) * 11).toFixed(1)}`).join(" ");
  }

  function start(all: Service[]) {
    // 整组启动只起没在跑的；单个服务跑着就是重跑（Rust 那边先停旧的、等端口空出来）
    const go = all.length === 1 ? all : all.filter((s) => !alive(runOf(s)));
    for (const s of go) void runTask(s.task, root);
  }
  /** 调试（docs/IDEA.md 第 18 节）：一次只调一个服务（IDEA 也是选中一个才能 Debug）；跑着的先停掉再调试着起（Rust 那边同名先停） */
  function debug(s: Service) {
    void debugTask(s.task, root);
  }
  function stop(all: Service[]) {
    for (const s of all) {
      const r = runOf(s);
      if (alive(r)) void stopRun(r!.id);
    }
  }

  function toggle(key: string) {
    const n = new Set(closed);
    if (n.has(key)) n.delete(key);
    else n.add(key);
    closed = n;
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const i = rows.findIndex((r) => r.key === sel);
    const row = rows[i];
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const j = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (rows[j]) sel = rows[j].key;
      treeEl?.querySelector(`[data-i="${j}"]`)?.scrollIntoView({ block: "nearest" });
    } else if (e.key === "ArrowRight" && row?.parent && closed.has(row.key)) {
      e.preventDefault();
      toggle(row.key);
    } else if (e.key === "ArrowLeft" && row) {
      e.preventDefault();
      if (row.parent && !closed.has(row.key)) toggle(row.key);
      else
        for (let j = i - 1; j >= 0; j--)
          if (rows[j].depth < row.depth) {
            sel = rows[j].key;
            break;
          }
    } else if (e.key === "Enter" && row?.svc) {
      e.preventDefault();
      start([row.svc]);
    }
  }

  /** 入口类的源文件：模块 + src/main/java + 包路径（Rust 认它的时候就是这么找到的） */
  const sourceOf = (s: Service) => [root, s.module, "src/main/java", `${s.mainClass.replace(/\./g, "/")}.java`].filter(Boolean).join("/");

  function openMenu(e: MouseEvent, row: Row) {
    e.preventDefault();
    sel = row.key;
    const all = row.svc ? [row.svc] : (row.members ?? []);
    const r = row.svc ? runOf(row.svc) : undefined;
    const items: MenuItem[] = [
      { label: row.svc ? (alive(r) ? "重新运行" : "启动") : "全部启动", disabled: !row.svc && all.every((s) => alive(runOf(s))), run: () => start(all) },
      { label: row.svc ? "停止" : "全部停止", disabled: !all.some((s) => alive(runOf(s))), run: () => stop(all) },
    ];
    if (row.svc) {
      const s = row.svc;
      items.splice(1, 0, { label: "调试", run: () => debug(s) });
      // 运行配置（docs/SETTINGS.md 13.6）：VM 参数、Spring profile、环境变量、JDK。IDEA 是在运行配置下拉里的 Edit Configurations…
      items.push({ label: "编辑配置…", sep: true, run: () => void import("../settings/open").then((m) => m.openRunConfigs(root, s.task)) });
      items.push({ label: "转到入口类", run: () => void nav.openAt(sourceOf(s), 1, false) });
      if (r) items.push({ label: "在编辑器中打开此输出", run: () => void openRunLog(r.id) });
    }
    items.push({ label: "重新识别服务", sep: true, run: () => void refresh() });
    menu = { x: e.clientX, y: e.clientY, title: row.label, items };
    cmenu.load();
  }
</script>

<div class="svc">
  <div class="left">
    <div class="bar">
      <button class="ibtn" disabled={!canStart} onclick={() => start(targets)} title={selSvc ? (alive(selRun) ? `重新运行「${short(selSvc)}」` : `启动「${short(selSvc)}」`) : "启动此组中未运行的服务"} aria-label="启动">
        <Icon name={selSvc && alive(selRun) ? "rerun" : "run"} />
      </button>
      <button class="ibtn" disabled={!selSvc} onclick={() => selSvc && debug(selSvc)} title={selSvc ? `调试「${short(selSvc)}」（⌃D 再次调试）` : "请选择要调试的服务"} aria-label="调试">
        <Icon name="debug" />
      </button>
      <button class="ibtn" disabled={!canStop} onclick={() => stop(targets)} title={selSvc ? `停止「${short(selSvc)}」` : "停止此组中正在运行的服务"} aria-label="停止">
        <Icon name="stop" />
      </button>
      <span class="gap"></span>
      <!--
        选中的那个服务的堆（docs/IDEA.md 第 27 节）。原来想放在树里每一行端口旁边：树那一栏固定 300px，服务名本来就紧，再加一截数字和曲线，
        「AdminApplication」被挤成「AdminA…」（浏览器里看出来的）—— 名字比堆要紧。工具栏这一段本来空着，选中会跟着刚起的服务走
      -->
      {#if selRun && alive(selRun) && heaps[selRun.id]?.length}
        {@const hs = heaps[selRun.id]}
        <span class="heap" title={heapTip(hs[hs.length - 1])}
          ><svg class="spark" viewBox="0 0 {HEAP_KEEP} 12" preserveAspectRatio="none" aria-hidden="true"><polyline points={spark(hs)} /></svg>{mb(hs[hs.length - 1].used)}</span
        >
      {/if}
      <button class="ibtn" onclick={() => void refresh()} title="重新识别服务（修改 pom 或新增入口类后使用）" aria-label="重新识别服务">
        <Icon name="refresh" />
      </button>
    </div>
    <div class="tree" role="tree" aria-label="服务" tabindex="0" bind:this={treeEl} onkeydown={onKey}>
      {#if error}
        <div class="empty-msg bad">{error}</div>
      {:else if !list}
        <div class="empty-msg wait"><span class="spinner sm"></span> 正在查找 Spring Boot 应用…</div>
      {:else if list.length === 0}
        <div class="empty-msg">
          没认出 Spring Boot 应用
          <div class="sub">各模块 src/main/java 中带有 @SpringBootApplication 注解且包含 main 方法的类将显示在此</div>
        </div>
      {:else}
        {#each rows as row, i (row.key)}
          {@const r = row.svc ? runOf(row.svc) : undefined}
          <div
            class="row"
            class:on={sel === row.key}
            data-i={i}
            role="treeitem"
            aria-selected={sel === row.key}
            aria-expanded={row.parent ? !closed.has(row.key) : undefined}
            tabindex="-1"
            style:padding-left="{6 + row.depth * 19}px"
            onclick={() => {
              sel = row.key;
              treeEl?.focus();
            }}
            ondblclick={() => (row.svc ? start([row.svc]) : toggle(row.key))}
            oncontextmenu={(e) => openMenu(e, row)}
            onkeydown={() => {}}
            title={row.svc ? `${row.svc.mainClass}${row.svc.module ? `（${row.svc.module}）` : ""}\n双击启动` : row.label}
          >
            {#if row.parent}
              <span
                class="chev"
                class:open={!closed.has(row.key)}
                role="presentation"
                onclick={(e) => {
                  e.stopPropagation();
                  toggle(row.key);
                }}><Icon name="chevron-right" /></span
              >
            {:else}
              <span class="chev-gap"></span>
            {/if}
            {#if row.svc}
              <span class="dot {r?.status ?? 'idle'}"></span>
            {:else if row.icon}
              <span class="ic"><Icon name={row.icon} /></span>
            {/if}
            <span class="name">{row.label}</span>
            {#if r?.debug && alive(r) && (r.debug.phase === "paused" || r.debug.phase === "running")}<span class="dbg-note" class:paused={r.debug.phase === "paused"}>{r.debug.phase === "paused" ? "已在断点处暂停" : "调试中"}</span>{/if}
            {#if row.port}<span class="port">{row.port}</span>{:else if r?.status === "running"}<span class="note">正在启动…</span>{:else if r?.status === "stopping"}<span class="note">正在停止</span>{/if}
            {#if row.note && !row.port && r?.status !== "running" && r?.status !== "stopping"}<span class="note">{row.note}</span>{/if}
            {#if row.members && row.depth === 1}<span class="count">{row.members.length}</span>{/if}
          </div>
        {/each}
      {/if}
    </div>
  </div>
  <div class="right">
    {#if selRun}
      {@const run = selRun}
      {#if run.holder}
        {@const h = run.holder}
        <!-- 端口被占：同运行窗那张卡片（ui.md 第十三条第三档） -->
        <div class="cards">
          <div class="confirm warn">
            <span>端口 <b>{h.port}</b> 被 <b>{h.command}</b>（PID {h.pid}）占用{h.ours ? `，为之前运行的「${h.ours}」，未完全停止` : ""}</span>
            <button class="btn" onclick={() => dismissHolder(run.id)}>忽略</button>
            <button class="btn {h.ours ? 'primary' : 'danger'}" onclick={() => void freePortAndRerun(run.id)}>结束进程并重新运行</button>
          </div>
        </div>
      {/if}
      {#key run.id}
        <RunView log={run.log} />
      {/key}
    {:else if selSvc}
      <div class="empty-msg">
        「{short(selSvc)}」还没跑过
        <div class="sub">双击服务或点击左上角的 ▶ 启动，输出将显示在此</div>
      </div>
    {:else if list && list.length > 0}
      <div class="empty-msg">选择服务以查看其输出</div>
    {/if}
  </div>
</div>

{#if menu && cmenu.comp}
  <cmenu.comp x={menu.x} y={menu.y} title={menu.title} label="服务" items={menu.items} onclose={() => (menu = null)} />
{/if}

<style>
  .svc { display: flex; height: 100%; min-height: 0; }
  /* 左边树、右边输出，同 IDEA 的 Services。树那一栏定宽：输出才是要看的，树只要放得下类名和端口 */
  .left { flex: none; width: 300px; display: flex; flex-direction: column; min-height: 0; border-right: 1px solid var(--border-soft); }
  .right { flex: 1; min-width: 0; min-height: 0; display: flex; flex-direction: column; }
  .right > :global(*:last-child) { flex: 1; min-height: 0; }
  /* 岛内的工具栏：34 高（ui.md 一之二） */
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 2px; padding: 0 6px; border-bottom: 1px solid var(--border-soft); }
  .bar .gap { flex: 1; }
  .tree { flex: 1; overflow: auto; padding: 4px 6px 8px; outline: none; }
  /* 行：同 Maven 窗 / 文件树（24 高、内缩的圆角块当选中，ui.md 第一条） */
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding-right: 8px;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: var(--fs-md);
    white-space: nowrap;
    cursor: default;
  }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .tree:focus-visible .row.on { outline: 1px solid var(--accent); outline-offset: -1px; }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .chev-gap { flex: none; width: 16px; }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  /* 状态点：同运行窗标签上那个（跑着绿、停止中琥珀、失败红、结束了灰、没跑过空心） */
  .dot { flex: none; width: 8px; height: 8px; margin: 0 6px 0 4px; border-radius: 99px; background: var(--text-faint); }
  .dot.running { background: var(--git-added); }
  .dot.stopping { background: var(--lvl-warn); }
  .dot.failed { background: var(--lvl-error); }
  .dot.idle { background: none; box-shadow: inset 0 0 0 1px var(--text-faint); }
  .name { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; }
  .note { flex: 0 1 auto; min-width: 0; margin-left: 6px; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
  /* 端口是「它能用了」的信号，人要一眼找到它：强调色、等宽数字 */
  .port { flex: none; margin-left: 6px; color: var(--accent); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  /* 堆：淡色、不抢按钮的眼 —— 扫一眼「它还好不好」就够 */
  .heap { flex: none; display: inline-flex; align-items: center; gap: 6px; margin-right: 4px; color: var(--text-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .spark { width: 40px; height: 12px; }
  .spark polyline { fill: none; stroke: currentColor; stroke-width: 1; vector-effect: non-scaling-stroke; }
  /* 调试着的：停着的那个要一眼找到（琥珀，同「要你处理」的颜色），在跑的灰字 */
  .dbg-note { flex: none; margin-left: 6px; color: var(--text-faint); font-size: var(--fs-xs); }
  .dbg-note.paused { color: var(--lvl-warn); }
  .count { flex: none; margin-left: auto; color: var(--text-faint); font-size: var(--fs-xs); font-variant-numeric: tabular-nums; }
  .cards { flex: none !important; padding: 6px 8px 0; }
</style>
