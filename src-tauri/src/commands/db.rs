//! 数据库客户端（docs/DATABASE.md）：连接配置的增删改、测试连接、表结构树（DB-1）；控制台、查询、取消、历史（DB-2）。规则在 `crate::db`，边车 / 驱动 / 钥匙串在 `dbsvc`

use super::*;
use tauri::Manager;

fn conf_of(d: DbConnDto) -> crate::db::Conf {
    crate::db::Conf { id: d.id, name: d.name, kind: d.kind, host: d.host, port: d.port, database: d.database, user: d.user, url: d.url, prod: d.prod }
}

fn dto_of(app: &tauri::AppHandle, c: crate::db::Conf) -> DbConnDto {
    let has_password = crate::db::keychain(app).get(&c.id).is_some_and(|p| !p.is_empty());
    DbConnDto { id: c.id, name: c.name, kind: c.kind, host: c.host, port: c.port, database: c.database, user: c.user, url: c.url, has_password, prod: c.prod }
}

fn info_of(v: serde_json::Value) -> DbInfoDto {
    let s = |k: &str| v[k].as_str().unwrap_or("").to_string();
    DbInfoDto { product: s("product"), version: s("version"), catalog: s("catalog"), schema: s("schema") }
}

/// 这个项目的连接（没有密码；`has_password` 说钥匙串里有没有）。碰盘、钥匙串，放阻塞池
#[tauri::command]
pub async fn db_list(app: tauri::AppHandle, root: String) -> Result<Vec<DbConnDto>, String> {
    blocking(move || Ok(crate::db::list(&app, &root).into_iter().map(|c| dto_of(&app, c)).collect())).await
}

/// 存一个连接。`password`：Some(非空) 存进钥匙串、Some("") 删掉、None 不动
#[tauri::command]
pub async fn db_save(app: tauri::AppHandle, root: String, conn: DbConnDto, password: Option<String>) -> Result<DbConnDto, String> {
    blocking(move || {
        let c = crate::db::save(&app, &root, conf_of(conn), password.as_deref())?;
        Ok(dto_of(&app, c))
    })
    .await
}

#[tauri::command]
pub async fn db_delete(app: tauri::AppHandle, root: String, id: String) -> Result<(), String> {
    blocking(move || crate::db::delete(&app, &root, &id)).await
}

/// 测试连接（对话框里此刻填的；`password` 为 None 用钥匙串里存的）。第一次连某种库要先下驱动
#[tauri::command]
pub async fn db_test(app: tauri::AppHandle, conn: DbConnDto, password: Option<String>) -> Result<DbInfoDto, String> {
    blocking(move || Ok(info_of(app.state::<AppState>().db.test(&app, &conf_of(conn), password.as_deref())?))).await
}

/// 打开一个连接（表结构树展开时）
#[tauri::command]
pub async fn db_open(app: tauri::AppHandle, window: tauri::Window, root: String, id: String) -> Result<DbInfoDto, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(info_of(app.state::<AppState>().db.open(&app, &owner, &root, &id)?))).await
}

/// 表结构树的一层：`what` 是 schemas / tables / columns
#[tauri::command]
pub async fn db_meta(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, what: String, catalog: Option<String>, schema: Option<String>, table: Option<String>) -> Result<Vec<DbNodeDto>, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let rows = app.state::<AppState>().db.meta(&app, &owner, &root, &id, &what, catalog.as_deref(), schema.as_deref(), table.as_deref())?;
        Ok(rows
            .into_iter()
            .map(|v| {
                let s = |k: &str| v[k].as_str().map(str::to_string);
                DbNodeDto {
                    name: s("name").unwrap_or_default(),
                    kind: s("kind"),
                    catalog: s("catalog"),
                    schema: s("schema"),
                    ty: s("type"),
                    size: v["size"].as_u64().map(|n| n as u32),
                    nullable: v["nullable"].as_bool(),
                    default: s("default"),
                    remarks: s("remarks"),
                    pk: v["pk"].as_bool().unwrap_or(false),
                }
            })
            .collect())
    })
    .await
}

#[tauri::command]
pub async fn db_close(app: tauri::AppHandle, window: tauri::Window, root: String, id: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || {
        app.state::<AppState>().db.close(&owner, &root, &id);
        Ok(())
    })
    .await
}

