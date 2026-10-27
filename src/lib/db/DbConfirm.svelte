<script lang="ts">
  /**
   * 执行前确认（docs/DATABASE.md 第 4 节，DB-3）：生产库上改数据的语句，和任何库上不带 WHERE 的 UPDATE / DELETE。
   * 每条一行：语句开头 + 它要做什么。单表的 UPDATE / DELETE 当场数一数会动几行（3 秒内数不出来就说数不出来 —— 生产库上一个没索引的 COUNT 可能扫很久）。
   * 照 ui.md 二之二的对话框：标题是问句、按钮写动词、默认按钮是「取消」（回车不该顺手执行）、「执行」是 danger（不可逆）
   */
  import { untrack } from "svelte";
  import { dbScalar } from "../ipc/db";
  import { countSqlFor, hasWhere, target, writeKind, type WriteKind } from "./sql-guard";

  let { root, conn, connName, prod, sqls, onanswer, onclose }: { root: string; conn: string; connName: string; prod: boolean; sqls: string[]; onanswer: (go: boolean) => void; onclose: () => void } = $props();

  interface Item {
    sql: string;
    kind: WriteKind | null;
    table: string | null;
    whole: boolean;
    /** 数出来的行数；还在数是 undefined；数不了是一句为什么 */
    count?: number | { why: string };
  }
  let items = $state<Item[]>(
    untrack(() => sqls).map((sql) => {
      const t = target(sql);
      return { sql, kind: writeKind(sql), table: t?.table ?? null, whole: !hasWhere(sql) };
    }),
  );
  // 打开时数一次（`untrack`：有意只取初值）
  untrack(() => {
    items.forEach((it, i) => {
      const q = countSqlFor(it.sql);
      if (it.kind !== "update" && it.kind !== "delete") return;
      if (!q) {
        items[i].count = { why: "多表、带 LIMIT 或 USING 的语句无法预先统计" };
        return;
      }
      dbScalar(root, conn, q, 3).then(
        (v) => (items[i].count = Number(v ?? 0)),
        (e) => (items[i].count = { why: /cancel|timed out|超时|57014/i.test(String(e)) ? "3 秒内未能统计完成" : String(e).slice(0, 120) }),
      );
    });
  });

  const verb = (k: WriteKind | null) => (k === "update" ? "更新" : "删除");
  function what(it: Item): string {
    if (it.kind === "update" || it.kind === "delete") {
      if (it.count === undefined) return "正在统计受影响的行数…";
      if (typeof it.count !== "number") return `将${verb(it.kind)}${it.table ? ` ${it.table} 中` : ""}的行（${it.count.why}）`;
      const n = it.count.toLocaleString("en-US");
      return it.whole ? `没有 WHERE 条件，将${verb(it.kind)} ${it.table} 中的全部 ${n} 行` : `将${verb(it.kind)} ${it.table} 中的 ${n} 行`;
    }
    if (it.kind === "insert") return "将插入数据";
    if (it.kind === "ddl") return "将修改数据库结构";
    return "可能修改数据";
  }

  function answer(go: boolean) {
    onanswer(go);
    onclose();
  }

  function onKey(e: KeyboardEvent) {
    e.stopPropagation();
    if (e.key === "Escape") {
      e.preventDefault();
      answer(false);
    }
  }

  function focusIn(el: HTMLButtonElement) {
    el.focus({ preventScroll: true });
  }
</script>

<div class="layer" role="presentation">
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
  <div class="dialog dbconfirm" class:prod role="alertdialog" aria-label="确认执行" tabindex="-1" onkeydown={onKey}>
    <p class="title">{prod ? `在生产库「${connName}」上执行？` : "执行不带 WHERE 条件的语句？"}</p>
    <div class="list">
      {#each items as it, i (i)}
        <div class="item">
          <code class="sql">{it.sql.split("\n").slice(0, 3).join("\n")}</code>
          <span class="what" class:bad={it.whole && (it.kind === "update" || it.kind === "delete")}>
            {#if it.count === undefined && (it.kind === "update" || it.kind === "delete")}<span class="spinner sm"></span>{/if}{what(it)}
          </span>
        </div>
      {/each}
    </div>
    <p class="body">此操作无法撤销。</p>
    <div class="actions">
      <span class="gap"></span>
      <button class="btn" data-dismiss data-default use:focusIn onclick={() => answer(false)}>取消</button>
      <button class="btn danger" onclick={() => answer(true)}>执行</button>
    </div>
  </div>
</div>

<style>
  .layer { position: fixed; inset: 0; z-index: 40; display: flex; justify-content: center; align-items: flex-start; padding-top: min(12vh, 96px); background: rgba(0, 0, 0, 0.38); }
  .dbconfirm { width: min(560px, calc(100% - 48px)); display: flex; flex-direction: column; gap: 10px; }
  .list { display: flex; flex-direction: column; gap: 8px; max-height: 40vh; overflow: auto; }
  .item { display: flex; flex-direction: column; gap: 3px; }
  .sql { font-family: var(--code-font); font-size: var(--fs-sm); color: var(--text); white-space: pre-wrap; overflow-wrap: anywhere; }
  .what { display: inline-flex; align-items: center; gap: 6px; font-size: var(--fs-sm); color: var(--text-muted); }
  .what.bad { color: var(--lvl-error); }
  .body { margin: 0; font-size: var(--fs-sm); color: var(--text-muted); }
</style>
