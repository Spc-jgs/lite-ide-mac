<script lang="ts">
  /**
   * 调试着起的那一格（docs/IDEA.md 第 18 节），照 IDEA 的 Debug 工具窗：
   *
   * ```
   * [▶ 继续][⏸][↷][↓][↑]  停在 Money.java:5（断点）              [调试器 | 控制台]
   * ┌ 帧 ──────────────────────┬ 变量 ───────────────────────────────
   * │ Money.show()  Money.java:5│ 求值：输入表达式 ↵
   * │ OrderApplication.run(…)   │ ▸ this = Money@21 "Money[cents=12444]"
   * │ SpringApplication…  (jar) │   total = 12444                long
   * ```
   *
   * 控制台就是原来那份输出（级别着色、过滤、跟随），两页都只藏不卸：切回来时输出滚到哪、变量展开到哪都还在。
   * 撞上断点自动切到「调试器」（同 IDEA）；单步之后同一个变量展开着的照样展开（按名字路径记）。
   */
  import type { Snippet } from "svelte";
  import Icon from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import type { RunTab } from "../state/runs.svelte";
  import type { DebugVar } from "../ipc/commands";
  import { debugEval, debugThread, debugVars } from "../ipc/debug";
  import { showFrame, step } from "../debug/debug-ops";
  import { cmenu } from "../shell/context-menu.svelte";

  let { run, console: consoleView }: { run: RunTab; console: Snippet } = $props();

  type Node = { key: string; v: DebugVar; open: boolean; kids: Node[] | null; more: number; err: string };

  let page = $state<"debugger" | "console">("console");
  let sel = $state(0);
  let vars = $state<Node[] | null>(null);
  let varsMore = $state(0);
  let varsErr = $state("");
  let expr = $state("");
  let evalNode = $state<Node | null>(null);
  let evalErr = $state("");
  let evaluating = $state(false);
  /** 展开着的变量（按名字路径）：单步之后照着重新展开 */
  const opened = new Set<string>();

  let d = $derived(run.debug);
  let paused = $derived(d?.phase === "paused");
  let frame = $derived(paused ? (d?.frames[sel] ?? null) : null);
  let where = $derived.by(() => {
    if (!d) return "";
    if (d.phase === "waiting") return d.note || "正在挂上调试器…";
    if (d.phase === "running") return "在跑";
    if (d.phase === "ended") return d.note ? `调试结束了：${d.note}` : "调试结束了";
    const f = d.frames[0];
    const file = f ? (f.path ? f.path.slice(f.path.lastIndexOf("/") + 1).replace(/\?.*$/, "").replace(/\.class$/, ".java") : f.name) : "";
    const why = d.reason === "breakpoint" ? "断点" : d.reason === "step" ? "单步" : d.reason === "exception" ? "异常" : d.reason === "pause" ? "暂停" : d.reason;
    const th = d.threads.length > 1 ? ` · ${d.threads.find((t) => t.id === d.thread)?.name ?? ""}` : "";
    return `停在 ${file}${f?.line ? `:${f.line}` : ""}（${why}）${th}`;
  });

  // 每停一次（帧那张表换了）：回到最顶上那一帧、切到调试器那一页
  let lastFrames: unknown = null;
  $effect(() => {
    const fs = d?.frames;
    if (fs === lastFrames) return;
    lastFrames = fs;
    if (d?.phase === "paused") {
      sel = 0;
      page = "debugger";
    }
  });

  // 选中的帧变了 → 拉它的局部变量。等回来时已经不是这一帧了就丢掉（frontend.md「异步 effect 里 await 之后要检查」）
  $effect(() => {
    const f = frame;
    const id = run.id;
    if (!f) {
      vars = null;
      return;
    }
    let dead = false;
    varsErr = "";
    debugVars(id, { frame: f.id })
      .then(async (r) => {
        if (dead) return;
        vars = r.vars.map((v) => node(v, v.name));
        varsMore = r.more;
        // 展开的是 `vars` 里那份（代理），不是刚建的原始对象 —— 改原始对象界面不动（frontend.md「`$state` 数组里的元素」）
        await reopen(vars, 0, () => dead);
      })
      .catch((e) => {
        if (!dead) varsErr = e instanceof Error ? e.message : String(e);
      });
    return () => {
      dead = true;
    };
  });

  const node = (v: DebugVar, key: string): Node => ({ key, v, open: false, kids: null, more: 0, err: "" });

  async function load(n: Node) {
    try {
      const r = await debugVars(run.id, { reference: n.v.reference, indexed: n.v.indexed });
      n.kids = r.vars.map((v) => node(v, `${n.key}/${v.name}`));
      n.more = r.more;
    } catch (e) {
      n.err = e instanceof Error ? e.message : String(e);
    }
  }

  /** 单步之后：上次展开着的照样展开（最多往下四层：再深的展开要在目标 JVM 里一层层调 toString，单步一下等好几秒不值） */
  async function reopen(nodes: Node[], depth: number, dead: () => boolean) {
    if (depth >= 4) return;
    for (const n of nodes) {
      if (dead() || !opened.has(n.key) || n.v.reference <= 0) continue;
      await load(n);
      n.open = true;
      if (n.kids) await reopen(n.kids, depth + 1, dead);
    }
  }

  async function toggle(n: Node) {
    if (n.v.reference <= 0) return;
    if (n.open) {
      n.open = false;
      opened.delete(n.key);
      return;
    }
    opened.add(n.key);
    if (!n.kids) await load(n);
    n.open = true;
  }

  async function evaluate() {
    const e = expr.trim();
    if (!e || !frame) return;
    evaluating = true;
    evalErr = "";
    try {
      evalNode = node(await debugEval(run.id, frame.id, e), `= ${e}`);
    } catch (x) {
      evalNode = null;
      evalErr = x instanceof Error ? x.message : String(x);
    } finally {
      evaluating = false;
    }
  }

  function onEvalKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void evaluate();
    }
  }

  function pick(i: number) {
    sel = i;
    const f = d?.frames[i];
    if (f) showFrame(run, f, i === 0);
  }

  /**
   * 不止一个线程停着时（并发的请求各撞上一次断点）换着看。**不用 `<select>`**：它会把 Svelte 处理下拉框的那段运行时拽进入口包
   * （运行时是入口和懒块共用的，2026-10-20 量出 +236 B、越过告警线），用现成的右键菜单那套
   */
  let threadMenu = $state<{ x: number; y: number } | null>(null);
  function openThreads(e: MouseEvent) {
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    threadMenu = { x: r.left, y: r.bottom + 2 };
    cmenu.load();
  }

  async function pickThread(id: number) {
    try {
      run.debug = await debugThread(run.id, id);
    } catch {
      /* 线程刚好接着跑了：下一个状态事件会改过来 */
    }
  }

  const fileOf = (p: string) => (p.startsWith("jdt://") ? "（jar）" : p.slice(p.lastIndexOf("/") + 1));
