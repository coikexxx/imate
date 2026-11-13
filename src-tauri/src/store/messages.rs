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
    /// 产生这条消息的运行所用的模型。没有链接运行（用户消息、系统消息、旧消息）时为 None。
    pub model_info: Option<ModelInfo>,
}

/// 运行记录里的模型值与状态。删除成员后运行可能不存在，此时状态保持未知。
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    pub requested_model: Option<String>,
    pub reported_model: Option<String>,
    pub run_status: Option<String>,
}

/// 联表查询的列：消息本身 8 列，之后依次是 run_id、请求的模型、CLI 报告的模型及运行状态。
const SELECT_COLUMNS: &str = "m.id, m.project_id, m.sender_type, m.sender_agent_id, m.kind, m.content, m.round, \
     m.created_at, m.run_id, r.requested_model, r.reported_model, r.status";

fn from_row(row: &Row) -> rusqlite::Result<Message> {
    let run_id: Option<String> = row.get(8)?;
    let model_info = if run_id.is_some() {
        Some(ModelInfo { requested_model: row.get(9)?, reported_model: row.get(10)?, run_status: row.get(11)? })
    } else {
        None
    };
    Ok(Message {
        id: row.get(0)?,
        project_id: row.get(1)?,
        sender_type: row.get(2)?,
        sender_agent_id: row.get(3)?,
        kind: row.get(4)?,
        content: row.get(5)?,
        round: row.get(6)?,
        created_at: row.get(7)?,
        model_info,
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
        &format!(
            "SELECT {SELECT_COLUMNS} FROM messages m LEFT JOIN runs r ON r.id = m.run_id WHERE m.id = ?1"
        ),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("消息 {id}")))
}

/// 把消息链接到产生它的运行。只给成员消息调用；用户消息、系统消息不链接。
pub fn set_run(conn: &Connection, message_id: i64, run_id: &str) -> AppResult<()> {
    let changed = conn.execute("UPDATE messages SET run_id = ?1 WHERE id = ?2", params![run_id, message_id])?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("消息 {message_id}")));
    }
    Ok(())
}

/// 返回游标之前的最新一页消息，并按自增 ID 升序排列。
pub fn list(
    conn: &Connection,
    project_id: &str,
    before_id: Option<i64>,
    limit: usize,
) -> AppResult<Vec<Message>> {
    let limit = limit.clamp(1, 200) as i64;
    let mut statement = conn.prepare(&format!(
        "SELECT {SELECT_COLUMNS} \
         FROM ( \
             SELECT id, project_id, sender_type, sender_agent_id, kind, content, round, created_at, run_id \
             FROM messages \
             WHERE project_id = ?1 AND (?2 IS NULL OR id < ?2) \
             ORDER BY id DESC LIMIT ?3 \
         ) m LEFT JOIN runs r ON r.id = m.run_id ORDER BY m.id ASC"
    ))?;
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

    fn member(conn: &Connection, project_id: &str) -> String {
        use crate::store::agents::{self, NewAgent};
        let persona = "## 定位\n开发\n## 职责\n测试\n## 边界\n不越界\n## 工作风格\n务实";
        agents::create(conn, project_id, &NewAgent {
            name: "成员".into(), role: "开发".into(), persona: persona.into(),
            cli: "claude".into(), model: None, is_leader: true, reasoning_effort: None,
        }).unwrap().id
    }

    #[test]
    fn message_without_run_has_no_model_info() {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "", "/tmp/项目").unwrap();
        let agent_id = member(&conn, &project.id);
        let message = insert(&conn, &project.id, "agent", Some(&agent_id), "chat", "旧消息").unwrap();
        assert_eq!(message.model_info, None);
        assert_eq!(get(&conn, message.id).unwrap().model_info, None);
        assert_eq!(list(&conn, &project.id, None, 10).unwrap()[0].model_info, None);
    }

    #[test]
    fn set_run_links_message_and_exposes_both_models_everywhere() {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "", "/tmp/项目").unwrap();
        let agent_id = member(&conn, &project.id);
        let message = insert(&conn, &project.id, "agent", Some(&agent_id), "chat", "").unwrap();
        let run = crate::store::runs::insert_running(&conn, Some(&agent_id), "execute", "{}", None).unwrap();
        crate::store::runs::set_requested_model(&conn, &run.id, Some("claude-opus-5-5")).unwrap();
        crate::store::runs::set_reported_model(&conn, &run.id, "claude-opus-5-5-20261001").unwrap();

        assert_eq!(get(&conn, message.id).unwrap().model_info, None);
        set_run(&conn, message.id, &run.id).unwrap();

        let expected = Some(ModelInfo {
            requested_model: Some("claude-opus-5-5".into()),
            reported_model: Some("claude-opus-5-5-20261001".into()),
            run_status: Some("running".into()),
        });
        assert_eq!(get(&conn, message.id).unwrap().model_info, expected);
        assert_eq!(list(&conn, &project.id, None, 10).unwrap()[0].model_info, expected);
        // 更新正文后返回的消息也带模型信息。
        assert_eq!(update_content(&conn, message.id, "最终正文").unwrap().model_info, expected);
    }

    #[test]
    fn run_without_recorded_model_is_some_with_empty_values() {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "", "/tmp/项目").unwrap();
        let agent_id = member(&conn, &project.id);
        let message = insert(&conn, &project.id, "agent", Some(&agent_id), "chat", "回复").unwrap();
        let run = crate::store::runs::insert_running(&conn, Some(&agent_id), "execute", "{}", None).unwrap();
        set_run(&conn, message.id, &run.id).unwrap();
        let empty = Some(ModelInfo { requested_model: None, reported_model: None, run_status: Some("running".into()) });
        assert_eq!(get(&conn, message.id).unwrap().model_info, empty);
        assert_eq!(list(&conn, &project.id, None, 10).unwrap()[0].model_info, empty);
        // 序列化为 camelCase，且无运行时是 null。
        let json = serde_json::to_value(get(&conn, message.id).unwrap()).unwrap();
        assert_eq!(json["modelInfo"], serde_json::json!({ "requestedModel": null, "reportedModel": null, "runStatus": "running" }));
    }

    #[test]
    fn set_run_rejects_missing_message() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(set_run(&conn, 9999, "不存在"), Err(AppError::NotFound(_))));
    }
    #[test]
    fn model_info_exposes_current_run_status_in_camel_case() {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "", "/tmp/项目").unwrap();
        let agent_id = member(&conn, &project.id);
        let message = insert(&conn, &project.id, "agent", Some(&agent_id), "chat", "回复").unwrap();
        let run = crate::store::runs::insert_running(&conn, Some(&agent_id), "execute", "{}", None).unwrap();
        set_run(&conn, message.id, &run.id).unwrap();
        for status in ["running", "ok", "failed", "timeout", "cancelled", "interrupted"] {
            conn.execute("UPDATE runs SET status=?1 WHERE id=?2", params![status, run.id]).unwrap();
            for fetched in [get(&conn, message.id).unwrap(), list(&conn, &project.id, None, 10).unwrap().remove(0), update_content(&conn, message.id, "更新").unwrap()] {
                let json = serde_json::to_value(fetched).unwrap();
                assert_eq!(json["modelInfo"]["runStatus"], status);
            }
        }
        // 删除成员会级联删除运行，消息仍可读；没有运行状态时保持未知。
        conn.execute("DELETE FROM agents WHERE id=?1", [&agent_id]).unwrap();
        let json = serde_json::to_value(get(&conn, message.id).unwrap()).unwrap();
        assert_eq!(json["modelInfo"]["runStatus"], serde_json::Value::Null);
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
