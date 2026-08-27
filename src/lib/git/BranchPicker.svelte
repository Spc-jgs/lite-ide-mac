<script lang="ts">
  import type { GitBranch, GitWorktree } from "../ipc/commands";
  import { gitBranches, gitWorktrees } from "../ipc/commands";

  let {
    open = $bindable(false),
    repo,
    onSwitch,
    onNewBranch,
    onOpenWorktree,
    onNewWorktree,
    onRemoveWorktree,
  }: {
    open?: boolean;
    repo: string;
    onSwitch: (name: string) => void;
    onNewBranch: (name: string) => void;
    onOpenWorktree: (path: string) => void;
    /** 目录名 + 分支名；分支不存在就新建 */
    onNewWorktree: (dir: string, branch: string) => void;
    onRemoveWorktree: (w: GitWorktree) => void;
  } = $props();

  let q = $state("");
  let branches = $state<GitBranch[]>([]);
  let trees = $state<GitWorktree[]>([]);
  let loading = $state(false);
  let err = $state("");
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  /** 新建工作树的两个输入 */
  let wtDir = $state("");
  let wtBranch = $state("");
  let mode = $state<"list" | "newWorktree">("list");

  // 每次打开都重新拉：分支和工作树在终端里随时会变，缓存只会骗人
  $effect(() => {
    if (!open) return;
    q = "";
    sel = 0;
    mode = "list";
    err = "";
    void (async () => {
      loading = true;
      try {
        [branches, trees] = await Promise.all([gitBranches(repo), gitWorktrees(repo)]);
      } catch (e) {
        err = String(e);
      } finally {
        loading = false;
      }
    })();
    // 等 DOM 出来再聚焦
    queueMicrotask(() => input?.focus());
  });

  interface Item {
    kind: "branch" | "remote" | "worktree" | "newBranch" | "newWorktree";
    label: string;
    hint: string;
    tag?: string;
    current?: boolean;
    branch?: GitBranch;
    tree?: GitWorktree;
  }

  let items = $derived.by(() => {
    const k = q.trim().toLowerCase();
    const hit = (s: string) => !k || s.toLowerCase().includes(k);
    const out: Item[] = [];

    for (const b of branches.filter((b) => !b.isRemote && hit(b.name))) {
      out.push({
        kind: "branch",
        label: b.name,
        hint: b.subject,
        tag: b.upstream ? `↗ ${b.upstream}` : "",
        current: b.isHead,
        branch: b,
      });
    }
    for (const w of trees.filter((w) => hit(w.branch || w.path))) {
      out.push({
        kind: "worktree",
        label: w.branch || `(${w.sha})`,
        hint: w.path,
        tag: w.locked ? "已锁定" : "",
        current: w.current,
        tree: w,
      });
    }
    for (const b of branches.filter((b) => b.isRemote && hit(b.name))) {
      out.push({ kind: "remote", label: b.name, hint: b.subject, branch: b });
    }
    // 输入了名字但没有同名分支 —— 直接给个「新建」出口，不用先切到别的界面
    if (k && !branches.some((b) => b.name.toLowerCase() === k)) {
      out.push({ kind: "newBranch", label: `新建分支「${q.trim()}」`, hint: "从当前 HEAD 分出" });
    }
    out.push({ kind: "newWorktree", label: "新建工作树…", hint: "把另一个分支检出到独立目录" });
    return out;
  });

  // 过滤变了就把选中项拉回顶部，否则会停在一个已经不存在的位置上
  $effect(() => {
    q;
    sel = 0;
  });

  function pick(it: Item) {
    switch (it.kind) {
      case "branch":
        if (!it.current) onSwitch(it.label);
        open = false;
        break;
      case "remote":
        // 检出远程分支：git 会自动建同名本地跟踪分支
        onSwitch(it.label);
        open = false;
        break;
      case "worktree":
        if (!it.current && it.tree) onOpenWorktree(it.tree.path);
        open = false;
        break;
      case "newBranch":
        onNewBranch(q.trim());
        open = false;
        break;
      case "newWorktree":
        wtBranch = q.trim();
        wtDir = "";
        mode = "newWorktree";
        break;
    }
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (mode === "newWorktree") mode = "list";
      else open = false;
      return;
    }
    if (mode !== "list") return;
    if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = (sel + 1) % Math.max(1, items.length);
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = (sel - 1 + items.length) % Math.max(1, items.length);
    } else if (e.key === "Enter") {
      e.preventDefault();
      const it = items[sel];
      if (it) pick(it);
    }
  }

  function submitWorktree() {
    if (!wtDir.trim()) return;
    // 目标分支留空就用目录名当分支名 —— 十有八九就是想要的
    const dir = wtDir.trim();
    const br = wtBranch.trim() || dir.slice(dir.lastIndexOf("/") + 1);
    onNewWorktree(dir, br);
    open = false;
  }
</script>

