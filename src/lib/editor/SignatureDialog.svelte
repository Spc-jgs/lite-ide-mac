<script lang="ts">
  /**
   * 改签名（⌘F6，docs/TAKEOVER.md G12，IDEA 的 Refactor › Change Signature）：光标在一个方法上，改名字、返回类型、可见性和参数
   * （加、删、挪次序、改名、改类型），调用处跟着改。
   *
   * 两步，同移动类（MoveDialog）：先在表里改、底下一行实时拼出新签名；「下一步」问 jdtls 要动哪些文件、列出来；点「改签名」才落盘。
   * 落盘和改名同一套（能撤销、开着的标签跟上）；落盘时 Rust 再问一次 jdtls，和这里列的对不上就不做。
   *
   * 新加的参数要填「调用处传什么」：jdtls 会把它写进每一处调用，不填调用处就编不过。抛的异常这一版不改（原样带回去）；
   * 不留委托方法（IDEA 的「Delegate via overloading method」），要的话以后加一个勾
   */
  import { untrack } from "svelte";
  import type { JavaMethodSig, JavaMovePreview } from "../ipc/commands";
  import { javaMethodSigPreview } from "../ipc/java";
  import { imeKey } from "../shell/ime";
  import Icon from "../shell/Icon.svelte";

  let {
    root,
    path,
    at,
    text,
    sig: orig,
    indent,
    onchange,
    onclose,
  }: {
    root: string;
    path: string;
    /** 光标那儿（从 0 数），jdtls 靠它认是哪个方法 */
    at: { line: number; character: number };
    text: () => string;
    sig: JavaMethodSig;
    indent: { tabSize: number; spaces: boolean };
    onchange: (sig: JavaMethodSig, p: JavaMovePreview) => void;
    onclose: () => void;
  } = $props();

  // 改的是一份拷贝（只在打开时拷一次）；原来那份留着比「改没改」
  let sig = $state<JavaMethodSig>(untrack(() => structuredClone($state.snapshot(orig))));
  let preview = $state<JavaMovePreview | null>(null);
  let stage = $state<"edit" | "preview">("edit");
  let err = $state("");
  let busy = $state(false);

  /** jdtls 的可见性写法（Eclipse 的 `JdtFlags`：包内是空串） */
  const VIS = [
    { v: "public", l: "public" },
    { v: "protected", l: "protected" },
    { v: "", l: "包内" },
    { v: "private", l: "private" },
  ];

  const IDENT = /^[A-Za-z_$][\w$]*$/;
  /** 哪里不对（一句话），没问题是空串：名字不是标识符、参数名重了、新参数没填调用处传什么 */
  const problem = $derived.by(() => {
    if (!IDENT.test(sig.name)) return "方法名不是合法的 Java 标识符";
    // 构造器没有返回类型（jdtls 给空串），那时不要求
    if (!sig.returnType.trim() && orig.returnType.trim()) return "返回类型不能为空（无返回值请填写 void）";
    const seen = new Set<string>();
    for (const [i, p] of sig.params.entries()) {
      if (!p.ty.trim()) return `第 ${i + 1} 个参数缺少类型`;
      if (!IDENT.test(p.name)) return `第 ${i + 1} 个参数的名称不是合法的标识符`;
      if (seen.has(p.name)) return `参数名 ${p.name} 重复`;
      seen.add(p.name);
      if (p.originalIndex < 0 && !p.defaultValue.trim()) return `请为新参数 ${p.name} 填写调用处的默认值`;
    }
    return "";
  });
  const same = $derived(JSON.stringify($state.snapshot(sig)) === JSON.stringify($state.snapshot(orig)));
  const line = $derived(`${sig.modifier ? `${sig.modifier} ` : ""}${sig.returnType ? `${sig.returnType} ` : ""}${sig.name}(${sig.params.map((p) => `${p.ty} ${p.name}`).join(", ")})`);

  function add() {
    let n = 1;
    while (sig.params.some((p) => p.name === `p${n}`)) n++;
    sig.params.push({ ty: "String", name: `p${n}`, defaultValue: "null", originalIndex: -1 });
  }
  function move(i: number, d: number) {
    const j = i + d;
    if (j < 0 || j >= sig.params.length) return;
    const ps = sig.params;
    [ps[i], ps[j]] = [ps[j], ps[i]];
  }

  async function next() {
    if (problem || same || busy) return;
    err = "";
    busy = true;
    stage = "preview";
    preview = null;
    try {
      preview = await javaMethodSigPreview(root, path, text(), at.line, at.character, $state.snapshot(sig), indent.tabSize, indent.spaces);
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
      if (stage === "preview") stage = "edit";
      else onclose();
    } else if (e.key === "Enter" && (e.metaKey || !(e.target instanceof HTMLButtonElement))) {
      e.preventDefault();
      if (stage === "edit") void next();
      else if (preview && !busy) onchange($state.snapshot(sig), preview);
    }
  }

  function focusIn(el: HTMLInputElement) {
    el.focus({ preventScroll: true });
    el.select();
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog wide csig" role="dialog" aria-label="更改签名" tabindex="-1" onkeydown={onKey}>
    {#if stage === "edit"}
      <p class="title">更改 {orig.name} 的签名</p>
      <div class="grid">
        <label for="csig-name">名称</label>
        <input id="csig-name" class="field mono" use:focusIn spellcheck="false" bind:value={sig.name} />
        {#if orig.returnType}
          <label for="csig-ret">返回类型</label>
          <input id="csig-ret" class="field mono" spellcheck="false" bind:value={sig.returnType} />
        {/if}
        <span class="lab">可见性</span>
        <div class="segs" role="radiogroup" aria-label="可见性">
          {#each VIS as v (v.v)}
            <button class="btn sm" class:on={sig.modifier === v.v} role="radio" aria-checked={sig.modifier === v.v} onclick={() => (sig.modifier = v.v)}>{v.l}</button>
          {/each}
        </div>
      </div>
      <div class="phead">
        <span>参数</span>
        <span class="gap"></span>
        <button class="btn sm" onclick={add}><Icon name="plus" /> 添加参数</button>
      </div>
      <div class="params">
        {#each sig.params as p, i (i)}
          <div class="prow">
            <input class="field mono ty" spellcheck="false" bind:value={p.ty} aria-label="第 {i + 1} 个参数的类型" />
            <input class="field mono nm" spellcheck="false" bind:value={p.name} aria-label="第 {i + 1} 个参数的名称" />
            {#if p.originalIndex < 0}
              <input class="field mono dv" spellcheck="false" bind:value={p.defaultValue} placeholder="调用处的默认值" aria-label="{p.name} 在调用处的默认值" title="新参数：每一处调用都将传入此值" />
            {:else}
              <span class="dv was">原第 {p.originalIndex + 1} 个</span>
            {/if}
            <button class="ibtn" disabled={i === 0} onclick={() => move(i, -1)} title="上移" aria-label="上移 {p.name}"><Icon name="chevron-up" /></button>
            <button class="ibtn" disabled={i === sig.params.length - 1} onclick={() => move(i, 1)} title="下移" aria-label="下移 {p.name}"><Icon name="chevron-down" /></button>
            <button class="ibtn" onclick={() => sig.params.splice(i, 1)} title="删除此参数（同时删除调用处对应的实参）" aria-label="删除 {p.name}"><Icon name="x" /></button>
          </div>
        {:else}
          <div class="empty-msg">没有参数<span class="sub">点击「添加参数」添加，新参数需填写调用处的默认值</span></div>
        {/each}
      </div>
      <p class="sig"><code>{line}</code></p>
      {#if problem}<p class="warn">{problem}</p>{/if}
      <div class="actions">
        <span class="gap"></span>
        <button class="btn" data-dismiss onclick={onclose}>取消</button>
        <button class="btn primary" data-default disabled={!!problem || same} title={same ? "尚未做任何更改" : ""} onclick={() => void next()}>下一步</button>
      </div>
    {:else}
      <p class="title">更改为 <code>{line}</code></p>
      <div class="list">
        {#if err}
          <div class="empty-msg bad">{err}</div>
        {:else if !preview}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在分析需要修改的位置（首次查找全部调用处可能较慢）…</div>
        {:else}
          <p class="body">将修改 {preview.files.length} 个文件（方法声明、所有调用处及覆盖它的方法）：</p>
          <ul class="files">
            {#each preview.files as f (f)}<li>{f}</li>{/each}
          </ul>
          <p class="note">完成后可撤销（提示中的「撤销」，或「编辑 › 撤销上次的替换 / 改名」）</p>
        {/if}
      </div>
      <div class="actions">
        <button class="btn" onclick={() => (stage = "edit")}>返回</button>
        <span class="gap"></span>
        <button class="btn" data-dismiss onclick={onclose}>取消</button>
        <button class="btn primary" data-default disabled={!preview || busy} onclick={() => preview && onchange($state.snapshot(sig), preview)}>更改签名</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .csig { width: min(640px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .title code, .sig code { font-family: var(--code-font); font-size: var(--fs-sm); }
  .grid { display: grid; grid-template-columns: auto 1fr; gap: 8px 12px; align-items: center; }
  .grid label, .lab { font-size: var(--fs-sm); color: var(--text-muted); }
  .grid .segs { justify-self: start; }
  .field { height: 28px; min-width: 0; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-md); outline: none; }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .mono { font-family: var(--code-font); font-size: var(--fs-sm); }
  .phead { display: flex; align-items: center; gap: 8px; font-size: var(--fs-sm); color: var(--text-muted); }
  .params { display: flex; flex-direction: column; gap: 6px; max-height: min(36vh, 300px); overflow: auto; }
  .prow { display: flex; align-items: center; gap: 6px; }
  .ty { flex: 3; }
  .nm { flex: 2; }
  .dv { flex: 2; min-width: 0; }
  .was { font-size: var(--fs-sm); color: var(--text-faint); padding-left: 8px; }
  .sig { margin: 0; padding: 8px 10px; background: var(--field-bg); border-radius: var(--r-sm); overflow-x: auto; white-space: nowrap; }
  .warn { margin: 0; font-size: var(--fs-sm); color: var(--lvl-warn); }
  .list { min-height: 120px; max-height: min(44vh, 360px); overflow: auto; }
  .body { margin: 0 0 6px; }
  .files { margin: 0 0 8px; padding-left: 18px; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-muted); }
  .note { margin: 0; font-size: var(--fs-sm); color: var(--text-faint); }
</style>
