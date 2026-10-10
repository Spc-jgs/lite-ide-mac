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
  import { openLog, closeLog } from "../ipc/commands";

  let { log }: { log: string } = $props();

  let handle = $state<number | null>(null);
  let err = $state("");
  /** 日志视图报上来的状态（索引中、轮转过几次…）。编辑区里它进状态栏，这里没有状态栏，底下一行小字，空的时候不占地方 */
  let status = $state("");

  $effect(() => {
    const path = log;
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
</script>

{#if handle !== null}
  <div class="run">
    <div class="body">
      {#key handle}
        <LogPane {handle} initialFilter={FOLLOW} onStatus={(s) => (status = s)} />
      {/key}
    </div>
    {#if status}<div class="status">{status}</div>{/if}
  </div>
{:else if err}
  <div class="msg">打不开输出：{err}</div>
{:else}
  <div class="msg"><span class="spinner"></span>正在打开输出…</div>
{/if}

<style>
  .run { display: flex; flex-direction: column; height: 100%; }
  .body { flex: 1; min-height: 0; }
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
