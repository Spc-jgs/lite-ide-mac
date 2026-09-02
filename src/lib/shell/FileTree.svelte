<script lang="ts">
  import { tick, untrack } from "svelte";
  import {
    createEntry,
    listDir,
    renameEntry,
    revealInFinder,
    trashEntry,
    type DirEntry,
    type GitEntry,
    type GitStatus,
  } from "../ipc/commands";
  import { notify } from "../state/notify.svelte";

  let {
    root,
    activePath,
    gitStatus = null,
    reloadTick = 0,
    revealPath = "",
    revealTick = 0,
    onOpen,
    dirtyUnder,
    onCreated,
    onRenamed,
    onTrashed,
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
    /**
     * 这条路径（含子树）下有几个**未保存**的标签。
     *
     * 文件树自己不知道有哪些标签开着，而「移到废纸篓」的确认框必须说清
     * 会连带丢掉多少未保存的改动 —— 一句「确定删除吗」在这种时候是不够的。
     */
    dirtyUnder?: (path: string) => number;
    /** 新建成功。App 负责决定要不要打开它，以及刷新 git 状态 */
    onCreated?: (path: string, isDir: boolean) => void;
    /** 改名成功。App 负责把打开着的标签的路径一起改掉 */
    onRenamed?: (from: string, to: string, isDir: boolean) => void;
    /** 进了废纸篓。App 负责关掉受影响的标签 */
    onTrashed?: (path: string, isDir: boolean) => void;
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
      // 键盘也能开菜单：⇧F10 是 Windows/Linux 的老约定，ContextMenu 是那个专用键。
      // 只有鼠标能开的菜单等于把功能藏起来了
      case "F10":
        if (!e.shiftKey) break;
        e.preventDefault();
        openMenuAtRow(i);
        break;
      case "ContextMenu":
        e.preventDefault();
        openMenuAtRow(i);
        break;
    }
  }

  // ─────────────────── 右键菜单 ───────────────────

  /**
   * 菜单两批：M22 立壳时只放了无破坏性的三项，这一轮（M24）补上会改盘的四项。
   *
   * **破坏性的那几项各自带一道闸**：新建撞名在 Rust 侧就失败（不覆盖），
   * 改名的目标已存在同样失败（`fs::rename` 自己是静默覆盖的），
   * 删除只进废纸篓 —— 应用里没有第二条删除路径。
   *
   * 项目根那一行（头部那个项目名）也能开菜单，但只有「新建」和只读那三项：
   * 改名或删掉工作区根，整棵树当场就散了。
   */
  let menu = $state<{ x: number; y: number; row: Row; fromHead?: boolean } | null>(null);
  let menuEl = $state<HTMLElement | null>(null);
  let headEl = $state<HTMLElement | null>(null);
  /** 菜单里的键盘游标 */
  let menuCursor = $state(0);

  const relOf = (p: string) => {
    const base = root.endsWith("/") ? root : `${root}/`;
    return p.startsWith(base) ? p.slice(base.length) : p;
  };

  const parentOf = (p: string) => p.slice(0, p.lastIndexOf("/")) || "/";

  interface MenuItem {
    label: string;
    run: () => void;
    /** 这一项上面画一条分隔线 */
    sep?: boolean;
    danger?: boolean;
  }

  /*
   * 分隔线做成条目自己的一个标记，而不是往数组里塞一个 `{ sep: true }` 占位 ——
   * 占位项会让键盘游标走到一条线上，于是 onMenuKey 里要额外写跳过逻辑，
   * 而那段逻辑一旦漏了边界，↑↓ 就会在菜单头尾卡住。
   */
  let items = $derived.by(() => {
    const row = menu?.row;
    if (!row) return [] as MenuItem[];
    // 在目录上右键 → 建在它里面；在文件上右键 → 建在它旁边（同 Finder / IDEA）
    const dir = row.isDir ? row.path : parentOf(row.path);
    const out: MenuItem[] = [
      { label: "新建文件…", run: () => openAsk("newFile", dir) },
      { label: "新建文件夹…", run: () => openAsk("newDir", dir) },
    ];
    if (row.path !== root) {
      out.push({ label: "重命名…", sep: true, run: () => openAsk("rename", parentOf(row.path), row) });
      out.push({ label: "移到废纸篓", danger: true, run: () => openTrashAsk(row) });
    }
    out.push({ label: "在 Finder 中显示", sep: true, run: () => void showInFinder(row.path) });
    out.push({ label: "复制路径", run: () => void copy(row.path, "路径") });
    out.push({ label: "复制相对路径", run: () => void copy(relOf(row.path), "相对路径") });
    return out;
  });

  // 名字别叫 reveal —— 上面那个 reveal() 是「在文件树里定位」（issue #4），
  // 两件事同名，读代码的人得每次回头确认调的是哪一个
  async function showInFinder(path: string) {
    try {
      await revealInFinder(path);
    } catch (e) {
      // 盘上没了是最常见的失败（切了分支、在终端里删了），一句话说得清 → fail
      notify.fail(String(e).replace(/^Error:\s*/, ""));
    }
  }

  async function copy(text: string, what: string) {
    try {
      await navigator.clipboard.writeText(text);
      notify.ok(`已复制${what}：${text}`);
    } catch {
      /*
       * 复制失败要说清是**剪贴板**不让写，不能只说"复制失败" ——
       * 后者会让人以为是路径有问题，去查文件树。
       * 顺带把内容放进消息里，至少能手动选中。
       */
      notify.fail(`剪贴板不可用，${what}是：${text}`);
    }
  }

  // ─────────────────── 会改盘的那几项 ───────────────────

  /**
   * 输入框：新建文件 / 新建文件夹 / 重命名共用一个。
   *
   * 错误显示在**框里**而不是走 notify：这时候人正站在输入框前，
   * 「已经有一个叫 x 的了」要就地看见并改掉，而不是被甩到状态栏那一格里，
   * 输入框还傻站着。
   */
  let ask = $state<{
    kind: "newFile" | "newDir" | "rename";
    x: number;
    y: number;
    /** 建在哪个目录 / 改的是哪个目录里的东西 */
    dir: string;
    /** 只有 rename 用：被改名的那一行 */
    row?: Row;
    name: string;
    error: string;
    busy: boolean;
  } | null>(null);
  let askEl = $state<HTMLElement | null>(null);
  let askInput = $state<HTMLInputElement | null>(null);

  /** 废纸篓确认 */
  let trash = $state<{ x: number; y: number; row: Row; dirty: number; busy: boolean } | null>(null);
  let trashEl = $state<HTMLElement | null>(null);

  const ASK_TITLE = {
    newFile: "新建文件",
    newDir: "新建文件夹",
    rename: "重命名",
  } as const;

  function openAsk(kind: "newFile" | "newDir" | "rename", dir: string, row?: Row) {
    const at = menu ?? { x: 0, y: 0 };
    ask = {
      kind,
      x: at.x,
      y: at.y,
      dir,
      row,
      name: kind === "rename" ? (row?.name ?? "") : "",
      error: "",
      busy: false,
    };
  }

  function openTrashAsk(row: Row) {
    const at = menu ?? { x: 0, y: 0 };
    trash = { x: at.x, y: at.y, row, dirty: dirtyUnder?.(row.path) ?? 0, busy: false };
  }

  /** 把 p 和它子树下的缓存与展开状态全忘掉 —— 改名之后这些 key 已经不存在了 */
  function forgetSubtree(p: string) {
    const c = new Map(children);
    const e = new Set(expanded);
    for (const k of [...c.keys()]) if (k === p || k.startsWith(`${p}/`)) c.delete(k);
    for (const k of [...e]) if (k === p || k.startsWith(`${p}/`)) e.delete(k);
    children = c;
    expanded = e;
  }

  /** 让某个目录下次被读到时重新拉 */
  function invalidate(dir: string) {
    const c = new Map(children);
    c.delete(dir);
    children = c;
  }

  const msgOf = (e: unknown) => String(e).replace(/^Error:\s*/, "");

  async function submitAsk() {
    const a = ask;
    if (!a || a.busy) return;
    const name = a.name;
    ask = { ...a, error: "", busy: true };
    try {
      if (a.kind === "rename") {
        const from = a.row!.path;
        const to = await renameEntry(from, name);
        if (to !== from) {
          // 子树的 key 全变了，父目录也要重列
          forgetSubtree(from);
          invalidate(a.dir);
          onRenamed?.(from, to, a.row!.isDir);
        }
        ask = null;
        await reveal(to);
        notify.ok(`已改名为 ${to.slice(to.lastIndexOf("/") + 1)}`);
      } else {
        const isDir = a.kind === "newDir";
        const path = await createEntry(a.dir, name, isDir);
        invalidate(a.dir);
        expanded = new Set(expanded).add(a.dir);
        onCreated?.(path, isDir);
        ask = null;
        await reveal(path);
        notify.ok(`已新建 ${name}`);
      }
    } catch (e) {
      // 失败时**留着输入框和已经打好的字** —— 撞名之后要改的是那个名字，
      // 框一关人得从右键菜单重走一遍
      ask = { ...a, error: msgOf(e), busy: false };
      askInput?.focus();
      askInput?.select();
    }
  }

  async function doTrash() {
    const t = trash;
    if (!t || t.busy) return;
    trash = { ...t, busy: true };
    try {
      await trashEntry(t.row.path);
      forgetSubtree(t.row.path);
      invalidate(parentOf(t.row.path));
      onTrashed?.(t.row.path, t.row.isDir);
      trash = null;
      await reload();
      notify.ok(`已移到废纸篓：${t.row.name}`);
    } catch (e) {
      trash = null;
      notify.fail(msgOf(e));
    }
  }

  function openMenu(e: MouseEvent, i: number) {
    e.preventDefault();
    // 右键也要选中这一行 —— 与 Finder / IDEA 一致。
    // 少了这句，菜单作用在哪一行全靠人自己记，而高亮还停在别处
    cursor = i;
    menuCursor = 0;
    menu = { x: e.clientX, y: e.clientY, row: rows[i] };
  }

  function openMenuAtRow(i: number) {
    const el = rowAt(i);
    if (!el) return;
    const r = el.getBoundingClientRect();
    menuCursor = 0;
    // 贴着行的左下角弹，和鼠标右键的落点语义一致
    menu = { x: r.left + 12, y: r.bottom - 2, row: rows[i] };
  }

  /**
   * 项目根的菜单（头部那个项目名）。
   *
   * 没有它，「在项目根下新建」就只能靠右键某个顶层条目再指望人明白
   * 「文件上右键是建在它旁边」—— 那是把功能藏起来。
   * 根这一行不在 `rows` 里（rows 从根的子项开始），所以现造一个 Row。
   */
  function openMenuAtHead(x: number, y: number) {
    menuCursor = 0;
    menu = {
      x,
      y,
      row: { name: rootName, path: root, isDir: true, depth: -1 },
      fromHead: true,
    };
  }

  function closeMenu(refocus = true) {
    if (!menu) return;
    const head = menu.fromHead;
    menu = null;
    // 菜单项刚开出一个输入框/确认框时不要抢焦点回来：
    // 那两个浮层自己会把焦点接过去，这里再插一脚就成了「输入框开着但打不了字」
    if (refocus && !ask && !trash) (head ? headEl : rowAt(cursor))?.focus();
  }

  /** 三个浮层同时只会开一个，但关的时候一起关，省得漏 */
  function closeAll(refocus = true) {
    closeMenu(refocus);
    ask = null;
    trash = null;
  }

  /*
   * 位置钳进视口。**改的是 DOM 而不是 menu 这个 state** ——
   * 写回 state 会让这个 effect 依赖自己写的值，一不小心就是 update 循环。
   * 这里读一次布局、写一次样式，一帧就完事。
   */
  function clamp(el: HTMLElement, at: { x: number; y: number }) {
    const r = el.getBoundingClientRect();
    const pad = 6;
    el.style.left = `${Math.max(pad, Math.min(at.x, window.innerWidth - r.width - pad))}px`;
    el.style.top = `${Math.max(pad, Math.min(at.y, window.innerHeight - r.height - pad))}px`;
  }

  $effect(() => {
    const el = menuEl;
    const m = menu;
    if (!el || !m) return;
    clamp(el, m);
    // 开完就把焦点交给菜单，否则 Esc 和方向键都落不到它身上
    el.focus();
  });

  // 输入框和确认框同样要钳进视口 —— 它们从菜单原地长出来，
  // 而它们比菜单矮，右下角开的时候位置不一样
  $effect(() => {
    const el = askEl;
    const a = ask;
    if (!el || !a) return;
    clamp(el, a);
  });
  $effect(() => {
    const el = trashEl;
    const t = trash;
    if (!el || !t) return;
    clamp(el, t);
    /*
     * 焦点落在**第一个按钮（取消）**上，不是落在框上。
     *
     * 差别在于顺手一个回车会发生什么：焦点在框上时回车什么都不做（还行），
     * 焦点在取消上时回车是取消（更好）—— 而这两种都比"回车即删除"强，
     * 所以下面 onTrashKey 里也没有把 Enter 绑到删除上。
     */
    el.querySelector("button")?.focus();
  });

  /*
   * 输入框开出来就选中已有的名字。
   *
   * 重命名时**只选中扩展名之前那段** —— 改名十有八九是改主干，
   * 连着 `.java` 一起选中，人得先按一下 → 再退回去。Finder / IDEA 都是这样。
   */
  $effect(() => {
    const el = askInput;
    const a = ask;
    if (!el || !a) return;
    untrack(() => {
      el.focus();
      const dot = a.name.lastIndexOf(".");
      el.setSelectionRange(0, dot > 0 ? dot : a.name.length);
    });
  });

  /*
   * 菜单开着时才挂全局监听，关掉就摘干净。
   *
   * scroll 用捕获阶段：文件树自己那个 .list 滚动不冒泡到 window，
   * 不捕获的话，滚一下菜单就飘在半空中指着一行早已滚走的东西。
   */
  $effect(() => {
    if (!menu && !ask && !trash) return;
    const cur = () => menuEl ?? askEl ?? trashEl;
    const onDown = (e: PointerEvent) => {
      const el = cur();
      if (el && !el.contains(e.target as Node)) closeAll(false);
    };
    const onScroll = () => closeAll(false);
    window.addEventListener("pointerdown", onDown, true);
    window.addEventListener("scroll", onScroll, true);
    window.addEventListener("resize", onScroll);
    window.addEventListener("blur", onScroll);
    return () => {
      window.removeEventListener("pointerdown", onDown, true);
      window.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("resize", onScroll);
      window.removeEventListener("blur", onScroll);
    };
  });

  // 换项目根 / 外部刷新之后，菜单指着的那一行可能已经不存在了
  $effect(() => {
    void root;
    void reloadTick;
    // 输入框和确认框也要关：它们指着的那一行可能已经不存在了，
    // 而「确认删除」框指着一个已经没了的东西是危险的
    untrack(() => closeAll(false));
  });

  function onMenuKey(e: KeyboardEvent) {
    const n = items.length;
    switch (e.key) {
      case "Escape":
        e.preventDefault();
        closeMenu();
        break;
      case "ArrowDown":
        e.preventDefault();
        menuCursor = (menuCursor + 1) % n;
        break;
      case "ArrowUp":
        e.preventDefault();
        menuCursor = (menuCursor - 1 + n) % n;
        break;
      case "Home":
        e.preventDefault();
        menuCursor = 0;
        break;
      case "End":
        e.preventDefault();
        menuCursor = n - 1;
        break;
      case "Enter":
      case " ":
        e.preventDefault();
        items[menuCursor]?.run();
        closeMenu();
        break;
    }
  }

  function onAskKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      ask = null;
      rowAt(cursor)?.focus();
    } else if (e.key === "Enter") {
      e.preventDefault();
      void submitAsk();
    }
  }

  function onTrashKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      trash = null;
      rowAt(cursor)?.focus();
    }
    // **Enter 故意不绑到「移到废纸篓」上。** 焦点默认停在「取消」，
    // 于是顺手一个回车是取消而不是删除 —— 删除要么点，要么 Tab 过去再按
  }
