/**
 * 改单元格的那一半纯函数（src/lib/db/edits.ts）。最要命的形状：改了主键那一格，WHERE 却用新值去找 —— 改不到那一行（0 行）、或者改到了别的行
 */
import { applyLocal, buildEdit, cellKey, previewSql, whyNotEditable } from "../src/lib/db/edits.ts";

let pass = 0, fail = 0;
const ok = (c: boolean, m: string) => { if (c) pass++; else { fail++; console.error("  ✗ " + m); } };
const eq = (a: unknown, b: unknown, m: string) => ok(JSON.stringify(a) === JSON.stringify(b), `${m}：${JSON.stringify(a)}，应为 ${JSON.stringify(b)}`);

const T = { catalog: "E", schema: "SHOP", name: "ORDERS", cols: ["ID", "AMOUNT", "NOTE"], pk: [0] };
const rows = [["1", "1.50", "a"], ["2", "3.00", null], ["3", "4.50", "c"]];

ok(whyNotEditable(null)!.includes("多张表"), "多表 / 计算列：说清为什么不能改");
ok(whyNotEditable({ ...T, pk: [] })!.includes("没有主键"), "没主键：说清为什么不能改");
eq(whyNotEditable(T), null, "单表、主键在结果里：能改");

const edits = { [cellKey(2, 2)]: "it's", [cellKey(0, 1)]: "9.99", [cellKey(0, 2)]: null };
const e = buildEdit(T, rows, edits);
eq(e.rows.map((r) => r.key[0].value), ["1", "3"], "一行一条、按行的次序");
eq(e.rows[0].set, [{ col: "AMOUNT", value: "9.99" }, { col: "NOTE", value: null }], "同一行改的几格合成一条、NULL 是 null");
eq({ schema: e.schema, table: e.table }, { schema: "SHOP", table: "ORDERS" }, "表名、schema 照结果的");

const alias = buildEdit({ ...T, cols: ["ID", "AMOUNT", "NOTE"] }, rows, { [cellKey(1, 1)]: "0" });
eq(alias.rows[0].set[0].col, "AMOUNT", "SET 用列在表里的真名（结果里那一列可能叫别名 amt）");

const pkEdit = buildEdit(T, rows, { [cellKey(1, 0)]: "20" });
eq(pkEdit.rows[0], { set: [{ col: "ID", value: "20" }], key: [{ col: "ID", value: "2" }] }, "改了主键那一格：SET 新值、WHERE 用原来的值");

eq(previewSql(e), ["UPDATE SHOP.ORDERS SET AMOUNT = '9.99', NOTE = NULL WHERE ID = '1';", "UPDATE SHOP.ORDERS SET NOTE = 'it''s' WHERE ID = '3';"], "给人看的 UPDATE：单引号写两遍、NULL 不带引号");

const after = applyLocal(rows, edits);
eq(after[0], ["1", "9.99", null], "提交后表格里那一行换成新值");
ok(after[1] === rows[1], "没改的行原样（不复制，十万行不白拷）");
ok(rows[0][1] === "1.50", "原来的数组没被改（换了新数组表格才重画）");

console.log(`${fail ? "❌" : "✅"} 改单元格：${pass} 通过，${fail} 失败`);
if (fail) process.exit(1);
