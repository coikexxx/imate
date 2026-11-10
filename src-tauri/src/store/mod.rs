pub mod agents;
pub mod migrations;
pub mod messages;
pub mod projects;
pub mod runs;

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;

use crate::error::AppResult;

/// 打开（必要时创建）磁盘上的数据库并升级到最新结构。
pub fn open(path: &Path) -> AppResult<Connection> {
    init(Connection::open(path)?)
}

/// 打开内存数据库，供测试使用。
pub fn open_in_memory() -> AppResult<Connection> {
    init(Connection::open_in_memory()?)
}

fn init(mut conn: Connection) -> AppResult<Connection> {
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrations::migrate(&mut conn)?;
    Ok(conn)
}

/// 当前时间，Unix 毫秒。
pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// 生成一个新的记录 ID。
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrate_creates_all_tables() {
        let conn = open_in_memory().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT name FROM sqlite_master \
                 WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name",
            )
            .unwrap();
        let names: Vec<String> = stmt
            .query_map([], |r| r.get(0))
            .unwrap()
            .map(|r| r.unwrap())
            .collect();
        assert_eq!(
            names,
            ["agents", "messages", "projects", "runs", "settings", "tasks"]
        );
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = open_in_memory().unwrap();
        migrations::migrate(&mut conn).unwrap();
        let version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn foreign_keys_are_enabled() {
        let conn = open_in_memory().unwrap();
        let on: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(on, 1);
    }

    #[test]
    fn ids_are_unique_and_time_is_positive() {
        assert_ne!(new_id(), new_id());
        assert!(now_ms() > 0);
    }
}
