<script lang="ts">
  import { tick, untrack } from "svelte";
  import { listDir, type DirEntry, type GitEntry, type GitStatus } from "../ipc/commands";

  let {
    root,
    activePath,
    gitStatus = null,
    reloadTick = 0,
    revealPath = "",
    revealTick = 0,
    onOpen,
  }: {
    root: string;
    activePath: string;
    /** 有仓库就给文件染色；没有就是 null，整块装饰不存在 */
    gitStatus?: GitStatus | null;
    /**
     * 自增即重新拉取目录内容。切分支、丢弃改动、在终端里 `mv` 之后都要刷 ——
     * 否则文件树一直显示的是打开那一刻的快照。
     */
    reloadTick?: number;
    /**
     * 「定位到这个路径」：展开沿途每一层，滚过去，闪一下。
     *
     * 和 reloadTick 一样用自增计数触发，而不是靠 revealPath 变化 ——
     * 连点同一个面包屑两次（中间可能手动把它收起来了）也得生效。
     */
    revealPath?: string;
    revealTick?: number;
    onOpen: (path: string, isDir: boolean) => void;
  } = $props();

  /**
   * 扁平化渲染：把展开的树拍平成一个带 depth 的列表，而不是递归组件。
   * 渲染就是一个 each，将来要给大仓库加虚拟滚动也直接可用。
   */
  interface Row {
    name: string;
    path: string;
    isDir: boolean;
    depth: number;
  }

  /** path → 子项。未加载过的目录不在表里，展开时才请求 */
  let children = $state(new Map<string, DirEntry[]>());
  let expanded = $state(new Set<string>());
  let loading = $state(new Set<string>());
  let error = $state("");

  /**
   * 拿到 dir 的子项，没加载过就去加载。
   *
   * 跟 `load` 分开是因为**定位需要回传内容** —— 逐层往下走时要判每一段
   * 到底是目录还是文件（是文件就不该展开，也走不下去了）。
   */
  async function ensure(dir: string): Promise<DirEntry[]> {
    const got = children.get(dir);
    if (got) return got;
    const items = await listDir(dir, false);
    children = new Map(children).set(dir, items);
    return items;
  }

  async function load(dir: string) {
    if (children.has(dir) || loading.has(dir)) return;
    loading = new Set(loading).add(dir);
    try {
      await ensure(dir);
    } catch (e) {
      error = String(e);
    } finally {
      const l = new Set(loading);
      l.delete(dir);
      loading = l;
    }
  }

  // 换项目根：清空缓存重新加载。
  //
  // 写操作必须包在 untrack 里：load() 开头会读 children 判重，
  // 而本 effect 又写 children —— 不隔离就是自己依赖自己，直接 update depth 爆栈。
  $effect(() => {
    const r = root;
    untrack(() => {
      children = new Map();
      expanded = new Set([r]);
      void load(r);
    });
  });

  /**
   * 重新拉取**已展开的目录**，但保住展开状态。
   *
   * 不走「清空 children 重来」那条路：那样会把整棵树收回根节点，
   * 而刷新最常发生在切完分支之后 —— 正是最不想丢失上下文的时候。
   * 把所有节点收起来是最烦人的刷新方式。
   */
  async function reload() {
    const dirs = [...expanded];
    const next = new Map(children);
    const gone: string[] = [];
    await Promise.all(
      dirs.map(async (d) => {
        try {
          next.set(d, await listDir(d, false));
        } catch {
          // 目录没了（切分支切掉了）：从缓存和展开集里一并摘掉
          next.delete(d);
          gone.push(d);
        }
      }),
    );
    children = next;
    if (gone.length) {
      const e = new Set(expanded);
      for (const g of gone) e.delete(g);
      expanded = e;
    }
  }

  // 外部要求刷新。untrack 的理由同上一个 effect：reload() 读 children 也写 children
  $effect(() => {
    const t = reloadTick;
    if (t === 0) return;
    untrack(() => void reload());
  });

  function toggle(path: string) {
    const next = new Set(expanded);
    if (next.has(path)) {
      next.delete(path);
    } else {
      next.add(path);
      void load(path);
    }
    expanded = next;
  }

  /** 深度优先展开成扁平列表 */
  let rows = $derived.by(() => {
    const out: Row[] = [];
    const walk = (dir: string, depth: number) => {
      const items = children.get(dir);
      if (!items) return;
      for (const it of items) {
        out.push({ name: it.name, path: it.path, isDir: it.isDir, depth });
        if (it.isDir && expanded.has(it.path)) walk(it.path, depth + 1);
      }
    };
    walk(root, 0);
    return out;
  });

  const rootName = $derived(root.slice(root.lastIndexOf("/") + 1) || root);

  /**
   * git 状态 → 绝对路径查找表。三样东西一起算，因为都要遍历同一份 entries：
   *
   * - `own`  —— 文件/目录**自身**的状态
   * - `roll` —— 祖先目录的「里面有东西改了」冒泡标记。IDE 里最有用的那个提示：
   *   目录收着也知道里面有动静
   * - `utDirs` —— 被折叠的未跟踪目录前缀。git 把整个未跟踪目录报成一条 `dir/`，
   *   里面的文件根本不在 entries 里，只能靠前缀匹配补上
   */
  let git = $derived.by(() => {
    const own = new Map<string, string>();
    const roll = new Set<string>();
    const utDirs: string[] = [];
    const st = gitStatus;
    if (!st) return { own, roll, utDirs };

    for (const e of st.entries) {
      const rel = e.isDir ? e.path.slice(0, -1) : e.path;
      const abs = `${st.root}/${rel}`;
      own.set(abs, klass(e));
      if (e.isDir) utDirs.push(`${abs}/`);
      // 一路冒泡到仓库根为止
      let p = abs;
      for (;;) {
        const i = p.lastIndexOf("/");
        if (i < 0) break;
        p = p.slice(0, i);
        if (p.length <= st.root.length) break;
        roll.add(p);
      }
    }
    return { own, roll, utDirs };
  });

  function klass(e: GitEntry): string {
    if (e.conflicted) return "conflict";
    if (e.untracked) return "untracked";
    // 工作区的状态更贴近「我现在看到的这个文件怎么了」，优先它
    const c = e.work !== "." && e.work !== " " ? e.work : e.index;
    switch (c) {
      case "A": return "added";
      case "D": return "deleted";
      case "R":
      case "C": return "renamed";
      default: return "modified";
    }
  }

  /**
   * 文件类型字形。
   *
   * **单色描边，不是彩色图标包** —— 颜色这条通道已经被 git 状态占了
   * （改动蓝、新增绿、未跟踪灰绿…），再叠一层彩色图标两边都读不清。
   * 只有配置类给一点 warn 黄，因为改错它的代价最大。
   *
   * 五类就够：目录 / 代码 / 标记文档 / 配置 / 纯文本。分得再细是给自己找活儿。
   */
  type Glyph = "dir" | "code" | "doc" | "conf" | "text";

  const CONF_EXT = new Set([
    "json", "yaml", "yml", "toml", "ini", "conf", "cfg", "properties", "env",
    "lock", "plist", "xml",
  ]);
  const CONF_NAME = new Set([
    "dockerfile", "makefile", "gemfile", "rakefile", "procfile", "justfile",
    ".gitignore", ".gitattributes", ".editorconfig", ".npmrc", ".nvmrc",
  ]);
  const DOC_EXT = new Set(["md", "markdown", "rst", "adoc", "org"]);
  const TEXT_EXT = new Set(["txt", "log", "csv", "tsv", "out"]);

  function glyphOf(name: string, isDir: boolean): Glyph {
    if (isDir) return "dir";
    const lower = name.toLowerCase();
    if (CONF_NAME.has(lower)) return "conf";
    const dot = lower.lastIndexOf(".");
    // 没有扩展名的多半是脚本或 README 之类，按纯文本处理
    if (dot <= 0) return "text";
    const ext = lower.slice(dot + 1);
    if (CONF_EXT.has(ext)) return "conf";
    if (DOC_EXT.has(ext)) return "doc";
    if (TEXT_EXT.has(ext)) return "text";
    return "code";
  }

  const LETTER: Record<string, string> = {
    modified: "M",
    added: "A",
    deleted: "D",
    untracked: "?",
    renamed: "R",
    conflict: "!",
  };

  /** 一行显示什么装饰：自身状态优先，其次未跟踪目录前缀，最后才是冒泡点 */
  function deco(path: string): { cls: string; ch: string } | null {
    const own = git.own.get(path);
    if (own) return { cls: own, ch: LETTER[own] ?? "·" };
    for (const d of git.utDirs) {
      if (path.startsWith(d)) return { cls: "untracked", ch: "?" };
    }
    if (git.roll.has(path)) return { cls: "roll", ch: "" };
    return null;
  }

  function click(row: Row) {
    if (row.isDir) toggle(row.path);
    else onOpen(row.path, false);
  }

  // ─────────────────── 键盘导航 ───────────────────

  /**
   * 用**游标式 tabindex**：只有游标那一行是 tabindex=0，其余是 -1。
   *
   * 早先每一行都是普通 button，于是 Tab 键会一个一个走过几百个文件才能离开
   * 文件树 —— 这是 tree 控件的标准坑，正确做法就是让整棵树只占一个 Tab 停靠点，
   * 树内部用方向键走。
   */
  let cursor = $state(0);
  /*
   * 按序号从容器里取行，而不是 `bind:this={els[i]}` 存一个数组。
   *
   * 那种写法 Svelte 每渲染一行就警告一次「binding to a non-reactive property」，
   * 一次展开刷七条 —— 真正该看的警告全被埋在里面了。而且 `{#each}` 是按
   * row.path keyed 的，下标和数组位置对不上，收起目录之后数组里留着一串
   * 早已不存在的行。查一次 DOM 就没这两个问题，按方向键是人手速度，不值得优化。
   */
  let listEl = $state<HTMLElement | null>(null);
  const rowAt = (i: number) =>
    listEl?.querySelectorAll<HTMLElement>('[role="treeitem"]')[i] ?? null;

  // 行数变了（展开、收起、刷新）游标可能越界
  $effect(() => {
    const n = rows.length;
    if (cursor >= n) cursor = Math.max(0, n - 1);
  });

  function focusRow(i: number) {
    const n = rows.length;
    if (n === 0) return;
    cursor = Math.min(Math.max(0, i), n - 1);
    rowAt(cursor)?.focus();
  }

  /** 刚定位到的那一行，短暂高亮 */
  let flash = $state("");
  let flashTimer: ReturnType<typeof setTimeout> | null = null;
  /** 并发守卫：连点两个面包屑时，只让最后一次的结果落地 */
  let revealSeq = 0;

  /**
   * 展开到 path 并滚过去。
   *
   * 必须逐层 `await`：下一层的行是在上一层的子项加载回来之后才存在的，
   * 一次性把所有祖先塞进 `expanded` 没用 —— `rows` 的 walk 只递归
   * `children` 里有的目录，没加载的那层直接断在那里。
   */
  async function reveal(path: string) {
    const seq = ++revealSeq;
    const base = root.endsWith("/") ? root : `${root}/`;
    if (path !== root && !path.startsWith(base)) return;

    const next = new Set(expanded).add(root);
    let acc = root;
    for (const seg of path === root ? [] : path.slice(base.length).split("/")) {
      let items: DirEntry[];
      try {
        items = await ensure(acc);
      } catch (e) {
        error = String(e);
        return;
      }
      if (seq !== revealSeq) return; // 中途又点了别处
      const hit = items.find((it) => it.name === seg);
      if (!hit) return; // 路径断了（外部删掉/改名了），什么都不做
      acc = hit.path;
      // 目标自身是目录也展开 —— 点面包屑想看的正是「这个目录里有什么」
      if (hit.isDir) next.add(acc);
    }
    if (next.has(acc)) {
      try {
        await ensure(acc);
      } catch (e) {
        error = String(e);
      }
      if (seq !== revealSeq) return;
    }
    expanded = next;

    // 等这一轮渲染落地，那一行才在 DOM 里
    await tick();
    if (seq !== revealSeq) return;
    const i = rows.findIndex((r) => r.path === acc);
    if (i < 0) return;
    cursor = i;
    rowAt(i)?.scrollIntoView({ block: "nearest" });

    // 目标可能本来就在视野里，滚动等于没反应 —— 闪一下才知道点中了
    flash = acc;
    if (flashTimer) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => {
      flash = "";
      flashTimer = null;
    }, 900);
  }

  $effect(() => {
    const t = revealTick;
    const p = revealPath;
    if (t === 0 || !p) return;
    untrack(() => void reveal(p));
    return () => {
      /*
       * 组件被销毁时，正在 await 的那次 reveal 还会往下走：它会
       * 装一个 900ms 的定时器，而这次 cleanup 早就跑完了，没人再清它。
       * 递一下 seq，在飞的那次下一个检查点就自己退出 ——
       * cleanup 只能清掉它当时看得见的东西，await 之后的赋值它看不见。
       */
      revealSeq++;
      if (flashTimer) clearTimeout(flashTimer);
      flashTimer = null;
    };
  });

  function onRowKey(e: KeyboardEvent, i: number) {
    const row = rows[i];
    switch (e.key) {
      case "ArrowDown":
        e.preventDefault();
        focusRow(i + 1);
        break;
      case "ArrowUp":
        e.preventDefault();
        focusRow(i - 1);
        break;
      case "ArrowRight":
        e.preventDefault();
        // 目录收着就展开，已经展开就走进去第一个子项 —— 与 Finder / IDEA 一致
        if (row.isDir && !expanded.has(row.path)) toggle(row.path);
        else focusRow(i + 1);
        break;
      case "ArrowLeft":
        e.preventDefault();
        if (row.isDir && expanded.has(row.path)) {
          toggle(row.path);
        } else {
          // 回到父目录那一行：往上找第一个层级更浅的
          let j = i - 1;
          while (j >= 0 && rows[j].depth >= row.depth) j--;
          focusRow(j < 0 ? 0 : j);
        }
        break;
      case "Home":
        e.preventDefault();
        focusRow(0);
        break;
      case "End":
        e.preventDefault();
        focusRow(rows.length - 1);
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        click(row);
        break;
    }
  }
