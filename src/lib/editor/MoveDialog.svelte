<script lang="ts">
  /**
   * 移动类（F6，docs/TAKEOVER.md G12，IDEA 的 Refactor › Move）：把这个类整个挪进另一个包。
   *
   * 两步，照 IDEA 的「Move + Preview」：先挑包（jdtls 列的现有包，能筛，现在在的那个标出来、不能选），
   * 再看一眼要改哪些（这个文件挪到哪儿、哪几个文件的 import / 包声明要改），点「移动」才落盘。
   * 落盘和改名是同一套（两段提交、能撤销、开着的标签跟上）；「移动」时 Rust 再问一次 jdtls，和这里列的对不上就不做（预览之后有人改了文件）。
   *
   * 只列已有的包：jdtls 的移动要一个已经存在的包当目标（新建包要先建目录、再等 jdtls 认出来，这一版不做）
   */
  import type { JavaMoveDest, JavaMovePreview } from "../ipc/commands";
  import { javaMoveDestinations, javaMovePreview } from "../ipc/java";
  import { imeKey } from "../shell/ime";

  let {
    root,
    path,
    text,
    onmove,
    onclose,
  }: { root: string; path: string; text: () => string; onmove: (d: JavaMoveDest, p: JavaMovePreview) => void; onclose: () => void } = $props();

  const file = $derived(path.slice(path.lastIndexOf("/") + 1));
  let dests = $state<JavaMoveDest[] | null>(null);
  let err = $state("");
  let q = $state("");
  let sel = $state(0);
  let pick = $state<JavaMoveDest | null>(null);
  let preview = $state<JavaMovePreview | null>(null);
  let busy = $state(false);

  // 对话框只为一个文件开一次（props 不变）：开的时候问一次
  $effect(() => {
    void javaMoveDestinations(root, path)
      .then((d) => (dests = d))
      .catch((e) => (err = e instanceof Error ? e.message : String(e)));
  });

  const list = $derived.by(() => {
    const words = q.toLowerCase().split(/\s+/).filter(Boolean);
    return (dests ?? []).filter((d) => !d.here && words.every((w) => d.name.toLowerCase().includes(w) || d.project.toLowerCase().includes(w)));
  });
  const here = $derived(dests?.find((d) => d.here) ?? null);
  /** 不止一个模块时才印模块名 */
  const multi = $derived(new Set((dests ?? []).map((d) => d.project)).size > 1);
  $effect(() => {
    if (sel >= list.length) sel = Math.max(0, list.length - 1);
  });

  async function choose(d: JavaMoveDest) {
    pick = d;
    preview = null;
    err = "";
    busy = true;
    try {
      preview = await javaMovePreview(root, path, text(), d.uri);
    } catch (e) {
      err = e instanceof Error ? e.message : String(e);
    } finally {
      busy = false;
    }
  }

  function back() {
    pick = null;
    preview = null;
    err = "";
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Escape") {
      e.preventDefault();
      if (pick) back();
      else onclose();
    } else if (!pick && (e.key === "ArrowDown" || e.key === "ArrowUp")) {
      e.preventDefault();
      if (list.length) sel = (sel + (e.key === "ArrowDown" ? 1 : list.length - 1)) % list.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (!pick && list[sel]) void choose(list[sel]);
      else if (pick && preview && !busy) onmove(pick, preview);
    }
  }

  function focusIn(el: HTMLElement) {
    el.focus({ preventScroll: true });
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog wide move" role="dialog" aria-label="移动类" tabindex="-1" onkeydown={onKey}>
    {#if !pick}
      <p class="title">把 {file} 挪到哪个包{#if here}<span class="sub">现在在 {here.name}</span>{/if}</p>
      <input class="field" use:focusIn placeholder="筛选包名…" spellcheck="false" bind:value={q} aria-label="筛选包" />
      <div class="list" role="listbox" aria-label="目标包">
        {#if err}
          <div class="empty-msg bad">{err}</div>
        {:else if dests === null}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在问 Java 语言服务器有哪些包…</div>
        {:else if !list.length}
          <div class="empty-msg">{q ? `没有带「${q}」的包` : "这个项目里没有别的包可以挪"}<span class="sub">只能挪进已经有的包；新包先在文件树里建好目录</span></div>
        {:else}
          {#each list as d, i (d.uri)}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div class="row" class:sel={i === sel} role="option" aria-selected={i === sel} tabindex="-1" onmouseenter={() => (sel = i)} onclick={() => void choose(d)}>
              <span class="pkg">{d.name}</span>
              {#if multi}<span class="proj">{d.project}</span>{/if}
            </div>
          {/each}
        {/if}
      </div>
      <div class="actions">
        <span class="gap"></span>
        <button class="btn" data-dismiss onclick={onclose}>取消</button>
        <button class="btn primary" data-default disabled={!list[sel]} onclick={() => list[sel] && void choose(list[sel])}>下一步</button>
      </div>
    {:else}
      <p class="title">把 {file} 挪到 {pick.name}</p>
      <div class="list">
        {#if err}
          <div class="empty-msg bad">{err}</div>
        {:else if !preview}
          <div class="empty-msg wait"><span class="spinner sm"></span> 正在算要改哪些（第一次要搜全部索引，会慢一点）…</div>
        {:else}
          <p class="body">文件 <code>{preview.from}</code> → <code>{preview.to}</code></p>
          <p class="body">改 {preview.files.length} 个文件（包声明、引用它的 import）：</p>
          <ul class="files">
            {#each preview.files as f (f)}<li>{f}</li>{/each}
          </ul>
          <p class="note">做完能撤销（胶囊上的「撤销」，或者「编辑 › 撤销上次的替换 / 改名」）</p>
        {/if}
      </div>
      <div class="actions">
        <button class="btn" onclick={back}>返回</button>
        <span class="gap"></span>
        <button class="btn" data-dismiss onclick={onclose}>取消</button>
        <button class="btn primary" data-default disabled={!preview || busy} onclick={() => preview && onmove(pick!, preview)}>移动</button>
      </div>
    {/if}
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .move { width: min(560px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .sub { margin-left: 8px; font-size: var(--fs-sm); font-weight: normal; color: var(--text-faint); }
  .field { height: 28px; padding: 0 8px; border: 1px solid var(--field-border); border-radius: var(--r-sm); background: var(--field-bg); color: var(--text); font-size: var(--fs-md); outline: none; }
  .field:focus { border-color: var(--accent); box-shadow: var(--focus-glow); }
  .list { min-height: 120px; max-height: min(44vh, 360px); overflow: auto; margin: 0 -8px; padding: 0 8px; }
  .row { display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 8px; border-radius: var(--r-sm); cursor: default; }
  .row.sel { background: var(--selected); }
  .pkg { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text); }
  .proj { flex: none; font-size: var(--fs-sm); color: var(--text-faint); }
  .body { margin: 0 0 6px; }
  .body code { font-family: var(--code-font); font-size: var(--fs-sm); }
  .files { margin: 0 0 8px; padding-left: 18px; font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text-muted); }
  .note { margin: 0; font-size: var(--fs-sm); color: var(--text-faint); }
</style>