<svelte:window onkeydown={open ? onKey : undefined} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div class="scrim" onclick={() => (open = false)}></div>
  <div class="popup" role="dialog" aria-label="分支与工作树">
    {#if mode === "list"}
      <input
        bind:this={input}
        bind:value={q}
        placeholder="切换分支 / 打开工作树，输入新名字可直接新建"
        spellcheck="false"
      />
      <div class="results">
        {#if err}
          <div class="none err">{err}</div>
        {:else if loading}
          <div class="none">载入中…</div>
        {:else if items.length === 0}
          <div class="none">没有匹配</div>
        {:else}
          {#each items as it, i (it.kind + it.label)}
            {#if i === 0 || items[i - 1].kind !== it.kind}
              <div class="sec">
                {#if it.kind === "branch"}本地分支
                {:else if it.kind === "worktree"}工作树
                {:else if it.kind === "remote"}远程分支
                {:else}操作{/if}
              </div>
            {/if}
            <div class="rowwrap" class:on={i === sel}>
              <button class="row" onclick={() => pick(it)} onmouseenter={() => (sel = i)}>
                <span class="ic {it.kind}"></span>
                <span class="lb" class:cur={it.current}>{it.label}</span>
                {#if it.current}<span class="now">当前</span>{/if}
                {#if it.tag}<span class="tg">{it.tag}</span>{/if}
                <span class="ht">{it.hint}</span>
              </button>
              {#if it.kind === "worktree" && it.tree && !it.current && !it.tree.bare}
                <button
                  class="rm"
                  onclick={() => { onRemoveWorktree(it.tree!); open = false; }}
                  title="移除这个工作树"
                  aria-label="移除工作树"
                >✕</button>
              {/if}
            </div>
          {/each}
        {/if}
      </div>
      <div class="foot">
        <span>↑↓ 选择 · ↵ 确认 · esc 关闭</span>
      </div>
    {:else}
      <div class="form">
        <div class="ftitle">新建工作树</div>
        <p class="fdesc">
          工作树是同一个仓库的第二个检出目录 —— 可以在不打断当前改动的前提下，
          把另一个分支同时摊开在磁盘上。
        </p>
        <label>
          <span>目录</span>
          <input
            class="fi"
            bind:value={wtDir}
            placeholder="../lite-ide-hotfix 或绝对路径"
            spellcheck="false"
            onkeydown={(e) => e.key === "Enter" && submitWorktree()}
          />
        </label>
        <label>
          <span>分支</span>
          <input
            class="fi"
            bind:value={wtBranch}
            placeholder="留空则用目录名，分支不存在就新建"
            spellcheck="false"
            onkeydown={(e) => e.key === "Enter" && submitWorktree()}
          />
        </label>
        <div class="frow">
          <button onclick={() => (mode = "list")}>返回</button>
          <span class="gap"></span>
          <button class="primary" disabled={!wtDir.trim()} onclick={submitWorktree}>创建并打开</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.35); z-index: 40; }
  .popup {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(620px, 88vw);
    max-height: 66vh;
    display: flex;
    flex-direction: column;
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 16px 48px rgba(0, 0, 0, 0.5);
    z-index: 41;
    overflow: hidden;
  }
  .popup > input {
    border: none;
    border-bottom: 1px solid var(--border-soft);
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 14px;
    padding: 11px 14px;
    outline: none;
  }
  .popup > input::placeholder { color: var(--text-faint); }

  .results { overflow-y: auto; padding: 4px 0; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; }
  .none.err { color: var(--lvl-error); font-family: var(--code-font); text-align: left; }
  .sec {
    padding: 7px 14px 3px;
    font-size: 10.5px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
  }

  .rowwrap { display: flex; align-items: center; }
  .rowwrap.on { background: var(--accent-sel); }
  .row {
    display: flex;
    align-items: center;
    gap: 8px;
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 6px 0 14px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .rowwrap.on .row { color: var(--text); }
  .lb { flex: none; max-width: 46%; overflow: hidden; text-overflow: ellipsis;
        font-family: var(--code-font); font-size: 12px; }
  .lb.cur { color: var(--accent); }
  .ht {
    flex: 1;
    min-width: 0;
    color: var(--text-faint);
    font-size: 11px;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .now, .tg {
    flex: none;
    font-size: 10px;
    padding: 0 5px;
    border-radius: 8px;
    background: var(--panel-bg-2);
    color: var(--text-faint);
  }
  .now { color: var(--accent); }
  .rm {
    flex: none;
    width: 22px;
    height: 22px;
    margin-right: 8px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-faint);
    font-size: 10px;
    cursor: default;
    opacity: 0;
  }
  .rowwrap:hover .rm { opacity: 1; }
  .rm:hover { background: var(--lvl-error); color: #fff; }

  /* 图标用纯 CSS 画，省一个 SVG：分支是圆点，工作树是方块，操作是加号 */
  .ic { flex: none; width: 9px; height: 9px; }
  .ic.branch, .ic.remote {
    border-radius: 50%;
    border: 1.6px solid var(--git-modified);
  }
  .ic.remote { border-color: var(--git-untracked); }
  .ic.worktree { border: 1.6px solid var(--git-renamed); border-radius: 2px; }
  .ic.newBranch, .ic.newWorktree {
    border-radius: 50%;
    border: 1.6px dashed var(--text-faint);
  }

  .foot {
    flex: none;
    padding: 6px 14px;
    border-top: 1px solid var(--border-soft);
    font-size: 10.5px;
    color: var(--text-faint);
    font-family: var(--code-font);
    user-select: none;
  }

  .form { padding: 16px 18px 14px; display: flex; flex-direction: column; gap: 10px; }
  .ftitle { font-size: 14px; color: var(--text); }
  .fdesc { margin: 0; font-size: 11.5px; line-height: 1.6; color: var(--text-faint); }
  .form label { display: flex; align-items: center; gap: 10px; font-size: 12px; color: var(--text-dim); }
  .form label > span { flex: none; width: 34px; }
  .fi {
    flex: 1;
    background: var(--editor-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    font-family: var(--code-font);
    font-size: 12px;
    padding: 5px 8px;
    outline: none;
  }
  .fi:focus { border-color: var(--accent); }
  .frow { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
  .frow .gap { flex: 1; }
  .frow button {
    padding: 4px 12px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-dim);
    font-size: 12px;
    cursor: default;
  }
  .frow button:hover { background: var(--panel-bg-2); color: var(--text); }
  .frow button.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .frow button.primary:disabled { background: transparent; border-color: var(--border); color: var(--text-faint); }
</style>
