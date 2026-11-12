use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::store::{messages, new_id, now_ms};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Run {
    pub id: String,
    pub agent_id: Option<String>,
    pub mode: String,
    pub command: String,
    pub session_id: Option<String>,
    pub started_at: i64,
    pub ended_at: Option<i64>,
    pub exit_code: Option<i32>,
    pub status: String,
    pub usage_json: Option<String>,
    pub error: Option<String>,
}

fn from_row(row: &Row) -> rusqlite::Result<Run> {
    Ok(Run {
        id: row.get(0)?,
        agent_id: row.get(1)?,
        mode: row.get(2)?,
        command: row.get(3)?,
        session_id: row.get(4)?,
        started_at: row.get(5)?,
        ended_at: row.get(6)?,
        exit_code: row.get(7)?,
        status: row.get(8)?,
        usage_json: row.get(9)?,
        error: row.get(10)?,
    })
}

const COLUMNS: &str =
    "id, agent_id, mode, command, session_id, started_at, ended_at, exit_code, status, usage_json, error";

pub fn insert_running(
    conn: &Connection,
    agent_id: Option<&str>,
    mode: &str,
    command: &str,
    session_id: Option<&str>,
) -> AppResult<Run> {
    let id = new_id();
    conn.execute(
        "INSERT INTO runs (id, agent_id, mode, command, session_id, started_at, status) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running')",
        params![id, agent_id, mode, command, session_id, now_ms()],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Run> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM runs WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("运行 {id}")))
}

/// 记录本次运行请求的模型（实际传给适配器的 `TurnRequest.model`）。空值或纯空白写 NULL，表示由 CLI 自己决定。
/// 与 `session_id` 一样，只改仍在运行的记录。
pub fn set_requested_model(conn: &Connection, run_id: &str, model: Option<&str>) -> AppResult<()> {
    let model = model.map(str::trim).filter(|model| !model.is_empty());
    conn.execute(
        "UPDATE runs SET requested_model = ?1 WHERE id = ?2 AND status = 'running'",
        params![model, run_id],
    )?;
    Ok(())
}

/// 记录 CLI 在事件流里回报的实际模型。只改仍在运行的记录，迟到的事件不覆盖已结束的运行。
pub fn set_reported_model(conn: &Connection, run_id: &str, model: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE runs SET reported_model = ?1 WHERE id = ?2 AND status = 'running'",
        params![model, run_id],
    )?;
    Ok(())
}

/// 仅将 running 转为终态，返回是否成功完成了这次状态转换。
pub fn finish(
    conn: &Connection,
    id: &str,
    status: &str,
    exit_code: Option<i32>,
    usage_json: Option<&str>,
    error: Option<&str>,
) -> AppResult<bool> {
    if !matches!(status, "ok" | "failed" | "timeout" | "cancelled" | "interrupted") {
        return Err(AppError::Invalid("运行终态无效".into()));
    }
    let changed = conn.execute(
        "UPDATE runs SET status = ?1, ended_at = ?2, exit_code = ?3, usage_json = ?4, error = ?5 \
         WHERE id = ?6 AND status = 'running'",
        params![status, now_ms(), exit_code, usage_json, error, id],
    )?;
    Ok(changed == 1)
}

/// 将当前仍运行的记录原子标记为 interrupted，并返回本次被中断的快照。
pub fn interrupt_running(conn: &Connection) -> AppResult<Vec<Run>> {
    let tx = conn.unchecked_transaction()?;
    let interrupted = interrupt_running_in_transaction(&tx)?;
    tx.commit()?;
    Ok(interrupted)
}

