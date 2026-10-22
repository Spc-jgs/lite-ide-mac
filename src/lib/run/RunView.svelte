<script lang="ts">
  /**
   * 运行窗里的一格（#48）：一次运行的输出文件，用日志视图看 —— 级别着色、过滤、跟随、堆栈帧跳源码全是现成的。
   *
   * 挂载时开日志句柄、卸载时关。**卸载不碰进程**：任务活在 Rust 那边，切到别的工具窗、切到别的任务都只是不看了
   * （和终端不一样，终端的 xterm 卸了 shell 就没了，所以那边只藏不卸）。
   *
   * 跟随默认开着：跑任务就是为了看它此刻在说什么。重跑时 Rust 把上一次的挪成 `.1`、新开一份同名的 ——
   * 日志视图是 `tail -F` 的语义（按名字重开、句柄不变，LogPane 里 `rotated` 那段），这里不用管。
   */
  import LogPane from "../logview/LogPane.svelte";
  import TestTree from "./TestTree.svelte";
  import BuildTree from "./BuildTree.svelte";
  import DebugPane from "./DebugPane.svelte";
  import HttpPane from "./HttpPane.svelte";
  import { openLog, closeLog } from "../ipc/commands";
  import { parseTestTask } from "../editor/java-tests";
  import { runs } from "../state/runs.svelte";

  let { log, id }: { log: string; id?: number } = $props();
  /** 这是哪一次运行：按输出文件找（每次运行一份，Rust 发的路径），调用方只传 `log` 就够。跑单测那一格（「模块 [test …]」）左边加结果树（docs/IDEA.md 第 17 节） */
  // 给了 id 按 id 找：HTTP 请求还在发的时候 `log` 是空串，按它找会认到别的格子上（运行窗传 id，Services 只有任务、按 log 就够）
  let run = $derived(runs.list.find((r) => (id !== undefined ? r.id === id : r.log === log)) ?? null);
  let isTest = $derived(!!run && !!parseTestTask(run.name));
  /** 构建那一格（⌘F9，docs/IDEA.md 第 22 节）：左边是错误树（同 IDEA 的 Build 窗） */
  let isBuild = $derived(run?.name === "构建项目" || run?.name === "重新构建项目");

  let handle = $state<number | null>(null);
  let err = $state("");
  /** 日志视图报上来的状态（索引中、轮转过几次…）。编辑区里它进状态栏，这里没有状态栏，底下一行小字，空的时候不占地方 */
  let status = $state("");

  $effect(() => {
    const path = log;
    // HTTP 请求还在发 / 没发成：还没有响应体（docs/IDEA.md 第 19 节）
    if (!path) return;
    let dead = false;
    let h: number | null = null;
    openLog(path)
      .then((r) => {
        // 等它回来的时候这一格已经被换掉了（frontend.md「异步 effect 里 await 之后要检查自己是不是已经被清理了」）
        if (dead) return void closeLog(r.handle);
        h = r.handle;
        handle = h;
      })
      .catch((e) => (err = e instanceof Error ? e.message : String(e)));
    return () => {
      dead = true;
      if (h !== null) void closeLog(h);
      handle = null;
    };
  });

  // onlyHits 和日志标签页的默认一样是 true：点 ERROR 就只剩 ERROR 行（issue #48 验收 1「能过滤」）。第 3 步写成了 false（全文 + 标命中），
  // 点了级别一行都不藏、看着像过滤坏了 —— 第 5 步拿真 Spring Boot 验收时撞见的。想看上下文，过滤条上「全文」一点就回去
  const FOLLOW = { levelBits: 0b111111, pattern: "", caseSensitive: false, tailing: true, collapseStacks: false, onlyHits: true };
  // HTTP 的响应体（docs/IDEA.md 第 19 节）：一次写完的东西，从头看 —— 跟随尾部的话一打开就在最底下（浏览器里看出来的）
  const STILL = { ...FOLLOW, tailing: false };
</script>

