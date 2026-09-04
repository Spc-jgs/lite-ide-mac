<script lang="ts">
  import { listProjectFiles, grepProject, type Hit } from "../ipc/commands";
  import { rank, segments } from "./fuzzy";
  import Icon from "../shell/Icon.svelte";
  import FileGlyph from "../shell/FileGlyph.svelte";

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
    /*
     * `dead` 不能省 —— 和 LogPane 里那条是同一个形状。
     *
     * cleanup 只能清掉它**当时看得见**的东西：`clearTimeout` 拦得住还没发出去的，
     * 拦不住已经在飞的那一趟。于是打字快一点时，先发的慢请求后到，
     * 把新查询的结果**盖回成旧的** —— 屏幕上是一份和输入框对不上的列表，
     * 而人只会觉得"搜得不准"。
     *
     * 这里不做真正的取消（IPC 没有取消通道），但 Rust 侧现在命中够数就
     * 掐掉 rg 了，在飞的那趟本身也短了很多。
     */
    let dead = false;
    const timer = setTimeout(() => {
      grepProject(r, q, 60)
        .then((h) => {
          if (!dead) hits = h;
        })
        .catch(() => {
          if (!dead) hits = [];
        })
        .finally(() => {
          if (!dead) searching = false;
        });
    }, 220);
    return () => {
      dead = true;
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

  const KIND_LABEL = { action: "操作", file: "文件", content: "内容" } as const;

  const fileName = (p: string) => p.slice(p.lastIndexOf("/") + 1);

  /**
   * 把整条路径的高亮片段裁到只剩文件名那一段。
   *
   * `seg` 是照**整条路径**算的（`segments(r.item, r.positions)`），
   * 而列表里显示的是文件名 + 单独一列目录。原来这份数据直接被扔了 ——
   * 于是「操作」那一档标命中、「文件」这一档不标，同一个列表两种行为。
   */
  function tailSeg(seg: { t: string; hit: boolean }[], from: number) {
    const out: { t: string; hit: boolean }[] = [];
    let pos = 0;
    for (const s of seg) {
      const end = pos + s.t.length;
      if (end > from) out.push(pos >= from ? s : { t: s.t.slice(from - pos), hit: s.hit });
      pos = end;
    }
    return out;
  }
  const dirName = (p: string) => {
    const i = p.lastIndexOf("/");
    return i < 0 ? "" : p.slice(0, i);
  };
</script>

{#if open}
  <!-- 点遮罩关闭；键盘路径由输入框的 onKey 负责，这里不重复挂监听 -->
  <div class="scrim" onclick={() => (open = false)} role="presentation"></div>
  <div class="popup" role="dialog" aria-modal="true" aria-label="随处搜索">
    <!--
      输入排第一。面板打开之后的下一个动作永远是打字，
      而范围十次里有九次不用改 —— 让它占第一行是把最常用的挤到了第二位。
    -->
    <div class="q">
      <span class="qic"><Icon name="search" size={16} /></span>
      <input
        bind:this={input}
        bind:value={query}
        onkeydown={onKey}
        placeholder={scope === "content" ? "在项目中搜索内容…" : "输入文件名、内容或操作…"}
        spellcheck="false"
        autocomplete="off"
      />
    </div>

    <div class="scopes">
      {#each SCOPES as s (s.id)}
        <button class="tab" class:on={scope === s.id} onclick={() => (scope = s.id)}>
          {s.label}
        </button>
      {/each}
    </div>

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
        <!--
          分组头代替每行的类型胶囊。结果本来就是按类型排好的，
          每行再印一遍「文件」「操作」等于把分组信息摊到了每一行上 ——
          七行结果七个胶囊，横着扫一眼全是噪声。
        -->
        {#if i === 0 || rows[i - 1].kind !== row.kind}
          <div class="sec">{KIND_LABEL[row.kind]}</div>
        {/if}
        <button class="row" class:sel={i === cursor} onclick={() => choose(row)} onmouseenter={() => (cursor = i)}>
          {#if row.kind === "action"}
            <span class="ic act"><Icon name="chevron-right" size={13} /></span>
            <span class="main">
              {#each row.seg as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}
            </span>
            <!--
              快捷键**不能**用 .side。那个类上有 `direction: rtl`（给长路径做
              左省略用的），而 ⌘(U+2318) 在 bidi 里是中性字符 —— 在 RTL 段落里
              它跟着段落方向走，于是 `⌘1` 显示成 `1⌘`、`⌃⇧\`` 显示成 `\`⇧⌃`。
              源码里一个字都没错，是这行 CSS 干的。
              快捷键永远只有两三个字符，从来不需要省略，所以单独一个类。
            -->
            {#if row.action.hint}<span class="key">{row.action.hint}</span>{/if}
          {:else if row.kind === "file"}
            <span class="ic"><FileGlyph name={fileName(row.path)} size={14} /></span>
            <span class="main">
              {#each tailSeg(row.seg, row.path.lastIndexOf("/") + 1) as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}
            </span>
            <span class="side">{dirName(row.path)}</span>
          {:else}
            <span class="ic"><FileGlyph name={fileName(row.path)} size={14} /></span>
            <span class="main mono">{row.text.trim()}</span>
            <span class="side">{row.path}:{row.line}</span>
          {/if}
        </button>
      {/each}
    </div>

    <!-- 键位是「忘了才看」的东西，不该跟范围切换抢顶栏那一行 -->
    <div class="foot">
      <span><kbd>↑↓</kbd> 选择</span>
      <span><kbd>↵</kbd> 打开</span>
      <span><kbd>Tab</kbd> 换范围</span>
      <span class="gap"></span>
      <span><kbd>esc</kbd> 关闭</span>
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
    /* 浮层不透明 —— 桌面在 webview 之外，半透明只会让壁纸清晰地穿过来 */
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-lg);
    box-shadow: var(--shadow-pop);
    z-index: 41;
    overflow: hidden;
  }

  .q {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 16px;
  }
  .qic { flex: none; display: flex; color: var(--text-faint); }
  input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 15px;
    padding: 13px 0;
    outline: none;
  }
  input::placeholder { color: var(--text-faint); }

  .scopes {
    flex: none;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 14px 9px;
    border-bottom: 1px solid var(--border-soft);
    user-select: none;
  }
  .tab {
    padding: 3px 11px;
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-size: 12px;
    cursor: default;
  }
  .tab:hover { background: var(--hover); }
  .tab.on { background: var(--selected); color: var(--text); }

  .results { overflow-y: auto; padding: 2px 0 4px; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; }

  /* 分组头。列表一长，它一滚就看不见了 —— 吸顶 */
  .sec {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 7px 14px 3px;
    background: var(--elevated);
    font-size: 10.5px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-faint);
    user-select: none;
  }

  /*
   * 当前项是**内缩的圆角块**，不是通栏色条 —— 和文件树、大纲、分支面板同一套。
   * 同一个应用里「当前项」该长一个样。
   */
  .row {
    display: flex;
    align-items: center;
    gap: 9px;
    width: calc(100% - 12px);
    margin: 0 6px;
    padding: 5px 8px;
    background: transparent;
    border: none;
    border-radius: var(--r-md);
    text-align: left;
    cursor: default;
    font-size: 12.5px;
    color: var(--text-dim);
  }
  .row.sel { background: var(--selected); color: var(--text); }
  .ic { flex: none; display: flex; color: var(--text-faint); }
  .ic.act { color: var(--lvl-warn); }
  .row.sel .ic :global(.glyph) { color: var(--text-dim); }

  .main {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: none;
    max-width: 60%;
  }
  .main.mono { font-family: var(--code-font); font-size: 11.5px; }

  /*
   * `direction: rtl` 让长路径从**左边**省略 —— 路径有用的是尾巴。
   *
   * 这一招只对路径成立。快捷键曾经也用这个类，而 ⌘(U+2318) 在 bidi 里是
   * 中性字符：RTL 段落里它跟着段落方向走，于是 `⌘1` 显示成 `1⌘`。
   * 所以快捷键单独走 .key，绝不合并回来。
   */
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
  .key {
    flex: none;
    margin-left: auto;
    font-family: var(--code-font);
    font-size: 10.5px;
    color: var(--text-faint);
    background: var(--hover);
    border-radius: var(--r-sm);
    padding: 1px 6px;
  }
  .row.sel .key { color: var(--text-dim); }

  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 14px;
    border-top: 1px solid var(--border-soft);
    background: var(--chrome-scrim);
    font-size: 10.5px;
    color: var(--text-faint);
    user-select: none;
  }
  .foot .gap { flex: 1; }
  kbd {
    font-family: var(--code-font);
    font-size: 10px;
    background: var(--hover);
    border-radius: 4px;
    padding: 1px 5px;
    margin-right: 3px;
  }

  mark { background: transparent; color: var(--accent); font-weight: 600; }
</style>
