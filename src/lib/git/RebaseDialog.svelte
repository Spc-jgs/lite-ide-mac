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
    { id: "pick", label: "保留", title: "保留此提交（pick）" },
    { id: "reword", label: "改写说明", title: "保留此提交并修改提交说明（reword）" },
    { id: "squash", label: "合并到上一个", title: "将更改合并到上一个提交，并合并两段提交说明（squash）" },
    { id: "fixup", label: "仅合并更改", title: "将更改合并到上一个提交，仅保留上一个提交的说明（fixup）" },
    { id: "drop", label: "删除", title: "删除此提交（drop）" },
  ];

  /** 第一个不是「丢掉」的那行要是「并入」，前面没东西可并 */
  const problem = $derived.by(() => {
    const first = rows.find((r) => r.action !== "drop");
    if (!first) return "至少需要保留一个提交";
    if (first.action === "squash" || first.action === "fixup") return `「${first.subject}」是保留的第一个提交，之前没有可合并的提交`;
    if (rows.some((r) => r.action === "reword" && !r.text.trim())) return "改写说明的提交不能使用空说明";
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
      if (r === "done") notify.ok(`变基已完成：${rows.length} 个提交 → ${after} 个`);
      else {
        notify.hint("变基因冲突暂停：请在 Git 面板中解决并暂存后点击「继续」，或中止变基");
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
    <p class="title">交互式变基{#if rows.length}<span class="sub">自 {rows[0].short} 起的 {rows.length} 个提交</span>{/if}</p>
    <p class="body">按从旧到新排列（与 Git 一致）。为每个提交选择操作，用 ↑↓ 调整次序，然后点击「开始变基」。如遇冲突将暂停，可在 Git 面板中继续或中止。</p>
    {#if pushed > 0}
      <p class="warn">其中 {pushed} 个提交已推送到 {git.status?.upstream}。变基后将与远程分叉，需要强制推送（此处不提供）。建议仅整理尚未推送的提交。</p>
    {/if}
    <div class="list">
      {#if loadErr}
        <div class="empty-msg bad">{loadErr}</div>
      {:else if !rows.length}
        <div class="empty-msg wait"><span class="spinner sm"></span> 正在读取提交…</div>
      {:else}
        {#each rows as r, i (r.sha)}
          <div class="row" class:dropped={r.action === "drop"}>
            <span class="mv">
              <button class="ibtn xs" aria-label="上移" title="上移（更早）" disabled={i === 0} onclick={() => move(i, -1)}><Icon name="chevron-up" /></button>
              <button class="ibtn xs" aria-label="下移" title="下移（更晚）" disabled={i === rows.length - 1} onclick={() => move(i, 1)}><Icon name="chevron-down" /></button>
            </span>
            <span class="segs" role="radiogroup" aria-label="{r.short} 的操作">
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
