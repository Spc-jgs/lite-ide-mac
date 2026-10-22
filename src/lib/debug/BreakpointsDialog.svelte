<script lang="ts">
  /**
   * 断点（⇧⌘F8，照 IDEA 的 View Breakpoints；docs/TAKEOVER.md G14）：左边是「Java 异常断点」和这个项目全部的断点（按文件分组，勾 = 启用），
   * 右边是选中那一项的属性。**改了当场生效**（IDEA 也是），没有「应用」—— 断点是调试时边看边改的东西，攒着再点一下是多一步。
   *
   * 文字框在离开 / 回车时才交（不是每敲一个字交一次）：每交一次都要过 IPC、正在调试的话当场重下到 JVM 里，打到一半的条件下下去会报「编译不过」。
   * 输入框、复选框一律 `bind:` 的取值 / 设值写法，不写 `value={…}` / `checked={…}`：后者用到 Svelte 运行时里入口原来没有的那段，会落进入口包（frontend.md，设置对话框那次 +239 B）。
   * 键：↑↓ 选、空格 启用 / 停用、⌫ 删除、↵ 跳过去并关掉、Esc 关
   */
  // bpTitle 在这里而不在编辑器扩展里：对话框引编辑器扩展的话 CM6 整串跟着进这个块
  import { allOf, bpTitle, excOf, setBp, setBps, setExc, bpsOf, type Bp, type ExcBp } from "./breakpoints";
  import { untrack } from "svelte";
  import { imeKey } from "../shell/ime";
  import { nav } from "../state/nav.svelte";

  let { root, onclose }: { root: string; onclose: () => void } = $props();

  /** 改了就加一，让左边的列表重读（表在 breakpoints.ts 里，不是响应式的） */
  let tick = $state(0);
  const files = $derived.by(() => {
    void tick;
    return allOf(root);
  });
  type Item = { kind: "exc" } | { kind: "bp"; path: string; bp: Bp };
  const items = $derived<Item[]>([{ kind: "exc" }, ...files.flatMap(([path, bps]) => bps.map((bp) => ({ kind: "bp" as const, path, bp })))]);
  let cur = $state(0);
  const sel = $derived(items[Math.min(cur, items.length - 1)]);

  // 打开那一刻的（对话框开着时项目不会换）
  const init = untrack(() => excOf(root));
  let exc = $state<ExcBp>(init);
  let typesText = $state(init.types.join("\n"));

  const name = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const rel = (p: string) => (p.startsWith(root + "/") ? p.slice(root.length + 1) : p);
  const count = $derived(items.length - 1);

  function saveExc() {
    exc = { ...exc, types: typesText.split(/[\s,]+/).filter(Boolean) };
    setExc(root, exc);
  }

  function put(path: string, bp: Bp) {
    setBp(root, path, bp);
    tick++;
  }

  function remove(it: Item) {
    if (it.kind !== "bp") return;
    setBps(root, it.path, bpsOf(root, it.path).filter((b) => b.line !== it.bp.line));
    tick++;
    cur = Math.min(cur, items.length - 2);
  }

  function toggleOn(it: Item) {
    if (it.kind !== "bp") return;
    const { off: _o, ...rest } = it.bp;
    put(it.path, it.bp.off ? rest : { ...it.bp, off: true });
  }

  function go(it: Item) {
    if (it.kind !== "bp") return;
    onclose();
    void nav.openAt(it.path, it.bp.line, false);
  }

  /** 右边那几个字段改完（离开 / 回车）：拿当前那个断点改一样再交 */
  function edit(field: "cond" | "hit" | "log", v: string) {
    if (sel?.kind !== "bp") return;
    const b: Bp = { ...sel.bp };
    const t = v.trim();
    if (t) b[field] = t;
    else delete b[field];
    put(sel.path, b);
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    const inField = (e.target as HTMLElement).tagName === "INPUT" || (e.target as HTMLElement).tagName === "TEXTAREA";
    if (e.key === "Escape") {
      e.preventDefault();
      onclose();
      return;
    }
    if (inField) {
      if (e.key === "Enter" && (e.target as HTMLElement).tagName === "INPUT") (e.target as HTMLInputElement).blur();
      return;
    }
    if (e.key === "ArrowDown") cur = Math.min(items.length - 1, cur + 1);
    else if (e.key === "ArrowUp") cur = Math.max(0, cur - 1);
    else if (e.key === " " && sel) toggleOn(sel);
    else if ((e.key === "Backspace" || e.key === "Delete") && sel) remove(sel);
    else if (e.key === "Enter" && sel) go(sel);
    else return;
    e.preventDefault();
  }

  const autofocus = (n: HTMLElement) => {
    queueMicrotask(() => n.focus({ preventScroll: true }));
  };
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog bps" role="dialog" aria-label="断点" tabindex="-1" onkeydown={onKey}>
    <div class="main">
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <div class="list" role="listbox" aria-label="断点列表" tabindex="0" use:autofocus>
        <button class="it" class:on={cur === 0} role="option" aria-selected={cur === 0} onclick={() => (cur = 0)}>
          <input type="checkbox" bind:checked={() => exc.uncaught || exc.caught, () => {}} disabled tabindex="-1" />
          <span class="nm">Java 异常断点</span>
          <span class="sm">{exc.uncaught || exc.caught ? (exc.types.length ? exc.types.map((t) => t.slice(t.lastIndexOf(".") + 1)).join("、") : "全部异常") : "关"}</span>
        </button>
        {#each files as [path, bps] (path)}
          <div class="file" title={rel(path)}>{name(path)}</div>
          {#each bps as bp (bp.line)}
            {@const i = items.findIndex((x) => x.kind === "bp" && x.path === path && x.bp.line === bp.line)}
            <div class="it" class:on={cur === i} class:off={bp.off} role="option" aria-selected={cur === i} tabindex="-1" onclick={() => (cur = i)} ondblclick={() => go(items[i])} onkeydown={() => {}} title={bpTitle(bp)}>
              <input type="checkbox" bind:checked={() => !bp.off, () => toggleOn(items[i])} tabindex="-1" onclick={(e) => e.stopPropagation()} aria-label="启用第 {bp.line} 行的断点" />
              <span class="nm">第 {bp.line} 行</span>
              <span class="sm">{[bp.log ? "日志" : "", bp.cond ?? "", bp.hit ? `第 ${bp.hit} 次` : ""].filter(Boolean).join(" · ")}</span>
            </div>
          {/each}
        {/each}
        {#if count === 0}
          <div class="empty-msg">还没有断点 —— 在 Java 文件里点行号、或者 ⌘F8 下一个</div>
        {/if}
      </div>
      <section class="pane">
        {#if sel?.kind === "exc"}
          <h3>Java 异常断点</h3>
          <label class="check"><input type="checkbox" bind:checked={() => exc.uncaught, (c) => { exc = { ...exc, uncaught: c }; saveExc(); }} /> 抛出<b>没被接住</b>的异常时停</label>
          <label class="check"><input type="checkbox" bind:checked={() => exc.caught, (c) => { exc = { ...exc, caught: c }; saveExc(); }} /> 被 catch 接住的也停</label>
          <div class="lab">只看这几种（类的全名，一行一个；空 = 全部）</div>
          <textarea class="field area" rows="4" spellcheck="false" placeholder={"java.lang.IllegalStateException\ncom.demo.OrderException"} bind:value={typesText} onchange={saveExc}></textarea>
          <p class="doc">
            Spring 的 Web 服务里异常基本都被框架接住了，只开「没被接住」几乎不会停；「被接住的也停」不写类型的话，启动时框架自己抛了又接住的能停几百次 ——
            写上你关心的那几种。停下来时调试器那一栏会说出抛的是什么。改了当场生效，正在调试的也是。
          </p>
        {:else if sel?.kind === "bp"}
          {@const b = sel.bp}
          <h3>{name(sel.path)} 第 {b.line} 行</h3>
          <label class="check"><input type="checkbox" bind:checked={() => !b.off, () => toggleOn(sel)} /> 启用</label>
          <div class="lab">条件（Java 表达式，为真才停）</div>
          <input class="field code" bind:value={() => b.cond ?? "", () => {}} placeholder="orderId == 42" spellcheck="false" onchange={(e) => edit("cond", e.currentTarget.value)} />
          <div class="lab">第几次走到才停（空 = 每次）</div>
          <input class="field num" bind:value={() => b.hit ?? "", () => {}} placeholder="3" inputmode="numeric" onchange={(e) => edit("hit", e.currentTarget.value)} />
          <div class="lab">只打一句、不停（日志断点；空 = 普通断点）</div>
          <input class="field code" bind:value={() => b.log ?? "", () => {}} placeholder={"进来了 orderId={orderId}"} spellcheck="false" onchange={(e) => edit("log", e.currentTarget.value)} />
          <p class="doc">{"{表达式}"} 换成它此刻的值，打进运行窗的输出（行首标「[日志断点]」）。改了当场生效，正在调试的也是。</p>
          <div class="row-acts">
            <button class="btn sm" onclick={() => go(sel)}>跳过去</button>
            <button class="btn sm quiet" onclick={() => remove(sel)}>删除这个断点</button>
          </div>
        {/if}
      </section>
    </div>
    <div class="actions">
      <span class="keys">↑↓ 选 · 空格 启用 / 停用 · ⌫ 删除 · ↵ 跳过去</span>
      <span class="gap"></span>
      <button class="btn primary" data-default onclick={onclose}>完成</button>
    </div>
  </div>
</div>

<style>
  .layer {
    position: fixed;
    inset: 0;
    z-index: 40;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    padding-top: min(8vh, 64px);
    background: rgba(0, 0, 0, 0.38);
  }
  .bps { width: min(820px, calc(100% - 48px)); height: min(540px, calc(100vh - 96px)); padding: 0; gap: 0; overflow: hidden; }
  .main { flex: 1; min-height: 0; display: flex; }
  .list { flex: none; width: 300px; overflow: auto; padding: 8px 6px; border-right: 1px solid var(--border-soft); outline: none; }
  .file { padding: 10px 10px 2px; font-size: var(--fs-xs); color: var(--text-faint); overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .it {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    height: 28px;
    padding: 0 8px;
    border: none;
    border-radius: var(--r-sm);
    background: none;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    text-align: left;
    cursor: default;
  }
  .it:hover { background: var(--hover); }
  .it.on { background: var(--selected); }
  .it.off .nm { color: var(--text-faint); }
  .nm { flex: none; }
  .sm { min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; color: var(--text-faint); font-size: var(--fs-sm); font-family: var(--code-font); }
  .pane { flex: 1; min-width: 0; overflow: auto; padding: 16px 20px; }
  h3 { margin: 0 0 10px; font-size: var(--fs-md); font-weight: 600; color: var(--text); }
  .check { display: flex; align-items: center; gap: 6px; margin: 8px 0; font-size: var(--fs-md); color: var(--text-dim); }
  .lab { margin: 12px 0 6px; font-size: var(--fs-sm); color: var(--text-muted); }
  .field {
    width: 100%;
    max-width: 460px;
    height: 28px;
    padding: 0 8px;
    border: 1px solid var(--field-border);
    border-radius: var(--r-sm);
    background: var(--field-bg);
    color: var(--text);
    font: inherit;
    font-size: var(--fs-md);
    outline: none;
  }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .field.code, .field.area { font-family: var(--code-font); }
  .field.num { width: 100px; }
  .field.area { height: auto; padding: 6px 8px; resize: vertical; }
  .doc { margin: 10px 0 0; font-size: var(--fs-sm); line-height: 1.5; color: var(--text-muted); max-width: 520px; }
  .row-acts { display: flex; gap: 6px; margin-top: 16px; }
  .actions .keys { font-size: var(--fs-xs); color: var(--text-faint); }
</style>