{#snippet plain(h: number)}
  <div class="split" class:tests={isTest || isBuild}>
    {#if isTest && run}
      <div class="tests-pane"><TestTree {run} /></div>
    {:else if isBuild && run}
      <div class="tests-pane"><BuildTree {run} /></div>
    {/if}
    {@render output(h)}
  </div>
{/snippet}

{#snippet output(h: number)}
  <div class="run">
    {#if run?.status === "stopping" && run.stuck?.length}
      {@const s = run.stuck}
      <!-- 停不掉（#68）：组里剩的是别的用户的进程，我们发不了信号。说清是谁、给一条能抄的命令 —— 别人的东西，人抄了才动（ui.md 第七条） -->
      <div class="cards">
        <div class="confirm warn stuck">
          <span>
            停不掉：还剩 {s.length} 个 <b>{s[0].user}</b>{s.some((x) => x.user !== s[0].user) ? " 等别的用户" : ""} 的进程，lite-ide 没权限结束它们（多半是 sudo 起的）。等它们自己退，或者在终端里：
            <code>sudo kill {s.map((x) => x.pid).join(" ")}</code>
            <span class="procs">{#each s as x (x.pid)}<span>{x.pid} · {x.user} · {x.command}</span>{/each}</span>
          </span>
          <button class="btn" onclick={() => void navigator.clipboard.writeText(`sudo kill ${s.map((x) => x.pid).join(" ")}`)}>复制命令</button>
        </div>
      </div>
    {/if}
    <div class="body">
      {#key h}
        <LogPane handle={h} initialFilter={run?.http ? STILL : FOLLOW} onStatus={(s) => (status = s)} />
      {/key}
    </div>
    {#if status}<div class="status">{status}</div>{/if}
  </div>
{/snippet}

{#if run?.http}
  <!-- `.http` 里发的请求（docs/IDEA.md 第 19 节）：状态、耗时、响应头；响应体是这份输出（日志视图那套） -->
  <HttpPane {run}>
    {#snippet body()}
      {#if handle !== null}{@render output(handle)}{:else if err}<div class="msg">打不开响应体：{err}</div>{/if}
    {/snippet}
  </HttpPane>
{:else if handle !== null}
  {@const h = handle}
  {#if run?.debug}
    <!-- 调试着起的（docs/IDEA.md 第 18 节）：调试器 / 控制台两页，控制台那页就是不调试时的样子（单测带结果树，同 IDEA 的 Debug 窗） -->
    <DebugPane {run}>
      {#snippet console()}{@render plain(h)}{/snippet}
    </DebugPane>
  {:else}
    {@render plain(h)}
  {/if}
{:else if err}
  <div class="msg">打不开输出：{err}</div>
{:else}
  <div class="msg"><span class="spinner"></span>正在打开输出…</div>
{/if}

<style>
  .split { display: flex; height: 100%; min-height: 0; }
  /* 结果树在左、输出在右（同 IDEA 的测试视图）。树那一栏定宽：要看的是失败原因和输出 */
  .tests-pane { flex: none; width: 320px; min-height: 0; border-right: 1px solid var(--border-soft); }
  .run { flex: 1; min-width: 0; display: flex; flex-direction: column; height: 100%; }
  .body { flex: 1; min-height: 0; }
  .cards { flex: none; padding: 8px 10px 0; }
  .stuck code { font-family: var(--code-font); font-size: var(--fs-sm); padding: 0 4px; border-radius: var(--r-sm); background: var(--hover); user-select: all; }
  .procs { display: flex; flex-direction: column; margin-top: 4px; font-family: var(--code-font); font-size: var(--fs-xs); color: var(--text-dim); }
  .status {
    flex: none;
    padding: 2px 10px;
    color: var(--text-faint);
    font-size: var(--fs-xs);
    border-top: 1px solid var(--border-soft);
  }
  .msg {
    display: grid;
    place-content: center;
    grid-auto-flow: column;
    gap: 6px;
    height: 100%;
    color: var(--text-faint);
    font-size: var(--fs-md);
  }
</style>
