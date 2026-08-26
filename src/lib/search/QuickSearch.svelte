<script lang="ts">
  import { listProjectFiles, grepProject, type Hit } from "../ipc/commands";
  import { rank, segments } from "./fuzzy";

  export interface Action {
    id: string;
    label: string;
    hint?: string;
    run: () => void;
  }

  type Scope = "all" | "file" | "content" | "action";

  let {
    open = $bindable(),
    root,
    scope = $bindable(),
    actions,
    onOpenFile,
  }: {
    open: boolean;
    root: string | null;
    scope: Scope;
    actions: Action[];
    onOpenFile: (path: string, line?: number) => void;
  } = $props();

  const SCOPES: { id: Scope; label: string }[] = [
    { id: "all", label: "全部" },
    { id: "file", label: "文件" },
    { id: "content", label: "内容" },
    { id: "action", label: "操作" },
  ];

  let query = $state("");
  let files = $state<string[]>([]);
  let hits = $state<Hit[]>([]);
  let searching = $state(false);
  let cursor = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let indexed = $state(false);

  // 打开时建一次文件索引，并把上次的输入清掉
  $effect(() => {
    if (!open) return;
    query = "";
    cursor = 0;
    queueMicrotask(() => input?.focus());
    if (indexed || !root) return;
    listProjectFiles(root)
      .then((f) => {
        files = f;
        indexed = true;
      })
      .catch(() => {});
  });

  // 换项目就重新索引
  $effect(() => {
    root;
    indexed = false;
    files = [];
  });

  // 内容搜索要跑子进程，必须 debounce，否则每敲一个字母扫一遍项目
  $effect(() => {
    const q = query;
    const sc = scope;
    const r = root;
    if (!open || !r || q.length < 2 || (sc !== "all" && sc !== "content")) {
      hits = [];
      return;
    }
    searching = true;
    const timer = setTimeout(() => {
      grepProject(r, q, 60)
        .then((h) => (hits = h))
        .catch(() => (hits = []))
        .finally(() => (searching = false));
    }, 220);
    return () => {
      clearTimeout(timer);
      searching = false;
    };
  });

  type Row =
    | { kind: "file"; path: string; seg: { t: string; hit: boolean }[] }
    | { kind: "content"; path: string; line: number; text: string }
    | { kind: "action"; action: Action; seg: { t: string; hit: boolean }[] };

  let rows = $derived.by(() => {
    const out: Row[] = [];
    if (scope === "all" || scope === "action") {
      for (const r of rank(actions, query, (a) => a.label, scope === "action" ? 20 : 4)) {
        out.push({ kind: "action", action: r.item, seg: segments(r.item.label, r.positions) });
      }
    }
    if (scope === "all" || scope === "file") {
      for (const r of rank(files, query, (f) => f, scope === "file" ? 40 : 8)) {
        out.push({ kind: "file", path: r.item, seg: segments(r.item, r.positions) });
      }
    }
    if (scope === "all" || scope === "content") {
      for (const h of hits.slice(0, scope === "content" ? 60 : 8)) {
        out.push({ kind: "content", path: h.path, line: h.line, text: h.text });
      }
    }
    return out;
  });

  // 结果变了就把选中项拉回可选范围
  $effect(() => {
    if (cursor >= rows.length) cursor = Math.max(0, rows.length - 1);
  });

  function choose(row: Row) {
    open = false;
    if (row.kind === "action") row.action.run();
    else if (row.kind === "file") onOpenFile(row.path);
    else onOpenFile(row.path, row.line);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      open = false;
    } else if (e.key === "ArrowDown") {
      e.preventDefault();
      cursor = rows.length ? (cursor + 1) % rows.length : 0;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      cursor = rows.length ? (cursor - 1 + rows.length) % rows.length : 0;
    } else if (e.key === "Enter") {
      e.preventDefault();
      const row = rows[cursor];
      if (row) choose(row);
    } else if (e.key === "Tab") {
      e.preventDefault();
      const i = SCOPES.findIndex((s) => s.id === scope);
      scope = SCOPES[(i + (e.shiftKey ? -1 + SCOPES.length : 1)) % SCOPES.length].id;
    }
  }

  const fileName = (p: string) => p.slice(p.lastIndexOf("/") + 1);
  const dirName = (p: string) => {
    const i = p.lastIndexOf("/");
    return i < 0 ? "" : p.slice(0, i);
  };
