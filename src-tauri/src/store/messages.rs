use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::store::{now_ms, projects};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    pub id: i64,
    pub project_id: String,
    pub sender_type: String,
    pub sender_agent_id: Option<String>,
    pub kind: String,
    pub content: String,
    pub round: Option<i64>,
    pub created_at: i64,
}

fn from_row(row: &Row) -> rusqlite::Result<Message> {
    Ok(Message {
        id: row.get(0)?,
        project_id: row.get(1)?,
        sender_type: row.get(2)?,
        sender_agent_id: row.get(3)?,
        kind: row.get(4)?,
        content: row.get(5)?,
        round: row.get(6)?,
        created_at: row.get(7)?,
    })
}

pub fn insert(
    conn: &Connection,
    project_id: &str,
    sender_type: &str,
    sender_agent_id: Option<&str>,
    kind: &str,
    content: &str,
) -> AppResult<Message> {
    projects::get(conn, project_id)?;
    conn.execute(
        "INSERT INTO messages (project_id, sender_type, sender_agent_id, kind, content, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![project_id, sender_type, sender_agent_id, kind, content, now_ms()],
    )?;
    let id = conn.last_insert_rowid();
    get(conn, id)
}

pub fn get(conn: &Connection, id: i64) -> AppResult<Message> {
    conn.query_row(
        "SELECT id, project_id, sender_type, sender_agent_id, kind, content, round, created_at \
         FROM messages WHERE id = ?1",
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("消息 {id}")))
}

/// 返回游标之前的最新一页消息，并按自增 ID 升序排列。
pub fn list(
    conn: &Connection,
    project_id: &str,
    before_id: Option<i64>,
    limit: usize,
) -> AppResult<Vec<Message>> {
    let limit = limit.clamp(1, 200) as i64;
    let mut statement = conn.prepare(
        "SELECT id, project_id, sender_type, sender_agent_id, kind, content, round, created_at \
         FROM ( \
             SELECT id, project_id, sender_type, sender_agent_id, kind, content, round, created_at \
             FROM messages \
             WHERE project_id = ?1 AND (?2 IS NULL OR id < ?2) \
             ORDER BY id DESC LIMIT ?3 \
         ) ORDER BY id ASC",
    )?;
    let rows = statement.query_map(params![project_id, before_id, limit], from_row)?;
    let mut messages = Vec::new();
    for row in rows {
        messages.push(row?);
    }
    Ok(messages)
}

pub fn update_content(conn: &Connection, id: i64, content: &str) -> AppResult<Message> {
    let changed = conn.execute(
        "UPDATE messages SET content = ?1 WHERE id = ?2",
        params![content, id],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("消息 {id}")));
    }
    get(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{open_in_memory, projects};

    #[test]
    fn pagination_is_scoped_and_returns_latest_messages_in_order() {
        let conn = open_in_memory().unwrap();
        let project_a = projects::create(&conn, "项目甲", "", "/tmp/甲").unwrap();
        let project_b = projects::create(&conn, "项目乙", "", "/tmp/乙").unwrap();
        let first = insert(&conn, &project_a.id, "user", None, "chat", "第一条").unwrap();
        insert(&conn, &project_b.id, "user", None, "chat", "不能混入").unwrap();
        let second = insert(&conn, &project_a.id, "system", None, "system", "第二条").unwrap();
        let third = insert(&conn, &project_a.id, "user", None, "chat", "第三条").unwrap();

        let page = list(&conn, &project_a.id, None, 2).unwrap();
        assert_eq!(page.iter().map(|message| message.id).collect::<Vec<_>>(), [second.id, third.id]);

        let previous = list(&conn, &project_a.id, Some(second.id), 2).unwrap();
        assert_eq!(previous.iter().map(|message| message.id).collect::<Vec<_>>(), [first.id]);

        update_content(&conn, third.id, "已经完成").unwrap();
        assert_eq!(list(&conn, &project_a.id, None, 1).unwrap()[0].content, "已经完成");
    }

    #[test]
    fn inserting_into_missing_project_fails() {
        let conn = open_in_memory().unwrap();
        assert!(insert(&conn, "不存在", "user", None, "chat", "内容").is_err());
    }

    #[test]
    fn pagination_clamps_size_and_empty_history_is_empty() {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "", "/tmp/项目").unwrap();
        assert!(list(&conn, &project.id, None, 10).unwrap().is_empty());
        for _ in 0..205 {
            insert(&conn, &project.id, "user", None, "chat", "内容").unwrap();
        }
        assert_eq!(list(&conn, &project.id, None, usize::MAX).unwrap().len(), 200);
        assert_eq!(list(&conn, &project.id, None, 0).unwrap().len(), 1);
    }
}
