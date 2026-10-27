/**
 * 控制台补全表名、列名（docs/DATABASE.md 第 1 节，DB-3）：边车给的「当前库每张表的每一列」拼成 lang-sql 认的命名空间
 * （`schema → 表 → 列`），补全本身用 lang-sql 的 `schemaCompletionSource` —— `schema.表`、`表.列`、别名（`o.` 认出 o 是哪张表）它都会。
 * 不碰编辑器、不碰 IPC：在裸 node 里测（tests/db-outline.test.ts）
 */
import type { Completion } from "@codemirror/autocomplete";
import type { DbOutline } from "../ipc/commands";

export type Namespace = Record<string, Record<string, Completion[]> | Completion[]>;

export interface Built {
  schema: Namespace;
  defaultSchema: string | undefined;
  tables: number;
  columns: number;
}

/** 有 schema 的库：`{ schema: { 表: [列] } }`；没有 schema 的（列上 schema 是 null）：表直接在顶层 */
export function namespaceOf(o: DbOutline): Built {
  const schema: Namespace = {};
  const seen = new Set<string>();
  for (const c of o.cols) {
    const col: Completion = { label: c.name, type: "property", detail: c.ty.toLowerCase() };
    if (c.schema) {
      const s = (schema[c.schema] ??= {}) as Record<string, Completion[]>;
      (s[c.table] ??= []).push(col);
    } else {
      ((schema[c.table] ??= []) as Completion[]).push(col);
    }
    seen.add(`${c.schema ?? ""}\n${c.table}`);
  }
  // 默认 schema：不写前缀时补哪个 schema 的表。连接默认的那个里一张表都没有（H2 默认 PUBLIC、表全建在 SHOP 里）、而有表的 schema 只有一个，就用那一个 ——
  // lang-sql 给一个没表的 schema 当默认时只是建一个空的层（读过 CompletionLevel.child），不写前缀就一个表名都补不出来
  const withTables = Object.keys(schema).filter((k) => !Array.isArray(schema[k]));
  const given = o.defaultSchema && withTables.includes(o.defaultSchema) ? o.defaultSchema : undefined;
  const def = given ?? (withTables.length === 1 ? withTables[0] : undefined);
  return { schema, defaultSchema: def, tables: seen.size, columns: o.cols.length };
}
