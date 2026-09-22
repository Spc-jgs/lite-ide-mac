<script lang="ts">
  /**
   * 侧边栏的「草稿」视图（issue #40）：草稿目录里有什么，最近的在上。
   *
   * 为什么不是「把草稿目录当项目根打开」（原来「打开草稿目录」那条路）：
   * 那会切走当前项目 —— 翻一条笔记的代价是换工作区，多数人选不翻。
   * 这里点一条只是**叠一个标签**，文件树、git、终端都还在原地。
   *
   * 长相照文件树：30px 的头 + 内缩圆角行（ui.md 第一条）。每行两段：
   * 上面是第一行摘要（没有就显示文件名），下面是「文件名 · 多久之前」——
   * 翻回来时唯一记得的线索就是「大概什么时候记的、记了什么」。
   *
   * 按需加载：它和文件树一样只在侧边栏露出来时才需要。
   */
  import Icon from "./Icon.svelte";
  import ContextMenu, { type MenuItem } from "./ContextMenu.svelte";
  import { scratches } from "../state/scratches.svelte";
  import { project } from "../state/project.svelte";
  import { nav } from "../state/nav.svelte";
  import { notify } from "../state/notify.svelte";
  import { ago } from "../state/ago";
  import { projectName } from "../state/crumbs";
  import { parseAt } from "../state/frontmatter";
  import { probePath, type ScratchEntry, type ScratchAnchor } from "../ipc/commands";

  let {
    activePath,
    onOpen,
    onNew,
    onTrash,
    onReveal,
    onRename,
    onSaveAs,
  }: {
    activePath: string;
    /** `keep` 为真是双击 —— 保留而不是预览，和文件树同一套约定 */
    onOpen: (path: string, keep: boolean) => void;
    onNew: () => void;
    onTrash: (path: string) => void;
    onReveal: (path: string) => void;
    /** 改盘上的文件名（主干，后缀那边补）；返回成没成 —— 没成输入框留着 */
    onRename: (path: string, stem: string) => Promise<boolean>;
    /** 「另存为…」：把这份草稿搬去别处当真文件 */
    onSaveAs: (path: string) => void;
  } = $props();

  /**
   * 行内改名：把那一行的第一行换成输入框，回车提交、Esc 取消。
   * 不弹框：名字就在这一行上，改名就该在这一行上改。
   */
  let renaming = $state<{ path: string; stem: string; busy: boolean } | null>(null);
  let renameInput = $state<HTMLInputElement | null>(null);

  function startRename(row: ScratchEntry) {
    renaming = { path: row.path, stem: row.name.replace(/\.md$/i, ""), busy: false };
    queueMicrotask(() => {
      renameInput?.focus();
      renameInput?.select();
    });
  }
  async function submitRename() {
    const r = renaming;
    if (!r || r.busy) return;
    renaming = { ...r, busy: true };
    if (await onRename(r.path, r.stem)) renaming = null;
    else {
      renaming = { ...r, busy: false };
      renameInput?.focus();
    }
  }

  // 露出来就拉一次；之后由写盘的那几处自己叫 refresh
  $effect(() => {
    void scratches.refresh();
  });

  let menu = $state<{ x: number; y: number; row: ScratchEntry } | null>(null);
  let menuFrom: HTMLElement | null = null;

  function openMenu(e: MouseEvent | { clientX: number; clientY: number }, row: ScratchEntry, from: HTMLElement) {
    menuFrom = from;
    menu = { x: e.clientX, y: e.clientY, row };
  }
  function closeMenu(refocus: boolean) {
    menu = null;
    if (refocus) menuFrom?.focus();
  }

  const items = $derived.by<MenuItem[]>(() => {
    const row = menu?.row;
    if (!row) return [];
    return [
      { label: "打开", run: () => onOpen(row.path, true) },
      { label: "重命名", sep: true, run: () => startRename(row) },
      { label: "另存为…", run: () => onSaveAs(row.path) },
      { label: "在 Finder 中显示", sep: true, run: () => onReveal(row.path) },
      { label: "移到废纸篓", danger: true, sep: true, run: () => onTrash(row.path) },
    ];
  });

  /**
   * 按项目分组（M10）：开着项目时，这个项目的草稿在上、其余折进「其他」。
   * **是排序不是过滤** —— 切了项目还想翻上一个项目的笔记是常事。
   * 没开项目、或者一条都不属于当前项目时，就是一张平铺的表。
   */
  let groups = $derived.by(() => {
    const root = project.root;
    const all = scratches.list;
    if (!root) return [{ label: "", rows: all, others: false }];
    const mine = all.filter((r) => r.anchor?.project === root);
    if (mine.length === 0) return [{ label: "", rows: all, others: false }];
    const rest = all.filter((r) => r.anchor?.project !== root);
    const out = [{ label: projectName(root), rows: mine, others: false }];
    if (rest.length) out.push({ label: "其他", rows: rest, others: true });
    return out;
  });
  /** 「其他」组默认折着：它是「别的项目的」，翻的时候才展开 */
  let othersOpen = $state(false);

  /** chip 上印什么：本项目的省掉项目名（分组头已经说了），其他组的带上 */
  function chipText(a: ScratchAnchor, inOthers: boolean): string {
    const parts: string[] = [];
    if (inOthers && a.project) parts.push(projectName(a.project));
    if (a.branch) parts.push(a.branch);
    if (a.at) parts.push(a.at.slice(a.at.lastIndexOf("/") + 1));
    return parts.join(" · ");
  }

  /**
   * 点 chip 跳回写这条时看的那一行。分支不同照跳（行号可能漂，先认了）；
   * 文件不在了要说一句，不能静默失败。
   */
  async function jump(a: ScratchAnchor) {
    const at = parseAt(a.at);
    if (!at || !a.project) return;
    const full = `${a.project}/${at.path}`;
    if (!(await probePath(full).catch(() => null))) {
      notify.fail(`${at.path} 已不在${a.branch ? ` ${a.branch} 上` : ""}`, 3200);
      return;
    }
    await nav.openAt(full, at.line);
  }

  const STAMP_NAME = /^(\d{4})-(\d{2})-(\d{2}) (\d{2})(\d{2})(?:-(\d+))?\.md$/;
  /** 标签里显示的名字：`2026-09-10 1644.md` → `09-10 16:44` */
  function shortName(name: string): string {
    const m = STAMP_NAME.exec(name);
    if (!m) return name;
    return `${m[2]}-${m[3]} ${m[4]}:${m[5]}${m[6] ? ` (${m[6]})` : ""}`;
  }
  /** 人起过名的（不是时间戳）：有 chip 时第二行也得把名字露出来，不然改了名看不见改成了什么 */
  const named = (name: string) => !STAMP_NAME.test(name);
