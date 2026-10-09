<script lang="ts">
  import { untrack } from "svelte";
  import { readText, type Hit, type ScratchEntry } from "../ipc/commands";
  import { grepProject, grepScratches } from "../ipc/search";
  import { files } from "../state/files.svelte";
  import { project } from "../state/project.svelte";
  import { rank, segments, snippet } from "./fuzzy";
  import { parseGoto, type Goto } from "./goto";
  import type { Sym } from "../editor/outline";
  import { tabs } from "../state/tabs.svelte";
  import { docs } from "../state/docs.svelte";
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
    seed = "",
    actions,
    scratches = [],
    onOpenFile,
  }: {
    open: boolean;
    root: string | null;
    scope: Scope;
    /**
     * 草稿（issue #40）。它们不在项目里，`listProjectFiles` 列不到，
     * 但「⌘P 打个时间戳就翻到那条笔记」是翻草稿最快的一条路。显示时标「草稿」，
     * 不显示那串 `~/Library/…` 的目录。内容命中那一档（M10 ③）用它的 `firstLine`
     * 当右边那列 —— 路径对草稿没意义，标题才认得出是哪条。
     */
    scratches?: ScratchEntry[];
    /**
     * 打开时预填的关键词。「在项目里找这个名字」用它把光标下那个词带进来 ——
     * 省掉「选中、复制、⇧⌘F、粘贴」这四下。
     *
     * **不 bindable**：这是一次性的初值，人一打字它就该失效。做成双向的话，
     * 上一次的输入会顺着这条线回流到 App 上存起来，下次打开又被当成 seed 塞回来。
     */
    seed?: string;
    actions: Action[];
    /**
     * `preview`：按文件名找到的（⌘P 那种）是「我要这个文件」，保留；
     * 按内容命中的是「看看这一处」，开成预览（issue #33 ⑯）。VS Code 的默认
     * 也是这么分的（`enablePreviewFromQuickOpen` 关、搜索结果开）。
     */
    onOpenFile: (path: string, line: number | undefined, preview: boolean, col?: number) => void;
  } = $props();

  const SCOPES: { id: Scope; label: string }[] = [
    { id: "all", label: "全部" },
    { id: "file", label: "文件" },
    { id: "content", label: "内容" },
    { id: "action", label: "操作" },
  ];

  let query = $state("");
  let hits = $state<Hit[]>([]);
  /** 草稿里的内容命中（M10 ③）。和项目那趟并行，各自 60 条上限 */
  let shits = $state<Hit[]>([]);
  let searching = $state(false);
  let cursor = $state(0);
  let input: HTMLInputElement | undefined = $state();

  /*
   * ── Goto Anything（issue #43）──
   *
   * `文件@符号` / `文件:行` 只在「全部」「文件」两个范围里认：「内容」里 `@` 和 `:` 就是要搜的字，
   * 「操作」里没有文件。认出来之后，文件那一档按 `@` / `:` **前面那半截**排，内容和操作那两档不出 ——
   * 拿 `order@list` 整串去搜正文只会搜出一片空白，还白跑一趟子进程。
   */
  const PLAIN: Goto = { kind: "plain" };
  let goto = $derived(scope === "all" || scope === "file" ? parseGoto(query) : PLAIN);
  /** 拿去给文件排序的那段：普通搜索是整串，跳转语法是分隔符前面那半截 */
  let fileQ = $derived(goto.kind === "plain" ? query : goto.file);

  /*
   * 打 `@` / `:` 之前，光标停在哪个文件上，就跳进哪个文件 —— 不一定是排第一的那个（Sublime 也是这样）。
   * 在 keydown 里记：那时输入框里的值还没带上这个字符，正好就是文件那半截。
   * 文件那半截一改，这个记录就不算数了（`pinned.q` 对不上）
   */
  let pinned = $state<{ q: string; path: string } | null>(null);

  const abs = (p: string) => (p.startsWith("/") || !root ? p : `${root}/${p}`);

  /** `@` / `:` 指向的那个文件（绝对路径）。文件那半截是空的 = 当前标签 */
  let target = $derived.by((): string | null => {
    if (goto.kind === "plain") return null;
    if (goto.file === "") return tabs.active?.path ?? null;
    if (pinned && pinned.q.trim() === goto.file) return abs(pinned.path);
    const top = rank(files.list, goto.file, (f) => f, 1)[0];
    return top ? abs(top.item) : null;
  });

  // 那个文件的符号。开着的用编辑器里的文本（可能有没存的改动），没开的读盘
  let syms = $state<Sym[]>([]);
  let symsOf = $state<string | null>(null);
  let symsLoading = $state(false);
  $effect(() => {
    const p = goto.kind === "symbol" ? target : null;
    if (!open || !p) {
      symsOf = null;
      syms = [];
      return;
    }
    if (p === untrack(() => symsOf)) return;
    let dead = false;
    symsLoading = true;
    void (async () => {
      try {
        const tab = tabs.byPath(p);
        const text = tab && tab.mode === "edit" ? docs.liveText(tab) : (await readText(p)).content;
        const { fileSymbols } = await import("../editor/file-symbols");
        const s = await fileSymbols(p, text);
        if (dead) return;
        syms = s;
        symsOf = p;
      } catch {
        if (dead) return;
        syms = [];
        symsOf = p;
      } finally {
        if (!dead) symsLoading = false;
      }
    })();
    return () => {
      dead = true;
    };
  });

  // 打开时把上次的输入换成这次的（没有 seed 就是清空）。文件索引不在这儿拉 ——
  // 那是 `files` store 的事，App 里一条 effect 跟着 root / treeTick 刷，⌘Click 跳转共用同一份
  $effect(() => {
    if (!open) return;
    query = untrack(() => seed);
    cursor = 0;
    pinned = null;
    queueMicrotask(() => input?.focus());
  });

  // 内容搜索要跑子进程，必须 debounce，否则每敲一个字母扫一遍项目
  $effect(() => {
    const q = query;
    const sc = scope;
    const r = root;
    if (!open || q.length < 2 || (sc !== "all" && sc !== "content") || goto.kind !== "plain") {
      hits = [];
      shits = [];
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
      // 项目和草稿两趟并行。没开项目时只搜草稿 —— 「上次那个坑我记在哪了」不需要先开项目
      const proj = r ? grepProject(r, q, 60) : Promise.resolve([] as Hit[]);
      const scr = scratches.length ? grepScratches(q, 60) : Promise.resolve([] as Hit[]);
      Promise.all([proj.catch(() => [] as Hit[]), scr.catch(() => [] as Hit[])])
        .then(([h, sh]) => {
          if (dead) return;
          hits = h;
          shits = sh;
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
    /** `line` / `col`：`文件:行` 时选它就跳到那儿 */
    | { kind: "file"; path: string; seg: { t: string; hit: boolean }[]; scratch?: true; line?: number; col?: number }
    /** `文件@符号`：`path` 是绝对路径 */
    | { kind: "symbol"; path: string; sym: Sym; seg: { t: string; hit: boolean }[] }
    /** 单独的 `:行`：当前文件 */
    | { kind: "line"; path: string; line: number; col?: number }
    /** ⌘E / ⌘P 空着的时候列的：最近打开的文件（绝对路径） */
    | { kind: "recent"; path: string }
    | { kind: "content"; path: string; line: number; text: string }
    /** 草稿里的内容命中：单独一组「草稿」，右边那列是那条草稿的标题 */
    | { kind: "scratch"; path: string; line: number; text: string; title: string }
    | { kind: "action"; action: Action; seg: { t: string; hit: boolean }[] };

  let rows = $derived.by(() => {
    const out: Row[] = [];
    if (goto.kind === "symbol") {
      if (!target || symsOf !== target) return out;
      const picked = goto.sym ? rank(syms, goto.sym, (s) => s.name, 60) : syms.slice(0, 200).map((s) => ({ item: s, positions: [] }));
      for (const r of picked) out.push({ kind: "symbol", path: target, sym: r.item, seg: segments(r.item.name, r.positions) });
      return out;
    }
    if (goto.kind === "line" && goto.file === "") {
      if (target && goto.line !== null) out.push({ kind: "line", path: target, line: goto.line, col: goto.col ?? undefined });
      return out;
    }
    /*
     * 什么都没输、范围是全部 / 文件：列最近打开的（IDEA 的 ⌘E）。这个工具的定位就是在三五个
     * 文件和一份日志之间来回，「上一个」比「找一个」常用得多。输入一个字它就让位给匹配。
     */
    if (query.length === 0 && (scope === "all" || scope === "file")) {
      for (const p of files.recent.slice(0, 10)) out.push({ kind: "recent", path: p });
      return out;
    }
    if ((scope === "all" && goto.kind === "plain") || scope === "action") {
      for (const r of rank(actions, query, (a) => a.label, scope === "action" ? 20 : 4)) {
        out.push({ kind: "action", action: r.item, seg: segments(r.item.label, r.positions) });
      }
    }
    if (scope === "all" || scope === "file") {
      // `文件:行` 时文件那一档放宽到 40 条（同「文件」范围）：这时候列表里只有它
      const line = goto.kind === "line" && goto.line !== null ? { line: goto.line, col: goto.col ?? undefined } : {};
      for (const r of rank(files.list, fileQ, (f) => f, scope === "file" || goto.kind !== "plain" ? 40 : 8)) {
        out.push({ kind: "file", path: r.item, seg: segments(r.item, r.positions), ...line });
      }
      // 草稿按文件名匹配（目录那串对所有草稿都一样，拿它排名只会全体并列）
      for (const r of rank(scratches, fileQ, (e) => e.name, scope === "file" ? 10 : 3)) {
        const off = r.item.path.lastIndexOf("/") + 1;
        out.push({
          kind: "file",
          path: r.item.path,
          scratch: true,
          seg: [{ t: r.item.path.slice(0, off), hit: false }, ...segments(r.item.name, r.positions)],
          ...line,
        });
      }
    }
    if ((scope === "all" && goto.kind === "plain") || scope === "content") {
      for (const h of hits.slice(0, scope === "content" ? 60 : 8)) {
        out.push({ kind: "content", path: h.path, line: h.line, text: h.text });
      }
      for (const h of shits.slice(0, scope === "content" ? 30 : 4)) {
        const e = scratches.find((s) => s.path === h.path);
        out.push({ kind: "scratch", path: h.path, line: h.line, text: h.text, title: e?.firstLine || fileName(h.path) });
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
    else if (row.kind === "file") onOpenFile(row.path, row.line, false, row.col);
    else if (row.kind === "recent") onOpenFile(row.path, undefined, false);
    else if (row.kind === "symbol") onOpenFile(row.path, row.sym.line, false);
    else if (row.kind === "line") onOpenFile(row.path, row.line, false, row.col);
    // 草稿命中也是「看看这一处」，但草稿标签不做预览（它是「我的东西」，被顶掉会莫名其妙）
    else onOpenFile(row.path, row.line, row.kind === "content");
  }

  function onKey(e: KeyboardEvent) {
    // 打 `@` / `:` 那一下记住光标停在哪个文件上（见 `pinned`）
    if ((e.key === "@" || e.key === ":") && goto.kind === "plain") {
      const r = rows[cursor];
      pinned = r && (r.kind === "file" || r.kind === "recent") ? { q: query, path: r.path } : null;
    }
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

  const KIND_LABEL = { action: "操作", file: "文件", content: "内容", scratch: "草稿", recent: "最近打开", symbol: "符号", line: "跳到行" } as const;

  /** 最近文件右边那列：项目里的显示相对目录，草稿标「草稿」，别处的显示整条目录 */
  const recentSide = (p: string) => {
    if (project.isScratch(p)) return "草稿";
    const dir = dirName(p);
    // 根上的文件右边留空，和上面「文件」那组的 `dirName` 一致
    if (root && p.startsWith(`${root}/`)) return dir.slice(root.length + 1);
    return dir;
  };

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
  <div class="scrim dim" onclick={() => (open = false)} role="presentation"></div>
  <div class="popup" role="dialog" aria-modal="true" aria-label="随处搜索">
    <!--
      输入排第一。面板打开之后的下一个动作永远是打字，
      而范围十次里有九次不用改 —— 让它占第一行是把最常用的挤到了第二位。
    -->
    <div class="q">
      <span class="qic"><Icon name="search" /></span>
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
          {#if goto.kind === "symbol" && !target}{#if goto.file}没有匹配的文件{:else}没有打开的文件 —— 在 <kbd>@</kbd> 前面写文件名{/if}
          {:else if goto.kind === "symbol" && (symsLoading || symsOf !== target)}<span class="wait"><span class="spinner"></span>解析中…</span>
          {:else if goto.kind === "symbol" && syms.length === 0}这个文件没有可列的符号 —— 只有带语法树的语言才有（Java、Python、TS…）
          {:else if goto.kind === "symbol"}没有叫这个的符号
          {:else if goto.kind === "line" && goto.file === "" && !target}没有打开的文件 —— 在 <kbd>:</kbd> 前面写文件名
          {:else if goto.kind === "line" && goto.file === ""}输入行号
          {:else if goto.kind === "line"}没有匹配的文件
          {:else if searching}<span class="wait"><span class="spinner"></span>搜索中…</span>
          {:else if query.length === 0}输入以开始{#if scope === "all" || scope === "file"} —— 打开过的文件会列在这儿{/if}
          {:else if (scope === "content" || scope === "all") && query.length < 2}内容搜索至少输入 2 个字符
          {:else if scope === "file"}没有匹配的文件 —— <kbd>Tab</kbd> 换到「内容」搜正文
          {:else if scope === "content"}正文里没有这个词 —— <kbd>Tab</kbd> 换到「文件」按名找
          {:else if scope === "action"}没有这个操作 —— <kbd>Tab</kbd> 换到「全部」
          {:else}没有匹配 —— 换个词，或 <kbd>Tab</kbd> 缩小范围{/if}
        </div>
      {/if}
      {#each rows as row, i (row.kind + (row.kind === "content" || row.kind === "scratch" || row.kind === "line" ? `${row.path}:${row.line}` : row.kind === "symbol" ? `${row.sym.line}:${row.sym.name}` : row.kind === "file" || row.kind === "recent" ? row.path : row.action.id))}
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
            <span class="ic act"><Icon name="chevron-right" /></span>
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
            <span class="ic"><FileGlyph name={fileName(row.path)} /></span>
            <span class="main">
              {#each tailSeg(row.seg, row.path.lastIndexOf("/") + 1) as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}{#if row.line !== undefined}<span class="at">:{row.line}{#if row.col !== undefined}:{row.col}{/if}</span>{/if}
            </span>
            <span class="side">{row.scratch ? "草稿" : dirName(row.path)}</span>
          {:else if row.kind === "symbol"}
            <span class="ic kind">{row.sym.kind}</span>
            <span class="main">
              {#each row.seg as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}
            </span>
            <span class="side plain">{fileName(row.path)}:{row.sym.line}</span>
          {:else if row.kind === "line"}
            <span class="ic"><FileGlyph name={fileName(row.path)} /></span>
            <span class="main">第 {row.line} 行{#if row.col !== undefined}，第 {row.col} 列{/if}</span>
            <span class="side plain">{fileName(row.path)}</span>
          {:else if row.kind === "recent"}
            <span class="ic"><FileGlyph name={fileName(row.path)} /></span>
            <span class="main">{fileName(row.path)}</span>
            <span class="side">{recentSide(row.path)}</span>
          {:else if row.kind === "scratch"}
            <span class="ic"><FileGlyph name={fileName(row.path)} /></span>
            <span class="main mono">{#each snippet(row.text.trim(), query) as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}</span>
            <!-- 右边是标题不是路径：`~/Library/…/2026-09-15 0930.md:31` 认不出是哪条 -->
            <span class="side plain">{row.title}</span>
          {:else}
            <span class="ic"><FileGlyph name={fileName(row.path)} /></span>
            <!-- 内容行也高亮命中，并把命中截到看得见的位置 —— 之前只有文件名和操作有 <mark>，十几行结果得自己再找一遍 -->
            <span class="main mono">{#each snippet(row.text.trim(), query) as s}{#if s.hit}<mark>{s.t}</mark>{:else}{s.t}{/if}{/each}</span>
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
      {#if scope === "all" || scope === "file"}
        <!-- 一个框走到底（issue #43）：不写出来没人知道能这么用 -->
        <span><kbd>@</kbd> 符号</span>
        <span><kbd>:</kbd> 行</span>
      {/if}
      <span class="gap"></span>
      {#if files.truncated && (scope === "all" || scope === "file")}
        <!-- 「没找到」和「索引没看到那儿」是两个答案，rust.md：truncated 必须一路传到界面 -->
        <span class="trunc">索引只看了前 {files.list.length.toLocaleString("en-US")} 个文件，后面的搜不到</span>
      {/if}
      <span><kbd>esc</kbd> 关闭</span>
    </div>
  </div>
{/if}

<style>
  .popup {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(680px, 88vw);
    max-height: 66vh;
    display: flex;
    flex-direction: column;
    /* 面（底、边、圆角、投影、淡入）在 app.css 的 `.popup`，这里只管位置和尺寸 */
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
    font-size: var(--fs-lg);
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
    font-size: var(--fs-md);
    cursor: default;
  }
  .tab:hover { background: var(--hover); }
  .tab.on { background: var(--selected); color: var(--text); }

  .results { overflow-y: auto; padding: 2px 0 4px; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: var(--fs-md); text-align: center; }
  .none .wait { display: inline-flex; align-items: center; gap: 8px; }

  /* 分组头。列表一长，它一滚就看不见了 —— 吸顶 */
  .sec {
    position: sticky;
    top: 0;
    z-index: 1;
    padding: 7px 14px 3px;
    background: var(--elevated);
    font-size: var(--fs-xs);
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
    font-size: var(--fs-md);
    color: var(--text-dim);
  }
  .row.sel { background: var(--selected); color: var(--text); }
  .ic { flex: none; display: flex; color: var(--text-faint); }
  .ic.act { color: var(--lvl-warn); }
  /* 符号行没有文件图标，图标那格放类别（类 / 方法 / 字段），和 ⇧⌘O 的大纲同一套字 */
  .ic.kind { width: 28px; justify-content: flex-end; font-size: var(--fs-xs); }
  /* `文件:行` 那行在文件名后面跟着 `:42`，淡一档 —— 它是「要去的地方」不是文件名的一部分 */
  .at { color: var(--text-faint); }
  .row.sel .ic :global(.glyph) { color: var(--text-dim); }

  .main {
    color: var(--text);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    flex: none;
    max-width: 60%;
  }
  .main.mono { font-family: var(--code-font); font-size: var(--fs-sm); }

  /*
   * `direction: rtl` 让长路径从**左边**省略 —— 路径有用的是尾巴。
   *
   * 这一招只对路径成立。快捷键曾经也用这个类，而 ⌘(U+2318) 在 bidi 里是
   * 中性字符：RTL 段落里它跟着段落方向走，于是 `⌘1` 显示成 `1⌘`。
   * 所以快捷键单独走 .key，绝不合并回来。
   */
  .side {
    color: var(--text-faint);
    font-size: var(--fs-sm);
    font-family: var(--ui-font);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    margin-left: auto;
    direction: rtl;
    max-width: 45%;
    text-align: right;
  }
  /* 草稿标题是句子不是路径：正向排、UI 字体（rtl 只给路径，ui.md 第九条） */
  .side.plain { direction: ltr; font-family: var(--ui-font); }
  .key {
    flex: none;
    margin-left: auto;
    font-family: var(--ui-font);
    font-size: var(--fs-xs);
    color: var(--text-faint);
    background: var(--hover);
    border-radius: var(--r-sm);
    padding: 1px 6px;
  }
  .row.sel .key { color: var(--text-dim); }

  .foot .trunc { color: var(--lvl-warn); }
  .foot {
    flex: none;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 7px 14px;
    border-top: 1px solid var(--border-soft);
    background: var(--chrome-scrim);
    font-size: var(--fs-xs);
    color: var(--text-faint);
    user-select: none;
  }
  .foot .gap { flex: 1; }
  kbd {
    font-family: var(--ui-font);
    font-size: var(--fs-xs);
    background: var(--hover);
    border-radius: var(--r-xs);
    padding: 1px 5px;
    margin-right: 3px;
  }

  mark { background: transparent; color: var(--accent); font-weight: 600; }
</style>
