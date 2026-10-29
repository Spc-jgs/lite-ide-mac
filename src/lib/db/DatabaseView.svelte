<script lang="ts">
  import HideSidebar from "../shell/HideSidebar.svelte";
  /**
   * 数据库工具窗（docs/DATABASE.md DB-1），照 IDEA 的 Database 工具窗：
   *
   * ```
   * 订单库（本地）            连接（双击编辑）
   *   shop                    库 / schema（MySQL 是库，PostgreSQL / H2 是 schema）
   *     orders                表 / 视图
   *       id   BIGINT · 主键  列
   * ```
   *
   * 每一层展开时才问（`db_meta`，连接没开的先开）：库多、表多的时候不一次拉全。连接配置和密码的存法见 db.rs。
   * 「打开查询控制台」开的是这个连接的 `.sql`（DB-2，editor/sql-console.ts）；还没有快捷键
   */
  import Icon from "../shell/Icon.svelte";
  import type { IconName } from "../shell/Icon.svelte";
  import { imeKey } from "../shell/ime";
  import type { DbConn, DbNode } from "../ipc/commands";
  import { dbClose, dbConsole, dbIdeaList, dbList, dbMeta } from "../ipc/db";
  import type { DbIdeaList } from "../ipc/commands";
  import DbImportDialog from "./DbImportDialog.svelte";
  import { notify } from "../state/notify.svelte";
  import { tabflow } from "../state/tabflow.svelte";
  import { openModal } from "../settings/modal-host";
  import DbConnDialog from "./DbConnDialog.svelte";

  let { root }: { root: string } = $props();

  let conns = $state<DbConn[] | null>(null);
  let error = $state("");
  /** 每个展开过的节点的孩子：还在问是 "wait"，问不到是一句话 */
  let kids = $state<Record<string, DbNode[] | "wait" | { error: string }>>({});
  let open = $state(new Set<string>());
  let sel = $state("");
  let treeEl = $state<HTMLDivElement | null>(null);

  /** 项目里 IDEA 存的数据源（DB-4）：有才在头上放「从 IDEA 导入」 */
  let idea = $state<DbIdeaList | null>(null);

  async function load() {
    error = "";
    try {
      conns = await dbList(root);
    } catch (e) {
      error = String(e);
    }
    idea = await dbIdeaList(root).catch(() => null);
  }

  const importable = $derived((idea?.found ?? []).filter((d) => d.kind && !d.exists).length);

  function importIdea() {
    if (!idea) return;
    openModal(DbImportDialog, { root, found: idea.found, problems: idea.problems, ondone: () => void load() });
  }
  void load();

  interface Row {
    key: string;
    depth: number;
    label: string;
    note: string;
    icon?: IconName;
    parent: boolean;
    conn?: DbConn;
    /** 生产库：名字后面一个红的「生产」 */
    prod?: boolean;
    info?: "wait" | "bad";
    /** 展开它要问什么 */
    ask?: () => Promise<DbNode[]>;
  }

  const SYSTEM = new Set(["information_schema", "performance_schema", "mysql", "sys", "pg_catalog", "pg_toast"]);

  const rows = $derived.by(() => {
    const out: Row[] = [];
    for (const c of conns ?? []) {
      const ck = `c\n${c.id}`;
      const where = c.kind === "h2" || c.kind === "sqlite" ? c.url : `${c.host}:${c.port}${c.database ? `/${c.database}` : ""}`;
      out.push({ key: ck, depth: 0, label: c.name, note: where, icon: "database", parent: true, conn: c, prod: c.prod, ask: () => dbMeta(root, c.id, "schemas", null, null) });
      if (!open.has(ck)) continue;
      const children = (key: string, depth: number, make: (n: DbNode) => Row) => {
        const k = kids[key];
        if (k === undefined || k === "wait") out.push({ key: `${key}\nwait`, depth, label: "正在读取…", note: "", parent: false, info: "wait" });
        else if (!Array.isArray(k)) out.push({ key: `${key}\nerr`, depth, label: k.error, note: "", parent: false, info: "bad" });
        else if (!k.length) out.push({ key: `${key}\nnone`, depth, label: "（空）", note: "", parent: false, info: "wait" });
        else for (const n of k) emit(make(n));
      };
      const emit = (r: Row) => {
        out.push(r);
        if (r.parent && open.has(r.key)) {
          if (r.depth === 1) children(r.key, 2, (t) => tableRow(c, r, t));
          else if (r.depth === 2) children(r.key, 3, (col) => colRow(r, col));
        }
      };
      children(ck, 1, (s) => ({
        key: `${ck}\n${s.catalog ?? ""}\n${s.schema ?? ""}`,
        depth: 1,
        label: s.name,
        note: SYSTEM.has(s.name.toLowerCase()) ? "系统" : "",
        icon: "folder",
        parent: true,
        ask: () => dbMeta(root, c.id, "tables", s.catalog, s.schema),
      }));
    }
    return out;
  });

  function tableRow(c: DbConn, schema: Row, t: DbNode): Row {
    const [, , catalog, sch] = schema.key.split("\n");
    return {
      key: `${schema.key}\n${t.name}`,
      depth: 2,
      label: t.name,
      note: [t.kind && t.kind !== "TABLE" && t.kind !== "BASE TABLE" ? t.kind.toLowerCase() : "", t.remarks ?? ""].filter(Boolean).join(" · "),
      icon: "structure",
      parent: true,
      ask: () => dbMeta(root, c.id, "columns", catalog || null, sch || null, t.name),
    };
  }

  function colRow(table: Row, col: DbNode): Row {
    const ty = col.ty ? `${col.ty}${col.size && /CHAR|DECIMAL|NUMERIC/i.test(col.ty) ? `(${col.size})` : ""}` : "";
    const note = [ty, col.pk ? "主键" : "", col.nullable === false && !col.pk ? "非空" : "", col.remarks ?? ""].filter(Boolean).join(" · ");
    return { key: `${table.key}\n${col.name}`, depth: 3, label: col.name, note, parent: false };
  }

  async function toggle(r: Row) {
    const next = new Set(open);
    if (next.has(r.key)) {
      next.delete(r.key);
      open = next;
      return;
    }
    next.add(r.key);
    open = next;
    if (r.ask && (kids[r.key] === undefined || (!Array.isArray(kids[r.key]) && kids[r.key] !== "wait"))) {
      kids[r.key] = "wait";
      try {
        const got = await r.ask();
        // 库的那一层：系统库排到最后（IDEA 默认把它们收在一边）
        kids[r.key] = r.conn ? [...got].sort((a, b) => Number(SYSTEM.has(a.name.toLowerCase())) - Number(SYSTEM.has(b.name.toLowerCase()))) : got;
      } catch (e) {
        kids[r.key] = { error: String(e) };
      }
    }
  }

  function edit(c: DbConn | null) {
    openModal(DbConnDialog, {
      root,
      conn: c,
      onsaved: () => {
        // 改了的连接：边车里那条按旧配置开的关掉、树收起来重问
        if (c) {
          void dbClose(root, c.id).catch(() => {});
          for (const k of Object.keys(kids)) if (k.startsWith(`c\n${c.id}`)) delete kids[k];
          open = new Set([...open].filter((k) => !k.startsWith(`c\n${c.id}`)));
        }
        void load();
      },
    });
  }

  function refresh() {
    for (const c of conns ?? []) void dbClose(root, c.id).catch(() => {});
    kids = {};
    open = new Set();
    void load();
  }

  /** 选中的那一行属于哪个连接（选中表、列也算它那个连接） */
  const selConn = $derived.by(() => {
    const id = sel.split("\n")[1];
    return id === undefined ? null : ((conns ?? []).find((c) => c.id === id) ?? null);
  });

  async function openConsole(c: DbConn) {
    try {
      await tabflow.openPath(await dbConsole(root, c.id), { preview: false });
    } catch (e) {
      notify.fail(`无法打开控制台：${String(e)}`);
    }
  }

  function onKey(e: KeyboardEvent) {
    if (imeKey(e)) return;
    const i = rows.findIndex((r) => r.key === sel);
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const j = Math.max(0, Math.min(rows.length - 1, i + (e.key === "ArrowDown" ? 1 : -1)));
      if (rows[j]) sel = rows[j].key;
    } else if (e.key === "ArrowRight" || e.key === "ArrowLeft") {
      const r = rows[i];
      if (r?.parent && open.has(r.key) !== (e.key === "ArrowRight")) {
        e.preventDefault();
        void toggle(r);
      }
    } else if (e.key === "Enter") {
      const r = rows[i];
      if (r?.parent) void toggle(r);
    }
  }