</script>

<div class="tree">
  <div class="head">
    <span class="proj" title={root}>{rootName}</span>
    <span class="gap"></span>
  </div>
  <div class="list" role="tree" aria-label="文件树" bind:this={listEl}>
    {#each rows as row, i (row.path)}
      {@const d = deco(row.path)}
      {@const gl = glyphOf(row.name, row.isDir)}
      <button
        class="row"
        class:dir={row.isDir}
        class:active={row.path === activePath}
        class:flash={row.path === flash}
        role="treeitem"
        tabindex={i === cursor ? 0 : -1}
        aria-level={row.depth + 1}
        aria-expanded={row.isDir ? expanded.has(row.path) : undefined}
        aria-selected={row.path === activePath}
        style:padding-left="{6 + row.depth * 13}px"
        onclick={() => {
          cursor = i;
          click(row);
        }}
        onfocus={() => (cursor = i)}
        onkeydown={(e) => onRowKey(e, i)}
        title={row.name}
      >
        {#if row.isDir}
          <span class="caret" class:open={expanded.has(row.path)}>▸</span>
        {:else}
          <span class="caret spacer"></span>
        {/if}
        <svg class="glyph {gl}" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
          {#if gl === "dir"}
            <path d="M1.8 12.5 V4.2 a1 1 0 0 1 1-1 h3.1 l1.4 1.6 h5.9 a1 1 0 0 1 1 1 v6.7 a1 1 0 0 1-1 1 H2.8 a1 1 0 0 1-1-1 z"
                  fill="none" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" />
          {:else if gl === "code"}
            <path d="M6 3.2 L3 8 L6 12.8" fill="none" stroke="currentColor" stroke-width="1.25"
                  stroke-linecap="round" stroke-linejoin="round" />
            <path d="M10 3.2 L13 8 L10 12.8" fill="none" stroke="currentColor" stroke-width="1.25"
                  stroke-linecap="round" stroke-linejoin="round" />
          {:else if gl === "doc"}
            <rect x="2.6" y="3.4" width="10.8" height="9.2" rx="1"
                  fill="none" stroke="currentColor" stroke-width="1.25" />
            <path d="M4.8 6.6 h3.2 M4.8 9.4 h6" stroke="currentColor" stroke-width="1.25"
                  stroke-linecap="round" />
          {:else if gl === "conf"}
            <circle cx="8" cy="8" r="2.1" fill="none" stroke="currentColor" stroke-width="1.25" />
            <path d="M8 1.9 v1.6 M8 12.5 v1.6 M1.9 8 h1.6 M12.5 8 h1.6 M3.7 3.7 l1.1 1.1 M11.2 11.2 l1.1 1.1 M12.3 3.7 l-1.1 1.1 M4.8 11.2 l-1.1 1.1"
                  stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
          {:else}
            <path d="M4.2 2.8 h5.2 l3 3 v7.6 a.9 .9 0 0 1-.9 .9 H4.2 a.9 .9 0 0 1-.9-.9 V3.7 a.9 .9 0 0 1 .9-.9 z"
                  fill="none" stroke="currentColor" stroke-width="1.25" stroke-linecap="round" />
            <path d="M9.2 2.9 v3.1 h3.1" fill="none" stroke="currentColor" stroke-width="1.25"
                  stroke-linecap="round" />
          {/if}
        </svg>
        <span class="name g-{d?.cls ?? 'none'}">{row.name}</span>
        {#if d}
          <span class="gap"></span>
          {#if d.ch}
            <span class="gmark g-{d.cls}">{d.ch}</span>
          {:else}
            <!-- 目录自身没改，但里面有东西改了：一个点，不喧宾夺主 -->
            <span class="gdot" aria-label="内含改动"></span>
          {/if}
        {/if}
      </button>
    {/each}
    {#if error}<div class="err">{error}</div>{/if}
  </div>
</div>

<style>
  .tree {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--panel-bg);
    border-right: 1px solid var(--border);
    overflow: hidden;
  }
  .head {
    flex: none;
    height: 30px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    border-bottom: 1px solid var(--border-soft);
    font-size: 11px;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .proj { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .head .gap { flex: 1; min-width: 6px; }
  .list { flex: 1; overflow-y: auto; padding: 4px 0; }
  .row {
    display: flex;
    align-items: center;
    gap: 3px;
    width: 100%;
    height: 22px;
    padding-right: 8px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .row:hover { background: var(--panel-bg-2); }
  .row.active { background: var(--accent-sel); color: var(--text); }
  .row.dir { color: var(--text); }
  .row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  /*
   * 定位命中：只描一圈边，不改底色 —— 底色是 git 装饰和 .active 在用的，
   * 抢过来会让「这行是当前文件」和「这行刚被定位到」混成一个样子。
   */
  .row.flash { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* 键盘走到的行给个底色，光有 outline 在长列表里不够醒目 */
  .row:focus-visible:not(.active) { background: var(--panel-bg-2); }
  .caret {
    flex: none;
    width: 11px;
    font-size: 9px;
    color: var(--text-faint);
    transition: transform 0.1s;
  }
  .caret.open { transform: rotate(90deg); }
  .caret.spacer { visibility: hidden; }
  .glyph { flex: none; color: var(--text-faint); }
  /* 只有配置类破例给个颜色 —— 改错它的代价最大 */
  .glyph.conf { color: var(--lvl-warn); opacity: 0.75; }
  .row.active .glyph, .row:hover .glyph { color: var(--text-dim); }
  .row.active .glyph.conf, .row:hover .glyph.conf { opacity: 1; }
  .name { overflow: hidden; text-overflow: ellipsis; }
  .row .gap { flex: 1; min-width: 4px; }

  /* git 装饰：文件名染色 + 右端一个状态字母。
     两样都给是有意的 —— 颜色扫得快，字母说得准（红绿色觉障碍也读得出） */
  .gmark {
    flex: none;
    font-family: var(--code-font);
    font-size: 10.5px;
    font-weight: 600;
    line-height: 1;
  }
  .gdot {
    flex: none;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--git-modified);
    opacity: 0.55;
  }
  .g-modified { color: var(--git-modified); }
  .g-added { color: var(--git-added); }
  .g-deleted { color: var(--git-deleted); }
  .g-untracked { color: var(--git-untracked); }
  .g-renamed { color: var(--git-renamed); }
  .g-conflict { color: var(--git-conflict); }
  /* 删除的文件划掉，但右端那个 D 字母不划 */
  .name.g-deleted { text-decoration: line-through; }
  .err {
    padding: 8px 10px;
    color: var(--lvl-error);
    font-size: 11.5px;
    font-family: var(--code-font);
  }
  @media (prefers-reduced-motion: reduce) { .caret { transition: none; } }
</style>
