/** 数据库客户端（docs/DATABASE.md）的命令包装：只有数据库工具窗用，它是懒的（同 ipc/agent.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { DbConn, DbHist, DbInfo, DbNode, DbResult } from "./commands";

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
/** 这个连接的控制台文件（应用数据目录里，没有就建一个空的） */
export const dbConsole = (root: string, id: string) => invoke<string>("db_console", { root, id });
/** 跑一条 SQL（连接没开的先开）。不设超时，停它走 dbCancel */
export const dbQuery = (root: string, id: string, sql: string) => invoke<DbResult>("db_query", { root, id, sql });
export const dbFetch = (root: string, id: string, cursor: number) => invoke<DbResult>("db_fetch", { root, id, cursor });
export const dbCancel = (root: string, id: string) => invoke<void>("db_cancel", { root, id });
export const dbCloseCursor = (root: string, id: string) => invoke<void>("db_close_cursor", { root, id });
export const dbHistory = (id: string) => invoke<DbHist[]>("db_history", { id });
