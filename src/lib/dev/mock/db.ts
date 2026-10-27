/**
 * 桩：数据库客户端（docs/DATABASE.md），对应 `src-tauri/src/commands/db.rs`（规则在 `src-tauri/src/db.rs`，边车 / 驱动 / 钥匙串在 `crates/dbsvc`）。
 * 一个假的 MySQL：shop 库里两张表。测试连接、打开都**故意等一会儿**（真的第一次要下驱动、握手）；密码叫 wrong 的连不上（演示报错那条路）
 */
import type { DbConn, DbNode } from "../../ipc/commands";
import { type A, NOT_MINE } from "./data";

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const CONNS = new Map<string, DbConn[]>();
const PW = new Map<string, string>();
const node = (p: Partial<DbNode> & { name: string }): DbNode => ({ kind: null, catalog: null, schema: null, ty: null, size: null, nullable: null, default: null, remarks: null, pk: false, ...p });
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
    default:
      return NOT_MINE;
  }
}