</script>

{#snippet tree(list: Node[], depth: number)}
  {#each list as n (n.key)}
    <div
      class="vrow"
      role="treeitem"
      aria-selected="false"
      aria-expanded={n.v.reference > 0 ? n.open : undefined}
      tabindex="-1"
      style:padding-left="{6 + depth * 16}px"
      onclick={() => void toggle(n)}
      onkeydown={() => {}}
      title={n.v.ty ? `${n.v.name}：${n.v.ty}` : n.v.name}
    >
      {#if n.v.reference > 0}
        <span class="chev" class:open={n.open}><Icon name="chevron-right" /></span>
      {:else}
        <span class="chev-gap"></span>
      {/if}
      <span class="vn">{n.v.name}</span>
      <span class="eq">=</span>
      <span class="vv" class:str={n.v.value.startsWith('"')}>{n.v.value}</span>
      {#if n.v.ty && n.v.reference <= 0}<span class="vt">{n.v.ty}</span>{/if}
    </div>
    {#if n.open}
      {#if n.err}<div class="vmsg bad" style:padding-left="{28 + depth * 16}px">{n.err}</div>{/if}
      {#if n.kids}
        {@render tree(n.kids, depth + 1)}
        {#if n.kids.length === 0}<div class="vmsg" style:padding-left="{28 + depth * 16}px">（空）</div>{/if}
        {#if n.more > 0}<div class="vmsg" style:padding-left="{28 + depth * 16}px">还有 {n.more} 个没列出来</div>{/if}
      {/if}
    {/if}
  {/each}
{/snippet}

{#if threadMenu && cmenu.comp && d}
  <cmenu.comp
    x={threadMenu.x}
    y={threadMenu.y}
    title="停着的线程"
    label="线程"
    items={d.threads.map((t) => ({ label: t.id === d.thread ? `${t.name}（正在看）` : t.name, run: () => void pickThread(t.id) }))}
    onclose={() => (threadMenu = null)}
  />
{/if}

<div class="dbg">
  <div class="bar">
    <button class="ibtn" disabled={!paused} onclick={() => void step("continue", run.id)} title="继续（F9）" aria-label="继续"><Icon name="resume" /></button>
    <button class="ibtn" disabled={d?.phase !== "running"} onclick={() => void step("pause", run.id)} title="暂停" aria-label="暂停"><Icon name="pause" /></button>
    <button class="ibtn" disabled={!paused} onclick={() => void step("next", run.id)} title="单步跳过（F8）" aria-label="单步跳过"><Icon name="step-over" /></button>
    <button class="ibtn" disabled={!paused} onclick={() => void step("stepIn", run.id)} title="单步进入（F7）" aria-label="单步进入"><Icon name="step-into" /></button>
    <button class="ibtn" disabled={!paused} onclick={() => void step("stepOut", run.id)} title="单步跳出（⇧F8）" aria-label="单步跳出"><Icon name="step-out" /></button>
    <span class="where" class:paused class:ended={d?.phase === "ended" && !!d.note}>
      {#if d?.phase === "waiting"}<span class="wait-ring"><span class="spinner sm"></span></span>{/if}
      {where}
    </span>
    {#if d && d.threads.length > 1}
      <button class="btn sm quiet threads" onclick={openThreads} title="停着的线程（{d.threads.length} 个）">
        {d.threads.find((t) => t.id === d.thread)?.name ?? "线程"} <Icon name="chevron-down" />
      </button>
    {/if}
    <span class="gap"></span>
    <div class="segs" role="tablist">
      <button class="btn sm" role="tab" aria-selected={page === "debugger"} class:on={page === "debugger"} onclick={() => (page = "debugger")}>调试器</button>
      <button class="btn sm" role="tab" aria-selected={page === "console"} class:on={page === "console"} onclick={() => (page = "console")}>控制台</button>
    </div>
  </div>
  <div class="pages">
    <div class="page split" class:hidden={page !== "debugger"}>
      <div class="frames" role="listbox" aria-label="调用栈">
        {#if !paused}
          <div class="empty-msg">
            {d?.phase === "running" ? "程序在跑" : d?.phase === "ended" ? "调试结束了" : "还没挂上"}
            <div class="sub">{d?.phase === "running" ? "撞上断点时停在这里；也可以点 ⏸ 暂停" : d?.phase === "ended" ? "重跑：Services 里的虫子，或者 ⌃D" : "挂上之后断点才生效"}</div>
          </div>
        {:else}
          {#each d?.frames ?? [] as f, i (f.id)}
            <div class="frow" class:on={i === sel} class:dim={!f.path} role="option" aria-selected={i === sel} tabindex="-1" onclick={() => pick(i)} onkeydown={() => {}} title={f.path ? `${f.name}\n${f.label || f.path}:${f.line}` : `${f.name}（没有源码）`}>
              <span class="fn">{f.name}</span>
              {#if f.path}<span class="ff">{fileOf(f.path)}{f.line > 0 ? `:${f.line}` : ""}</span>{/if}
            </div>
          {/each}
          {#if d && d.totalFrames > d.frames.length}<div class="vmsg">还有 {d.totalFrames - d.frames.length} 帧没列出来</div>{/if}
        {/if}
      </div>
      <div class="vars" role="tree" aria-label="变量">
        {#if paused}
          <div class="eval">
            <input class="expr" placeholder="求值：输入表达式，↵" spellcheck="false" bind:value={expr} onkeydown={onEvalKey} />
            {#if evaluating}<span class="wait-ring"><span class="spinner sm"></span></span>{/if}
          </div>
          {#if evalErr}<div class="vmsg bad">{evalErr}</div>{/if}
          {#if evalNode}{@render tree([evalNode], 0)}{/if}
          {#if varsErr}
            <div class="vmsg bad">{varsErr}</div>
          {:else if vars}
            {@render tree(vars, 0)}
            {#if vars.length === 0}<div class="vmsg">这一帧没有局部变量</div>{/if}
            {#if varsMore > 0}<div class="vmsg">还有 {varsMore} 个没列出来</div>{/if}
          {:else}
            <div class="vmsg wait"><span class="spinner sm"></span> 正在读变量…</div>
          {/if}
        {/if}
      </div>
    </div>
    <div class="page" class:hidden={page !== "console"}>{@render consoleView()}</div>
  </div>
</div>

<style>
  .dbg { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  /* 岛内的工具栏：34 高（ui.md 一之二） */
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 2px; padding: 0 6px; border-bottom: 1px solid var(--border-soft); }
  .bar .gap { flex: 1; }
  .where { display: inline-flex; align-items: center; gap: 6px; min-width: 0; margin-left: 8px; color: var(--text-dim); font-size: var(--fs-sm); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .where.paused { color: var(--text); }
  .where.ended { color: var(--lvl-error); }
  /* 环 150ms 之内不出现：同 .spinner 的延迟（ui.md 第十四条） */
  .wait-ring { display: inline-flex; }
  .threads { margin-left: 8px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; }
  .pages { flex: 1; min-height: 0; position: relative; }
  .page { position: absolute; inset: 0; }
  .page.hidden { visibility: hidden; pointer-events: none; }
  .split { display: flex; }
  /* 帧在左、变量在右（同 IDEA 的 Threads & Variables）。按比例不定宽：Services 里右边只剩一半宽，定 300 的话方法名全被截断（浏览器里看出来的） */
  .frames { flex: none; width: 40%; min-width: 180px; max-width: 360px; overflow: auto; padding: 4px 6px 8px; border-right: 1px solid var(--border-soft); }
  .vars { flex: 1; min-width: 0; overflow: auto; padding: 4px 6px 8px; }
  .frow, .vrow { display: flex; align-items: center; gap: 4px; height: 24px; padding: 0 8px 0 6px; border-radius: var(--r-sm); color: var(--text-dim); font-size: var(--fs-md); white-space: nowrap; cursor: default; }
  .frow:hover, .vrow:hover { background: var(--hover); color: var(--text); }
  .frow.on { background: var(--selected); color: var(--text); }
  .frow.dim { color: var(--text-faint); }
  /* 窄的时候文件名先让（收缩系数 100），方法名最后才截 —— 方法名是认帧用的，文件名悬停里有 */
  .fn { flex: 0 1 auto; min-width: 40px; overflow: hidden; text-overflow: ellipsis; }
  .ff { flex: 0 100 auto; min-width: 0; margin-left: auto; padding-left: 8px; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .chev-gap { flex: none; width: 16px; }
  /* 值是程序里的东西：等宽（ui.md 八之二「代码类内容」） */
  .vn { flex: none; color: var(--text); font-family: var(--code-font); font-size: var(--fs-sm); }
  .eq { flex: none; color: var(--text-faint); }
  .vv { flex: 0 1 auto; min-width: 0; overflow: hidden; text-overflow: ellipsis; font-family: var(--code-font); font-size: var(--fs-sm); }
  .vv.str { color: var(--git-added); }
  .vt { flex: none; margin-left: auto; padding-left: 8px; color: var(--text-faint); font-size: var(--fs-xs); }
  .eval { display: flex; align-items: center; gap: 6px; padding: 2px 2px 6px; }
  /* 输入框一个样子（ui.md 二之二）：底色、静止时不描边、聚焦时强调色边 + 光晕。可能输表达式，等宽 */
  .expr { flex: 1; min-width: 0; height: 26px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-family: var(--code-font); font-size: var(--fs-sm); }
  .expr:focus { outline: none; border-color: var(--accent); box-shadow: var(--focus-glow); }
  .vmsg { padding: 3px 8px; color: var(--text-faint); font-size: var(--fs-sm); }
  .vmsg.bad { color: var(--lvl-error); white-space: pre-wrap; }
  .vmsg.wait { display: flex; align-items: center; gap: 6px; }
</style>
