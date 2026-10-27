/** 数据库客户端（docs/DATABASE.md）的命令包装：只有数据库工具窗用，它是懒的（同 ipc/agent.ts 的理由） */
import { invoke } from "@tauri-apps/api/core";
import type { DbConn, DbEdit, DbExport, DbHist, DbIdeaList, DbInfo, DbNode, DbOutline, DbResult } from "./commands";

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
/** 导出一条查询的结果（边车直接写盘）。`table` 是导出成 INSERT 时的表名 */
export const dbExport = (root: string, id: string, sql: string, path: string, format: "csv" | "json" | "sql", table: string) =>
  invoke<DbExport>("db_export", { root, id, sql, path, format, table });
/** 第一行第一格，`seconds` 秒内出不来就报错（生产库保护数影响的行数）。不记历史 */
export const dbScalar = (root: string, id: string, sql: string, seconds: number) => invoke<string | null>("db_scalar", { root, id, sql, seconds });
/** 导出的保存面板（同「另存为」那个，标题不同）。这个包装不放进 commands.ts：那个文件在入口包里 */
export const pickExportPath = (dir: string | null, name: string) => invoke<string | null>("pick_save_path", { dir, name, title: "导出查询结果" });
/** 提交改过的格子：一个事务、每条正好改 1 行，不是就整笔回滚。回改了几行 */
export const dbApply = (root: string, id: string, edit: DbEdit) => invoke<number>("db_apply", { root, id, edit });
/** 控制台补全用的表和列 */
export const dbOutline = (root: string, id: string) => invoke<DbOutline>("db_outline", { root, id });
/** 项目里 IDEA 存的数据源（导入前给人挑） */
export const dbIdeaList = (root: string) => invoke<DbIdeaList>("db_idea_list", { root });
/** 导入选中的那几个（按 uuid），回导进来了几个。密码不带 */
export const dbIdeaImport = (root: string, uuids: string[]) => invoke<number>("db_idea_import", { root, uuids });