/// 这个连接的控制台文件（没有就建一个空的），前端当普通文件打开
#[tauri::command]
pub async fn db_console(app: tauri::AppHandle, root: String, id: String) -> Result<String, String> {
    blocking(move || Ok(crate::db::console(&app, &root, &id)?.to_string_lossy().into_owned())).await
}

fn result_of(v: serde_json::Value) -> DbResultDto {
    let cols = v["cols"]
        .as_array()
        .map(|a| a.iter().map(|c| DbColDto { name: c["name"].as_str().unwrap_or("").to_string(), ty: c["type"].as_str().unwrap_or("").to_string(), num: c["num"].as_bool().unwrap_or(false) }).collect())
        .unwrap_or_default();
    let rows = v["rows"].as_array().map(|a| a.iter().map(|r| r.as_array().map(|r| r.iter().map(|c| c.as_str().map(str::to_string)).collect()).unwrap_or_default()).collect()).unwrap_or_default();
    DbResultDto {
        kind: v["kind"].as_str().unwrap_or("rows").to_string(),
        cols,
        rows,
        more: v["more"].as_bool().unwrap_or(false),
        count: v["count"].as_i64(),
        ms: v["ms"].as_u64().unwrap_or(0),
        cursor: v["cursor"].as_u64().unwrap_or(0),
        table: serde_json::from_value(v["table"].clone()).ok(),
    }
}

/// 跑一条 SQL。不设超时：慢查询停它走 `db_cancel`
#[tauri::command]
pub async fn db_query(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, sql: String) -> Result<DbResultDto, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(result_of(app.state::<AppState>().db.query(&app, &owner, &root, &id, &sql)?))).await
}

#[tauri::command]
pub async fn db_fetch(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, cursor: u64) -> Result<DbResultDto, String> {
    let owner = window.label().to_string();
    blocking(move || Ok(result_of(app.state::<AppState>().db.fetch(&app, &owner, &root, &id, cursor)?))).await
}

#[tauri::command]
pub async fn db_cancel(app: tauri::AppHandle, window: tauri::Window, root: String, id: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || {
        app.state::<AppState>().db.cancel(&owner, &root, &id);
        Ok(())
    })
    .await
}

/// 结果标签关了：边车里那个游标关掉
#[tauri::command]
pub async fn db_close_cursor(app: tauri::AppHandle, window: tauri::Window, root: String, id: String) -> Result<(), String> {
    let owner = window.label().to_string();
    blocking(move || {
        app.state::<AppState>().db.close_cursor(&owner, &root, &id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn db_history(app: tauri::AppHandle, id: String) -> Result<Vec<DbHistDto>, String> {
    blocking(move || Ok(crate::db::history(&app, &id).into_iter().map(|h| DbHistDto { sql: h.sql, at: h.at, ms: h.ms, ok: h.ok, count: h.count, error: h.error }).collect())).await
}

/// 把一条查询的结果导出到 `path`（csv / json / sql；sql 是 INSERT，表名用 `table`）。边车直接写盘，不经过界面
#[tauri::command]
pub async fn db_export(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, sql: String, path: String, format: String, table: String) -> Result<DbExportDto, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let v = app.state::<AppState>().db.export(&app, &owner, &root, &id, &sql, &path, &format, &table)?;
        Ok(DbExportDto { rows: v["rows"].as_u64().unwrap_or(0), bytes: v["bytes"].as_u64().unwrap_or(0), ms: v["ms"].as_u64().unwrap_or(0) })
    })
    .await
}

/// 第一行第一格，`seconds` 秒内出不来就报错（生产库保护：先数 UPDATE / DELETE 会动几行）。不记历史
#[tauri::command]
pub async fn db_scalar(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, sql: String, seconds: u32) -> Result<Option<String>, String> {
    let owner = window.label().to_string();
    blocking(move || app.state::<AppState>().db.scalar(&app, &owner, &root, &id, &sql, seconds)).await
}

/// 提交改过的格子：一个事务，每条正好改 1 行，不是就整笔回滚。回改了几行
#[tauri::command]
pub async fn db_apply(app: tauri::AppHandle, window: tauri::Window, root: String, id: String, edit: DbEditDto) -> Result<u64, String> {
    let owner = window.label().to_string();
    blocking(move || {
        let v = app.state::<AppState>().db.apply(&app, &owner, &root, &id, serde_json::to_value(&edit).map_err(|e| e.to_string())?)?;
        Ok(v["applied"].as_u64().unwrap_or(0))
    })
    .await
}
