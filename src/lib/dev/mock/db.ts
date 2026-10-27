/**
 * 桩：数据库客户端（docs/DATABASE.md），对应 `src-tauri/src/commands/db.rs`（规则在 `src-tauri/src/db.rs`，边车 / 驱动 / 钥匙串在 `crates/dbsvc`）。
 * 一个假的 MySQL：shop 库里两张表。测试连接、打开都**故意等一会儿**（真的第一次要下驱动、握手）；密码叫 wrong 的连不上（演示报错那条路）
 *
 * 查询（DB-2）：`select … from orders` 回 10 万行、一批 500（滚到底再要）；带 `sleep` 的**一直等到取消**（演示忙碌态和取消）；
 * `update` / `delete` 回影响的行数；别的语法回 MySQL 那种报错。每次执行记一条历史
 *
 * DB-3：`db_scalar`（生产库确认框数行数）**故意等一会儿**（演示「正在统计」），没 WHERE 的数成 100,000、有的数成 3；
 * `db_export` 等 1.5 秒（演示状态栏的进度格和取消），名字里带 fail 的路径报错
 */
import type { DbConn, DbHist, DbNode, DbResult } from "../../ipc/commands";
import { type A, FILES, NOT_MINE } from "./data";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const CONNS = new Map<string, DbConn[]>();
const PW = new Map<string, string>();
const node = (p: Partial<DbNode> & { name: string }): DbNode => ({ kind: null, catalog: null, schema: null, ty: null, size: null, nullable: null, default: null, remarks: null, pk: false, ...p });
const HIST = new Map<string, DbHist[]>();
/** 每个连接当前开着的那个结果：游标号、取到第几行、一共多少行；正在跑的慢查询停它的函数 */
const CURSOR = new Map<string, { id: number; at: number; total: number }>();
const RUNNING = new Map<string, () => void>();
let nextCursor = 1;
const ROW = (i: number) => [String(i), (i * 1.5).toFixed(2), i % 7 === 0 ? null : `2026-10-${String((i % 28) + 1).padStart(2, "0")} 09:00:00`];
const COLS = [
  { name: "id", ty: "BIGINT", num: true },
  { name: "amount", ty: "DECIMAL", num: true },
  { name: "created_at", ty: "DATETIME", num: false },
];
function batch(key: string): Pick<DbResult, "rows" | "more"> {
  const c = CURSOR.get(key)!;
  const rows = [];
  for (let i = c.at; i < Math.min(c.total, c.at + 500); i++) rows.push(ROW(i + 1));
  c.at += rows.length;
  const more = c.at < c.total;
  if (!more) CURSOR.delete(key);
  return { rows, more };
}
const TABLES: Record<string, DbNode[]> = {
  orders: [node({ name: "id", ty: "BIGINT", size: 19, nullable: false, pk: true }), node({ name: "amount", ty: "DECIMAL", size: 12, nullable: false, remarks: "金额（元）" }), node({ name: "created_at", ty: "DATETIME", size: 19, nullable: true })],
  users: [node({ name: "id", ty: "BIGINT", size: 19, nullable: false, pk: true }), node({ name: "name", ty: "VARCHAR", size: 64, nullable: false })],
};

