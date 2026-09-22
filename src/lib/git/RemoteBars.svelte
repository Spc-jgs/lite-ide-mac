<script lang="ts">
  /**
   * 拉取 / 推送路上的三条确认条。
   *
   * # 为什么单独一个组件
   *
   * 它们原来长在 `App.svelte` 里，而 App.svelte 是**入口包**的大头
   * （35KB 归因，第二名的三倍）。这三条只有在真去拉/推的时候才会出现，
   * 而那时 Git 那一组早就加载完了 —— 判据同 ARCHITECTURE 那条：
   * **问一句「这东西在窗口出现之前有用吗」**。
   *
   * 挪出来之后入口包 137.0 → 134.6 KB（告警线 138）。
   *
   * # 三条各是什么
   *
   * - **分岔**：快进不了，要先决定合并还是变基。**不给默认选项** ——
   *   两者后果不一样，替人选一个是越权。照 IDEA 带「记一下」。
   * - **推送确认**：照 IDEA 的推送对话框**列出要推的提交**，不是只给计数。
   * - **失败**：转译 + git 的原话，两个都要 —— 转译错了的时候
   *   人得有办法绕过我们（同差异视图的 `truncated`）。
   */
  import type { RemoteErr } from "../ipc/commands";

  let {
    progress = null,
    diverge = null,
    push = null,
    err = null,
    upstream = "",
    ahead = 0,
    onMerge,
    onPush,
    onPull,
    onDismiss,
    onCancel = null,
  }: {
    /**
     * 正在跑的远程操作的进度（M9，从 Git 页的分支行底下搬过来 —— 那一行删了）。
     * 是卡片不是模态：拉取的时候人还想接着看代码。百分比可能是 null
     * （git 的措辞不是稳定接口），那时只有一条来回跑的条。
     */
    progress?: { what: "pull" | "push" | "fetch"; phase: string; percent: number | null } | null;
    diverge: { upstream: string } | null;
    push: { branch: string; setUpstream: boolean; commits: string[] } | null;
    err: (RemoteErr & { hint: string }) | null;
    upstream?: string;
    ahead?: number;
    /** `remember` 为真时上层要记住这次选择 */
    onMerge: (mode: "merge" | "rebase", remember: boolean) => void;
    onPush: () => void;
    onPull: () => void;
    /** which: 关掉哪一条 */
    onDismiss: (which: "diverge" | "push" | "err") => void;
    /** 取消正在跑的远程操作。push 进行中不给（状态不确定），由上层决定传不传 */
    onCancel?: (() => void) | null;
  } = $props();

  const WHAT = { pull: "拉取", push: "推送", fetch: "抓取" } as const;

  let remember = $state(false);
  /** git 的原话展开了没 */
  let rawOpen = $state(false);
</script>

