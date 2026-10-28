<script lang="ts">
  /**
   * 请求历史（#77，IDEA 的 `http-requests-log.http`）：这个项目发过的请求，最近的在前。
   * 每行可以「打开」（跳回 `.http` 文件里那个请求）或「重新发送」（照文件**此刻**的写法再发，同运行窗的重发 —— 历史里本来就不存头和体）。
   * 只记了「是哪个请求、结果怎样」，URL 里像密钥的查询参数打了码（`httpsvc::history`）
   */
  import { untrack } from "svelte";
  import { httpHistory, httpHistoryClear } from "../ipc/http";
  import type { HttpHist } from "../ipc/commands";
  import { notify } from "../state/notify.svelte";
  import { nav } from "../state/nav.svelte";
  import { readText } from "../ipc/commands";
  import { parseHttp } from "./http-file";

  let { root, onclose }: { root: string; onclose: () => void } = $props();
  let list = $state<HttpHist[] | null>(null);
  let err = $state("");

  untrack(() => {
    httpHistory(root).then(
      (v) => (list = v),
      (e) => (err = String(e)),
    );
  });

  const time = (ms: number) => {
    const d = new Date(ms);
    const p = (n: number) => String(n).padStart(2, "0");
    const today = new Date().toDateString() === d.toDateString();
    return `${today ? "" : `${d.getMonth() + 1}-${p(d.getDate())} `}${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
  };
  const tone = (s: number) => (s === 0 || s >= 500 ? "bad" : s >= 400 ? "warn" : s >= 300 ? "" : "good");
  const fileName = (f: string) => f.slice(f.lastIndexOf("/") + 1);

  async function openIt(h: HttpHist) {
    const { findRequest } = await import("./http-ops");
    const text = await readText(h.file).then((t) => t.content).catch(() => null);
    const req = text === null ? null : findRequest(parseHttp(text), h.name, h.method, h.template);
    onclose();
    if (text === null) return void notify.fail(`无法读取 ${h.file}`);
    await nav.openAt(h.file, req?.line, false);
    if (!req) notify.hint("文件中已找不到该请求（可能已被修改或删除）");
  }

  async function resend(h: HttpHist) {
    const { resendHist } = await import("./http-ops");
    if (await resendHist(root, h.file, h.name, h.method, h.template)) onclose();
  }

  async function clearAll() {
    try {
      await httpHistoryClear(root);
      list = [];
      notify.ok("已清除请求历史");
    } catch (e) {
      notify.fail(String(e));
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    }
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog httphist" role="dialog" aria-label="请求历史" tabindex="-1" onkeydown={onKey}>
    <p class="title">请求历史</p>
    {#if err}
      <p class="body bad">无法读取请求历史：{err}</p>
    {:else if list === null}
      <p class="body"><span class="spinner sm"></span> 正在读取…</p>
    {:else if list.length === 0}
      <p class="body">暂无记录。发送过的请求会按时间列在这里（不记录请求头和请求体）</p>
    {:else}
      <div class="rows">
        {#each list as h, i (i)}
          <div class="row">
            <span class="t">{time(h.at)}</span>
            <span class="st {tone(h.status)}" title={h.error}>{h.status || "失败"}</span>
            <span class="what" title={`${h.method} ${h.url}\n${h.file}`}><b>{h.method}</b> {h.name || h.url}</span>
            <span class="meta">{fileName(h.file)}{h.env ? ` · ${h.env}` : ""} · {h.ms} ms</span>
            <button class="btn sm quiet" onclick={() => void openIt(h)}>打开</button>
            <button class="btn sm" onclick={() => void resend(h)}>重新发送</button>
          </div>
        {/each}
      </div>
    {/if}
    <div class="actions">
      {#if list?.length}<button class="btn quiet" onclick={() => void clearAll()}>清除历史</button>{/if}
      <span class="gap"></span>
      <button class="btn" onclick={onclose}>关闭</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .httphist { width: min(760px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 8px; }
  .rows { max-height: 52vh; overflow: auto; display: flex; flex-direction: column; }
  .row { display: flex; align-items: center; gap: 8px; padding: 3px 0; font-size: var(--fs-sm); }
  .t { flex: none; width: 92px; color: var(--text-faint); font-variant-numeric: tabular-nums; }
  .st { flex: none; min-width: 34px; text-align: center; padding: 0 5px; border-radius: 99px; font-variant-numeric: tabular-nums; background: rgba(255, 255, 255, 0.08); }
  .st.good { color: var(--git-added); }
  .st.warn { color: var(--lvl-warn); }
  .st.bad { color: var(--lvl-error); }
  .what { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--code-font); color: var(--text-dim); }
  .what b { color: var(--text); font-weight: 600; }
  .meta { flex: none; color: var(--text-faint); white-space: nowrap; }
  .bad { color: var(--lvl-error); }
</style>
