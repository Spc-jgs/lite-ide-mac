<script lang="ts">
  /**
   * 标题栏右边的运行挂件，照 IDEA 新 UI 的 Run widget（docs/TAKEOVER.md I3）：
   * 「当前运行配置 ▾」+ ▶ 运行 + 🐞 调试 + ■ 停。「我该去哪起它、改它的 VM 参数」只有这一个答案。
   *
   * 当前那份：你在下拉里挑的 > 这个项目最近跑过的（⌃R 那份）> 列表里的第一个。挑的只记在这次运行里（同 IDEA 换项目就换）。
   * 列表现问（下拉一打开就问一次）：tasks.json、你建的配置随时会改，缓存一份会过时。
   * 懒的：标题栏（入口包）只有一行懒加载
   */
  import Icon from "../shell/Icon.svelte";
  import type { MenuItem } from "../shell/ContextMenu.svelte";
  import { cmenu } from "../shell/context-menu.svelte";
  import type { TaskDef } from "../ipc/commands";
  import { taskList } from "../ipc/tasks";
  import { runs } from "../state/runs.svelte";
  import { last, runTask, stopRun } from "../state/runs-ops";

  let { root }: { root: string } = $props();

  let list = $state<TaskDef[]>([]);
  /** 下拉里挑过的（按项目根） */
  let picked = $state<Record<string, string>>({});
  let menu = $state<{ x: number; y: number } | null>(null);

  async function refresh() {
    list = (await taskList(root).catch(() => null))?.tasks ?? [];
  }
  $effect(() => {
    root;
    void refresh();
  });

  const current = $derived(picked[root] ?? last[root] ?? list.find((t) => t.source === "spring")?.name ?? list[0]?.name ?? "");
  const def = $derived(list.find((t) => t.name === current) ?? null);
  /** 能调试的：命令是我们拼的 java（认出来的 Spring 服务、你建的应用配置）。组合配置按里面的一份份起，调试时各自判 */
  const debuggable = $derived(!!def && (def.source === "spring" || (def.source === "user" && (def.kind === "application" || def.kind === "compound" || def.kind === "remote"))));
  /** 它（组合配置就是它里面那几份）此刻在跑的那几格 */
  const live = $derived.by(() => {
    const names = def?.kind === "compound" ? def.items : [current];
    return runs.list.filter((r) => r.root === root && names.includes(r.name) && (r.status === "running" || r.status === "stopping"));
  });

  async function openMenu(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    await refresh();
    cmenu.load();
    menu = { x: r.left, y: r.bottom + 4 };
  }

  const GROUP: Record<TaskDef["source"], string> = { spring: "Spring Boot", user: "你建的", package: "package.json", file: "tasks.json", goal: "Maven" };
  const items = $derived.by<MenuItem[]>(() => {
    const out: MenuItem[] = [];
    let group = "";
    // 按组排：常起的（Spring 服务、你建的）在上，脚本类在下；组内保持列表原来的次序
    const order: TaskDef["source"][] = ["spring", "user", "package", "file", "goal"];
    for (const t of [...list].sort((a, b) => order.indexOf(a.source) - order.indexOf(b.source))) {
      const g = GROUP[t.source];
      out.push({ label: t.name, run: () => (picked = { ...picked, [root]: t.name }), sep: !!out.length && g !== group, checked: t.name === current });
      group = g;
    }
    out.push({
      label: "编辑配置…",
      sep: out.length > 0,
      run: () => void import("../settings/open").then((m) => m.openRunConfigs(root, current)),
    });
    return out;
  });
</script>

<div class="rw" role="toolbar" aria-label="运行">
  <button class="cfg" onclick={(e) => void openMenu(e)} title={def ? `${def.command || "组合：" + def.items.join("、")}\n点开换一份、或者编辑配置` : "运行配置"}>
    {#if live.length}<span class="dot" aria-label="在跑"></span>{/if}
    <span class="wlabel">{current || "运行配置"}</span>
    <Icon name="chevron-down" />
  </button>
  <button class="ibtn" disabled={!current} onclick={() => void runTask(current, root)} title={live.length ? `重跑「${current}」（⌃R）` : `运行「${current}」`} aria-label="运行">
    <Icon name={live.length ? "rerun" : "run"} />
  </button>
  <button class="ibtn" disabled={!debuggable} onclick={() => void runTask(current, root, true)} title={debuggable ? `调试「${current}」（⌃D）` : "此配置不是 Java 程序，无法调试"} aria-label="调试">
    <Icon name="debug" />
  </button>
  <button class="ibtn" disabled={!live.length} onclick={() => { for (const r of live) void stopRun(r.id); }} title={`停止「${current}」（⌘F2）`} aria-label="停止">
    <Icon name="stop" />
  </button>
</div>

{#if menu && cmenu.comp}
  <cmenu.comp x={menu.x} y={menu.y} label="运行配置" {items} onclose={() => (menu = null)} />
{/if}

<style>
  .rw { display: flex; align-items: center; gap: 2px; padding-right: 8px; }
  /* 同标题栏左边的项目 / 分支挂件（TitleBar 的 .twidget 是那个组件私有的，这里照抄一份，名字是正文色 —— 它是「现在选的是哪份」） */
  .cfg {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 240px;
    height: 24px;
    padding: 0 6px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    cursor: default;
  }
  .cfg:hover { background: var(--hover); }
  .cfg:active { background: var(--pressed); }
  .wlabel { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .dot { flex: none; width: 6px; height: 6px; border-radius: 99px; background: var(--ok, #4caf50); }
</style>