/// 与运行所有权恢复共用调用方事务，防止 interrupted 和清理标记分离。
pub fn interrupt_running_in_transaction(tx: &Connection) -> AppResult<Vec<Run>> {
    let project_ids = {
        let mut statement = tx.prepare(
            "SELECT DISTINCT p.id FROM runs r \
             JOIN agents a ON a.id = r.agent_id \
             JOIN projects p ON p.id = a.project_id \
             WHERE r.status = 'running' ORDER BY p.id",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut projects = Vec::new();
        for row in rows {
            projects.push(row?);
        }
        projects
    };
    let mut runs = {
        let mut statement = tx.prepare(&format!(
            "SELECT {COLUMNS} FROM runs WHERE status = 'running' ORDER BY started_at, rowid"
        ))?;
        let rows = statement.query_map([], from_row)?;
        let mut active = Vec::new();
        for row in rows {
            active.push(row?);
        }
        active
    };

    if !runs.is_empty() {
        let ended_at = now_ms();
        tx.execute(
            "UPDATE runs SET status = 'interrupted', ended_at = ?1, \
             error = COALESCE(error, '应用重启时运行状态被中断') \
             WHERE status = 'running'",
            [ended_at],
        )?;
        for project_id in project_ids {
            tx.execute(
                "UPDATE projects SET status = 'waiting_user', updated_at = ?1 WHERE id = ?2",
                params![ended_at, project_id],
            )?;
            messages::insert(
                &tx,
                &project_id,
                "system",
                None,
                "system",
                "应用重启时运行状态被中断，请检查项目和文件状态后再继续。",
            )?;
        }
        for run in &mut runs {
            run.status = "interrupted".into();
            run.ended_at = Some(ended_at);
            if run.error.is_none() {
                run.error = Some("应用重启时运行状态被中断".into());
            }
        }
    }
    Ok(runs)
}

/// Grok 只续接明确在当前模式下成功过、且没有混用模式的会话。
pub fn resumable_session<'a>(conn: &Connection, agent: &'a super::agents::Agent, mode: &str) -> AppResult<Option<&'a str>> {
    let session = agent.session_id.as_deref();
    let Some(id) = session.filter(|_| agent.cli == "grok") else { return Ok(session); };
    let previous: Option<String> = conn.query_row(
        "SELECT CASE WHEN COUNT(DISTINCT mode) = 1 THEN MIN(mode) END FROM runs \
         WHERE agent_id = ?1 AND session_id = ?2 AND status = 'ok'",
        params![agent.id, id], |row| row.get(0),
    )?;
    Ok(session.filter(|_| previous.as_deref() == Some(mode)))
}

