<script lang="ts">
  /**
   * 任务列表（#48，⌃⌥R；⌃R 一个都没跑过时也开它）。骨架照 ui.md 第五节：输入 → 分组结果 → 脚栏键位。
   *
   * 每次打开都现读一遍（`task_list`）：tasks.json 刚改过、package.json 刚加了一条 script，开出来就是新的 ——
   * 读的是几个 KB 的文件，不值得缓存再操心它什么时候过期。写坏的那几条在顶上一条一条说出来（`problems`），
   * 不能只是从列表里消失：那会让人以为自己写对了。
   */
  import Icon from "../shell/Icon.svelte";
  import { taskList } from "../ipc/tasks";
  import type { TaskDef } from "../ipc/commands";
  import { project } from "../state/project.svelte";
  import { runs, type RunStatus } from "../state/runs.svelte";

  let { open = $bindable(false) }: { open?: boolean } = $props();

  let q = $state("");
  let defs = $state<TaskDef[]>([]);
  let file = $state(false);
  let problems = $state<string[]>([]);
  let loading = $state(false);
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);

  $effect(() => {
    if (!open) return;
    const root = project.root;
    q = "";
    sel = 0;
    problems = [];
    let dead = false;
    if (root) {
      loading = true;
      taskList(root)
        .then((l) => {
          if (dead) return;
          defs = l.tasks;
          file = l.file;
          problems = l.problems;
        })
        .catch((e) => !dead && (problems = [e instanceof Error ? e.message : String(e)]))
        .finally(() => !dead && (loading = false));
    } else {
      defs = [];
    }
    queueMicrotask(() => input?.focus({ preventScroll: true }));
    return () => {
      dead = true;
    };
  });

  type Row = { kind: "task"; def: TaskDef } | { kind: "file" };

  /** 空格隔开的每个词都要出现在名字或命令里（不分大小写）。几十条任务，不需要模糊打分 */
  let rows = $derived.by<Row[]>(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    const hit = defs.filter((d) => {
      const hay = `${d.name} ${d.command}`.toLowerCase();
      return words.every((w) => hay.includes(w));
    });
    return [...hit.map((def): Row => ({ kind: "task", def })), { kind: "file" }];
  });

  $effect(() => {
    // 过滤一改，选中项钳回范围里
    if (sel >= rows.length) sel = Math.max(0, rows.length - 1);
  });

  function statusOf(name: string): RunStatus | undefined {
    return runs.list.find((r) => r.root === project.root && r.name === name)?.status;
  }

  function pick(i: number) {
    const r = rows[i];
    open = false;
    if (!r) return;
    void import("../state/runs-ops").then((m) => (r.kind === "file" ? m.openTasksFile() : m.runTask(r.def.name)));
  }

  function onKey(e: KeyboardEvent) {
    if (!open) return;
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = (sel + 1) % rows.length;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = (sel - 1 + rows.length) % rows.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      pick(sel);
    }
  }

  const GROUP: Record<TaskDef["source"], string> = { file: "tasks.json", package: "package.json" };
  const STATUS: Partial<Record<RunStatus, string>> = { running: "在跑", stopping: "正在停" };
</script>

<svelte:window onkeydown={onKey} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim dim" onclick={() => (open = false)}></div>
  <div class="popup picker" role="dialog" aria-label="运行任务">
    <div class="input">
      <Icon name="run" />
      <input bind:this={input} bind:value={q} placeholder="运行哪个任务…" aria-label="搜任务" spellcheck="false" />
    </div>
    {#if problems.length}
      <div class="problems">
        {#each problems as p (p)}<div>{p}</div>{/each}
      </div>
    {/if}
    <div class="results" role="listbox">
      {#each rows as r, i (r.kind === "task" ? `t:${r.def.name}` : "file")}
        {#if r.kind === "task" && (i === 0 || (rows[i - 1].kind === "task" && (rows[i - 1] as { def: TaskDef }).def.source !== r.def.source))}
          <div class="group">{GROUP[r.def.source]}</div>
        {:else if r.kind === "file" && i > 0}
          <div class="sep"></div>
        {/if}
        <button
          class="row"
          class:on={i === sel}
          role="option"
          aria-selected={i === sel}
          onmouseenter={() => (sel = i)}
          onclick={() => pick(i)}
        >
          {#if r.kind === "task"}
            {@const st = statusOf(r.def.name)}
            <span class="name">{r.def.name}</span>
            <span class="cmd" title={r.def.cwd ? `${r.def.cwd}： ${r.def.command}` : r.def.command}>{r.def.command}</span>
            {#if st && STATUS[st]}<span class="st {st}">{STATUS[st]}</span>{/if}
          {:else}
            <span class="name action">{file ? "打开 tasks.json" : "新建 tasks.json"}</span>
            <span class="cmd">{file ? "改任务、加任务" : "写下这个项目要跑的命令（带例子）"}</span>
          {/if}
        </button>
      {/each}
      {#if !loading && defs.length === 0 && !q}
        <div class="empty">
          这个项目还没有任务。package.json 里的 scripts 会自动列在这儿；mvn、python 这类在 tasks.json 里写一行。
        </div>
      {/if}
    </div>
    <div class="foot">
      <span><kbd>↵</kbd> {rows[sel]?.kind === "file" ? "打开" : rows[sel]?.kind === "task" && statusOf((rows[sel] as { def: TaskDef }).def.name) === "running" ? "重跑" : "运行"}</span>
      <span><kbd>⌃R</kbd> 再跑上一个</span>
      <span><kbd>⌘F2</kbd> 停止</span>
    </div>
  </div>
{/if}

<style>
  .picker {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(620px, 90vw);
    max-height: 64vh;
    display: flex;
    flex-direction: column;
    z-index: 41;
    overflow: hidden;
  }
  .input {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    border-bottom: 1px solid var(--border-soft);
  }
  .input input {
    flex: 1;
    min-width: 0;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: var(--fs-lg);
  }
  .problems {
    padding: 6px 14px;
    background: rgba(214, 174, 88, 0.1);
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-dim);
    font-size: var(--fs-sm);
    line-height: 1.6;
  }
  .results { overflow-y: auto; padding: 4px 6px 6px; }
  /* 分组头吸顶（ui.md 第四条），底色跟着浮层走 */
  .group {
    position: sticky;
    top: 0;
    padding: 6px 8px 3px;
    background: var(--elevated);
    color: var(--text-faint);
    font-size: var(--fs-xs);
  }
  .sep { height: 1px; margin: 4px 6px; background: var(--border-soft); }
  /* 当前项：内缩的圆角块（ui.md 第一条），不是通栏色条 */
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    height: 26px;
    padding: 0 8px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .row.on { background: var(--selected); color: var(--text); }
  .name { flex: none; max-width: 40%; overflow: hidden; text-overflow: ellipsis; }
  .name.action { color: var(--accent); }
  /* 命令是原样的命令行：等宽（ui.md 八之二「等宽只给代码类内容」） */
  .cmd {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    color: var(--text-faint);
    font-family: var(--code-font);
    font-size: var(--fs-sm);
  }
  .st { flex: none; font-size: var(--fs-xs); }
  .st.running { color: var(--git-added); }
  .st.stopping { color: var(--lvl-warn); }
  .empty { padding: 10px 8px; color: var(--text-faint); font-size: var(--fs-sm); line-height: 1.6; }
  .foot {
    display: flex;
    gap: 14px;
    padding: 6px 14px;
    background: var(--chrome-scrim);
    color: var(--text-faint);
    font-size: var(--fs-xs);
  }
  kbd { font-family: var(--ui-font); color: var(--text-dim); }
</style>
