<script lang="ts">
  /**
   * 分支浮层，照 IDEA 的分支 popup 排（M9 之二，2026-09-16）：
   *
   * ```
   * [🔍 搜索分支和操作                     ] [↙]
   *  ↙ 更新项目…                        ⇧⌘P
   *  ⊸ 提交…                            ⇧⌘G
   *  ↗ 推送…                            ⌥⌘P
   *  ──
   *  ＋ 新建分支…
   *     检出标签或提交…
   *  ──
   *  ⌄ 最近
   *      ⌄ 📁 feature
   *            ◉ dev-spc  ↑15      origin/feature/dev-spc ›
   *  ⌄ 本地 / ⌄ 远程 / ⌄ 工作树
   * ```
   *
   * 用户拿 IDEA 的截图说「这也啥样就好了」。和上一版比，三处：
   * 1. **动作在浮层顶上**，不在挂件旁边。M9 曾把「↑N 推送」做成挂件旁的胶囊，
   *    用户一眼「好丑」——IDEA 的挂件就是一个名字，动作跟着浮层走。
   * 2. **分支按 `/` 前缀折成文件夹**（`feature/` 底下三条），几十条分支一眼扫得过来。
   * 3. **「最近」一组**：最近切到过的几条（`branches-ops` 每次切完记一条）。
   *
   * 键盘：↑↓ 走遍所有可选行（动作 + 分支 + 工作树），↵ 做主动作（动作执行 / 切分支 /
   * 打开工作树），⇧F10 或单击开一行的菜单。单击不直接切的理由见 `openRowMenuAt`。
   */
  import type { GitBranch, GitWorktree } from "../ipc/commands";
  import { gitBranches, gitWorktrees } from "../ipc/commands";
  import Icon from "../shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import { showInFinder, copyText } from "../shell/pathactions";
  import { byId } from "../state/keymap";
  import { readListPref } from "../state/prefs";
  import { recentKey } from "../state/branches-ops";

  let {
    open = $bindable(false),
    anchor = null,
    repo,
    ahead = 0,
    behind = 0,
    onSwitch,
    onNewBranch,
    onOpenWorktree,
    onNewWorktree,
    onRemoveWorktree,
    onPull,
    onPush,
    onFetch,
    onCommit,
    onMerge,
    onRename,
    onDelete,
  }: {
    open?: boolean;
    /**
     * 挂件的位置（视口坐标，通常传它的 `left` / `bottom`）。
     * 这个浮层是从分支挂件底下掉下来的，不是弹在屏幕中间的对话框（ui.md 十二）。
     * 给 `null` 时退回居中 —— 挂件还没挂上时的兜底，不是另一种用法。
     */
    anchor?: { x: number; y: number } | null;
    repo: string;
    /** 当前分支与上游差多少。这两个值在 `GitStatus` 上，不在 `GitBranch` 上，只能由 App 传 */
    ahead?: number;
    behind?: number;
    onSwitch: (name: string) => void;
    /** `base` 为空 = 从当前 HEAD 分出 */
    onNewBranch: (name: string, base?: string) => void;
    onOpenWorktree: (path: string) => void;
    /** 目录名 + 分支名；分支不存在就新建 */
    onNewWorktree: (dir: string, branch: string) => void;
    onRemoveWorktree: (w: GitWorktree) => void;
    /** 顶上那三条 + 抓取。和菜单栏走的是同一个 `remote`，这里只是多一个入口 */
    onPull: () => void;
    onPush: () => void;
    onFetch: () => void;
    /** 「提交…」= 打开侧边栏的改动页，提交框在那儿 */
    onCommit: () => void;
    /** 把 `ref` 合进当前分支 */
    onMerge: (ref: string) => void;
    onRename: (old: string, next: string) => void;
    /** 不可逆，由上层弹确认条 */
    onDelete: (name: string) => void;
  } = $props();

  let q = $state("");
  let branches = $state<GitBranch[]>([]);
  let trees = $state<GitWorktree[]>([]);
  let loading = $state(false);
  let err = $state("");
  let sel = $state(0);
  let input = $state<HTMLInputElement | null>(null);
  let popEl = $state<HTMLElement | null>(null);
  let listEl = $state<HTMLElement | null>(null);

  /** 新建工作树的两个输入 */
  let wtDir = $state("");
  let wtBranch = $state("");
  /**
   * 四个表单（新建分支 / 重命名 / 检出提交 / 新建工作树），一个输入框 + 两个按钮。
   * 分支名不走 `prompt()`：WKWebView 里那是系统对话框，和整个应用的浮层不是一套。
   */
  let mode = $state<"list" | "newWorktree" | "newBranch" | "rename" | "checkoutRev">("list");
  let formName = $state("");
  let formBase = $state("");
  let formInput = $state<HTMLInputElement | null>(null);
  function openForm(m: "newBranch" | "rename" | "checkoutRev", base = "") {
    formBase = base;
    formName = m === "rename" ? base : "";
    mode = m;
    queueMicrotask(() => {
      formInput?.focus();
      if (m === "rename") formInput?.select();
    });
  }
  let formOk = $derived.by(() => {
    const n = formName.trim();
    if (!n || /\s/.test(n)) return false;
    if (mode === "checkoutRev") return true;
    if (mode === "rename" && n === formBase) return false;
    return !branches.some((b) => !b.isRemote && b.name === n);
  });
  function submitForm() {
    if (!formOk) return;
    const n = formName.trim();
    if (mode === "newBranch") onNewBranch(n, formBase);
    else if (mode === "rename") onRename(formBase, n);
    else onSwitch(n);
    open = false;
  }
  function submitWorktree() {
    if (!wtDir.trim()) return;
    // 目标分支留空就用目录名当分支名 —— 十有八九就是想要的
    const dir = wtDir.trim();
    const br = wtBranch.trim() || dir.slice(dir.lastIndexOf("/") + 1);
    onNewWorktree(dir, br);
    open = false;
  }

  /*
   * 位置钳进视口。抄 `ContextMenu` 那一套：**改的是 DOM 而不是 props**。
   * 依赖里要带上行数和 `mode`：过滤一改高度就变，钳完的 top 会过时。
   */
  $effect(() => {
    const e = popEl;
    const a = anchor;
    if (!e || !a) return;
    void [rows.length, mode, loading];
    const r = e.getBoundingClientRect();
    const pad = 8;
    e.style.left = `${Math.max(pad, Math.min(a.x, window.innerWidth - r.width - pad))}px`;
    e.style.top = `${Math.max(pad, Math.min(a.y, window.innerHeight - r.height - pad))}px`;
  });

  // 每次打开都重新拉：分支和工作树在终端里随时会变，缓存只会骗人
  $effect(() => {
    if (!open) return;
    q = "";
    sel = 0;
    mode = "list";
    err = "";
    rowMenu = null;
    void (async () => {
      loading = true;
      try {
        [branches, trees] = await Promise.all([gitBranches(repo), gitWorktrees(repo)]);
      } catch (e) {
        err = String(e);
      } finally {
        loading = false;
      }
    })();
    queueMicrotask(() => input?.focus());
  });

  /** 当前分支名。给「从谁分出」和菜单里的措辞用 */
  let cur = $derived(branches.find((b) => b.isHead && !b.isRemote)?.name ?? "");

  // ── 行 ──────────────────────────────────────────────────────────

  /**
   * 列表里的一行。`sec` / `folder` / `divider` 是结构（不可选），其余可选。
   * `depth` 决定缩进：分组 0，文件夹 1，文件夹里的分支 2，直接在分组里的分支 1。
   */
  type Row =
    | { kind: "action"; id: string; label: string; icon: "pull" | "commit" | "push" | "plus" | "tag" | null; accel: string; run: () => void; depth: 0 }
    | { kind: "divider" }
    | { kind: "sec"; id: string; label: string; open: boolean }
    | { kind: "folder"; id: string; label: string; open: boolean; depth: number }
    | { kind: "branch"; label: string; branch: GitBranch; current: boolean; depth: number; group: "recent" | "local" | "remote" }
    | { kind: "worktree"; label: string; tree: GitWorktree; current: boolean; depth: number };

  /** 折起来的分组 / 文件夹。只在这一次打开里记，关了就忘 —— 它是这一刻的事 */
  let collapsed = $state(new Set<string>());
  function toggle(id: string) {
    const next = new Set(collapsed);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    collapsed = next;
  }

  const ACCEL = (id: string) => byId(id)?.accel ?? "";

  let rows = $derived.by((): Row[] => {
    const k = q.trim().toLowerCase();
    const hit = (s: string) => !k || s.toLowerCase().includes(k);
    const out: Row[] = [];

    // ── 动作：照 IDEA 的顺序。搜索时按标签过滤，和分支一起搜
    // 输入了名字但没有同名分支：「新建分支」带上名字常驻在结果里；有同名的就别抢 ↵
    const exact = !!k && branches.some((b) => b.name.toLowerCase() === k || b.name.toLowerCase().endsWith(`/${k}`));
    const newLabel = k && !exact ? `新建分支「${q.trim()}」…` : "新建分支…";
    const actions: Row[] = [
      { kind: "action", id: "pull", label: "更新项目…", icon: "pull", accel: ACCEL("git-pull"), run: onPull, depth: 0 },
      { kind: "action", id: "commit", label: "提交…", icon: "commit", accel: ACCEL("git-changes"), run: onCommit, depth: 0 },
      { kind: "action", id: "push", label: "推送…", icon: "push", accel: ACCEL("git-push"), run: onPush, depth: 0 },
      { kind: "divider" },
      // 输入了名字但没有同名分支：直接把名字带进表单，省一次重打
      { kind: "action", id: "new", label: newLabel, icon: "plus", accel: "", run: () => { const typed = q.trim(); openForm("newBranch", cur); if (k) formName = typed; }, depth: 0 },
      { kind: "action", id: "rev", label: "检出标签或提交…", icon: "tag", accel: "", run: () => openForm("checkoutRev"), depth: 0 },
      { kind: "action", id: "wt", label: "新建工作树…", icon: null, accel: "", run: () => { wtBranch = q.trim(); wtDir = ""; mode = "newWorktree"; }, depth: 0 },
    ];
    for (const a of actions) {
      if (a.kind === "divider") {
        if (out.length && out[out.length - 1].kind !== "divider") out.push(a);
      } else if (a.kind === "action" && (hit(a.label) || (a.id === "new" && k && !exact))) out.push(a);
    }
    if (out.length && out[out.length - 1].kind === "divider") out.pop();
    if (out.length) out.push({ kind: "divider" });

    /** 一组分支按 `/` 第一段折成文件夹（只折一层，够用；IDEA 折到底但那要展开好几下） */
    const section = (id: string, label: string, list: GitBranch[], group: "recent" | "local" | "remote") => {
      const shown = list.filter((b) => hit(b.name));
      if (shown.length === 0) return;
      const isOpen = !collapsed.has(id);
      out.push({ kind: "sec", id, label, open: isOpen });
      if (!isOpen) return;
      const folders = new Map<string, GitBranch[]>();
      const loose: GitBranch[] = [];
      for (const b of shown) {
        // 远程分支的第一段是远程名（origin/），那不是文件夹，跳过它再看
        const name = group === "remote" ? b.name.slice(b.name.indexOf("/") + 1) : b.name;
        const i = name.indexOf("/");
        if (i > 0) {
          const f = name.slice(0, i);
          const arr = folders.get(f) ?? [];
          arr.push(b);
          folders.set(f, arr);
        } else loose.push(b);
      }
      const row = (b: GitBranch, depth: number): Row => ({ kind: "branch", label: b.name, branch: b, current: b.isHead && !b.isRemote, depth, group });
      for (const b of loose) out.push(row(b, 1));
      for (const [f, arr] of folders) {
        const fid = `${id}/${f}`;
        const fopen = !collapsed.has(fid) || !!k; // 搜索时文件夹强制展开，不然命中的行看不见
        out.push({ kind: "folder", id: fid, label: f, open: fopen, depth: 1 });
        if (fopen) for (const b of arr) out.push(row(b, 2));
      }
    };

    const local = branches.filter((b) => !b.isRemote);
    const remote = branches.filter((b) => b.isRemote);
    // 最近：存的是名字，按名字在本地分支里找；改名 / 删掉的自动消失。当前分支永远在最前
    const recentNames = readListPref(recentKey(repo));
    const recent = [cur, ...recentNames.filter((n) => n !== cur)]
      .map((n) => local.find((b) => b.name === n))
      .filter((b): b is GitBranch => !!b);
    if (recent.length) section("recent", "最近", recent, "recent");
    section("local", "本地", local, "local");
    section("remote", "远程", remote, "remote");

    const wts = trees.filter((w) => hit(w.branch || w.path));
    if (wts.length) {
      const isOpen = !collapsed.has("wt");
      out.push({ kind: "sec", id: "wt", label: "工作树", open: isOpen });
      if (isOpen) for (const w of wts) out.push({ kind: "worktree", label: w.branch || `(${w.sha})`, tree: w, current: w.current, depth: 1 });
    }
    return out;
  });

  /** 可选的行的下标（跳过分组头、文件夹、分隔线） */
  let selectable = $derived(rows.map((r, i) => (r.kind === "action" || r.kind === "branch" || r.kind === "worktree" ? i : -1)).filter((i) => i >= 0));

  // 过滤变了就把选中项拉回顶部，否则会停在一个已经不存在的位置上
  $effect(() => {
    q;
    sel = 0;
  });
  // 选中项挪到视口里
  $effect(() => {
    const i = selectable[sel];
    if (i === undefined || !listEl) return;
    listEl.querySelector<HTMLElement>(`[data-i="${i}"]`)?.scrollIntoView({ block: "nearest" });
  });

  /** `/Users/shaopc/` 那一截缩成 `~/`：工作树都在同一个用户下 */
  function shortPath(p: string) {
    const m = /^\/Users\/[^/]+\//.exec(p);
    return m ? `~/${p.slice(m[0].length)}` : p;
  }
  /** 文件夹里的分支只显示文件夹后面那截 */
  function leaf(r: Extract<Row, { kind: "branch" }>): string {
    if (r.depth < 2) return r.label;
    const name = r.group === "remote" ? r.label.slice(r.label.indexOf("/") + 1) : r.label;
    return name.slice(name.indexOf("/") + 1);
  }

  // ── 行菜单 ──────────────────────────────────────────────────────

  /**
   * 单击一行开的是它的菜单，↵ 直接切（M9）。
   *
   * 切分支不是打开一个文件：错了是几千个文件被检出、所有标签重读。而列表是 hover
   * 就移动选中项的，鼠标划过去顺手一点就切了 —— 所以鼠标路径多一层。键盘没有「划过」，
   * ↵ 之前人已经打了几个字把目标筛出来，直接切。IDEA 正是这么分的。
   *
   * 锚点钉在行的左下角，不跟鼠标：菜单讲的是「这一行」的事。
   */
  let rowMenu = $state<{ x: number; y: number; row: Row } | null>(null);
  let rowEls = $state<HTMLElement[]>([]);
  function openRowMenuAt(i: number) {
    const r = rows[i];
    const el = rowEls[i];
    if (!r || !el || (r.kind !== "branch" && r.kind !== "worktree")) return;
    const b = el.getBoundingClientRect();
    rowMenu = { x: b.left + 24, y: b.bottom - 4, row: r };
  }

  let rowMenuItems = $derived.by((): MenuItem[] => {
    const r = rowMenu?.row;
    if (!r) return [];
    const close = () => (open = false);
    if (r.kind === "worktree") {
      const w = r.tree;
      const out: MenuItem[] = [];
      if (!r.current) out.push({ label: "打开", run: () => { onOpenWorktree(w.path); close(); } });
      out.push({ label: "在 Finder 中显示", run: () => void showInFinder(w.path) });
      out.push({ label: "复制路径", run: () => void copyText(w.path, "路径") });
      // 移除**会删掉那个目录**：只在菜单里、带 danger、带省略号
      if (!r.current && !w.bare) out.push({ label: "移除工作树…", danger: true, sep: true, run: () => { onRemoveWorktree(w); close(); } });
      return out;
    }
    if (r.kind !== "branch") return [];
    const name = r.label;
    const wt = (): MenuItem => ({ label: `从 '${name}' 新建工作树…`, run: () => { wtBranch = name; wtDir = ""; mode = "newWorktree"; } });
    const copy = (): MenuItem => ({ label: "复制分支名", sep: true, run: () => void copyText(name, "分支名") });
    // 照 IDEA 的子菜单顺序：新建 / 工作树 ─ 更新 / 推送 ─ 重命名；别的分支前面多一条检出、后面多一条删除
    if (r.current) {
      return [
        { label: `从 '${name}' 新建分支…`, run: () => openForm("newBranch", name) },
        wt(),
        { label: "更新", sep: true, run: () => { onPull(); close(); }, disabled: !r.branch.upstream },
        { label: "推送…", run: () => { onPush(); close(); } },
        { label: "抓取远程", run: () => { onFetch(); close(); } },
        { label: "重命名…", sep: true, run: () => openForm("rename", name) },
        copy(),
      ];
    }
    if (r.branch.isRemote) {
      const short = name.split("/").slice(1).join("/") || name;
      return [
        // 说清它要做什么：不是「切过去」，是**建一个本地分支**再切
        { label: `检出为本地分支 '${short}'`, run: () => { onSwitch(name); close(); } },
        { label: `从 '${name}' 新建分支…`, sep: true, run: () => openForm("newBranch", name) },
        { label: `合并到 '${cur || "当前分支"}'`, run: () => { onMerge(name); close(); }, disabled: !cur },
        copy(),
      ];
    }
    return [
      { label: "检出", run: () => { onSwitch(name); close(); } },
      { label: `从 '${name}' 新建分支…`, sep: true, run: () => openForm("newBranch", name) },
      wt(),
      { label: `合并到 '${cur || "当前分支"}'`, sep: true, run: () => { onMerge(name); close(); }, disabled: !cur },
      { label: "重命名…", sep: true, run: () => openForm("rename", name) },
      copy(),
      { label: "删除…", danger: true, sep: true, run: () => { onDelete(name); close(); } },
    ];
  });

  /** 单击：动作执行；分支 / 工作树开菜单；分组头 / 文件夹折叠 */
  function click(i: number) {
    const r = rows[i];
    if (!r) return;
    if (r.kind === "sec" || r.kind === "folder") return toggle(r.id);
    const si = selectable.indexOf(i);
    if (si >= 0) sel = si;
    if (r.kind === "action") {
      r.run();
      // 开表单的三条留在浮层里，其余做完就关
      if (r.id !== "new" && r.id !== "rev" && r.id !== "wt") open = false;
    } else openRowMenuAt(i);
  }

  /** ↵：主动作。当前分支 / 当前工作树没有主动作，退回开菜单 */
  function enter() {
    const i = selectable[sel];
    const r = rows[i];
    if (!r) return;
    if (r.kind === "action") return click(i);
    if (r.kind === "branch") {
      if (r.current) return openRowMenuAt(i);
      onSwitch(r.label);
      open = false;
    } else if (r.kind === "worktree") {
      if (r.current) return openRowMenuAt(i);
      onOpenWorktree(r.tree.path);
      open = false;
    }
  }

  function onKey(e: KeyboardEvent) {
    /*
     * 行菜单开着的时候键盘归它。正常情况下这里轮不到（菜单拿着焦点、自己 stopPropagation），
     * 但焦点万一不在菜单上，直接 return 会让 Esc 变成死键 —— 兜底关掉它。
     */
    if (rowMenu) {
      if (e.key === "Escape") {
        e.preventDefault();
        rowMenu = null;
      }
      return;
    }
    if (e.key === "Escape") {
      e.preventDefault();
      if (mode !== "list") mode = "list";
      else open = false;
      return;
    }
    if (mode !== "list") return;
    const n = Math.max(1, selectable.length);
    if (e.key === "ArrowDown") {
      e.preventDefault();
      sel = (sel + 1) % n;
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      sel = (sel - 1 + n) % n;
    } else if (e.key === "Enter") {
      e.preventDefault();
      enter();
    } else if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
      e.preventDefault();
      const i = selectable[sel];
      if (i !== undefined) openRowMenuAt(i);
    }
  }

  /** 脚栏 ↵ 的说明按当前项变 */
  let enterLabel = $derived.by(() => {
    const r = rows[selectable[sel]];
    if (!r) return "确认";
    if (r.kind === "action") return "执行";
    if (r.kind === "worktree") return r.current ? "操作…" : "打开";
    if (r.kind === "branch") return r.current ? "操作…" : r.branch.isRemote ? "检出" : "切换";
    return "确认";
  });

  function rowKey(r: Row, i: number): string {
    switch (r.kind) {
      case "divider": return `d${i}`;
      case "sec": case "folder": case "action": return `${r.kind}:${r.id}`;
      // 同一条分支会在「最近」和「本地」各出现一次，key 要带上组 —— 重了 Svelte 整块不渲染
      case "branch": return `branch:${r.group}:${r.label}`;
      default: return `${r.kind}:${r.tree.path}`;
    }
  }
