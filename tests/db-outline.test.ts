/**
 * 控制台补全用的命名空间（src/lib/db/outline.ts）。错的形状：默认 schema 给了一个其实没有表的名字 —— 不带 schema 前缀时一个表名都补不出来
 */
import { namespaceOf } from "../src/lib/db/outline.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const h2 = namespaceOf({
  defaultSchema: "PUBLIC",
  truncated: false,
  cols: [
    { schema: "PUBLIC", table: "ITEMS", name: "SKU", ty: "CHARACTER VARYING" },
    { schema: "SHOP", table: "ORDERS", name: "ID", ty: "BIGINT" },
    { schema: "SHOP", table: "ORDERS", name: "AMOUNT", ty: "DECIMAL" },
  ],
});
eq(Object.keys(h2.schema), ["PUBLIC", "SHOP"], "按 schema 分");
eq((h2.schema.SHOP as Record<string, { label: string }[]>).ORDERS.map((c) => c.label), ["ID", "AMOUNT"], "同一张表的列按顺序收在一起");
eq((h2.schema.SHOP as Record<string, { detail: string }[]>).ORDERS[1].detail, "decimal", "列的类型当说明");
eq(h2.defaultSchema, "PUBLIC", "默认 schema 有表：给");
eq([h2.tables, h2.columns], [2, 3], "数出几张表、几列（控制台顶上说）");

const only = namespaceOf({ defaultSchema: "PUBLIC", truncated: false, cols: [{ schema: "SHOP", table: "ORDERS", name: "ID", ty: "BIGINT" }] });
eq(only.defaultSchema, "SHOP", "连接默认的 schema 里没表、有表的只有一个：用那一个");
const two = namespaceOf({ defaultSchema: "PUBLIC", truncated: false, cols: [{ schema: "A", table: "T", name: "X", ty: "INT" }, { schema: "B", table: "U", name: "Y", ty: "INT" }] });
eq(two.defaultSchema, undefined, "默认的没表、有表的不止一个：不猜");

const mysql = namespaceOf({ defaultSchema: "shop", truncated: false, cols: [{ schema: "shop", table: "orders", name: "id", ty: "BIGINT" }] });
eq([Object.keys(mysql.schema), mysql.defaultSchema], [["shop"], "shop"], "MySQL：库名当 schema 用");

const flat = namespaceOf({ defaultSchema: null, truncated: false, cols: [{ schema: null, table: "t", name: "a", ty: "INT" }] });
eq(Object.keys(flat.schema), ["t"], "没有 schema 的：表在顶层");

console.log(`${fail ? "❌" : "✅"} 补全命名空间：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