</script>

{#if open}
  <!-- 点遮罩关闭；键盘路径由输入框的 onKey 负责，这里不重复挂监听 -->
  <div class="scrim" onclick={() => (open = false)} role="presentation"></div>
  <div class="popup" role="dialog" aria-modal="true" aria-label="随处搜索">
    <div class="tabs">
      {#each SCOPES as s (s.id)}
        <button class="tab" class:on={scope === s.id} onclick={() => (scope = s.id)}>
          {s.label}
        </button>
      {/each}
      <span class="gap"></span>
      <span class="hint">Tab 切换范围 · ↑↓ 选择 · ↵ 打开 · Esc 关闭</span>
    </div>

    <input
      bind:this={input}
      bind:value={query}
      onkeydown={onKey}
      placeholder={scope === "content" ? "在项目中搜索内容…" : "输入文件名、内容或操作…"}
      spellcheck="false"
      autocomplete="off"
    />

    <div class="results">
      {#if rows.length === 0}
        <div class="none">
          {#if searching}搜索中…
          {:else if query.length === 0}输入以开始
          {:else if (scope === "content" || scope === "all") && query.length < 2}内容搜索至少输入 2 个字符
          {:else}没有匹配{/if}
        </div>
      {/if}
      {#each rows as row, i (row.kind + (row.kind === "content" ? `${row.path}:${row.line}` : row.kind === "file" ? row.path : row.action.id))}
        <button class="row" class:sel={i === cursor} onclick={() => choose(row)} onmouseenter={() => (cursor = i)}>
          <span class="kind {row.kind}">
            {row.kind === "file" ? "文件" : row.kind === "content" ? "内容" : "操作"}
          </span>
          {#if row.kind === "action"}
            <span class="main">
              {#each row.seg as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}
            </span>
            {#if row.action.hint}<span class="side">{row.action.hint}</span>{/if}
          {:else if row.kind === "file"}
            <span class="main">{fileName(row.path)}</span>
            <span class="side">{dirName(row.path)}</span>
          {:else}
            <span class="main mono">{row.text.trim()}</span>
            <span class="side">{row.path}:{row.line}</span>
          {/if}
        </button>
      {/each}
    </div>
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; background: rgba(0, 0, 0, 0.35); z-index: 40; }
  .popup {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(680px, 88vw);
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

  .tabs {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 6px 8px;
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
  }
  .tab {
    padding: 3px 10px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text-dim);
    font-size: 12px;
    cursor: default;
  }
  .tab:hover { background: var(--panel-bg-2); }
  .tab.on { background: var(--accent-sel); color: var(--text); }
  .tabs .gap { flex: 1; }
  .tabs .hint { font-size: 10.5px; color: var(--text-faint); font-family: var(--code-font); }

  input {
    border: none;
    border-bottom: 1px solid var(--border-soft);
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 15px;
    padding: 11px 14px;
    outline: none;
  }
  input::placeholder { color: var(--text-faint); }

  .results { overflow-y: auto; padding: 4px 0; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; }
  .row {
    display: flex;
    align-items: baseline;
    gap: 9px;
    width: 100%;
    padding: 5px 14px;
    background: transparent;
    border: none;
    text-align: left;
    cursor: default;
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .row.sel { background: var(--accent-sel); }
  .kind {
    flex: none;
    font-size: 9.5px;
    font-family: var(--code-font);
    padding: 1px 4px;
    border-radius: 2px;
    background: var(--panel-bg-2);
    color: var(--text-faint);
  }
  .kind.action { color: var(--lvl-warn); }
  .kind.content { color: var(--lvl-info); }
  .main {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: none;
    max-width: 60%;
  }
  .main.mono { font-family: var(--code-font); font-size: 11.5px; }
  .side {
    color: var(--text-faint);
    font-size: 11px;
    font-family: var(--code-font);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-left: auto;
    direction: rtl;
    text-align: right;
  }
  mark { background: transparent; color: var(--accent); font-weight: 600; }
</style>