</script>

<svelte:window onkeydown={open ? onKey : undefined} />

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <!-- scrim 不压暗：从挂件底下掉出来的浮层，不是要你先处理完的对话框；它只干一件事 —— 点外面关掉 -->
  <div class="scrim" onclick={() => (open = false)}></div>
  <div class="popup" class:anchored={!!anchor} bind:this={popEl} role="dialog" aria-label="分支与工作树">
    {#if mode === "list"}
      <div class="search">
        <span class="sic"><Icon name="search" size={14} /></span>
        <input bind:this={input} bind:value={q} placeholder="搜索分支和操作" spellcheck="false" autocomplete="off" />
        <!-- IDEA 搜索框右边那个 ↙ 是 Fetch。抓取只读、不动工作区，放在这儿随手点 -->
        <button class="sbtn" onclick={() => { onFetch(); open = false; }} title="抓取远程（fetch --prune）" aria-label="抓取远程">
          <Icon name="pull" size={14} />
        </button>
      </div>
      <div class="results" bind:this={listEl}>
        {#if err}
          <div class="none err">{err}</div>
        {:else if loading && branches.length === 0}
          <div class="none">载入中…</div>
        {:else if selectable.length === 0}
          <div class="none">没有匹配</div>
        {/if}
        {#each rows as r, i (rowKey(r, i))}
          {#if r.kind === "divider"}
            <div class="divider"></div>
          {:else if r.kind === "sec" || r.kind === "folder"}
            <button class="hdr" class:folder={r.kind === "folder"} style:padding-left="{14 + (r.kind === "folder" ? r.depth : 0) * 18}px" onclick={() => click(i)} aria-expanded={r.open}>
              <span class="caret" class:closed={!r.open}><Icon name="chevron-right" size={10} /></span>
              {#if r.kind === "folder"}<span class="ic faint"><Icon name="folder" size={14} /></span>{/if}
              <span class="hlabel">{r.label}</span>
            </button>
          {:else}
            {@const i2 = selectable.indexOf(i)}
            <!-- svelte-ignore a11y_no_static_element_interactions -->
            <!-- 右键挂在整行上：「对着这一行右键」是人的直觉 -->
            <div
              class="rowwrap"
              class:on={i2 === sel}
              data-i={i}
              role="group"
              bind:this={rowEls[i]}
              oncontextmenu={(e) => { e.preventDefault(); e.stopPropagation(); if (i2 >= 0) sel = i2; openRowMenuAt(i); }}
            >
              <button class="row" style:padding-left="{8 + r.depth * 18}px" onclick={() => click(i)} onmouseenter={() => { if (i2 >= 0) sel = i2; }}>
                {#if r.kind === "action"}
                  <span class="ic act">{#if r.icon}<Icon name={r.icon} size={14} />{/if}</span>
                  <span class="lb ui">{r.label}</span>
                  <span class="gap"></span>
                  {#if r.accel}<span class="key">{r.accel}</span>{/if}
                {:else if r.kind === "branch"}
                  <!-- 每行自己的图标：分组头一滚就看不见了，「origin/dev 是远程的」不该靠记得滚过哪个标题 -->
                  <span class="ic" class:cur={r.current} class:remote={r.branch.isRemote}>
                    {#if r.current}
                      <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.4" aria-hidden="true">
                        <circle cx="8" cy="8" r="3.1" /><circle cx="8" cy="8" r="6.1" opacity="0.45" />
                      </svg>
                    {:else if r.branch.isRemote}
                      <svg viewBox="0 0 16 16" width="14" height="14" fill="none" stroke="currentColor" stroke-width="1.25" aria-hidden="true">
                        <circle cx="8" cy="8" r="5.8" /><path d="M2.4 8h11.2" stroke-linecap="round" />
                        <path d="M8 2.2c1.5 1.7 2.3 3.7 2.3 5.8S9.5 12.1 8 13.8C6.5 12.1 5.7 10.1 5.7 8S6.5 3.9 8 2.2z" />
                      </svg>
                    {:else}
                      <Icon name="git" size={14} />
                    {/if}
                  </span>
                  <span class="lb" class:cur={r.current}>{leaf(r)}</span>
                  {#if r.current && ahead}<span class="ab">↑{ahead}</span>{/if}
                  {#if r.current && behind}<span class="ab">↓{behind}</span>{/if}
                  <span class="gap"></span>
                  {#if r.branch.upstream}<span class="up">{r.branch.upstream}</span>{/if}
                  <span class="rm" aria-hidden="true"><Icon name="chevron-right" size={11} /></span>
                {:else}
                  <span class="ic worktree"><Icon name="files" size={14} /></span>
                  <span class="lb" class:cur={r.current}>{r.label}</span>
                  {#if r.current}<span class="now">当前</span>{/if}
                  <span class="gap"></span>
                  <span class="up">{shortPath(r.tree.path)}</span>
                  <span class="rm" aria-hidden="true"><Icon name="chevron-right" size={11} /></span>
                {/if}
              </button>
            </div>
          {/if}
        {/each}
      </div>
      {#if rowMenu}
        <ContextMenu x={rowMenu.x} y={rowMenu.y} title={rowMenu.row.kind === "branch" || rowMenu.row.kind === "worktree" ? rowMenu.row.label : ""} label="分支的操作" items={rowMenuItems} onclose={() => (rowMenu = null)} />
      {/if}
      <!-- 脚栏和随处搜索长一样（ui.md 五）：键位是「忘了才看」的东西 -->
      <div class="foot">
        <span><kbd>↑↓</kbd> 选择</span>
        <span><kbd>↵</kbd> {enterLabel}</span>
        <span><kbd>⇧F10</kbd> 操作</span>
        <span class="gap"></span>
        <span><kbd>esc</kbd> 关闭</span>
      </div>
    {:else if mode === "newWorktree"}
      <div class="form">
        <div class="ftitle">新建工作树</div>
        <p class="fdesc">工作树是同一个仓库的第二个检出目录 —— 可以在不打断当前改动的前提下，把另一个分支同时摊开在磁盘上。</p>
        <label>
          <span>目录</span>
          <input class="fi" bind:value={wtDir} placeholder="../lite-ide-hotfix 或绝对路径" spellcheck="false" onkeydown={(e) => e.key === "Enter" && submitWorktree()} />
        </label>
        <label>
          <span>分支</span>
          <input class="fi" bind:value={wtBranch} placeholder="留空则用目录名，分支不存在就新建" spellcheck="false" onkeydown={(e) => e.key === "Enter" && submitWorktree()} />
        </label>
        <div class="frow">
          <button class="btn" onclick={() => (mode = "list")}>返回</button>
          <span class="gap"></span>
          <button class="btn primary" disabled={!wtDir.trim()} onclick={submitWorktree}>创建并打开</button>
        </div>
      </div>
    {:else}
      <div class="form">
        <div class="ftitle">
          {#if mode === "rename"}重命名 '{formBase}'
          {:else if mode === "newBranch"}{formBase ? `从 '${formBase}' 新建分支` : "新建分支"}
          {:else}检出标签或提交{/if}
        </div>
        {#if mode === "rename"}
          <p class="fdesc">只改本地的名字；上游跟踪配置会跟着走，远程那条不动。</p>
        {:else if mode === "newBranch"}
          <p class="fdesc">新分支从 {formBase || "当前 HEAD"} 的最新提交分出，建好之后直接切过去。</p>
        {:else}
          <p class="fdesc">标签名或提交 sha。检出之后是游离状态（不在任何分支上），要改东西先从它新建分支。</p>
        {/if}
        <label>
          <span>{mode === "checkoutRev" ? "引用" : "名字"}</span>
          <input class="fi" bind:this={formInput} bind:value={formName} placeholder={mode === "checkoutRev" ? "v1.0.0 或 b6176f3" : "feature/xxx"} spellcheck="false" onkeydown={(e) => e.key === "Enter" && submitForm()} />
        </label>
        <div class="frow">
          <button class="btn" onclick={() => (mode = "list")}>返回</button>
          <span class="gap"></span>
          <button class="btn primary" disabled={!formOk} onclick={submitForm}>{mode === "rename" ? "重命名" : mode === "newBranch" ? "新建并切换" : "检出"}</button>
        </div>
      </div>
    {/if}
  </div>
{/if}

<style>
  .scrim { position: fixed; inset: 0; z-index: 40; }
  .popup {
    position: fixed;
    top: 14vh;
    left: 50%;
    transform: translateX(-50%);
    width: min(620px, 88vw);
    max-height: 66vh;
    display: flex;
    flex-direction: column;
    background: var(--elevated);
    border: var(--island-border);
    border-radius: var(--r-md);
    box-shadow: var(--shadow-pop), inset 0 0 0 0.5px rgba(255, 255, 255, 0.06);
    z-index: 41;
    overflow: hidden;
  }
  /* 挂在挂件底下时：不居中、窄一档。left/top 由 effect 写进 style，这里只关掉居中那套 */
  .popup.anchored {
    top: 0;
    left: 0;
    transform: none;
    width: min(520px, calc(100vw - 16px));
    max-height: min(72vh, calc(100vh - 60px));
  }

  /* 搜索框：照 IDEA —— 圆角描边的一格，不是通栏输入行；右边挂一个 fetch */
  .search {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    margin: 10px 10px 4px;
    height: 32px;
    padding: 0 6px 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    background: var(--elevated-hi);
  }
  .search:focus-within { border-color: var(--accent); }
  .sic { flex: none; display: flex; color: var(--text-faint); }
  .search input {
    flex: 1;
    min-width: 0;
    border: none;
    background: transparent;
    color: var(--text);
    font-family: var(--ui-font);
    font-size: 13px;
    outline: none;
  }
  .search input::placeholder { color: var(--text-faint); }
  .sbtn {
    flex: none;
    display: grid;
    place-content: center;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: var(--r-sm);
    background: transparent;
    color: var(--text-faint);
    cursor: default;
  }
  .sbtn:hover { background: var(--hover); color: var(--text); }

  .results { overflow-y: auto; padding: 4px 0 6px; }
  .none { padding: 18px 14px; color: var(--text-faint); font-size: 12.5px; text-align: center; }
  .none.err { color: var(--lvl-error); font-family: var(--code-font); text-align: left; }
  .divider { height: 1px; background: var(--border-soft); margin: 5px 12px; }

  /* 分组头 / 文件夹：带折叠箭头的一行，照 IDEA 的树。分组头吸顶 */
  .hdr {
    display: flex;
    align-items: center;
    gap: 5px;
    width: 100%;
    height: 24px;
    border: none;
    background: var(--elevated);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12px;
    text-align: left;
    cursor: default;
    user-select: none;
  }
  .hdr:not(.folder) { position: sticky; top: 0; z-index: 1; font-weight: 600; }
  .hdr:hover { color: var(--text); }
  .hdr .caret { display: inline-flex; color: var(--text-faint); transform: rotate(90deg); transition: transform 0.12s; }
  .hdr .caret.closed { transform: none; }
  .hdr .hlabel { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }

  /* 当前项是内缩圆角块，和随处搜索、文件树同一套 */
  .rowwrap { display: flex; align-items: center; margin: 0 6px; border-radius: var(--r-sm); }
  .rowwrap.on { background: var(--selected); }
  .row {
    display: flex;
    align-items: center;
    gap: 7px;
    flex: 1;
    min-width: 0;
    height: 26px;
    padding: 0 6px 0 8px;
    background: transparent;
    border: none;
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: 12.5px;
    text-align: left;
    cursor: default;
    white-space: nowrap;
  }
  .rowwrap.on .row { color: var(--text); }
  .row .gap { flex: 1; min-width: 8px; }
  .lb { flex: none; max-width: 50%; overflow: hidden; text-overflow: ellipsis; font-family: var(--code-font); font-size: 12px; }
  .lb.ui { font-family: var(--ui-font); font-size: 12.5px; }
  .lb.cur { color: var(--text); font-weight: 600; }
  /* 右边那列：上游 / 工作树路径。IDEA 就是这么把 origin/… 靠右放的 */
  .up { flex: none; max-width: 42%; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-family: var(--code-font); font-size: 11px; }
  .key { flex: none; color: var(--text-faint); font-family: var(--code-font); font-size: 11px; }
  .now { flex: none; font-size: 10px; padding: 0 5px; border-radius: var(--r-sm); background: var(--elevated-hi); color: var(--accent); }
  .ab { flex: none; font-family: var(--code-font); font-size: 10.5px; color: var(--accent); }
  /* 行尾的 ›：按下去还有一层。常驻但压暗，选中那行才提亮 */
  .rm { flex: none; display: flex; color: var(--text-faint); opacity: 0.45; }
  .rowwrap:hover .rm, .rowwrap.on .rm { opacity: 1; }

  .ic { flex: none; display: flex; width: 16px; justify-content: center; color: var(--git-modified); }
  .ic.faint, .ic.act { color: var(--text-faint); }
  .rowwrap.on .ic.act { color: var(--text); }
  .ic.remote { color: var(--lvl-info); }
  .ic.worktree { color: var(--git-renamed); }
  .ic.cur { color: var(--git-added); }

  /* 与随处搜索的脚栏一模一样 */
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
  kbd { font-family: var(--code-font); font-size: 10px; background: var(--hover); border-radius: 4px; padding: 1px 5px; margin-right: 3px; }

  .form { padding: 16px 18px 14px; display: flex; flex-direction: column; gap: 10px; }
  .ftitle { font-size: 14px; color: var(--text); }
  .fdesc { margin: 0; font-size: 11.5px; line-height: 1.6; color: var(--text-faint); }
  .form label { display: flex; align-items: center; gap: 10px; font-size: 12px; color: var(--text-dim); }
  .form label > span { flex: none; width: 34px; }
  .fi {
    flex: 1;
    background: var(--elevated-hi);
    border: 1px solid var(--border);
    border-radius: var(--r-sm);
    color: var(--text);
    font-family: var(--code-font);
    font-size: 12px;
    padding: 5px 8px;
    outline: none;
  }
  .fi:focus { border-color: var(--accent); }
  .frow { display: flex; align-items: center; gap: 8px; margin-top: 4px; }
  .frow .gap { flex: 1; }
</style>