</script>

<div class="db">
  <div class="head">
    <span class="title">数据库</span>
    <span class="gap"></span>
    <button class="ibtn" onclick={() => edit(null)} title="新建连接" aria-label="新建连接"><Icon name="plus" /></button>
    {#if idea?.found.length}
      <button class="ibtn" onclick={importIdea} title="从 IDEA 导入连接（{importable} 个可导入）" aria-label="从 IDEA 导入连接"><Icon name="pull" /></button>
    {/if}
    <button class="ibtn" disabled={!selConn} onclick={() => selConn && void openConsole(selConn)} title="打开查询控制台" aria-label="打开查询控制台"><Icon name="terminal" /></button>
    <button class="ibtn" disabled={!selConn} onclick={() => selConn && edit(selConn)} title="编辑连接" aria-label="编辑连接"><Icon name="more-h" /></button>
    <button class="ibtn" onclick={refresh} title="刷新（关闭连接并重新读取）" aria-label="刷新"><Icon name="refresh" /></button>
    <HideSidebar />
  </div>
  <div class="tree" role="tree" aria-label="数据库" tabindex="0" bind:this={treeEl} onkeydown={onKey}>
    {#if error}
      <div class="empty-msg bad">{error}</div>
    {:else if !conns}
      <div class="empty-msg wait"><span class="spinner sm"></span> 正在读取连接…</div>
    {:else if conns.length === 0}
      <div class="empty-msg">
        此项目尚无数据库连接
        <span class="sub">点击右上角的 ＋ 新建（MySQL、PostgreSQL、H2、SQLite）</span>
        {#if importable}<span class="sub"><button class="btn sm" onclick={importIdea}>从 IDEA 导入 {importable} 个连接</button></span>{/if}
      </div>
    {:else}
      {#each rows as row (row.key)}
        <div
          class="row"
          class:on={sel === row.key}
          class:info={!!row.info}
          class:bad={row.info === "bad"}
          role="treeitem"
          aria-selected={sel === row.key}
          aria-expanded={row.parent ? open.has(row.key) : undefined}
          tabindex="-1"
          style:padding-left="{6 + row.depth * 19}px"
          onclick={() => {
            sel = row.key;
            treeEl?.focus();
            if (row.parent) void toggle(row);
          }}
          ondblclick={() => row.conn && edit(row.conn)}
          onkeydown={() => {}}
          title={row.conn ? `${row.label}（${row.note}）\n双击编辑` : row.note ? `${row.label}：${row.note}` : row.label}
        >
          {#if row.parent}
            <span class="chev" class:open={open.has(row.key)}><Icon name="chevron-right" /></span>
          {:else}
            <span class="chev-gap"></span>
          {/if}
          {#if row.info === "wait"}
            <span class="spinner sm"></span>
          {:else if row.icon}
            <span class="ic"><Icon name={row.icon} /></span>
          {/if}
          <span class="name">{row.label}</span>
          {#if row.prod}<span class="prod" title="生产环境：执行修改数据的语句前确认">生产</span>{/if}
          {#if row.note}<span class="note">{row.note}</span>{/if}
        </div>
      {/each}
    {/if}
  </div>
</div>

<style>
  .db { display: flex; flex-direction: column; height: 100%; overflow: hidden; }
  /* 头、行的样子和 Maven / Gradle 工具窗一模一样（同一个侧边栏里长得不一样，人会以为是两种东西） */
  .head {
    flex: none;
    height: 38px;
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 0 4px 0 10px;
    font-size: var(--fs-sm);
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-dim);
    user-select: none;
  }
  .head .gap { flex: 1; }
  .tree { flex: 1; overflow: auto; padding: 2px 6px 8px; outline: none; }
  .row {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding-right: 8px;
    border-radius: var(--r-sm);
    color: var(--text-dim);
    font-family: var(--ui-font);
    font-size: var(--fs-md);
    white-space: nowrap;
    cursor: default;
  }
  .row:hover { background: var(--hover); color: var(--text); }
  .row.on { background: var(--selected); color: var(--text); }
  .tree:focus-visible .row.on { outline: 1px solid var(--accent); outline-offset: -1px; }
  .row.info { color: var(--text-faint); font-size: var(--fs-sm); }
  .row.bad { color: var(--lvl-error); white-space: normal; height: auto; min-height: 24px; }
  .chev { flex: none; display: inline-flex; color: var(--text-faint); transition: transform 70ms ease-out; }
  .chev.open { transform: rotate(90deg); }
  .chev-gap { flex: none; width: 16px; }
  .ic { flex: none; display: inline-flex; margin-right: 2px; }
  .name { flex: 0 0 auto; max-width: 75%; overflow: hidden; text-overflow: ellipsis; }
  .prod { flex: none; margin-left: 6px; padding: 0 5px; border-radius: var(--r-xs); background: color-mix(in srgb, var(--lvl-error) 18%, transparent); color: var(--lvl-error); font-size: var(--fs-xs); line-height: 16px; }
  .note { flex: 1 1 0; min-width: 0; margin-left: 6px; overflow: hidden; text-overflow: ellipsis; color: var(--text-faint); font-size: var(--fs-xs); }
</style>
