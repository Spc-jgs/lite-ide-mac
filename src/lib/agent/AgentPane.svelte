<script lang="ts">
  import HideSidebar from "../shell/HideSidebar.svelte";
  /**
   * AI 助手（docs/TAKEOVER.md MVP-6），侧边栏里的一个视图。设计稿在 docs/TAKEOVER.md 那一节链着的画布上（主界面、第一次、审阅三张）。
   *
   * 三个样子：没选过 agent → 选一个；对话；审阅 agent 这几轮写过的文件（留下 = 存盘、退回 = 编辑器里换回去）。
   * agent 的回答是 Markdown，这里**不渲染成 HTML**（页面里不许插 HTML，frontend.md）：按 ``` 切成正文和代码两种块，正文保留换行
   */
  import { untrack } from "svelte";
  import { agent, rel } from "./agent.svelte";
  import { layout } from "../state/layout.svelte";
  import { nav } from "../state/nav.svelte";
  import { project } from "../state/project.svelte";
  import { imeKey } from "../shell/ime";
  import { unifiedDiff } from "../git/linediff";
  import Icon from "../shell/Icon.svelte";

  /** 对话要的宽度：侧边栏是文件树和它共用的，240 放不下一段代码。进来时拉宽、走时还回去（只在这个懒块里记，入口一个字节不多） */
  const WIDE = 400;
  const WIDTH_KEY = "lite.agent.width";
  $effect(() => {
    const prev = untrack(() => layout.sidebarWidth);
    let want = WIDE;
    try {
      want = Number(localStorage.getItem(WIDTH_KEY)) || WIDE;
    } catch {
      /* 用默认宽度 */
    }
    if (prev < want) untrack(() => (layout.sidebarWidth = Math.min(want, window.innerWidth - 480)));
    return () => {
      const w = untrack(() => layout.sidebarWidth);
      try {
        localStorage.setItem(WIDTH_KEY, String(w));
      } catch {
        /* 下次用默认宽度 */
      }
      untrack(() => (layout.sidebarWidth = prev));
    };
  });

  // 宿主给每个侧边栏工具窗都传项目根；这里跟着 `project.root` 走（会话在哪个根上起的记在会话里）
  let {}: { root: string } = $props();

  let draft = $state("");
  let list: HTMLElement | undefined = $state();
  let input: HTMLTextAreaElement | undefined = $state();
  /** 用户往上翻了就不再自动滚到底（同运行窗的日志跟随） */
  let follow = true;

  const items = $derived(agent.items);
  $effect(() => {
    // 字一块块来时贴着底
    void items.length;
    const last = items[items.length - 1];
    void (last && "text" in last ? last.text : 0);
    if (follow && list) queueMicrotask(() => list && (list.scrollTop = list.scrollHeight));
  });

  // 右键「发给 AI 助手」挂上一段之后，光标进输入框接着打字
  $effect(() => {
    if (agent.ctx.length) untrack(() => input?.focus());
  });

  function onScroll() {
    if (list) follow = list.scrollHeight - list.scrollTop - list.clientHeight < 40;
  }

  function send() {
    const t = draft;
    if (agent.running || (!t.trim() && agent.ctx.length === 0)) return;
    draft = "";
    follow = true;
    void agent.send(t);
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
      e.preventDefault();
      send();
    } else if (e.key === "Escape" && agent.running) {
      e.preventDefault();
      agent.cancel();
    }
  }

  /** 正文和代码块：按 ``` 切 */
  function blocks(text: string): { code: boolean; text: string }[] {
    const out: { code: boolean; text: string }[] = [];
    const parts = text.split(/^```[^\n]*\n?/m);
    parts.forEach((p, i) => {
      const t = i % 2 === 1 ? p.replace(/\n$/, "") : p.trim();
      if (t) out.push({ code: i % 2 === 1, text: t });
    });
    return out;
  }

  const modeName = $derived.by(() => {
    const m = agent.session?.modes;
    return m?.availableModes.find((x) => x.id === m.currentModeId)?.name ?? "";
  });
  const agentName = $derived(agent.session?.name ?? (agent.kind === "codex" ? "Codex" : "Claude Code"));

  /** 审阅单里一份的差异（统一格式），加减行数 */
  function diffOf(before: string | null, after: string, path: string) {
    const d = unifiedDiff(before ?? "", after, rel(path), 2) ?? "";
    const lines = d.split("\n").filter((l) => !l.startsWith("---") && !l.startsWith("+++") && !l.startsWith("diff "));
    return {
      lines: lines.slice(0, 400),
      more: Math.max(0, lines.length - 400),
      add: lines.filter((l) => l.startsWith("+")).length,
      del: lines.filter((l) => l.startsWith("-")).length,
    };
  }
  let open = $state<string | null>(null);
</script>

<div class="agent">
  {#if !agent.kind}
    <div class="head">
      <span class="title">AI 助手</span>
      <HideSidebar />
    </div>
    <div class="first">
      <div class="h">选一个 Agent</div>
      <p class="dim">它在这个项目里读代码、改文件、跑命令。先不先问你，跟着 agent 自己的模式走（输入框下面那个按钮，点一下换；Claude Code 的「Manual」每次都问）。用的是你自己的账号和额度。</p>
      {#each [{ k: "claude", n: "Claude Code", b: "C", s: "要先在终端里登录过 claude" }, { k: "codex", n: "Codex", b: "X", s: "要先在终端里登录过 codex" }] as const as a}
        <button class="pick" onclick={() => agent.choose(a.k)}>
          <span class="badge {a.k}">{a.b}</span>
          <span class="col"><span class="nm">{a.n}</span><span class="sub">{a.s}</span></span>
        </button>
      {/each}
      <p class="note">第一次起要经 npx 从 npm 下一个适配器（约 20 秒），之后秒起。机器上要有 Node.js。<br />每开一个新会话，agent 先读一遍项目上下文（约 3 万 token），会话尽量接着用。</p>
    </div>
  {:else if agent.reviewing}
    <div class="head">
      <button class="btn sm quiet" onclick={() => (agent.reviewing = false)}>‹ 对话</button>
      <span class="ttl">改了 {agent.edits.length} 个文件</span>
      <HideSidebar />
    </div>
    <p class="dim pad">已经写进编辑器、还没存盘。留下的存盘；退回的换回改之前那份（⌘Z 撤得回）。它新建的文件退回就进废纸篓。</p>
    <div class="edits">
      {#each agent.edits as e (e.path)}
        {@const d = diffOf(e.before, e.after, e.path)}
        <div class="edit">
          <div class="erow">
            <button class="fn" title={e.path} onclick={() => (open = open === e.path ? null : e.path)}>{rel(e.path)}</button>
            {#if e.before === null}<span class="faint">新文件</span>{/if}
            <span class="add">+{d.add}</span><span class="del">−{d.del}</span>
            <span class="gap"></span>
            {#if e.state === "pending"}
              <button class="btn sm" onclick={() => void agent.revert(e)}>退回</button>
              <button class="btn sm primary" onclick={() => void agent.keep(e)}>留下</button>
            {:else}
              <span class={e.state === "kept" ? "ok" : "faint"}>{e.state === "kept" ? "✓ 留下了" : "退回了"}</span>
            {/if}
          </div>
          {#if open === e.path}
            <div class="diff">
              {#each d.lines as l, i (i)}
                <div class={l.startsWith("+") ? "a" : l.startsWith("-") ? "d" : l.startsWith("@@") ? "h" : ""}>{l || " "}</div>
              {/each}
              {#if d.more}<div class="h">…还有 {d.more} 行，打开文件看</div>{/if}
            </div>
          {/if}
        </div>
      {/each}
    </div>
    <div class="foot">
      <button class="btn sm" disabled={!agent.pendingEdits.length} onclick={() => void agent.revertAll()}>全部退回</button>
      <button class="btn sm primary" disabled={!agent.pendingEdits.length} onclick={() => void agent.keepAll()}>剩下的都留下</button>
    </div>
  {:else}
    <div class="head">
      <span class="title">AI 助手</span>
      <span class="faint">{agentName}{agent.session?.version ? ` ${agent.session.version}` : ""}</span>
      <span class="gap"></span>
      {#if agent.pendingEdits.length}
        <button class="btn sm" onclick={() => (agent.reviewing = true)} title="agent 写过、还没留下 / 退回的文件">审阅 {agent.pendingEdits.length}</button>
      {/if}
      <button class="ibtn sm" title="新对话（停掉现在这个会话）" aria-label="新对话" disabled={agent.running} onclick={() => agent.reset()}><Icon name="plus" /></button>
      <HideSidebar />
    </div>
    <div class="items" bind:this={list} onscroll={onScroll}>
      {#each items as it, i (i)}
        {#if it.t === "user"}
          <div class="user">
            {#each it.ctx as c (c.path + c.from)}<span class="chip">{rel(c.path)}:{c.from}{c.to > c.from ? `–${c.to}` : ""}</span>{/each}
            {#if it.text}<div class="txt">{it.text}</div>{/if}
          </div>
        {:else if it.t === "agent"}
          <div class="say">
            {#each blocks(it.text) as b, j (j)}
              {#if b.code}<pre class="code">{b.text}</pre>{:else}<div class="txt">{b.text}</div>{/if}
            {/each}
          </div>
        {:else if it.t === "thought"}
          <details class="thought"><summary>想法</summary><div class="txt">{it.text}</div></details>
        {:else if it.t === "tool"}
          <details class="tool" class:bad={it.status === "failed"}>
            <summary>
              <span class="st">{#if it.status === "failed"}<Icon name="error" />{:else if it.status === "completed"}<Icon name="check" />{:else}<span class="spinner sm"></span>{/if}</span>
              <span class="tt">{it.title || it.kind}</span>
              {#if it.path && !(it.title || "").includes(rel(it.path))}
                <button class="loc" title={it.path} onclick={(e) => { e.preventDefault(); void nav.openAt(it.path, 1, false); }}>{rel(it.path)}</button>
              {/if}
            </summary>
            {#if it.out}<pre class="code out">{it.out}</pre>{/if}
          </details>
        {:else if it.t === "plan"}
          <div class="plan">
            {#each it.entries as p, j (j)}
              <div class="pe" class:done={p.status === "completed"} class:now={p.status === "in_progress"}>{p.status === "completed" ? "✓" : p.status === "in_progress" ? "▸" : "○"} {p.content}</div>
            {/each}
          </div>
        {:else if it.t === "perm"}
          <div class="perm">
            <div class="ptitle">{it.title}</div>
            {#if it.chosen}
              <div class="faint">{it.chosen === "—" ? "没回答（这一轮停了）" : `选了：${it.options.find((o) => o.optionId === it.chosen)?.name ?? it.chosen}`}</div>
            {:else}
              <div class="opts">
                {#each it.options as o (o.optionId)}
                  <button class="btn sm" class:primary={o.kind === "allow_once"} onclick={() => agent.answer(it, o)}>{o.name}</button>
                {/each}
              </div>
            {/if}
          </div>
        {:else}
          <div class="note" class:bad={it.bad}>{it.text}</div>
        {/if}
      {:else}
        <div class="empty-msg">
          问它这个项目里的事、让它改代码
          <span class="sub">编辑器里选中一段、右键「发给 AI 助手」，那段会带着位置挂在输入框上。⌘↵ 发出，Esc 停这一轮</span>
        </div>
      {/each}
      {#if agent.starting}
        <div class="note wait"><span class="spinner sm"></span> 正在起 {agentName}… {agent.starting} 秒{agent.starting > 4 ? "（第一次要从 npm 下适配器）" : ""}</div>
      {:else if agent.running && items.some((x) => x.t === "perm" && !x.chosen)}
        <div class="note">等你回答上面那个</div>
      {:else if agent.running}
        <div class="note wait"><span class="spinner sm"></span> {agentName} 在想…</div>
      {/if}
    </div>
    <div class="composer">
      {#if agent.ctx.length}
        <div class="chips">
          {#each agent.ctx as c, i (i)}
            <span class="chip">{rel(c.path)}:{c.from}{c.to > c.from ? `–${c.to}` : ""}<button class="ibtn xs" aria-label="去掉这段" title="去掉这段" onclick={() => agent.ctx.splice(i, 1)}><Icon name="x" /></button></span>
          {/each}
        </div>
      {/if}
      <textarea bind:this={input} bind:value={draft} onkeydown={onKey} rows="3" placeholder={project.root ? `问 ${agentName}…（⌘↵ 发出）` : "先打开一个项目"} disabled={!project.root}></textarea>
      <div class="bar">
        <button class="btn sm quiet" title="换一个 agent（停掉现在这个会话）" disabled={agent.running} onclick={() => agent.switchTo(agent.kind === "claude" ? "codex" : "claude")}>{agent.kind === "codex" ? "Codex" : "Claude Code"}</button>
        {#if modeName}
          <button class="btn sm quiet" title="换会话模式（agent 自己的：要不要先问再改、只做计划…），点一下换下一个" disabled={agent.running} onclick={() => void agent.cycleMode()}>{modeName}</button>
        {/if}
        <span class="gap"></span>
        {#if agent.running}
          <button class="btn sm" onclick={() => agent.cancel()} title="停这一轮 Esc">停止</button>
        {:else}
          <button class="btn sm primary" disabled={!project.root || (!draft.trim() && !agent.ctx.length) || agent.starting > 0} onclick={send}>发送 ⌘↵</button>
        {/if}
      </div>
    </div>
  {/if}
</div>

<style>
  .agent { height: 100%; display: flex; flex-direction: column; min-height: 0; font-size: var(--fs-md); }
  .head { flex: none; height: 38px; display: flex; align-items: center; gap: 8px; padding: 0 8px 0 10px; font-size: var(--fs-sm); color: var(--text-dim); }
  .title { letter-spacing: 0.06em; text-transform: uppercase; }
  .ttl { color: var(--text); font-weight: 600; }
  .gap { flex: 1; }
  .faint { color: var(--text-faint); font-size: var(--fs-sm); }
  .dim { color: var(--text-muted); line-height: 1.6; margin: 0; }
  .pad { padding: 0 12px 10px; font-size: var(--fs-sm); }
  .first { display: flex; flex-direction: column; gap: 12px; padding: 24px 14px; }
  .first .h { font-size: var(--fs-lg); font-weight: 600; }
  .pick { display: flex; align-items: center; gap: 12px; text-align: left; padding: 10px 12px; border: none; border-radius: var(--r-md); background: var(--hover); color: var(--text); font: inherit; cursor: pointer; }
  .pick:hover { background: var(--selected); }
  .badge { width: 30px; height: 30px; border-radius: var(--r-sm); display: grid; place-items: center; flex: none; font-weight: 600; }
  .badge.claude { background: #3a3530; color: #e8a17f; }
  .badge.codex { background: #2c3133; color: #9fd3c7; }
  .col { display: flex; flex-direction: column; gap: 2px; min-width: 0; }
  .nm { font-weight: 600; }
  .sub { font-size: var(--fs-sm); color: var(--text-faint); }
  .note { color: var(--text-faint); font-size: var(--fs-sm); line-height: 1.6; }
  .first .note { background: var(--hover); border-radius: var(--r-md); padding: 10px 12px; margin: 0; }
  .note.bad { color: var(--lvl-error); white-space: pre-wrap; }
  .items { flex: 1; min-height: 0; overflow: auto; padding: 4px 12px 12px; display: flex; flex-direction: column; gap: 10px; }
  .txt { white-space: pre-wrap; word-break: break-word; line-height: 1.6; }
  .user { align-self: flex-end; max-width: 92%; background: var(--elevated); border-radius: var(--r-md); padding: 8px 10px; display: flex; flex-direction: column; gap: 6px; align-items: flex-start; }
  .say { display: flex; flex-direction: column; gap: 8px; }
  .code { margin: 0; padding: 8px 10px; background: var(--content-solid); border-radius: var(--r-sm); font-family: var(--code-font); font-size: var(--fs-sm); line-height: 1.5; overflow-x: auto; white-space: pre; }
  .code.out { max-height: 240px; overflow: auto; margin-top: 4px; color: var(--text-muted); }
  .thought summary, .tool summary { cursor: pointer; color: var(--text-faint); font-size: var(--fs-sm); list-style: none; display: flex; align-items: center; gap: 6px; min-width: 0; }
  .thought .txt { color: var(--text-faint); font-size: var(--fs-sm); padding: 4px 0 0 10px; }
  .tool summary { color: var(--text-muted); }
  .st { width: 16px; height: 16px; flex: none; display: grid; place-items: center; color: var(--git-added); }
  .tool.bad .st { color: var(--lvl-error); }
  .tt { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .loc { border: none; background: none; padding: 0; color: var(--accent); font: inherit; font-family: var(--code-font); cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  .plan { display: flex; flex-direction: column; gap: 2px; font-size: var(--fs-sm); color: var(--text-muted); }
  .pe.done { color: var(--text-faint); text-decoration: line-through; }
  .pe.now { color: var(--text); }
  .perm { background: var(--elevated); border-radius: var(--r-md); box-shadow: 0 0 0 1px var(--accent-sel); padding: 10px; display: flex; flex-direction: column; gap: 8px; }
  .ptitle { font-family: var(--code-font); font-size: var(--fs-sm); word-break: break-all; }
  .opts { display: flex; gap: 6px; flex-wrap: wrap; }
  .chip { display: inline-flex; align-items: center; gap: 4px; font-family: var(--code-font); font-size: var(--fs-xs); background: var(--selected); border-radius: var(--r-xs); padding: 1px 6px; }
  .composer { flex: none; margin: 0 8px 8px; background: var(--field-bg); border: 1px solid var(--field-border); border-radius: var(--r-md); display: flex; flex-direction: column; }
  .composer:focus-within { box-shadow: var(--focus-glow); }
  .chips { display: flex; flex-wrap: wrap; gap: 4px; padding: 6px 8px 0; }
  textarea { resize: none; border: none; background: transparent; color: var(--text); font: inherit; padding: 8px 10px; outline: none; min-height: 60px; max-height: 200px; }
  .bar { display: flex; align-items: center; gap: 4px; padding: 0 6px 6px; }
  .edits { flex: 1; min-height: 0; overflow: auto; padding: 0 8px; display: flex; flex-direction: column; gap: 6px; }
  .edit { background: var(--elevated); border-radius: var(--r-md); overflow: hidden; }
  .erow { display: flex; align-items: center; gap: 8px; padding: 6px 8px; }
  .fn { border: none; background: none; color: var(--text); font-family: var(--code-font); font-size: var(--fs-sm); cursor: pointer; padding: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; min-width: 0; }
  .add { color: var(--diff-add-fg); font-size: var(--fs-sm); }
  .del { color: var(--diff-del-fg); font-size: var(--fs-sm); }
  .ok { color: var(--git-added); font-size: var(--fs-sm); }
  .diff { font-family: var(--code-font); font-size: var(--fs-xs); line-height: 1.6; border-top: 1px solid var(--border-soft); max-height: 360px; overflow: auto; white-space: pre; }
  .diff > div { padding: 0 8px; }
  .diff .a { background: var(--diff-add-bg); }
  .diff .d { background: var(--diff-del-bg); }
  .diff .h { color: var(--text-faint); }
  .foot { flex: none; display: flex; gap: 6px; justify-content: flex-end; padding: 8px 10px 10px; border-top: 1px solid var(--border-soft); }
</style>
