/**
 * 改单元格（docs/DATABASE.md 第 5 节，DB-3）里不碰界面、不碰 IPC 的那一半：能不能改、攒下来的改动拼成一次提交、给人看的 UPDATE、提交成功后把表格里的值换掉。
 * 在裸 node 里测（tests/db-edits.test.ts）。
 *
 * 定位一行用的是**原来的**主键值（`rows` 里的，不是改过的）：主键那一格也改了的话，WHERE 还得指向改之前那一行，不然改的是别的行、或者 0 行
 */
import type { DbEdit, DbTable } from "../ipc/commands";

/** 攒下来的改动：`行\n列` → 新值（null 是 NULL） */
export type Edits = Record<string, string | null>;

export const cellKey = (row: number, col: number) => `${row}\n${col}`;

/** 不能改的话，为什么（给人看的一句）；能改是 null */
export function whyNotEditable(table: DbTable | null): string | null {
  if (!table) return "此结果来自多张表或包含计算列，无法直接修改";
  if (!table.pk.length) return `表 ${table.name} 没有主键，或结果中缺少主键列，无法定位要修改的行`;
  return null;
}

/** 攒下来的改动拼成一次提交：一行一条，SET 用列在表里的真名，WHERE 用原来的主键值 */
export function buildEdit(table: DbTable, rows: (string | null)[][], edits: Edits): DbEdit {
  const byRow = new Map<number, [number, string | null][]>();
  for (const [k, v] of Object.entries(edits)) {
    const [r, c] = k.split("\n").map(Number);
    byRow.set(r, [...(byRow.get(r) ?? []), [c, v]]);
  }
  return {
    catalog: table.catalog,
    schema: table.schema,
    table: table.name,
    rows: [...byRow.keys()]
      .sort((a, b) => a - b)
      .map((r) => ({
        set: byRow.get(r)!.sort((a, b) => a[0] - b[0]).map(([c, v]) => ({ col: table.cols[c], value: v })),
        key: table.pk.map((c) => ({ col: table.cols[c], value: rows[r][c] })),
      })),
  };
}

const lit = (v: string | null) => (v === null ? "NULL" : `'${v.replace(/'/g, "''")}'`);

/** 给人看的 UPDATE（真执行的是参数化的那一份，值不拼进 SQL；这里只是把要做的事说清楚） */
export function previewSql(e: DbEdit): string[] {
  const name = [e.schema ?? e.catalog, e.table].filter(Boolean).join(".");
  return e.rows.map((r) => `UPDATE ${name} SET ${r.set.map((x) => `${x.col} = ${lit(x.value)}`).join(", ")} WHERE ${r.key.map((x) => `${x.col} = ${lit(x.value)}`).join(" AND ")};`);
}

/** 提交成功后表格里的值：改过的那几行换成新数组（`$state.raw`，换引用表格才重画），别的行原样 */
export function applyLocal(rows: (string | null)[][], edits: Edits): (string | null)[][] {
  const out = rows.slice();
  for (const [k, v] of Object.entries(edits)) {
    const [r, c] = k.split("\n").map(Number);
    if (out[r] === rows[r]) out[r] = rows[r].slice();
    out[r][c] = v;
  }
  return out;
}