{#if progress}
  <div class="confirm prog">
    <div class="pline">
      <!-- 先说是在推还是在拉，再引 git 的原话 —— 只印 `Enumerating objects` 人不知道这是哪个动作的哪一步 -->
      <span class="ptext"><b>{WHAT[progress.what]}</b> · {progress.phase}</span>
      {#if progress.percent !== null}<span class="ppct">{progress.percent}%</span>{/if}
      {#if onCancel}<button class="btn sm" onclick={onCancel}>取消</button>{/if}
    </div>
    <div class="pbar" class:indet={progress.percent === null}>
      {#if progress.percent !== null}
        <div class="pfill" style:width="{progress.percent}%"></div>
      {/if}
    </div>
  </div>
{/if}

{#if diverge}
  <div class="confirm">
    <span>本地和 <b>{diverge.upstream}</b> 分岔了 —— 快进不了，得选一种</span>
    <span class="gap"></span>
    <label class="remember">
      <input type="checkbox" bind:checked={remember} />
      记一下
    </label>
    <button class="btn" onclick={() => onMerge("rebase", remember)}>变基</button>
    <button class="btn" onclick={() => onDismiss("diverge")}>取消</button>
    <button class="btn primary" onclick={() => onMerge("merge", remember)}>合并</button>
  </div>
{/if}

{#if push}
  <div class="confirm tall">
    <div class="info">
      <span>
        {#if push.setUpstream}
          这个分支还没有上游。推送会在远程<b>新建</b>
          <span class="mono">origin/{push.branch}</span>，并把本地这条跟过去
        {:else}
          要推 {push.commits.length || ahead} 个提交到 <span class="mono">{upstream}</span>
        {/if}
      </span>
      {#if push.commits.length}
        <ul class="list">
          {#each push.commits as c (c)}<li>{c}</li>{/each}
        </ul>
      {/if}
    </div>
    <span class="gap"></span>
    <button class="btn" onclick={() => onDismiss("push")}>取消</button>
    <button class="btn primary" onclick={onPush}>{push.setUpstream ? "推送并跟踪" : "推送"}</button>
  </div>
{/if}

{#if err}
  <div class="confirm tall bad">
    <div class="info">
      <span><b>{err.message}</b></span>
      <!-- 给的是能直接粘的那条命令 —— 「去终端里认证一下」等于没说 -->
      {#if err.hint}<pre class="hint">{err.hint}</pre>{/if}
      {#if rawOpen && err.raw}<pre class="raw">{err.raw}</pre>{/if}
    </div>
    <span class="gap"></span>
    {#if err.raw}
      <button class="btn" onclick={() => (rawOpen = !rawOpen)}>{rawOpen ? "收起" : "看 git 的原话"}</button>
    {/if}
    <button
      class="btn"
      onclick={() => {
        rawOpen = false;
        onDismiss("err");
      }}>知道了</button>
    {#if err.kind === "rejected"}
      <!-- 给下一步，不是给句号 —— 它是主动作，排最右 -->
      <button class="btn primary" onclick={onPull}>先拉取</button>
    {/if}
  </div>
{/if}

<style>
  /* 卡片的面在 app.css 的 `.confirm`（和 Confirms.svelte 那几条浮在同一叠里，2026-09-21 收成一份） */
  .mono { font-family: var(--code-font); }
  .info { display: flex; flex-direction: column; gap: 5px; min-width: 0; }

  .list {
    margin: 0;
    padding: 0 0 0 2px;
    list-style: none;
    font-family: var(--code-font);
    font-size: 10.5px;
    line-height: 1.7;
    color: var(--text-faint);
  }
  .list li::before { content: "· "; }

  .hint,
  .raw {
    margin: 2px 0 0;
    padding: 7px 9px;
    /* 要挡光：这块是内容，不是外壳 */
    background: var(--content-solid);
    border: 1px solid var(--border-soft);
    border-radius: var(--r-sm);
    font-family: var(--code-font);
    line-height: 1.6;
    white-space: pre-wrap;
    user-select: text;
  }
  .hint { font-size: 11px; color: var(--text-dim); }
  .raw { font-size: 10.5px; color: var(--text-faint); max-height: 120px; overflow: auto; }

  .remember { display: flex; align-items: center; gap: 4px; flex: none; font-size: 11.5px; }
  .remember input { margin: 0; }

  /* 进度卡片：一行文字 + 3px 的条。indet 那条来回跑，只说「还在动」，**不能显示成 0%** */
  .confirm.prog { flex-direction: column; align-items: stretch; gap: 6px; width: min(420px, calc(100% - 32px)); }
  .pline { display: flex; align-items: center; gap: 8px; }
  .ptext { flex: 1; min-width: 0; font-size: 11.5px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .ppct { flex: none; font-family: var(--code-font); font-size: 11px; color: var(--text-faint); }
  .pbar { height: 3px; border-radius: 2px; background: var(--hover); overflow: hidden; }
  .pfill { height: 100%; background: var(--accent); transition: width 0.12s linear; }
  .pbar.indet::after {
    content: "";
    display: block;
    width: 34%;
    height: 100%;
    background: var(--accent);
    animation: slide 1.1s ease-in-out infinite;
  }
  @keyframes slide {
    0% { transform: translateX(-100%); }
    100% { transform: translateX(300%); }
  }
  @media (prefers-reduced-motion: reduce) {
    .pbar.indet::after { animation: none; width: 100%; opacity: 0.4; }
    .pfill { transition: none; }
  }
</style>
