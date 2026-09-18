<script lang="ts">
  import Icon from "./Icon.svelte";
  import FileGlyph from "./FileGlyph.svelte";
  import { tick, untrack } from "svelte";
  import { trashEntry, type DirEntry, type GitEntry, type GitStatus } from "../ipc/commands";
import { createEntry, listDir, renameEntry, moveEntry } from "../ipc/fs";
  import { notify } from "../state/notify.svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { copyText, relTo, showInFinder } from "./pathactions";
  import { readPref, writePref } from "../state/prefs";

  let {
    root,
    activePath,
    gitStatus = null,
    ignored = null,
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
     * git 说被忽略的目录（相对项目根）。`null` = 问不到 git，退回按名字判。
     *
     * 只对**有争议**的那一档（`dist` / `build` / `vendor`）起作用 ——
     * `node_modules` 之类没有第二种可能，不用问也知道。见 issue #13。
     */
    ignored?: Set<string> | null;
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
    /**
     * `keep`：双击给 true。单击开的是预览标签（issue #33 ⑯），双击才永久占格 ——
     * 双击前面那下单击已经把它开成预览了，这一下只是把它保留下来。
     */
    onOpen: (path: string, isDir: boolean, keep?: boolean) => void;
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
    /** 生成物目录：压暗、不自动展开。**它在树里、点得开**（issue #13） */
    generated: boolean;
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
    const items = await listDir(dir);
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
          next.set(d, await listDir(d));
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

  /**
   * 这一条要不要压暗。
   *
   * `generated`（名字命中生成物名单）是 Rust 侧给的**怀疑**；
   * `contested` 说这个名字还有第二种可能（`build/` 在 CMake 项目里是源码）。
   * 有争议的那几个要拿 git 的答案对一遍 —— 那才是证据。
   *
   * 问不到 git（`ignored === null`）时退回按名字：那时我们没有别的依据，
   * 而压暗的代价只是「它有点灰」，它仍然在树里、点得开。
   *
   * **判据必须和搜索那边一致**（`searchsvc::Skip::skips`）—— 不一致的话，
   * 树里压暗的东西搜得到、树里正常的东西搜不到，那比两边都错更难理解。
   */
  function dims(it: DirEntry): boolean {
    if (!it.generated) return false;
    if (!it.contested) return true;
    if (ignored === null) return true;
    return ignored.has(relTo(root, it.path));
  }

  /**
   * 深度优先展开成扁平列表。
   *
   * `inGen` 往下传：**生成物是整棵子树的性质，不是那一行的性质**。
   * 只标记顶上那一行的话，展开 `node_modules/` 往下滚两屏，那行早就滚没了 ——
   * 剩下的是一片看着和自己代码一模一样的东西。
   */
  let rows = $derived.by(() => {
    const out: Row[] = [];
    const walk = (dir: string, depth: number, inGen: boolean) => {
      const items = children.get(dir);
      if (!items) return;
      for (const it of items) {
        const gen = inGen || dims(it);
        out.push({ name: it.name, path: it.path, isDir: it.isDir, depth, generated: gen });
        if (it.isDir && expanded.has(it.path)) walk(it.path, depth + 1, gen);
      }
    };
    walk(root, 0, false);
    return out;
  });

  const rootName = $derived(root.slice(root.lastIndexOf("/") + 1) || root);

  /**
   * git 状态 → 绝对路径查找表。三样东西一起算，因为都要遍历同一份 entries：
   *
   * - `own`  —— 文件/目录**自身**的状态
   * - `roll` —— 祖先目录的「里面有东西改了」冒泡标记。IDE 里最有用的那个提示：
   *   目录收着也知道里面有动静
   * - `utDirs` —— 整个未跟踪的目录。里面的文件 Rust 侧已经摊开进 entries 了，
   *   这份名单是给**目录自己**上色用的：少了它，一个全新的目录只剩
   *   「里面有东西改了」的冒泡标记，和一个改了一行的老目录长得一样。
   *   前缀匹配那半边留着兜底 —— 条目撞上 5000 条上限被截断时，
   *   里面的文件可能一条都没进来
   */
  let git = $derived.by(() => {
    const own = new Map<string, string>();
    const roll = new Set<string>();
    const utDirs: string[] = [];
    const st = gitStatus;
    if (!st) return { own, roll, utDirs };

    // 一路冒泡到仓库根为止
    const bubble = (abs: string) => {
      let p = abs;
      for (;;) {
        const i = p.lastIndexOf("/");
        if (i < 0) break;
        p = p.slice(0, i);
        if (p.length <= st.root.length) break;
        roll.add(p);
      }
    };

    for (const e of st.entries) {
      const abs = `${st.root}/${e.path}`;
      own.set(abs, klass(e));
      bubble(abs);
    }
    // 目录名带着末尾的斜杠，去掉它才是目录自己的路径
    for (const d of st.untrackedDirs ?? []) {
      const abs = `${st.root}/${d.slice(0, -1)}`;
      own.set(abs, "untracked");
      utDirs.push(`${abs}/`);
      bubble(abs);
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

  // ─────────────────── 多选（issue #33 ⑦） ───────────────────

  /**
   * ⌘点 逐个加减、⇧点 / ⇧↑↓ 连选一段，和 Finder / IDEA 一样。只为一件事：
   * **一次把一批文件移到废纸篓**（右键菜单只剩批量能做的那几项）。
   * 存路径不存下标 —— 中间折叠了一个目录，下标全变，路径不变。
   * `anchor` 是 ⇧连选的起点：上一次普通点击或 ⌘点 的那一行。
   * 普通点击、Esc、做完一次批量操作都清掉。
   */
  let selected = $state(new Set<string>());
  let anchor = -1;

  function selectRange(from: number, to: number) {
    const [a, b] = from <= to ? [from, to] : [to, from];
    const next = new Set<string>();
    for (let k = Math.max(0, a); k <= Math.min(rows.length - 1, b); k++) next.add(rows[k].path);
    selected = next;
  }

  function onRowClick(e: MouseEvent, i: number) {
    if (justDragged) {
      justDragged = false;
      return;
    }
    const row = rows[i];
    speed = "";
    if (e.metaKey) {
      // 第一次 ⌘点：把此刻的游标行也算进去，不然选出来的是「B」而不是「A + B」
      const next = new Set(selected);
      if (next.size === 0 && cursor >= 0 && cursor < rows.length && cursor !== i) next.add(rows[cursor].path);
      if (next.has(row.path)) next.delete(row.path);
      else next.add(row.path);
      selected = next;
      anchor = i;
      cursor = i;
      return;
    }
    if (e.shiftKey) {
      selectRange(anchor < 0 ? cursor : anchor, i);
      cursor = i;
      return;
    }
    selected = new Set();
    anchor = i;
    cursor = i;
    click(row);
  }

  /** 选中的行（按树里的顺序）；没有多选时是空表 */
  let selectedRows = $derived(rows.filter((r) => selected.has(r.path)));

  // ─────────────────── 拖拽移动（issue #33 ⑨） ───────────────────

  /**
   * 拖一行（或选中的一批）到某个目录上 → 先问一句再挪。改盘的动作一律过确认，
   * 而拖拽比右键菜单更容易手滑 —— 松手位置差一行就是另一个目录。
   *
   * **不走 HTML5 的 dragstart / drop。** 在 Tauri 的 WKWebView 里那套事件不完整：
   * wry 为了接住从 Finder 拖进来的文件，接管了 NSView 的拖拽入口，页面内部的
   * `draggable` 元素按下去拖过去，`drop` 从来不来（浏览器里桩上一切正常，
   * 真 .app 里一次都没弹过确认框 —— 2026-09-15 用 CGEvent 模拟鼠标验的）。
   * 所以自己用 pointer 事件做：按下记住是谁，挪过 5px 算开始拖，随手一个小标签
   * 跟着鼠标，松手时 `elementFromPoint` 看落在哪个目录行上。
   */
  let dragging = $state<Row[] | null>(null);
  /** 此刻悬在哪个目录上（路径）。根目录用 `root` */
  let dropTarget = $state<string | null>(null);
  let move = $state<{ x: number; y: number; rows: Row[]; dest: string; busy: boolean } | null>(null);
  let moveEl = $state<HTMLElement | null>(null);
  /** 跟着鼠标的小标签 */
  let ghost = $state<{ x: number; y: number; text: string } | null>(null);
  /** 按下但还没拖过阈值：可能只是一次点击 */
  let press: { x: number; y: number; row: Row } | null = null;
  /** 刚拖完的那一下 click 不算打开 */
  let justDragged = false;
  const DRAG_START = 5;

  function onRowPointerDown(e: PointerEvent, row: Row) {
    if (e.button !== 0 || e.metaKey || e.shiftKey || e.altKey || e.ctrlKey) return;
    press = { x: e.clientX, y: e.clientY, row };
    window.addEventListener("pointermove", onDragMove);
    window.addEventListener("pointerup", onDragUp, { once: true });
  }

  /** 鼠标底下是哪个目录（行或项目头）；不是目录给 null */
  function dirUnder(x: number, y: number): string | null {
    const el = document.elementFromPoint(x, y);
    const rowEl = el?.closest<HTMLElement>(".row[data-path]");
    if (rowEl) return rowEl.dataset.isdir === "1" ? rowEl.dataset.path! : null;
    if (el?.closest(".proj")) return root;
    return null;
  }

  function onDragMove(e: PointerEvent) {
    if (!press) return;
    if (!dragging) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_START) return;
      const row = press.row;
      dragging = selected.size > 1 && selected.has(row.path) ? selectedRows : [row];
      speed = "";
    }
    e.preventDefault();
    const batch = dragging;
    ghost = { x: e.clientX + 12, y: e.clientY + 12, text: batch.length === 1 ? batch[0].name : `${batch.length} 个条目` };
    const dir = dirUnder(e.clientX, e.clientY);
    dropTarget = dir !== null && canDropOn(batch, dir) ? dir : null;
  }

  function onDragUp(e: PointerEvent) {
    window.removeEventListener("pointermove", onDragMove);
    const batch = dragging;
    const dir = dropTarget;
    press = null;
    dragging = null;
    dropTarget = null;
    ghost = null;
    if (!batch) return;
    justDragged = true;
    if (dir === null) return;
    // 目录和它里面的文件一起拖：只挪顶层的，子树跟着走
    const tops = batch.filter((r) => !batch.some((o) => o !== r && o.isDir && r.path.startsWith(`${o.path}/`)));
    move = { x: e.clientX, y: e.clientY, rows: tops, dest: dir, busy: false };
  }

  /** 这个目录能不能接：不是自己、不是自己的子目录、不是它原来就在的地方 */
  function canDropOn(batch: Row[] | null, dir: string): boolean {
    if (!batch) return false;
    return batch.every((r) => r.path !== dir && !dir.startsWith(`${r.path}/`) && parentOf(r.path) !== dir);
  }

  async function doMove() {
    const m = move;
    if (!m || m.busy) return;
    move = { ...m, busy: true };
    const failed: string[] = [];
    let last = "";
    for (const row of m.rows) {
      try {
        const to = await moveEntry(row.path, m.dest);
        forgetSubtree(row.path);
        invalidate(parentOf(row.path));
        onRenamed?.(row.path, to, row.isDir);
        last = to;
      } catch (err) {
        failed.push(`${row.name}：${msgOf(err)}`);
      }
    }
    invalidate(m.dest);
    move = null;
    selected = new Set();
    expanded = new Set(expanded).add(m.dest);
    await reload();
    if (last) await reveal(last, m.rows.length > 1);
    const done = m.rows.length - failed.length;
    if (failed.length === 0) {
      notify.ok(m.rows.length === 1 ? `已移到 ${relOf(m.dest) || "项目根"}：${m.rows[0].name}` : `已移动 ${done} 个到 ${relOf(m.dest) || "项目根"}`);
    } else {
      notify.fail(`${done} 个已移动，${failed.length} 个没动：${failed.join("；")}`, 5000);
    }
  }

  function onMoveKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      move = null;
      rowAt(cursor)?.focus();
    }
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
  async function reveal(path: string, quiet = false) {
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

    // 跟随（标签换了树自己走过去）不闪：那不是人点的，闪一下反而分神
    if (quiet) return;
    // 目标可能本来就在视野里，滚动等于没反应 —— 闪一下才知道点中了
    flash = acc;
    if (flashTimer) clearTimeout(flashTimer);
    flashTimer = setTimeout(() => {
      flash = "";
      flashTimer = null;
    }, 900);
  }

  /**
   * 跟随：活动标签换了，树自己展开到那个文件并选中（issue #33 ⑤）。
   *
   * IDEA 的「Always Select Opened File」、VS Code 的 `autoReveal`。默认开 ——
   * 树不跟着走的话，⌘P 开的文件在树里永远是「不知道在哪」。纯偏好，
   * 存 localStorage，不进会话快照（理由见 state/prefs.ts）。
   *
   * 走 `reveal(path, quiet)`：展开沿途、滚到 nearest、不闪。**只在文件在项目根
   * 底下时**，草稿、应用日志这类项目外的标签树里本来就没有。
   */
  let follow = $state(readPref("tree-follow", true));
  $effect(() => {
    writePref("tree-follow", follow);
  });
  $effect(() => {
    const p = activePath;
    if (!follow || !p) return;
    untrack(() => void reveal(p, true));
  });

  /** 树头的「定位」：和面包屑那条路一样，展开、滚过去、闪一下 */
  function locate() {
    if (activePath) void reveal(activePath);
  }

  /** 树头的「折叠全部」：只留根那一层。IDEA / VS Code 都有 */
  function collapseAll() {
    expanded = new Set([root]);
    cursor = 0;
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

  // ─────────────────── 打字定位（issue #33 ⑧） ───────────────────

  /**
   * 照 IDEA 的 speed search：焦点在树上直接打字，游标跳到第一个名字里**包含**
   * 这串字的行，所有命中的行把那几个字标亮；↑↓ 在命中之间走，⌫ 退一个字，
   * Esc 清掉，开了文件也清掉。**不过滤、不藏行** —— VS Code 那种打字出过滤框
   * 会把树的形状换掉，人刚记住的位置全没了；IDEA 的做法是树不动、只挪游标。
   *
   * 只在**已经展开**的行里找：打字定位是「我看得见它、懒得用鼠标」，
   * 要找没展开的东西是 ⌘P 的活。
   *
   * 焦点离开树就清：这串字是键盘上的临时状态，不是搜索条件。
   */
  let speed = $state("");
  let speedLower = $derived(speed.toLowerCase());

  /** 名字里命中的那一段 [起, 止)，没命中 null。大小写不敏感 */
  function speedHit(name: string): [number, number] | null {
    if (speedLower === "") return null;
    const k = name.toLowerCase().indexOf(speedLower);
    return k < 0 ? null : [k, k + speedLower.length];
  }
  let speedAny = $derived(speedLower === "" || rows.some((r) => speedHit(r.name) !== null));

  /** 从 `from` 起（含）往 `dir` 方向找下一个命中的行，绕圈；没有给 -1 */
  function speedNext(from: number, dir: 1 | -1): number {
    const n = rows.length;
    for (let k = 0; k < n; k++) {
      const j = (((from + dir * k) % n) + n) % n;
      if (speedHit(rows[j].name)) return j;
    }
    return -1;
  }

  /** 返回 true = 这次按键归打字定位，别的分支不用再看 */
  function speedKey(e: KeyboardEvent, i: number): boolean {
    if (e.metaKey || e.ctrlKey || e.altKey) return false;
    if (speed === "") {
      // 只有可打印字符才开始；空格留给「打开」
      if (e.key.length !== 1 || e.key === " ") return false;
    }
    switch (e.key) {
      case "Escape":
        speed = "";
        return true;
      case "Backspace":
        speed = speed.slice(0, -1);
        return true;
      case "ArrowDown":
      case "ArrowUp": {
        const j = speedNext(i + (e.key === "ArrowDown" ? 1 : -1), e.key === "ArrowDown" ? 1 : -1);
        if (j >= 0) focusRow(j);
        return true;
      }
    }
    if (e.key.length !== 1) return false;
    speed += e.key;
    // 当前行还命中就不动（多打一个字不该把人甩到别处），不命中才往下找
    if (!speedHit(rows[i].name)) {
      const j = speedNext(i + 1, 1);
      if (j >= 0) focusRow(j);
    }
    return true;
  }

  function onRowKey(e: KeyboardEvent, i: number) {
    const row = rows[i];
    if (speedKey(e, i)) {
      e.preventDefault();
      return;
    }
    switch (e.key) {
      case "ArrowDown":
      case "ArrowUp": {
        e.preventDefault();
        const to = e.key === "ArrowDown" ? i + 1 : i - 1;
        if (e.shiftKey) {
          // ⇧↑↓ 连选：起点是 anchor（没有就是当前行），终点跟着游标走
          if (anchor < 0 || selected.size === 0) anchor = i;
          focusRow(to);
          selectRange(anchor, cursor);
        } else {
          selected = new Set();
          focusRow(to);
          anchor = cursor;
        }
        break;
      }
      case "Escape":
        if (selected.size > 0) {
          e.preventDefault();
          selected = new Set();
        }
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
        speed = "";
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
  let menu = $state<{ x: number; y: number; row: Row; fromHead?: boolean; multi?: boolean } | null>(null);
  let headEl = $state<HTMLElement | null>(null);

  const relOf = (p: string) => relTo(root, p);
  const parentOf = (p: string) => p.slice(0, p.lastIndexOf("/")) || "/";

  let items = $derived.by(() => {
    const row = menu?.row;
    if (!row) return [] as MenuItem[];
    // 多选：只有批量能做的几项。新建 / 重命名说不清「对哪一个」，不给
    if (menu?.multi) {
      const many = selectedRows;
      return [
        { label: `移到废纸篓（${many.length} 个）`, danger: true, run: () => openTrashAsk(many) },
        {
          label: `复制路径（${many.length} 个）`,
          sep: true,
          run: () => void copyText(many.map((r) => r.path).join("\n"), "路径"),
        },
        {
          label: `复制相对路径（${many.length} 个）`,
          run: () => void copyText(many.map((r) => relOf(r.path)).join("\n"), "相对路径"),
        },
      ] as MenuItem[];
    }
    // 在目录上右键 → 建在它里面；在文件上右键 → 建在它旁边（同 Finder / IDEA）
    const dir = row.isDir ? row.path : parentOf(row.path);
    const out: MenuItem[] = [
      { label: "新建文件…", run: () => openAsk("newFile", dir) },
      { label: "新建文件夹…", run: () => openAsk("newDir", dir) },
    ];
    if (row.path !== root) {
      out.push({ label: "重命名…", sep: true, run: () => openAsk("rename", parentOf(row.path), row) });
      out.push({ label: "移到废纸篓", danger: true, run: () => openTrashAsk([row]) });
    }
    out.push({ label: "在 Finder 中显示", sep: true, run: () => void showInFinder(row.path) });
    out.push({ label: "复制路径", run: () => void copyText(row.path, "路径") });
    out.push({ label: "复制相对路径", run: () => void copyText(relOf(row.path), "相对路径") });
    return out;
  });

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
  let trash = $state<{ x: number; y: number; rows: Row[]; dirty: number; busy: boolean } | null>(null);
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

  function openTrashAsk(list: Row[]) {
    const at = menu ?? { x: 0, y: 0 };
    // 选了目录又选了它里面的文件：算未保存标签时别数两遍 —— 子树在父目录里已经算过
    const tops = list.filter((r) => !list.some((o) => o !== r && o.isDir && r.path.startsWith(`${o.path}/`)));
    const dirty = tops.reduce((n, r) => n + (dirtyUnder?.(r.path) ?? 0), 0);
    trash = { x: at.x, y: at.y, rows: tops, dirty, busy: false };
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
    // 一个一个扔，扔不动的记下来继续 —— 一批里有一个没权限，不该让其余的也留着
    const failed: string[] = [];
    for (const row of t.rows) {
      try {
        await trashEntry(row.path);
        forgetSubtree(row.path);
        invalidate(parentOf(row.path));
        onTrashed?.(row.path, row.isDir);
      } catch (e) {
        failed.push(`${row.name}：${msgOf(e)}`);
      }
    }
    trash = null;
    selected = new Set();
    await reload();
    const done = t.rows.length - failed.length;
    if (failed.length === 0) {
      notify.ok(t.rows.length === 1 ? `已移到废纸篓：${t.rows[0].name}` : `已移到废纸篓 ${done} 个`);
    } else {
      notify.fail(`${done} 个已移到废纸篓，${failed.length} 个没动：${failed.join("；")}`, 5000);
    }
  }

  function openMenu(e: MouseEvent, i: number) {
    e.preventDefault();
    // 右键也要选中这一行 —— 与 Finder / IDEA 一致。
    // 少了这句，菜单作用在哪一行全靠人自己记，而高亮还停在别处
    cursor = i;
    // 在选中的一批里右键 → 菜单作用在整批上；在批外右键 → 批取消，回到单个
    const multi = selected.size > 1 && selected.has(rows[i].path);
    if (!multi) selected = new Set();
    menu = { x: e.clientX, y: e.clientY, row: rows[i], multi };
  }

  function openMenuAtRow(i: number) {
    const el = rowAt(i);
    if (!el) return;
    const r = el.getBoundingClientRect();
    const multi = selected.size > 1 && selected.has(rows[i].path);
    if (!multi) selected = new Set();
    // 贴着行的左下角弹，和鼠标右键的落点语义一致
    menu = { x: r.left + 12, y: r.bottom - 2, row: rows[i], multi };
  }

  /**
   * 项目根的菜单（头部那个项目名）。
   *
   * 没有它，「在项目根下新建」就只能靠右键某个顶层条目再指望人明白
   * 「文件上右键是建在它旁边」—— 那是把功能藏起来。
   * 根这一行不在 `rows` 里（rows 从根的子项开始），所以现造一个 Row。
   */
  function openMenuAtHead(x: number, y: number) {
    menu = {
      x,
      y,
      // 项目根永远不是生成物 —— 你是特意把它当项目打开的
      row: { name: rootName, path: root, isDir: true, depth: -1, generated: false },
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
    move = null;
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

  // 输入框和确认框同样要钳进视口 —— 它们从菜单原地长出来，
  // 而它们比菜单矮，右下角开的时候位置不一样
  $effect(() => {
    const el = askEl;
    const a = ask;
    if (!el || !a) return;
    clamp(el, a);
  });
  // 移动确认框同废纸篓那个：钳进视口、焦点落在「取消」上
  $effect(() => {
    const el = moveEl;
    const m = move;
    if (!el || !m) return;
    clamp(el, m);
    el.querySelector("button")?.focus();
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
   *
   * **但捕获阶段听到的是整个应用的滚动**，而这里只该关心「菜单锚着的那一行
   * 动没动」。M22 少了这层过滤，症状是：日志 tail 开着的时候右键菜单
   * 根本打不开 —— tail 每 500ms 让日志面板自动滚一次，菜单开出来不到半秒
   * 就被清掉，看上去就像右键失灵。M24 之后更糟，连输入框都会在打字途中消失。
   */
  // 菜单的这套监听在 ContextMenu 里，这里只管输入框和确认框
  $effect(() => {
    if (!ask && !trash && !move) return;
    const cur = () => askEl ?? trashEl ?? moveEl;
    const onDown = (e: PointerEvent) => {
      const el = cur();
      if (el && !el.contains(e.target as Node)) closeAll(false);
    };
    const onScroll = (e: Event) => {
      const t = e.target as Node | null;
      // 只有文件树自己的列表（或整页）滚了才关。别的面板滚动跟这个菜单没关系
      if (t && t !== document && listEl && !t.contains(listEl)) return;
      closeAll(false);
    };
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
      class:droptarget={dropTarget === root}
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
    <!--
      树头的三个动作，照 IDEA 项目工具窗的头：定位当前文件 / 折叠全部 / 跟随开关。
      整条栏各只有一个，不是每一项上重复的东西，所以常驻（ui.md 第三条的例外），
      但只用 --text-faint，hover 才亮。
    -->
    {#if activePath}
      <button class="hb" onclick={locate} title="在树里定位当前文件" aria-label="定位当前文件">
        <Icon name="locate" size={14} />
      </button>
    {/if}
    <button class="hb" onclick={collapseAll} title="折叠全部" aria-label="折叠全部">
      <Icon name="collapse" size={14} />
    </button>
    <button
      class="hb"
      class:on={follow}
      onclick={() => (follow = !follow)}
      title={follow ? "跟随标签：开（切标签时树自动定位）" : "跟随标签：关"}
      aria-label="跟随标签"
      aria-pressed={follow}
    >
      <Icon name="follow" size={14} />
    </button>
  </div>
  {#if speed !== ""}
    <!-- 打的字浮在列表右上角（IDEA 的那个小框），没命中时说一声，别让人以为键盘坏了 -->
    <div class="speed" class:none={!speedAny} role="status" aria-live="polite">
      {speed}{#if !speedAny}<span class="hint">没有匹配</span>{/if}
    </div>
  {/if}
  <div
    class="list"
    role="tree"
    aria-label="文件树"
    bind:this={listEl}
    onfocusout={(e) => {
      if (!listEl?.contains(e.relatedTarget as Node | null)) speed = "";
    }}
  >
    {#each rows as row, i (row.path)}
      {@const d = deco(row.path)}
      {@const hit = speedHit(row.name)}
      <button
        class="row"
        class:dir={row.isDir}
        class:gen={row.generated}
        class:active={row.path === activePath}
        class:selected={selected.has(row.path)}
        class:droptarget={dropTarget === row.path}
        class:flash={row.path === flash}
        data-path={row.path}
        data-isdir={row.isDir ? "1" : "0"}
        onpointerdown={(e) => onRowPointerDown(e, row)}
        role="treeitem"
        tabindex={i === cursor ? 0 : -1}
        aria-level={row.depth + 1}
        aria-expanded={row.isDir ? expanded.has(row.path) : undefined}
        aria-selected={row.path === activePath}
        style:padding-left="{6 + row.depth * 13}px"
        onclick={(e) => onRowClick(e, i)}
        ondblclick={() => {
          if (!row.isDir) onOpen(row.path, false, true);
        }}
        onfocus={() => (cursor = i)}
        onkeydown={(e) => onRowKey(e, i)}
        oncontextmenu={(e) => openMenu(e, i)}
        title={row.generated
          ? `${row.name} —— 生成物目录。搜索（⌘P / ⇧⌘F）不进这里，点开仍然可以看`
          : row.name}
      >
        {#if row.isDir}
          <span class="caret" class:open={expanded.has(row.path)}>
            <Icon name="chevron-right" size={10} />
          </span>
        {:else}
          <span class="caret spacer"></span>
        {/if}
        <!--
          目录用的是共用图标（Icon 的 files），别的类型是文件树自己那一套
          「按扩展名分色」的字形，两者不是一个家族，不必强行统一。
          但**文件夹这一个形状必须只有一处定义** —— 导轨上和树里画的是同一样东西。
        -->
        <FileGlyph name={row.name} isDir={row.isDir} size={14} />
        <span class="name g-{d?.cls ?? 'none'}">
          {#if hit}{row.name.slice(0, hit[0])}<mark>{row.name.slice(hit[0], hit[1])}</mark>{row.name.slice(hit[1])}{:else}{row.name}{/if}
        </span>
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

{#if menu}
  <ContextMenu
    x={menu.x}
    y={menu.y}
    title={menu.row.name}
    titleTip={menu.row.path}
    label="{menu.row.name} 的操作"
    {items}
    onclose={closeMenu}
  />
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

{#if ghost}
  <div class="ghost" style:left="{ghost.x}px" style:top="{ghost.y}px" aria-hidden="true">{ghost.text}</div>
{/if}

{#if move}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div
    class="pop"
    role="dialog"
    aria-label="移动"
    tabindex="-1"
    bind:this={moveEl}
    style:left="{move.x}px"
    style:top="{move.y}px"
    onkeydown={onMoveKey}
  >
    <div class="mhead" title={move.dest}>移动到 {relOf(move.dest) || "项目根"}</div>
    <div class="ptext">
      {#if move.rows.length === 1}
        「<b>{move.rows[0].name}</b>」{move.rows[0].isDir ? "连同里面的全部内容" : ""}会被移到
      {:else}
        <b>{move.rows.length} 个条目</b>会被移到
      {/if}
      <span class="mono">{relOf(move.dest) || "项目根"}</span>。打开着的标签会跟过去。
    </div>
    <div class="prow">
      <button
        class="pbtn"
        onclick={() => {
          move = null;
          rowAt(cursor)?.focus();
        }}>取消</button
      >
      <button class="pbtn primary" disabled={move.busy} onclick={() => void doMove()}>移动</button>
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
    <div class="mhead" title={trash.rows.map((r) => r.path).join("\n")}>移到废纸篓</div>
    <div class="ptext">
      {#if trash.rows.length === 1}
        「<b>{trash.rows[0].name}</b>」{trash.rows[0].isDir ? "连同里面的全部内容" : ""}会被移到废纸篓，
      {:else}
        <b>{trash.rows.length} 个条目</b>{trash.rows.some((r) => r.isDir) ? "（目录连同里面的全部内容）" : ""}会被移到废纸篓，
      {/if}
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
  /*
   * **右边不画线。** 这条线归 `App.svelte` 的 `.side-resizer` 管 ——
   * 那条 4px 的热区自己用一个居中的 1px 伪元素画线，而这里再画一条，
   * 两条只隔 1.5px，看上去就是一条又粗又脏的双线（收起侧边栏时还只剩一条，
   * 对不上）。同一条边界只能有一个人负责，负责的是能被拖动的那个。
   */
  .tree {
    position: relative;
    display: flex;
    flex-direction: column;
    height: 100%;
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    overflow: hidden;
  }
  .head {
    flex: none;
    /* 30 → 38（M8）：和右边的标签栏齐平，两块的第一行才在同一条水平线上；不画下边线 */
    height: 38px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
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
  /* 树头的动作按钮：22px 方格子，和底部面板头上那几个一个尺寸 */
  .head .hb {
    flex: none;
    display: grid;
    place-content: center;
    width: 24px;
    height: 24px; /* M8：工具按钮统一 24 */
    background: transparent;
    border: none;
    border-radius: var(--r-sm);
    color: var(--text-faint);
    cursor: default;
  }
  .head .hb:hover { background: var(--hover); color: var(--text); }
  .head .hb:active { background: var(--pressed); }
  /* 跟随开着时用中性白点亮，同导轨的选中态：accent 留给「有改动」那类状态 */
  .head .hb.on { color: var(--text); background: var(--selected); }
  .head .hb:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* 横向 6px 是给行的圆角块留的余地 —— 贴着面板边的圆角看着像被切了一半 */
  .list { flex: 1; overflow: auto; padding: 4px 6px; }
  /*
   * `width: max-content` + `min-width: 100%`：短行铺满整宽（悬停高亮、
   * 右端那个 git 字母的位置都跟原来一样），长行按自己的实际宽度撑出去，
   * 由 `.list` 横向滚。
   *
   * 原来是死的 `width: 100%`，于是深层目录的缩进（每层 13px）把名字挤出边界，
   * `.name` 的 ellipsis 一路吃到**只剩一个省略号**，第 8 层往下每一行都长得一样 ——
   * 那不是「省略」，是把这一列的全部信息删干净了。
   */
  .row {
    display: flex;
    align-items: center;
    gap: 3px;
    width: max-content;
    min-width: 100%;
    height: 24px;
    padding-right: 8px;
    border-radius: var(--r-sm); /* M8：行和标签、按钮一个圆角 */
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  /*
   * 悬停/选中是**内缩的圆角块**，不是通栏色条。
   *
   * 通栏色条是文件管理器的做法：它在说"这一整行都是它"。而在侧边栏里
   * 一行就是一个条目，圆角块把它框成一个"物件"，边缘不贴着面板的边，
   * 看着轻得多。列表容器给 6px 的横向内边距，就是给这个留的。
   */
  .row:hover { background: var(--hover); }
  .row.active { background: var(--selected); color: var(--text); }
  /* 多选的行：和当前文件同一块底色 —— 它们此刻就是「被选中」这一个意思 */
  .row.selected { background: var(--selected); color: var(--text); }
  /* 拖到目录上：一圈 accent 描边，和「定位闪一下」同一种语言 —— 这里要落的是它 */
  .row.droptarget, .proj.droptarget { outline: 1px solid var(--accent); outline-offset: -1px; background: var(--hover); }
  .ghost {
    position: fixed;
    z-index: 60;
    padding: 2px 8px;
    font-size: 12px;
    color: var(--text);
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    box-shadow: var(--shadow-pop);
    pointer-events: none;
    white-space: nowrap;
  }
  .row.dir { color: var(--text); }
  .row:focus-visible { outline: 1px solid var(--accent); outline-offset: -1px; }
  /*
   * 定位命中：只描一圈边，不改底色 —— 底色是 git 装饰和 .active 在用的，
   * 抢过来会让「这行是当前文件」和「这行刚被定位到」混成一个样子。
   */
  .row.flash { outline: 1px solid var(--accent); outline-offset: -1px; }
  /* 键盘走到的行给个底色，光有 outline 在长列表里不够醒目 */
  .row:focus-visible:not(.active) { background: var(--hover); }
  /*
   * 展开箭头。**必须是 SVG，不能是文字里的 ▸。**
   *
   * 原来是 `<span style="font-size:9px">▸</span>` + `rotate(90deg)`。
   * 盒子确实是垂直居中的（上下各 4.8px），但**字形在自己的 em 盒里本来就偏上**，
   * 一旋转，那点偏移就从"偏上"变成"偏左上"，箭头看着离开了它该在的位置。
   * 加上 9px 的字本来就渲染得糊，两件事叠在一起就是"这里怎么怪怪的"。
   *
   * SVG 的几何是自己说了算的：给一个方盒、内容居中，绕盒心转 90° 前后都对齐。
   */
  .caret {
    flex: none;
    display: grid;
    place-content: center;
    width: 12px;
    height: 12px;
    color: var(--text-faint);
    transition: transform 0.12s ease;
  }
  .caret.open { transform: rotate(90deg); }
  .caret.spacer { visibility: hidden; }
  /* 只有配置类破例给个颜色 —— 改错它的代价最大 */
  /* 字形本身在 FileGlyph 里，这里只管「行被选中/悬停时它跟着提亮」 */
  .row.active :global(.glyph), .row:hover :global(.glyph) { color: var(--text-dim); }
  .row.active :global(.glyph.conf), .row:hover :global(.glyph.conf) { opacity: 1; }
  .name { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  /* 打字定位命中的那几个字：底色不换字色，斜体 / 压暗 / git 色都保得住 */
  .name mark { background: var(--selection-match); color: inherit; border-radius: 2px; }
  .speed {
    position: absolute;
    top: 34px;
    right: 8px;
    z-index: 2;
    padding: 2px 8px;
    font-family: var(--code-font);
    font-size: 12px;
    color: var(--text);
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    box-shadow: var(--shadow-pop);
    pointer-events: none;
  }
  .speed.none { color: var(--lvl-warn); }
  .speed .hint { margin-left: 8px; font-family: inherit; font-size: 11px; color: var(--text-faint); }
  /*
   * 生成物目录（issue #13）。**压暗，不隐藏。**
   *
   * 原来它们根本不返回 —— 一个真叫 `build/` 的源码目录（CMake 项目很常见）
   * 在树里凭空消失，而且没有任何提示。「名字叫 build」只是怀疑不是证据，
   * 那条判据不该由文件树替人做完。
   *
   * 只降不透明度、不换颜色：git 的染色（改了 / 新增 / 冲突）还得读得出来，
   * 换一层灰会把那个信息盖掉。`node_modules` 里也可能有你正在改的补丁。
   *
   * 悬停和选中时恢复全亮 —— 你已经在看它了，这时候再压暗只是碍事。
   */
  .row.gen { opacity: 0.42; }
  .row.gen:hover, .row.gen.active { opacity: 1; }
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
  /* 菜单本体的样式在 ContextMenu.svelte；这条留着是因为输入框和确认框也用它当标题 */
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
  /* 输入框 / 确认框：位置和外观都跟着菜单走，它们是从菜单原地长出来的 */
  .pop {
    position: fixed;
    z-index: 60;
    width: 260px;
    padding: 4px 4px 6px;
    background: var(--elevated);
    border: 1px solid var(--border);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  .pinput {
    width: 100%;
    margin: 2px 0 0;
    padding: 5px 8px;
    background: var(--elevated-hi);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
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
    border-radius: var(--r-sm);
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
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    cursor: default;
  }
  .pbtn:hover { background: var(--hover); color: var(--text); }
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
