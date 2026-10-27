//! 数据库客户端（docs/DATABASE.md DB-1）：连接配置的增删改、测试连接、表结构树。规则在 `crate::db`，边车 / 驱动 / 钥匙串在 `dbsvc`

use super::*;
use tauri::Manager;

fn conf_of(d: DbConnDto) -> crate::db::Conf {
    crate::db::Conf { id: d.id, name: d.name, kind: d.kind, host: d.host, port: d.port, database: d.database, user: d.user, url: d.url }
}

fn dto_of(app: &tauri::AppHandle, c: crate::db::Conf) -> DbConnDto {
    let has_password = crate::db::keychain(app).get(&c.id).is_some_and(|p| !p.is_empty());
    DbConnDto { id: c.id, name: c.name, kind: c.kind, host: c.host, port: c.port, database: c.database, user: c.user, url: c.url, has_password }
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