</script>

<div class="scratch">
  <div class="head">
    <span class="title">草稿</span>
    <span class="gap"></span>
    <button class="ibtn" onclick={onNew} title="新建草稿 ⌘N" aria-label="新建草稿">
      <Icon name="plus" size={14} />
    </button>
  </div>
  <div class="list">
    {#if scratches.loaded && scratches.list.length === 0}
      <!-- 空态要给下一步，不是给句号（ui.md 第六条） -->
      <div class="empty">还没有草稿 —— ⌘N 记第一条</div>
    {/if}
    {#each groups as g (g.label)}
      {#if g.label}
        <button class="sec" class:closed={g.others && !othersOpen} onclick={() => g.others && (othersOpen = !othersOpen)} disabled={!g.others}>
          {#if g.others}<span class="caret"><Icon name="chevron-right" size={10} /></span>{/if}
          <span class="sname">{g.label}</span>
          <span class="cnt">{g.rows.length}</span>
        </button>
      {/if}
      {#if !g.others || othersOpen}
        {#each g.rows as row (row.path)}
          <!-- 行和 chip 是两个按钮并排：按钮里不能再套按钮 -->
          <div class="rowwrap" class:active={row.path === activePath}>
            {#if renaming?.path === row.path}
              <!-- 改名中：这一行换成输入框。不能放进下面那个 button 里（button 里套 input 是非法的，焦点会被吃掉） -->
              <div class="row renaming">
                <span class="line rename">
                  <input
                    bind:this={renameInput}
                    bind:value={renaming.stem}
                    disabled={renaming.busy}
                    spellcheck="false"
                    aria-label="新名字"
                    onkeydown={(e) => {
                      if (e.key === "Enter") { e.preventDefault(); void submitRename(); }
                      else if (e.key === "Escape") { e.preventDefault(); renaming = null; }
                      e.stopPropagation();
                    }}
                    onblur={() => { if (renaming && !renaming.busy) renaming = null; }}
                  /><span class="ext">.md</span>
                </span>
                <span class="meta">回车确认 · Esc 取消</span>
              </div>
            {:else}
            <button
              class="row"
              title={row.path}
              onclick={() => onOpen(row.path, false)}
              ondblclick={() => onOpen(row.path, true)}
              oncontextmenu={(e) => {
                e.preventDefault();
                openMenu(e, row, e.currentTarget as HTMLElement);
              }}
              onkeydown={(e) => {
                if ((e.key === "F10" && e.shiftKey) || e.key === "ContextMenu") {
                  e.preventDefault();
                  const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
                  openMenu({ clientX: r.left + 12, clientY: r.bottom - 2 }, row, e.currentTarget as HTMLElement);
                }
              }}
            >
              <span class="line" class:faint={row.firstLine === ""}>{row.firstLine || "（空）"}</span>
              <!-- 有 chip 时第二行只留「多久之前」：日期在文件名里（tooltip 有），chip 更值这块地方 -->
              <span class="meta">{#if row.anchor && chipText(row.anchor, g.others) && !named(row.name)}{row.mtimeMs ? ago(row.mtimeMs / 1000) : shortName(row.name)}{:else}{shortName(row.name)} · {row.mtimeMs ? ago(row.mtimeMs / 1000) : ""}{/if}</span>
            </button>
            {/if}
            {#if row.anchor && chipText(row.anchor, g.others)}
              {@const a = row.anchor}
              <!-- 锚点 chip（M10）：写这条时在哪。有 `at` 才能点（跳回那一行），只有分支的就是个标签 -->
              <button
                class="chip"
                class:link={!!a.at}
                disabled={!a.at}
                title={[a.project, a.branch, a.head, a.at].filter(Boolean).join("\n")}
                onclick={() => void jump(a)}
              >{chipText(a, g.others)}</button>
            {/if}
          </div>
        {/each}
      {/if}
    {/each}
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

<style>
  .scratch {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    overflow: hidden;
  }
  /* 头和文件树的一模一样：同一个侧边栏里两个视图的头长得不一样，人会以为是两种东西 */
  .head {
    flex: none;
    height: 38px; /* 同 FileTree（M8） */
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
  .head .title { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .head .gap { flex: 1; min-width: 6px; }
  /* 头上的工具按钮是 `.ibtn`（app.css） */
  .list { flex: 1; overflow: auto; padding: 4px 6px; }
  .empty { padding: 10px 6px; font-size: 12px; color: var(--text-faint); }
  /* 分组头：吸顶、底色跟外壳走（ui.md 第四条）；「其他」能折 */
  .sec {
    position: sticky;
    top: 0;
    z-index: 1;
    display: flex;
    align-items: center;
    gap: 4px;
    width: 100%;
    padding: 8px 6px 4px;
    background: transparent; /* 在岛里：底由岛画，这里不画（web 壳下 --panel-bg 是实色，画了会盖住岛） */
    border: none;
    color: var(--text-faint);
    font-size: 10.5px;
    letter-spacing: 0.05em;
    text-transform: uppercase;
    text-align: left;
    cursor: default;
  }
  .sec:disabled { color: var(--text-faint); }
  .sec .caret { display: inline-flex; transform: rotate(90deg); transition: transform 0.12s; }
  .sec.closed .caret { transform: none; }
  .sec .sname { overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .sec .cnt { font-family: var(--code-font); background: var(--selected); border-radius: var(--r-sm); padding: 0 5px; font-size: 10px; letter-spacing: 0; }
  /* 两行一条，内缩圆角块；当前项的长相和文件树、标签栏同一套（ui.md 第一条） */
  .rowwrap { position: relative; border-radius: var(--r-sm); }
  .rowwrap:hover { background: var(--hover); }
  .rowwrap.active { background: var(--selected); }
  .rowwrap.active .row { color: var(--text); }
  .row {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    gap: 1px;
    width: 100%;
    padding: 4px 8px;
    border: none;
    border-radius: var(--r-sm); /* M8 */
    background: transparent;
    color: var(--text-dim);
    font: inherit;
    text-align: left;
    cursor: default;
  }
  /*
   * 锚点 chip：贴在第二行右端。字色比 meta 亮一档 —— 它是这条草稿区别于备忘录的
   * 那一样东西；能跳的（有 at）hover 变 accent，只有分支的不变。
   */
  .chip {
    position: absolute;
    right: 6px;
    bottom: 4px;
    max-width: 68%;
    height: 16px;
    padding: 0 5px;
    border: none;
    border-radius: var(--r-sm);
    background: var(--selected);
    color: var(--text-dim);
    font-family: var(--code-font);
    font-size: 10px;
    line-height: 16px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: default;
  }
  .chip:disabled { opacity: 0.8; }
  .chip.link:hover { background: var(--pressed); color: var(--accent); }
  .chip:focus-visible { outline: 1px solid var(--accent); outline-offset: 1px; }
  /* chip 占了右下角，meta 那行给它让位 */
  .rowwrap:has(.chip) .row .meta { padding-right: 68%; }
  .row .line {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
    color: var(--text);
  }
  .row .line.faint { color: var(--text-faint); }
  .row .line.rename { display: flex; align-items: center; gap: 2px; }
  .row .line.rename input {
    flex: 1;
    min-width: 0;
    height: 18px;
    padding: 0 4px;
    background: var(--elevated);
    border: 1px solid var(--accent);
    border-radius: var(--r-sm);
    color: var(--text);
    font: inherit;
    outline: none;
  }
  .row .line.rename .ext { color: var(--text-faint); }
  .row .meta {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 11px;
    color: var(--text-faint);
    font-family: var(--code-font);
  }
</style>
