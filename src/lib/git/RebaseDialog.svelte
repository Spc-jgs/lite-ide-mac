<script lang="ts">
  /**
   * 交互式变基（docs/TAKEOVER.md G25，IDEA 的 Interactively Rebase from Here）：提交历史里右键一个提交 →「从这里交互式变基…」。
   * 列出从它（含）到 HEAD 的提交，**从旧到新**（git 的 todo 也是这个次序，读着和 `git rebase -i` 一样）。每个提交选一个动作、↑↓ 挪次序，
   * 「开始变基」照着做；撞了冲突停住，Git 面板里那块横幅接手（继续 / 跳过 / 放弃）。
   *
   * 不让 git 弹编辑器：改说明在这里写好，合并的说明在 Rust 里按次序拼（`gitsvc::rebase_todo`）。
   * 规矩当场判：第一个留下来的提交不能「并入上一个」（前面没有东西可并）—— 不对就不让开始，说为什么
   */
  import { gitRebasePlan, gitRebaseRun, type RebaseAction } from "../ipc/git";
  import type { RebaseCommit } from "../ipc/commands";
  import { git } from "../state/git.svelte";
  import { layout } from "../state/layout.svelte";
  import { notify } from "../state/notify.svelte";
  import { worktree } from "../state/worktree.svelte";
  import { imeKey } from "../shell/ime";
  import Icon from "../shell/Icon.svelte";

  let { repo, from, onclose }: { repo: string; from: string; onclose: () => void } = $props();

  type Row = RebaseCommit & { action: RebaseAction; text: string };
  let rows = $state<Row[]>([]);
  let loadErr = $state("");
  let busy = $state(false);
  let err = $state("");

  $effect(() => {
    void gitRebasePlan(repo, from)
      .then((p) => (rows = p.map((c) => ({ ...c, action: "pick", text: c.message }))))
      .catch((e) => (loadErr = e instanceof Error ? e.message : String(e)));
  });

  const ACTIONS: { id: RebaseAction; label: string; title: string }[] = [
    { id: "pick", label: "保留", title: "照原样留着（pick）" },
    { id: "reword", label: "改说明", title: "留着、换一段提交说明（reword）" },
    { id: "squash", label: "并入上一个", title: "改动并进上一个，两段说明接在一起（squash）" },
    { id: "fixup", label: "只并改动", title: "改动并进上一个，只留上一个的说明（fixup）" },
    { id: "drop", label: "丢掉", title: "这个提交不要了（drop）" },
  ];

  /** 第一个不是「丢掉」的那行要是「并入」，前面没东西可并 */
  const problem = $derived.by(() => {
    const first = rows.find((r) => r.action !== "drop");
    if (!first) return "全丢掉的话，用不着变基 —— 至少留一个";
    if (first.action === "squash" || first.action === "fixup") return `「${first.subject}」是留下来的第一个，前面没有可以并进去的提交`;
    if (rows.some((r) => r.action === "reword" && !r.text.trim())) return "改说明的那一行说明是空的";
    return "";
  });
  /** 已经推上去的有几个：有上游时，领先的那几个是没推的，其余都推过 —— 变基之后要强推（这里没有强推） */
  const pushed = $derived(git.status?.upstream ? Math.max(0, rows.length - (git.status?.ahead ?? 0)) : 0);
  const after = $derived(rows.filter((r) => r.action === "pick" || r.action === "reword").length);

  function move(i: number, d: -1 | 1) {
    const j = i + d;
    if (j < 0 || j >= rows.length) return;
    const next = [...rows];
    [next[i], next[j]] = [next[j], next[i]];
    rows = next;
  }

  async function start() {
    if (problem || busy) return;
    busy = true;
    err = "";
    try {
      const r = await gitRebaseRun(repo, from, rows.map((x) => ({ sha: x.sha, action: x.action, message: x.action === "reword" ? x.text : "" })));
      onclose();
      await git.refresh();
      await worktree.changed();
      if (r === "done") notify.ok(`变基做完了：${rows.length} 个提交 → ${after} 个`);
      else {
        notify.hint("停在冲突上了：Git 面板里解决、暂存之后点「继续」，或者放弃");
        layout.sideView = "git";
        layout.sidebar = true;
      }
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
    } else if (e.key === "Enter" && e.metaKey) {
      e.preventDefault();
      void start();
    }
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog rebase" role="dialog" aria-label="交互式变基" tabindex="-1" onkeydown={onKey}>
    <p class="title">交互式变基{#if rows.length}<span class="sub">从 {rows[0].short} 起的 {rows.length} 个提交</span>{/if}</p>
    <p class="body">从旧到新（和 git 的次序一样）。给每个提交选做什么、↑↓ 挪次序，「开始变基」照着做；撞了冲突停住，在 Git 面板里继续或者放弃。</p>
    {#if pushed > 0}
      <p class="warn">其中 {pushed} 个已经推到 {git.status?.upstream} 了：变基之后和远程分叉，要强推才推得上去（这里没有强推）—— 只整理还没推的那几个更省事</p>
    {/if}
    <div class="list">
      {#if loadErr}
        <div class="empty-msg bad">{loadErr}</div>
      {:else if !rows.length}
        <div class="empty-msg wait"><span class="spinner sm"></span> 正在读提交…</div>
      {:else}
        {#each rows as r, i (r.sha)}
          <div class="row" class:dropped={r.action === "drop"}>
            <span class="mv">
              <button class="ibtn xs" aria-label="往前挪" title="往前挪（更早）" disabled={i === 0} onclick={() => move(i, -1)}><Icon name="chevron-up" /></button>
              <button class="ibtn xs" aria-label="往后挪" title="往后挪（更晚）" disabled={i === rows.length - 1} onclick={() => move(i, 1)}><Icon name="chevron-down" /></button>
            </span>
            <span class="segs" role="radiogroup" aria-label="{r.short} 做什么">
              {#each ACTIONS as a (a.id)}
                <button class="btn sm" class:on={r.action === a.id} role="radio" aria-checked={r.action === a.id} title={a.title} onclick={() => (r.action = a.id)}>{a.label}</button>
              {/each}
            </span>
            <span class="sha">{r.short}</span>
            <span class="subj" title={r.message}>{r.subject}</span>
          </div>
          {#if r.action === "reword"}
            <textarea class="msg" rows="3" spellcheck="false" bind:value={() => r.text, (v) => (r.text = v)} aria-label="{r.short} 的新说明"></textarea>
          {/if}
        {/each}
      {/if}
    </div>
    <div class="actions">
      {#if problem && rows.length}<span class="err">{problem}</span>{:else if err}<span class="err">{err}</span>{/if}
      <span class="gap"></span>
      <button class="btn" data-dismiss onclick={onclose}>取消</button>
      <button class="btn primary" data-default disabled={!!problem || busy || !rows.length} class:busy onclick={() => void start()}>开始变基</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(8vh, 64px); background: rgba(0, 0, 0, 0.38); }
  .rebase { width: min(820px, calc(100% - 48px)); max-height: calc(100vh - 96px); }
  .sub { margin-left: 8px; font-size: var(--fs-sm); font-weight: normal; color: var(--text-faint); }
  .warn { margin: 0; padding: 6px 10px; border-radius: var(--r-sm); background: color-mix(in srgb, var(--lvl-warn) 14%, transparent); font-size: var(--fs-sm); color: var(--text); }
  .list { flex: 1; min-height: 120px; overflow: auto; margin: 4px -8px 0; padding: 0 8px; }
  .row { display: flex; align-items: center; gap: 8px; min-height: 30px; }
  .mv { flex: none; display: flex; gap: 2px; }
  .segs { flex: none; }
  .sha { flex: none; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-faint); }
  .subj { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-size: var(--fs-md); color: var(--text); }
  .dropped .subj, .dropped .sha { text-decoration: line-through; color: var(--text-faint); }
  .msg { width: 100%; margin: 2px 0 6px; padding: 6px 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-family: var(--code-font); font-size: var(--fs-sm); resize: vertical; outline: none; box-sizing: border-box; }
  .msg:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .err { font-size: var(--fs-sm); color: var(--lvl-error); max-width: 480px; }
</style>
