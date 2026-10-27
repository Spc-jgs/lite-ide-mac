<script lang="ts">
  /**
   * `.http` 里发的一个请求在运行窗里的那一格（docs/IDEA.md 第 19 节），照 IDEA 的 Services → HTTP Request 那一页：
   *
   * ```
   * POST http://localhost:8080/api/orders     201 Created · 35 ms · 1.2 KB · 环境 dev        [响应体 | 响应头] ⋯
   * {                                          ← 响应体用日志视图那套（搜索、过滤、几十 MB 也能翻），JSON 已经按原样的次序排好
   *   "id": 7,
   * ```
   *
   * 还在发：「正在发…」（150ms 之后才出现，ui.md 第十四条）和取消；没发成：原因（连不上、超时、证书），照 curl 说的翻成人话。
   */
  import type { Snippet } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import type { RunTab } from "../state/runs.svelte";
  import { openRunLog } from "../state/runs-ops";

  let { run, body }: { run: RunTab; body: Snippet } = $props();

  let page = $state<"body" | "headers">("body");
  let h = $derived(run.http!);
  let resp = $derived(h.resp);
  let sending = $derived(run.status === "running");

  const size = (n: number) => (n < 1024 ? `${n} B` : n < 1 << 20 ? `${(n / 1024).toFixed(1)} KB` : `${(n / (1 << 20)).toFixed(1)} MB`);
  /** 状态码的颜色：2xx 绿、3xx 中性、4xx 琥珀、5xx 红（IDEA 也是按段上色） */
  const tone = (s: number) => (s >= 500 ? "bad" : s >= 400 ? "warn" : s >= 300 ? "" : "good");

  async function cancel() {
    const { cancel } = await import("../http/http-ops");
    cancel(run.id);
  }
</script>

<div class="http">
  <div class="bar">
    <span class="req" title={`${h.method} ${h.url}`}><b>{h.method}</b> {h.url}</span>
    {#if sending}
      <span class="note wait"><span class="spinner sm"></span> 正在发…</span>
      <button class="btn sm" onclick={() => void cancel()}>取消</button>
    {:else if resp}
      <span class="status {tone(resp.status)}">{resp.status}{resp.reason ? ` ${resp.reason}` : ""}</span>
      <span class="note">
        {resp.ms} ms · {size(resp.bytes)}{resp.redirects ? ` · 重定向 ${resp.redirects} 次` : ""}{h.env ? ` · 环境 ${h.env}` : ""}
      </span>
    {/if}
    <span class="gap"></span>
    {#if resp}
      <div class="segs" role="tablist">
        <button class="btn sm" role="tab" aria-selected={page === "body"} class:on={page === "body"} onclick={() => (page = "body")}>响应体</button>
        <button class="btn sm" role="tab" aria-selected={page === "headers"} class:on={page === "headers"} onclick={() => (page = "headers")}>响应头 {resp.headers.length}</button>
      </div>
      <button class="ibtn" onclick={() => void openRunLog(run.id)} title="在编辑区打开响应体（语法着色、折叠、和别的文件并排）" aria-label="在编辑区打开"><Icon name="expand" /></button>
    {/if}
  </div>
  {#if resp && (resp.truncated || h.req.handler)}
    <div class="notes">
      {#if resp.truncated}<div>响应体太大：收到了 {size(resp.bytes)} 以上，只存了前 32 MB</div>{/if}
      {#if h.req.handler}<div>这个请求写了响应处理脚本（<code>&gt; &#123;% … %&#125;</code>），当前版本不执行：这需要在应用中执行文件自带的代码</div>{/if}
    </div>
  {/if}
  <div class="pages">
    {#if h.error}
      <div class="empty-msg" class:bad={h.error !== "已取消"}>
        {h.error === "已取消" ? "取消了，没等到响应" : h.error}
        {#if h.error !== "已取消"}<div class="sub">点右上角的重跑再发一次；请求照文件此刻的样子</div>{/if}
      </div>
    {:else if sending}
      <div class="empty-msg wait"><span class="spinner sm"></span> 等服务器回话…（停在断点上的服务会一直等，点「取消」不等了）</div>
    {:else if resp}
      <div class="page" class:hidden={page !== "body"}>
        {#if resp.bytes === 0}
          <div class="empty-msg">没有响应体</div>
        {:else}
          {@render body()}
        {/if}
      </div>
      <div class="page heads" class:hidden={page !== "headers"}>
        <div class="hrow first"><span class="hn">{resp.version}</span><span class="hv">{resp.status} {resp.reason}</span></div>
        {#each resp.headers as x, i (i)}
          <div class="hrow"><span class="hn">{x.name}</span><span class="hv">{x.value}</span></div>
        {/each}
        {#if resp.finalUrl !== h.url}<div class="hrow"><span class="hn">最后落在</span><span class="hv">{resp.finalUrl}</span></div>{/if}
      </div>
    {/if}
  </div>
</div>

<style>
  .http { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  /* 岛内的工具栏：34 高（ui.md 一之二） */
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 8px; padding: 0 6px 0 10px; border-bottom: 1px solid var(--border-soft); white-space: nowrap; }
  .bar .gap { flex: 1; }
  /* URL 是程序里的东西：等宽（ui.md 八之二）。长了从右边省略 —— 有用的是路径，主机一排格子里都一样也看得出 */
  .req { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; color: var(--text-dim); font-family: var(--code-font); font-size: var(--fs-sm); }
  .req b { color: var(--text); font-weight: 600; }
  .status { flex: none; padding: 1px 7px; border-radius: 99px; font-size: var(--fs-sm); font-variant-numeric: tabular-nums; background: rgba(255, 255, 255, 0.08); color: var(--text); }
  .status.good { color: var(--git-added); background: color-mix(in srgb, var(--git-added) 14%, transparent); }
  .status.warn { color: var(--lvl-warn); background: color-mix(in srgb, var(--lvl-warn) 14%, transparent); }
  .status.bad { color: var(--lvl-error); background: color-mix(in srgb, var(--lvl-error) 14%, transparent); }
  .note { flex: none; display: inline-flex; align-items: center; gap: 6px; color: var(--text-faint); font-size: var(--fs-sm); font-variant-numeric: tabular-nums; }
  .notes { flex: none; padding: 4px 10px; color: var(--lvl-warn); font-size: var(--fs-sm); border-bottom: 1px solid var(--border-soft); }
  .notes code { font-family: var(--code-font); }
  .pages { flex: 1; min-height: 0; position: relative; }
  .page { position: absolute; inset: 0; }
  .page.hidden { visibility: hidden; pointer-events: none; }
  .heads { overflow: auto; padding: 6px 10px; }
  /* 出错那一句是红的；底下那句是下一步怎么做，不是错（ui.md 第九条「空态 = 为什么 + 下一步」） */
  .pages :global(.empty-msg.bad .sub) { color: var(--text-faint); font-family: var(--ui-font); }
  .hrow { display: flex; gap: 12px; padding: 2px 0; font-family: var(--code-font); font-size: var(--fs-sm); user-select: text; }
  .hrow.first { color: var(--text); padding-bottom: 6px; }
  .hn { flex: none; min-width: 160px; color: var(--text-dim); }
  .hv { flex: 1; min-width: 0; color: var(--text); word-break: break-all; }
</style>
