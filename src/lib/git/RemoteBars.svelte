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
  import { cardOut } from "../shell/motion";
  import type { RemoteErr } from "../ipc/commands";
  import { takeFocus } from "../shell/dialog-focus";

  let {
    part,
    diverge = null,
    push = null,
    err = null,
    upstream = "",
    ahead = 0,
    onMerge,
    onPush,
    onPull,
    onDismiss,
  }: {
    /**
     * `ask` = 分岔、推送确认（你点了拉取 / 推送才有的选择题，对话框，放在 Confirms 的压暗层里）；
     * `tell` = 失败（告诉你结果，卡片，放在顶上那叠里）。2026-10-12 设计稿①
     */
    part: "ask" | "tell";
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
  } = $props();

  let remember = $state(false);
  /** git 的原话展开了没 */
  let rawOpen = $state(false);
</script>

{#if part === "ask" && diverge}
  <!-- 不设默认按钮（上面「三条各是什么」）：合并和变基后果不一样，回车不该替人选 —— 焦点给「取消」 -->
  <div class="dialog" role="alertdialog" aria-label="本地和远程分岔了" use:takeFocus>
    <p class="title">本地和 {diverge.upstream} 分岔了</p>
    <p class="body">快进不了，得选一种：合并会多一个合并提交；变基把你的提交挪到它们后面，历史是一条线。</p>
    <div class="actions">
      <label class="remember">
        <input type="checkbox" bind:checked={remember} />
        记住这次的选择
      </label>
      <span class="gap"></span>
      <button class="btn" data-dismiss data-default onclick={() => onDismiss("diverge")}>取消</button>
      <button class="btn" onclick={() => onMerge("rebase", remember)}>变基</button>
      <button class="btn" onclick={() => onMerge("merge", remember)}>合并</button>
    </div>
  </div>
{:else if part === "ask" && push}
  <div class="dialog" role="alertdialog" aria-label="推送吗" use:takeFocus>
    {#if push.setUpstream}
      <p class="title">推送并在远程新建 origin/{push.branch}？</p>
      <p class="body">这个分支还没有上游。推送会在远程新建它，并把本地这条跟过去。</p>
    {:else}
      <p class="title">推送 {push.commits.length || ahead} 个提交到 {upstream}？</p>
    {/if}
    {#if push.commits.length}
      <ul class="list">
        {#each push.commits as c (c)}<li>{c}</li>{/each}
      </ul>
    {/if}
    <div class="actions">
      <span class="gap"></span>
      <button class="btn" data-dismiss onclick={() => onDismiss("push")}>取消</button>
      <button class="btn primary" data-default onclick={onPush}>{push.setUpstream ? "推送并跟踪" : "推送"}</button>
    </div>
  </div>
{/if}

{#if part === "tell" && err}
  <div class="confirm tall bad" out:cardOut>
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
      data-dismiss
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
  .info { display: flex; flex-direction: column; gap: 5px; min-width: 0; }

  .list {
    margin: 0;
    padding: 8px 10px;
    max-height: 9em;
    overflow: auto;
    list-style: none;
    border-radius: var(--r-sm);
    background: rgba(0, 0, 0, 0.22);
    font-family: var(--ui-font);
    font-size: var(--fs-sm);
    line-height: 1.7;
    color: var(--text-dim);
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
  .hint { font-size: var(--fs-sm); color: var(--text-dim); }
  .raw { font-size: var(--fs-xs); color: var(--text-faint); max-height: 120px; overflow: auto; }

  .remember { display: flex; align-items: center; gap: 6px; flex: none; font-size: var(--fs-sm); color: var(--text-muted); }
  .remember input { margin: 0; }

</style>