/// 读取同成员、同 CLI 会话最近一次保存的 Usage JSON，包含累计 rawUsage。
pub fn latest_usage(conn: &Connection, agent_id: &str, session_id: &str) -> AppResult<Option<Value>> {
    let usage_json: Option<String> = conn
        .query_row(
            "SELECT usage_json FROM runs \
             WHERE agent_id = ?1 AND session_id = ?2 AND usage_json IS NOT NULL \
             ORDER BY started_at DESC, rowid DESC LIMIT 1",
            params![agent_id, session_id],
            |row| row.get(0),
        )
        .optional()?
        .flatten();
    usage_json
        .map(|raw| {
            serde_json::from_str(&raw)
                .map_err(|_| AppError::Invalid("已保存的用量数据格式无效".into()))
        })
        .transpose()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{agents, open_in_memory, projects};

    const PERSONA: &str =
        "## 定位\n\n测试成员\n\n## 职责\n\n干活\n\n## 边界\n\n不越界\n\n## 工作风格\n\n务实\n";

    fn setup() -> (rusqlite::Connection, String, String) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "测试项目", "", "/tmp/测试项目").unwrap();
        let agent = agents::create(
            &conn,
            &project.id,
            &agents::NewAgent {
                name: "开发".into(),
                role: "开发".into(),
                persona: PERSONA.into(),
                cli: "codex".into(),
                model: None,
                is_leader: true,
            },
        )
        .unwrap();
        (conn, project.id, agent.id)
    }

    #[test]
    fn insert_running_persists_initial_state_without_prompt() {
        let (conn, _, agent_id) = setup();
        let run = insert_running(
            &conn,
            Some(&agent_id),
            "execute",
            "/usr/local/bin/codex exec --json -",
            Some("thread-1"),
        )
        .unwrap();

        assert_eq!(run.status, "running");
        assert!(run.started_at > 0);
        assert_eq!(run.agent_id.as_deref(), Some(agent_id.as_str()));
        assert_eq!(run.session_id.as_deref(), Some("thread-1"));
        assert_eq!(run.ended_at, None);
        assert_eq!(run.exit_code, None);
        assert_eq!(run.usage_json, None);
        assert_eq!(run.error, None);
        assert!(!run.command.contains("提示词"));
    }

    #[test]
    fn finish_records_success_and_failure() {
        let (conn, _, agent_id) = setup();
        let success = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        let usage = r#"{"inputTokens":12,"rawUsage":{"input_tokens":12}}"#;
        assert!(finish(&conn, &success.id, "ok", Some(0), Some(usage), None).unwrap());
        let saved = get(&conn, &success.id).unwrap();
        assert_eq!(saved.status, "ok");
        assert!(saved.ended_at.is_some());
        assert_eq!(saved.exit_code, Some(0));
        assert_eq!(saved.usage_json.as_deref(), Some(usage));
        assert_eq!(saved.error, None);

        let failed = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        assert!(finish(&conn, &failed.id, "failed", Some(1), None, Some("CLI 失败")).unwrap());
        let saved = get(&conn, &failed.id).unwrap();
        assert_eq!(saved.status, "failed");
        assert_eq!(saved.exit_code, Some(1));
        assert_eq!(saved.error.as_deref(), Some("CLI 失败"));
    }

    #[test]
    fn finish_only_transitions_running_run_once() {
        let (conn, _, agent_id) = setup();
        let run = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        assert!(finish(&conn, &run.id, "cancelled", None, None, Some("已停止")).unwrap());
        assert!(!finish(&conn, &run.id, "ok", Some(0), None, None).unwrap());
        let saved = get(&conn, &run.id).unwrap();
        assert_eq!(saved.status, "cancelled");
        assert_eq!(saved.error.as_deref(), Some("已停止"));
    }

    #[test]
    fn interrupt_running_is_idempotent_and_preserves_finished_runs() {
        let (conn, project_id, agent_id) = setup();
        let succeeded = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        let running = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        finish(&conn, &succeeded.id, "ok", Some(0), None, None).unwrap();

        let interrupted = interrupt_running(&conn).unwrap();
        assert_eq!(interrupted.len(), 1);
        assert_eq!(interrupted[0].id, running.id);
        assert_eq!(get(&conn, &running.id).unwrap().status, "interrupted");
        assert_eq!(get(&conn, &succeeded.id).unwrap().status, "ok");
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "waiting_user");
        let notifications = crate::store::messages::list(&conn, &project_id, None, 200).unwrap();
        assert_eq!(notifications.len(), 1);
        assert_eq!(notifications[0].sender_type, "system");
        assert!(interrupt_running(&conn).unwrap().is_empty());
        assert_eq!(crate::store::messages::list(&conn, &project_id, None, 200).unwrap(), notifications);
    }

    fn models(conn: &rusqlite::Connection, run_id: &str) -> (Option<String>, Option<String>) {
        conn.query_row("SELECT requested_model, reported_model FROM runs WHERE id = ?1", [run_id],
            |row| Ok((row.get(0)?, row.get(1)?))).unwrap()
    }

    #[test]
    fn requested_model_is_written_while_running_and_empty_means_null() {
        let (conn, _, agent_id) = setup();
        let run = insert_running(&conn, Some(&agent_id), "execute", "codex exec", None).unwrap();
        assert_eq!(models(&conn, &run.id), (None, None));
        set_requested_model(&conn, &run.id, Some("gpt-5.5")).unwrap();
        assert_eq!(models(&conn, &run.id), (Some("gpt-5.5".into()), None));
        for empty in [None, Some(""), Some("   ")] {
            set_requested_model(&conn, &run.id, empty).unwrap();
            assert_eq!(models(&conn, &run.id), (None, None), "{empty:?}");
        }
        // 不存在的运行不报错，也不会写出任何行。
        set_requested_model(&conn, "不存在", Some("x")).unwrap();
    }

    #[test]
    fn reported_model_is_written_only_while_running() {
        let (conn, _, agent_id) = setup();
        let running = insert_running(&conn, Some(&agent_id), "execute", "claude -p", None).unwrap();
        set_reported_model(&conn, &running.id, "claude-sonnet-5-5").unwrap();
        assert_eq!(models(&conn, &running.id), (None, Some("claude-sonnet-5-5".into())));

        let finished = insert_running(&conn, Some(&agent_id), "execute", "claude -p", None).unwrap();
        finish(&conn, &finished.id, "ok", Some(0), None, None).unwrap();
        set_reported_model(&conn, &finished.id, "迟到的模型").unwrap();
        assert_eq!(models(&conn, &finished.id), (None, None));
    }

    #[test]
    fn latest_usage_is_scoped_to_agent_and_session() {
        let (conn, _, agent_id) = setup();
        let previous = insert_running(
            &conn,
            Some(&agent_id),
            "execute",
            "codex exec",
            Some("thread-1"),
        )
        .unwrap();
        let first = r#"{"inputTokens":12,"rawUsage":{"input_tokens":12}}"#;
        finish(&conn, &previous.id, "ok", Some(0), Some(first), None).unwrap();

        assert_eq!(latest_usage(&conn, &agent_id, "thread-1").unwrap().unwrap(), serde_json::from_str::<serde_json::Value>(first).unwrap());
        assert_eq!(latest_usage(&conn, &agent_id, "thread-2").unwrap(), None);
        assert_eq!(latest_usage(&conn, "other-agent", "thread-1").unwrap(), None);
    }
}