export function dbCmd(cmd: string, a: A): unknown {
  const root = String(a.root ?? "");
  const list = () => CONNS.get(root) ?? [];
  switch (cmd) {
    case "db_list":
      return list().map((c) => ({ ...c, hasPassword: PW.has(c.id) }));
    case "db_save": {
      const c = { ...(a.conn as DbConn) };
      if (!c.name.trim()) return Promise.reject("名称不能为空");
      if (!c.id) c.id = `c${Date.now().toString(16)}`;
      if (list().some((x) => x.id !== c.id && x.name === c.name)) return Promise.reject(`已存在名为「${c.name}」的连接`);
      CONNS.set(root, [...list().filter((x) => x.id !== c.id), c]);
      if (typeof a.password === "string") (a.password ? PW.set(c.id, a.password) : PW.delete(c.id));
      return { ...c, hasPassword: PW.has(c.id) };
    }
    case "db_delete":
      CONNS.set(root, list().filter((x) => x.id !== String(a.id)));
      PW.delete(String(a.id));
      return null;
    case "db_test":
    case "db_open":
      return sleep(700).then(() => {
        const pw = cmd === "db_test" && typeof a.password === "string" ? a.password : PW.get(String(a.id ?? (a.conn as DbConn | undefined)?.id ?? ""));
        if (pw === "wrong") throw "Access denied for user 'root'@'localhost' (using password: YES)（SQLState 28000，错误码 1045）";
        return { product: "MySQL", version: "8.4.3", catalog: "shop", schema: "" };
      });
    case "db_meta":
      return sleep(150).then(() => {
        if (a.what === "schemas") return [node({ name: "shop", catalog: "shop" }), node({ name: "information_schema", catalog: "information_schema" })];
        if (a.what === "tables") return a.catalog === "shop" ? Object.keys(TABLES).map((name) => node({ name, kind: "TABLE" })) : [];
        return TABLES[String(a.table)] ?? [];
      });
    case "db_close":
      return null;
    case "db_console": {
      const c = list().find((x) => x.id === String(a.id));
      if (!c) return Promise.reject("连接配置不存在");
      const p = `/Users/you/Library/Application Support/com.liteide.app/db/console/${c.id}/${c.name.replace(/[/:]/g, "_")}.sql`;
      if (FILES[p] === undefined) FILES[p] = "";
      return p;
    }
    case "db_query": {
      const id = String(a.id);
      const key = `${root}\n${id}`;
      const sql = String(a.sql).trim();
      const t0 = Date.now();
      CURSOR.delete(key);
      const note = (ok: boolean, count: number | null, error: string | null) =>
        HIST.set(id, [{ sql, at: Date.now(), ms: Date.now() - t0, ok, count, error }, ...(HIST.get(id) ?? [])].slice(0, 1000));
      if (/sleep/i.test(sql)) {
        return new Promise((_, reject) => {
          RUNNING.set(key, () => {
            const e = "Query execution was interrupted（SQLState 70100，错误码 1317）";
            note(false, null, e);
            reject(e);
          });
        });
      }
      return sleep(120).then(() => {
        if (/^(update|delete|insert)\b/i.test(sql)) {
          note(true, 3, null);
          return { kind: "count", cols: [], rows: [], more: false, count: 3, ms: 4, cursor: nextCursor++, table: null };
        }
        if (!/^select\b/i.test(sql)) {
          const e = `You have an error in your SQL syntax; check the manual that corresponds to your MySQL server version for the right syntax to use near '${sql.slice(0, 20)}' at line 1（SQLState 42000，错误码 1064）`;
          note(false, null, e);
          throw e;
        }
        const cursor = nextCursor++;
        CURSOR.set(key, { id: cursor, at: 0, total: /orders/i.test(sql) ? 100_000 : 3 });
        note(true, null, null);
        // 只查 orders 一张表的结果能改格子（同真实现：单表、主键在结果里）
        const table = /^select \* from orders\b/i.test(sql) && !/join|,/i.test(sql) ? { catalog: "shop", schema: null, name: "orders", cols: ["id", "amount", "created_at"], pk: [0] } : null;
        return { kind: "rows", cols: COLS, ...batch(key), count: null, ms: 18, cursor, table };
      });
    }
    case "db_fetch": {
      const key = `${root}\n${String(a.id)}`;
      const c = CURSOR.get(key);
      // 同真实现：慢一点（真的要从数据库再取一批）
      return sleep(60).then(() => {
        if (!c || c.id !== Number(a.cursor)) throw "结果集已关闭：同一连接上已执行新的查询，请重新运行此查询";
        return { kind: "rows", cols: [], ...batch(key), count: null, ms: 0, cursor: c.id, table: null };
      });
    }
    case "db_cancel": {
      const key = `${root}\n${String(a.id)}`;
      RUNNING.get(key)?.();
      RUNNING.delete(key);
      return null;
    }
    case "db_close_cursor":
      CURSOR.delete(`${root}\n${String(a.id)}`);
      return null;
    case "db_scalar":
      return sleep(600).then(() => (/\bwhere\b/i.test(String(a.sql)) ? "3" : "100000"));
    case "db_export": {
      const key = `${root}\n${String(a.id)}`;
      return new Promise((res, rej) => {
        const t = setTimeout(() => {
          RUNNING.delete(key);
          if (String(a.path).includes("fail")) rej("无法写入文件：Permission denied");
          else res({ rows: /orders/i.test(String(a.sql)) ? 100_000 : 3, bytes: 2_734_112, ms: 1480 });
        }, 1500);
        RUNNING.set(key, () => {
          clearTimeout(t);
          rej("Query execution was interrupted（SQLState 70100，错误码 1317）");
        });
      });
    }
    case "db_outline":
      // 同真实现的形状：当前库每张表的每一列（MySQL：库名当 schema）
      return sleep(150).then(() => ({
        defaultSchema: "shop",
        truncated: false,
        cols: Object.entries(TABLES).flatMap(([t, cols]) => cols.map((c) => ({ schema: "shop", table: t, name: c.name, ty: c.ty ?? "" }))),
      }));
    case "db_apply": {
      // 主键值是 999 的那一行「已经被别人删了」：演示整笔回滚那条路
      const e = a.edit as { rows: { key: { value: string | null }[] }[] };
      CURSOR.delete(`${root}\n${String(a.id)}`);
      return sleep(300).then(() => {
        const bad = e.rows.findIndex((r) => r.key.some((k) => k.value === "999"));
        if (bad >= 0) throw `第 ${bad + 1} 处更改影响了 0 行（应为 1 行）：该行可能已被修改或删除，全部更改已撤回`;
        return e.rows.length;
      });
    }
    case "db_history":
      return HIST.get(String(a.id)) ?? [];
    default:
      return NOT_MINE;
  }
}