</script>

<div class="tree">
  <div class="head">
    <!--
      项目名是个按钮，因为「在项目根下新建」得有个地方点。
      左键也开菜单：一个只认右键的按钮，等于没有告诉任何人它能点。
    -->
    <button
      class="proj"
      bind:this={headEl}
      title="{root}（右键或点击：项目根的操作）"
      onclick={(e) => {
        const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
        openMenuAtHead(r.left, r.bottom + 2);
      }}
      oncontextmenu={(e) => {
        e.preventDefault();
        openMenuAtHead(e.clientX, e.clientY);
      }}
      onkeydown={(e) => {
        if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
          e.preventDefault();
          const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
          openMenuAtHead(r.left, r.bottom + 2);
        }
      }}
    >{rootName}</button>
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
        oncontextmenu={(e) => openMenu(e, i)}
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

<!--
  菜单挂在 .tree **外面**：.list 是 overflow-y: auto，画在里面会被裁掉半截，
  还会跟着滚。position: fixed 逃得出 overflow，但逃不出 transform 祖先 ——
  将来给侧边栏加动画时要留意这条。
-->
{#if menu}
  <div
    class="menu"
    role="menu"
    tabindex="-1"
    aria-label="{menu.row.name} 的操作"
    bind:this={menuEl}
    style:left="{menu.x}px"
    style:top="{menu.y}px"
    onkeydown={onMenuKey}
  >
    <div class="mhead" title={menu.row.path}>{menu.row.name}</div>
    {#each items as it, i (it.label)}
      <button
        class="mitem"
        class:on={i === menuCursor}
        class:sep={it.sep}
        class:danger={it.danger}
        role="menuitem"
        tabindex="-1"
        onmouseenter={() => (menuCursor = i)}
        onclick={() => {
          it.run();
          closeMenu();
        }}
      >
        {it.label}
      </button>
    {/each}
  </div>
{/if}

{#if ask}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="pop"
    role="dialog"
    tabindex="-1"
    aria-label={ASK_TITLE[ask.kind]}
    bind:this={askEl}
    style:left="{ask.x}px"
    style:top="{ask.y}px"
    onkeydown={onAskKey}
  >
    <div class="mhead" title={ask.kind === "rename" ? ask.row?.path : ask.dir}>
      {ASK_TITLE[ask.kind]}{ask.kind === "rename" ? "" : ` · ${relOf(ask.dir) || rootName}`}
    </div>
    <input
      class="pinput"
      bind:this={askInput}
      bind:value={ask.name}
      oninput={() => {
        // 报错留在框里不动的话，改完名字它还在那儿说"已经有一个叫 xxx 的了"，
        // 而 xxx 已经不是框里这个名字了 —— 一句过期的错误比没有错误更难判断
        if (ask) ask.error = "";
      }}
      spellcheck="false"
      autocapitalize="off"
      autocorrect="off"
      placeholder={ask.kind === "newDir" ? "文件夹名" : "文件名"}
    />
    {#if ask.error}
      <div class="perr">{ask.error}</div>
    {/if}
    <div class="prow">
      <button
        class="pbtn"
        onclick={() => {
          ask = null;
          rowAt(cursor)?.focus();
        }}>取消</button
      >
      <button class="pbtn primary" disabled={ask.busy} onclick={() => void submitAsk()}>
        {ask.kind === "rename" ? "改名" : "新建"}
      </button>
    </div>
  </div>
{/if}

{#if trash}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="pop"
    role="dialog"
    aria-label="移到废纸篓"
    tabindex="-1"
    bind:this={trashEl}
    style:left="{trash.x}px"
    style:top="{trash.y}px"
    onkeydown={onTrashKey}
  >
    <div class="mhead" title={trash.row.path}>移到废纸篓</div>
    <div class="ptext">
      「<b>{trash.row.name}</b>」{trash.row.isDir ? "连同里面的全部内容" : ""}会被移到废纸篓，
      可以在 Finder 里放回原处。
    </div>
    {#if trash.dirty > 0}
      <!--
        未保存的改动在废纸篓里是**找不回来的**：文件回来的是磁盘上那一份，
        编辑器里没保存的那些字随着标签一起没。这句必须说在前面
      -->
      <div class="pwarn">
        有 {trash.dirty} 个未保存的标签会被一起关掉，那些改动找不回来。
      </div>
    {/if}
    <div class="prow">
      <button
        class="pbtn"
        onclick={() => {
          trash = null;
          rowAt(cursor)?.focus();
        }}>取消</button
      >
      <button class="pbtn danger" disabled={trash.busy} onclick={() => void doTrash()}>
        移到废纸篓
      </button>
    </div>
  </div>
{/if}

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
  /* 项目名现在是个按钮（要能开菜单），外观得跟原来那个 span 一模一样 */
  .head .proj {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    padding: 0;
    background: none;
    border: none;
    color: inherit;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    cursor: default;
  }
  .head .proj:hover { color: var(--text); }
  .head .proj:focus-visible { outline: 1px solid var(--accent); outline-offset: 1px; }
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
  .menu {
    position: fixed;
    z-index: 60;
    min-width: 168px;
    padding: 4px;
    background: var(--panel-bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    outline: none;
  }
  .mhead {
    padding: 3px 9px 5px;
    margin-bottom: 3px;
    border-bottom: 1px solid var(--border-soft);
    color: var(--text-faint);
    font-family: var(--ui-font);
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 260px;
  }
  .mitem {
    display: block;
    width: 100%;
    padding: 4px 9px;
    background: transparent;
    border: none;
    border-radius: 3px;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    white-space: nowrap;
    cursor: default;
  }
  /* 鼠标和键盘共用一个高亮：菜单里同时有两个高亮是最容易看错的写法 */
  .mitem.on { background: var(--accent-sel); }

  .mitem.sep {
    margin-top: 4px;
    padding-top: 6px;
    border-top: 1px solid var(--border-soft);
    border-radius: 0 0 3px 3px;
  }
  /*
   * 危险项常驻红色，不是只在 hover 时变红：手滑点中的那一下发生在 hover 之后，
   * 而人是靠"扫一眼菜单"决定往哪儿点的
   */
  .mitem.danger { color: var(--lvl-error); }
  .mitem.danger.on { background: rgba(247, 84, 100, 0.16); }

  /* 输入框 / 确认框：位置和外观都跟着菜单走，它们是从菜单原地长出来的 */
  .pop {
    position: fixed;
    z-index: 60;
    width: 260px;
    padding: 4px 4px 6px;
    background: var(--panel-bg-2);
    border: 1px solid var(--border);
    border-radius: 6px;
    box-shadow: 0 8px 24px rgba(0, 0, 0, 0.45);
    outline: none;
  }
  .pinput {
    width: 100%;
    margin: 2px 0 0;
    padding: 5px 8px;
    background: var(--panel-bg);
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text);
    font-family: var(--code-font);
    font-size: 12.5px;
  }
  .pinput:focus { outline: none; border-color: var(--accent); }
  .ptext {
    padding: 4px 6px 2px;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    line-height: 1.55;
  }
  .ptext b { color: var(--text); font-weight: 600; }
  .perr {
    padding: 5px 6px 1px;
    color: var(--lvl-error);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .pwarn {
    margin: 5px 4px 0;
    padding: 5px 7px;
    background: rgba(247, 84, 100, 0.1);
    border-radius: 4px;
    color: var(--lvl-error);
    font-size: 11.5px;
    line-height: 1.5;
  }
  .prow {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: 8px;
    padding: 0 2px;
  }
  .pbtn {
    padding: 4px 11px;
    background: transparent;
    border: 1px solid var(--border);
    border-radius: 4px;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .pbtn:hover { background: var(--panel-bg); color: var(--text); }
  .pbtn.primary { background: var(--accent); border-color: var(--accent); color: #fff; }
  .pbtn.danger { border-color: var(--lvl-error); color: var(--lvl-error); }
  .pbtn.danger:hover { background: rgba(247, 84, 100, 0.14); }
  .pbtn:disabled { opacity: 0.55; }

  .err {
    padding: 8px 10px;
    color: var(--lvl-error);
    font-size: 11.5px;
    font-family: var(--code-font);
  }
  @media (prefers-reduced-motion: reduce) { .caret { transition: none; } }
</style>
