/** 数据库客户端（docs/DATABASE.md DB-1）的命令包装：只有数据库工具窗用，它是懒的（同 ipc/agent.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { DbConn, DbInfo, DbNode } from "./commands";

export const dbList = (root: string) => invoke<DbConn[]>("db_list", { root });
/** `password`：非空存进钥匙串、空串删掉、null 不动 */
export const dbSave = (root: string, conn: DbConn, password: string | null) => invoke<DbConn>("db_save", { root, conn, password });
export const dbDelete = (root: string, id: string) => invoke<void>("db_delete", { root, id });
/** 测试连接：`password` 为 null 用钥匙串里存的。第一次连某种库要先下驱动 */
export const dbTest = (conn: DbConn, password: string | null) => invoke<DbInfo>("db_test", { conn, password });
export const dbOpen = (root: string, id: string) => invoke<DbInfo>("db_open", { root, id });
export const dbMeta = (root: string, id: string, what: "schemas" | "tables" | "columns", catalog: string | null, schema: string | null, table: string | null = null) =>
  invoke<DbNode[]>("db_meta", { root, id, what, catalog, schema, table });
export const dbClose = (root: string, id: string) => invoke<void>("db_close", { root, id });
