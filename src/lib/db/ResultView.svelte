<script lang="ts">
  /**
   * 一条 SQL 的结果（docs/DATABASE.md 第 5 节 DB-2）：上面一条工具条（状态、取消、重新运行），下面是表格。
   *
   * 表格只画看得见的那几十行（同日志视图）：十万行的结果 DOM 里始终是一屏的量；滚到离底部还有一屏时要下一批。
   * 列宽按列名和第一批里的字数估一次（之后不再变 —— 滚着滚着列宽跳，眼睛就跟丢了）。
   * 格子用等宽字（ui.md 八之二：原样的数据）；NULL 画成灰的 `<null>`，和空串分得开；数字右对齐。
   * 选中按行：单击选一行、⇧ 单击连选、↑↓ 挪；⌘C 复制成制表符分隔（贴进表格软件就是一行一行）
   *
   * 「导出」（DB-3）：CSV / JSON / INSERT，导出的是**整条查询的全部结果**、不是已经取到界面上的那几批 —— 边车把这条 SQL 再跑一遍、直接写盘，
   * 几百万行也不经过这里。进度在状态栏的进度格（ui.md 十一：后台在跑的只画在那一格），点那一格的取消就停
   *
   * 改格子（DB-3）：双击一格就地改，↵ 攒下、Esc 不改、点别处也攒下；「NULL」设成 NULL。攒下的格子带底色，工具条上「提交…」先给人看 UPDATE。
   * 只有单表、主键在结果里的才让改（同 IDEA）：不然没法确定改的是哪一行；双击时说清为什么不能改
   */
  import Icon from "../shell/Icon.svelte";
  import ContextMenu, { type MenuItem } from "../shell/ContextMenu.svelte";
  import { revealInFinder } from "../ipc/commands";
  import { dbCancel, dbExport, pickExportPath } from "../ipc/db";
  import { notify } from "../state/notify.svelte";
  import { progress } from "../state/progress.svelte";
  import { results, type DbRes } from "./results.svelte";
  import { cellKey, whyNotEditable } from "./edits";
  import { imeKey } from "../shell/ime";

  let { r }: { r: DbRes } = $props();

  const ROW = 24;
  let scroller = $state<HTMLDivElement | null>(null);
  let top = $state(0);
  let height = $state(300);
  let anchor = $state<number | null>(null);
  let focus = $state<number | null>(null);

  const widths = $derived.by(() => {
    const sample = r.rows.slice(0, 200);
    return r.cols.map((c, i) => {
      // 表头是「列名 + 小一号的类型」，两样都要放得下（只按列名估的话 `id bigint` 被截成 `id bi…`）
      let n = c.name.length * 1.1 + c.ty.length * 0.85 + 2;
      for (const row of sample) n = Math.max(n, (row[i] ?? "<null>").length);
      return Math.round(Math.min(320, Math.max(56, n * 7.4 + 18)));
    });
  });
  const gutter = $derived(Math.max(40, String(r.rows.length).length * 8 + 18));
  const total = $derived(gutter + widths.reduce((a, b) => a + b, 0));
  const first = $derived(Math.max(0, Math.floor(top / ROW) - 8));
  const last = $derived(Math.min(r.rows.length, Math.ceil((top + height) / ROW) + 8));
  const visible = $derived(r.rows.slice(first, last));
  const lo = $derived(anchor === null || focus === null ? -1 : Math.min(anchor, focus));
  const hi = $derived(anchor === null || focus === null ? -1 : Math.max(anchor, focus));

  // 离底部不到一屏了：要下一批（换了结果、或者一批不够一屏时也要接着要）
  $effect(() => {
    if (r.more && !r.fetching && !r.stale && (last + Math.ceil(height / ROW) >= r.rows.length)) void results.fetchMore(r);
  });

  function onScroll() {
    if (scroller) top = scroller.scrollTop;
  }

  function pick(i: number, e: MouseEvent) {
    if (e.shiftKey && anchor !== null) focus = i;
    else anchor = focus = i;
    scroller?.focus({ preventScroll: true });
  }

  function reveal(i: number) {
    if (!scroller) return;
    const y = (i + 1) * ROW;
    if (y - ROW < scroller.scrollTop + ROW) scroller.scrollTop = y - 2 * ROW;
    else if (y > scroller.scrollTop + scroller.clientHeight) scroller.scrollTop = y - scroller.clientHeight;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      const n = r.rows.length;
      if (!n) return;
      const next = Math.max(0, Math.min(n - 1, (focus ?? -1) + (e.key === "ArrowDown" ? 1 : -1)));
      if (e.shiftKey && anchor !== null) focus = next;
      else anchor = focus = next;
      reveal(next);
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c" && lo >= 0) {
      e.preventDefault();
      const text = r.rows.slice(lo, hi + 1).map((row) => row.map((v) => (v ?? "").replace(/[\t\n]/g, " ")).join("\t")).join("\n");
      void navigator.clipboard.writeText(text).then(
        () => notify.ok(hi > lo ? `已复制 ${hi - lo + 1} 行` : "已复制 1 行"),
        (err) => notify.fail(`复制失败：${String(err)}`),
      );
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "a" && r.rows.length) {
      e.preventDefault();
      anchor = 0;
      focus = r.rows.length - 1;
    }
  }

  let editing = $state<{ row: number; col: number; value: string | null } | null>(null);
  const pending = $derived(Object.keys(r.edits).length);

  function startEdit(row: number, col: number) {
    const why = whyNotEditable(r.table);
    if (why) {
      notify.hint(why);
      return;
    }
    const k = cellKey(row, col);
    editing = { row, col, value: k in r.edits ? r.edits[k] : r.rows[row][col] };
  }

  /** 攒下这一格。点 NULL、↵、点别处都走这里；只认一次（↵ 之后输入框没了还会来一下 blur） */
  function finish(value: string | null) {
    const e = editing;
    if (!e) return;
    editing = null;
    results.stage(r, e.row, e.col, value);
    scroller?.focus({ preventScroll: true });
  }

  function editKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (imeKey(e)) return;
    if (e.key === "Enter") {
      e.preventDefault();
      finish(editing?.value ?? null);
    } else if (e.key === "Escape") {
      e.preventDefault();
      editing = null;
      scroller?.focus({ preventScroll: true });
    }
  }

  /** 拿焦点、全选（一打字就是换掉原值）。全选要等一拍：`bind:value` 往框里写值在 action 之后，先选的话写值时选区就没了（浏览器里撞见的：打出来是「3.008.88」） */
  function grab(el: HTMLInputElement) {
    el.focus({ preventScroll: true });
    queueMicrotask(() => el.select());
  }

  let menu = $state<{ x: number; y: number } | null>(null);
  let exporting = $state(false);
  /** 导出成 INSERT 时用的表名：语句里最外层 FROM 后面那个（认不出就是 t，导出完自己改） */
  const tableOf = (sql: string) => /\bfrom\s+([A-Za-z0-9_$."`]+)/i.exec(sql)?.[1] ?? "t";
  const FORMATS: { f: "csv" | "json" | "sql"; label: string; ext: string }[] = [
    { f: "csv", label: "CSV", ext: "csv" },
    { f: "json", label: "JSON", ext: "json" },
    { f: "sql", label: "SQL INSERT 语句", ext: "sql" },
  ];
  const menuItems = $derived<MenuItem[]>(FORMATS.map((x) => ({ label: `导出为 ${x.label}…`, run: () => void exportAs(x.f, x.ext) })));

  async function exportAs(format: "csv" | "json" | "sql", ext: string) {
    const table = tableOf(r.sql);
    const path = await pickExportPath(r.root, `${table.replace(/[`"]/g, "").split(".").pop() || "result"}.${ext}`).catch(() => null);
    if (!path) return;
    const name = path.slice(path.lastIndexOf("/") + 1);
    exporting = true;
    const task = progress.start(`正在导出 ${name}`, { cancel: () => void dbCancel(r.root, r.conn).catch(() => {}) });
    try {
      const done = await dbExport(r.root, r.conn, r.sql, path, format, table);
      const mb = done.bytes >= 1 << 20 ? `${(done.bytes / (1 << 20)).toFixed(1)} MB` : `${Math.ceil(done.bytes / 1024)} KB`;
      notify.ok(`已导出 ${done.rows.toLocaleString("en-US")} 行到 ${name}（${mb}，${done.ms} ms）`, 10_000, { label: "在访达中显示", run: () => void revealInFinder(path) });
    } catch (e) {
      notify.fail(`导出失败：${String(e)}`);
    } finally {
      task.end();
      exporting = false;
    }
  }

  const status = $derived.by(() => {
    if (r.status === "running") return "";
    if (r.status === "cancelled") return r.error.startsWith("前一条") ? r.error : `已取消 · ${r.ms} ms`;
    if (r.status === "failed") return `执行失败 · ${r.ms} ms`;
    if (r.kind === "count") return r.count ? `已影响 ${r.count} 行 · ${r.ms} ms` : `执行成功 · ${r.ms} ms`;
    const n = r.rows.length.toLocaleString("en-US");
    if (r.more && !r.stale) return `已读取 ${n} 行，滚动以读取更多 · ${r.ms} ms`;
    if (r.more && r.stale) return `已读取 ${n} 行（结果集已关闭，重新运行以读取其余行）· ${r.ms} ms`;
    return `共 ${n} 行 · ${r.ms} ms`;
  });
</script>

<div class="dbres">
  <div class="bar">
    {#if r.status === "running"}
      <span class="wait"><span class="spinner sm"></span> 正在执行…</span>
    {:else}
      <span class="stat" class:bad={r.status === "failed"}>{status}</span>
    {/if}
    {#if r.fetching}<span class="wait"><span class="spinner sm"></span></span>{/if}
    <span class="gap"></span>
    {#if pending}
      <span class="pend">{pending} 处更改未提交</span>
      <button class="btn sm quiet" disabled={r.committing} onclick={() => results.discard(r)} title="放弃所有未提交的更改">放弃</button>
      <button class="btn sm primary" class:busy={r.committing} disabled={r.committing} onclick={() => void results.commit(r)} title="先列出要执行的 UPDATE，确认后在一个事务中提交">提交…</button>
    {/if}
    <span class="conn" title={r.sql}>{r.connName}</span>
    {#if r.status === "running"}
      <button class="ibtn" onclick={() => results.cancel(r)} title="取消执行" aria-label="取消执行"><Icon name="stop" /></button>
    {:else}
      {#if r.kind === "rows" && r.cols.length}
        <button
          class="btn sm quiet"
          class:busy={exporting}
          disabled={exporting}
          onclick={(e) => {
            const b = (e.currentTarget as HTMLElement).getBoundingClientRect();
            menu = { x: b.left, y: b.bottom + 2 };
          }}
          title="将全部结果导出为文件（重新执行此查询，直接写入文件）">导出…</button>
      {/if}
      <button class="ibtn" onclick={() => results.rerun(r)} title="重新运行" aria-label="重新运行"><Icon name="rerun" /></button>
    {/if}
  </div>
  {#if r.status === "failed"}
    <div class="empty-msg bad">{r.error}</div>
  {:else if r.status === "cancelled" && !r.cols.length}
    <!-- 取消是你按的：中性色（ui.md 第二条：颜色说结果），数据库的原话放第二行 -->
    <div class="empty-msg">{r.error.startsWith("前一条") ? r.error : "已取消执行"}{#if !r.error.startsWith("前一条")}<div class="sub mono">{r.error}</div>{/if}</div>
  {:else if r.status === "running"}
    <div class="empty-msg wait"><span class="mono">{r.sql}</span></div>
  {:else if r.kind === "count"}
    <div class="empty-msg">{r.count ? `已影响 ${r.count} 行` : "执行成功"}<div class="sub mono">{r.sql}</div></div>
  {:else if !r.cols.length}
    <div class="empty-msg">此语句没有返回列</div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <div class="grid" bind:this={scroller} bind:clientHeight={height} onscroll={onScroll} onkeydown={onKey} tabindex="0" role="grid" aria-rowcount={r.rows.length} aria-label="查询结果">
      <div class="inner" style:width="{total}px" style:height="{(r.rows.length + 1) * ROW}px">
        <div class="hrow" style:width="{total}px" role="row">
          <span class="num gut" style:width="{gutter}px"></span>
          {#each r.cols as c, i (i)}
            <span class="hcell" class:right={c.num} style:width="{widths[i]}px" role="columnheader" title="{c.name}（{c.ty}）">{c.name}<span class="ty">{c.ty.toLowerCase()}</span></span>
          {/each}
        </div>
        {#each visible as row, k (first + k)}
          {@const i = first + k}
          <div class="row" class:on={i >= lo && i <= hi} style:top="{(i + 1) * ROW}px" style:width="{total}px" role="row" tabindex="-1" aria-rowindex={i + 1} onpointerdown={(e) => pick(i, e)}>
            <span class="num gut" style:width="{gutter}px">{i + 1}</span>
            {#each row as v0, j (j)}
              {@const k = cellKey(i, j)}
              {@const staged = k in r.edits}
              {@const v = staged ? r.edits[k] : v0}
              {#if editing && editing.row === i && editing.col === j}
                <span class="cell editing" style:width="{widths[j]}px">
                  <input
                    class="ed"
                    spellcheck="false"
                    aria-label="编辑 {r.cols[j]?.name}"
                    use:grab
                    bind:value={() => editing?.value ?? "", (x) => editing && (editing.value = x)}
                    onkeydown={editKey}
                    onblur={() => finish(editing?.value ?? null)}
                  />
                  <button class="nullb" tabindex="-1" onpointerdown={(e) => (e.preventDefault(), finish(null))} title="设为 NULL">NULL</button>
                </span>
              {:else}
                <span
                  class="cell"
                  class:right={r.cols[j]?.num}
                  class:null={v === null}
                  class:edited={staged}
                  style:width="{widths[j]}px"
                  ondblclick={() => startEdit(i, j)}
                  role="gridcell"
                  tabindex="-1"
                  title={staged ? `已修改，原值：${v0 ?? "NULL"}` : v !== null && v.length > 40 ? v.slice(0, 2000) : undefined}>{v === null ? "<null>" : v}</span>
              {/if}
            {/each}
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} label="导出格式" items={menuItems} onclose={() => (menu = null)} />
{/if}

<style>
  .dbres { display: flex; flex-direction: column; height: 100%; min-height: 0; }
  .bar { flex: none; height: 34px; display: flex; align-items: center; gap: 8px; padding: 0 6px 0 10px; border-bottom: 1px solid var(--border-soft); font-size: var(--fs-sm); color: var(--text-dim); }
  .bar .gap { flex: 1; }
  .stat { font-variant-numeric: tabular-nums; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .stat.bad { color: var(--lvl-error); }
  .conn { color: var(--text-faint); white-space: nowrap; max-width: 30%; overflow: hidden; text-overflow: ellipsis; }
  .mono { font-family: var(--code-font); white-space: pre-wrap; }
  .grid { flex: 1; min-height: 0; overflow: auto; outline: none; position: relative; font-family: var(--code-font); font-size: var(--fs-sm); }
  .inner { position: relative; }
  .hrow, .row { position: absolute; left: 0; display: flex; height: 24px; align-items: center; }
  .hrow { position: sticky; top: 0; z-index: 1; background: var(--content-solid); border-bottom: 1px solid var(--border-soft); font-family: var(--ui-font); color: var(--text-dim); }
  .row { color: var(--text); }
  .row:hover { background: var(--hover); }
  .row.on { background: var(--selected); }
  .grid:focus-visible .row.on { outline: 1px solid var(--accent); outline-offset: -1px; }
  .hcell, .cell, .num { flex: none; padding: 0 8px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; box-sizing: border-box; }
  .hcell { font-weight: 600; }
  .hcell .ty { margin-left: 6px; font-weight: 400; color: var(--text-faint); font-size: var(--fs-xs); }
  .right { text-align: right; font-variant-numeric: tabular-nums; }
  .cell.null { color: var(--text-faint); font-style: italic; }
  /* 攒着没提交的那几格：淡强调色底（同编辑器里「有改动」的那一类颜色），悬停说原值 */
  .cell.edited { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .cell.editing { display: flex; align-items: center; gap: 2px; padding: 0 2px; }
  .ed { flex: 1; min-width: 0; height: 20px; padding: 0 4px; border: 1px solid var(--accent); border-radius: var(--r-xs); background: var(--field-bg); color: var(--text); font: inherit; outline: none; }
  .nullb { flex: none; height: 18px; padding: 0 4px; border: none; border-radius: var(--r-xs); background: var(--hover); color: var(--text-muted); font: inherit; font-size: var(--fs-xs); cursor: pointer; }
  .pend { color: var(--accent); white-space: nowrap; }
  .gut { position: sticky; left: 0; z-index: 1; text-align: right; color: var(--text-faint); background: var(--content-solid); font-variant-numeric: tabular-nums; }
  .hrow .gut { z-index: 2; }
</style>
