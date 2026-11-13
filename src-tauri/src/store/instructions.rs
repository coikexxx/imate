//! 指令、运行中的输入队列，以及同一批次的任务。
//! 同一项目同时只能有一条 `running` 或 `waiting_user` 指令。

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::store::{agents, messages, new_id, now_ms, projects};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Instruction {
    pub id: String,
    pub project_id: String,
    pub source_message_id: i64,
    pub kind: String,
    pub target_agent_id: Option<String>,
    pub phase: String,
    pub status: String,
    pub discuss_rounds: i64,
    pub turn_count: i64,
    pub current_batch: Option<String>,
    pub resume_phase: Option<String>,
    pub context_snapshot_json: Option<String>,
    pub wait_reason: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub finished_at: Option<i64>,
    /// 用户允许这条指令在权限受阻时自动用最高权限重跑。
    pub elevated_allowed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InstructionInput {
    pub id: String,
    pub instruction_id: String,
    pub message_id: i64,
    pub kind: String,
    pub target_agent_id: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskRecord {
    pub id: String,
    pub project_id: String,
    pub instruction_message_id: Option<i64>,
    pub assignee_agent_id: Option<String>,
    pub title: String,
    pub detail: String,
    pub order_index: i64,
    pub status: String,
    pub rework_count: i64,
    pub report_message_id: Option<i64>,
    pub created_at: i64,
    pub finished_at: Option<i64>,
    pub instruction_id: Option<String>,
    pub batch_id: Option<String>,
    pub superseded_by_task_id: Option<String>,
}

/// 一批派工里的单项。`replaces_task_id` 指向同一指令里被替代的旧任务。
#[derive(Debug, Clone, PartialEq)]
pub struct NewTask {
    pub assignee_agent_id: String,
    pub title: String,
    pub detail: String,
    pub replaces_task_id: Option<String>,
}

/// 一次派工事务的结果。失败时决议消息和任务都不会留下。
#[derive(Debug, Clone, PartialEq)]
pub struct InsertedBatch {
    pub batch_id: String,
    pub decision_message_id: i64,
    pub tasks: Vec<TaskRecord>,
}

const INSTRUCTION_COLUMNS: &str = "id, project_id, source_message_id, kind, target_agent_id, phase, status, \
     discuss_rounds, turn_count, current_batch, resume_phase, context_snapshot_json, wait_reason, \
     created_at, updated_at, finished_at, elevated_allowed";

const TASK_COLUMNS: &str = "id, project_id, instruction_message_id, assignee_agent_id, title, detail, order_index, \
     status, rework_count, report_message_id, created_at, finished_at, instruction_id, batch_id, superseded_by_task_id";

const INPUT_COLUMNS: &str = "id, instruction_id, message_id, kind, target_agent_id, status, created_at, finished_at";

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Invalid(message.into())
}

fn instruction_from_row(row: &Row) -> rusqlite::Result<Instruction> {
    Ok(Instruction {
        id: row.get(0)?,
        project_id: row.get(1)?,
        source_message_id: row.get(2)?,
        kind: row.get(3)?,
        target_agent_id: row.get(4)?,
        phase: row.get(5)?,
        status: row.get(6)?,
        discuss_rounds: row.get(7)?,
        turn_count: row.get(8)?,
        current_batch: row.get(9)?,
        resume_phase: row.get(10)?,
        context_snapshot_json: row.get(11)?,
        wait_reason: row.get(12)?,
        created_at: row.get(13)?,
        updated_at: row.get(14)?,
        finished_at: row.get(15)?,
        elevated_allowed: row.get(16)?,
    })
}

fn task_from_row(row: &Row) -> rusqlite::Result<TaskRecord> {
    Ok(TaskRecord {
        id: row.get(0)?,
        project_id: row.get(1)?,
        instruction_message_id: row.get(2)?,
        assignee_agent_id: row.get(3)?,
        title: row.get(4)?,
        detail: row.get(5)?,
        order_index: row.get(6)?,
        status: row.get(7)?,
        rework_count: row.get(8)?,
        report_message_id: row.get(9)?,
        created_at: row.get(10)?,
        finished_at: row.get(11)?,
        instruction_id: row.get(12)?,
        batch_id: row.get(13)?,
        superseded_by_task_id: row.get(14)?,
    })
}

fn input_from_row(row: &Row) -> rusqlite::Result<InstructionInput> {
    Ok(InstructionInput {
        id: row.get(0)?,
        instruction_id: row.get(1)?,
        message_id: row.get(2)?,
        kind: row.get(3)?,
        target_agent_id: row.get(4)?,
        status: row.get(5)?,
        created_at: row.get(6)?,
        finished_at: row.get(7)?,
    })
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Instruction> {
    conn.query_row(
        &format!("SELECT {INSTRUCTION_COLUMNS} FROM instructions WHERE id = ?1"),
        [id],
        instruction_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("指令 {id}")))
}

pub fn get_task(conn: &Connection, id: &str) -> AppResult<TaskRecord> {
    conn.query_row(
        &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
        [id],
        task_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("任务 {id}")))
}

/// 项目里唯一的进行中或等待用户的指令。同时出现多条时拒绝继续调度。
pub fn active_instruction(conn: &Connection, project_id: &str) -> AppResult<Option<Instruction>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INSTRUCTION_COLUMNS} FROM instructions \
         WHERE project_id = ?1 AND status IN ('running', 'waiting_user') ORDER BY created_at"
    ))?;
    let rows = stmt.query_map([project_id], instruction_from_row)?;
    let mut found = Vec::new();
    for row in rows {
        found.push(row?);
    }
    match found.len() {
        0 => Ok(None),
        1 => Ok(found.pop()),
        _ => Err(invalid("该项目有多条未结束指令")),
    }
}

pub fn list_tasks(conn: &Connection, instruction_id: &str) -> AppResult<Vec<TaskRecord>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {TASK_COLUMNS} FROM tasks WHERE instruction_id = ?1 ORDER BY order_index, created_at"
    ))?;
    let rows = stmt.query_map([instruction_id], task_from_row)?;
    let mut tasks = Vec::new();
    for row in rows {
        tasks.push(row?);
    }
    Ok(tasks)
}

pub fn list_inputs(conn: &Connection, instruction_id: &str) -> AppResult<Vec<InstructionInput>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {INPUT_COLUMNS} FROM instruction_inputs WHERE instruction_id = ?1 ORDER BY created_at, message_id"
    ))?;
    let rows = stmt.query_map([instruction_id], input_from_row)?;
    let mut inputs = Vec::new();
    for row in rows {
        inputs.push(row?);
    }
    Ok(inputs)
}

/// 新建指令。已有进行中的指令时拒绝；已有等待用户的指令时，在同一事务里将其改为放弃。
pub fn insert_instruction(
    conn: &Connection,
    project_id: &str,
    source_message_id: i64,
    kind: &str,
    target_agent_id: Option<&str>,
    phase: &str,
) -> AppResult<Instruction> {
    if !matches!(kind, "team" | "direct") {
        return Err(invalid("指令类型无效"));
    }
    if !matches!(phase, "triage" | "discuss" | "decide" | "execute" | "review" | "intervention" | "direct") {
        return Err(invalid("指令阶段无效"));
    }
    projects::get(conn, project_id)?;
    let message = messages::get(conn, source_message_id)?;
    if message.project_id != project_id {
        return Err(invalid("消息不属于该项目"));
    }
    if let Some(agent_id) = target_agent_id {
        let agent = agents::get(conn, agent_id)?;
        if agent.project_id != project_id {
            return Err(invalid("目标成员不属于该项目"));
        }
    } else if kind == "direct" {
        return Err(invalid("直接指令必须指定成员"));
    }

    let tx = conn.unchecked_transaction()?;
    if let Some(existing) = active_instruction(&tx, project_id)? {
        if existing.status == "running" {
            return Err(invalid("该项目已有进行中的指令"));
        }
        let now = now_ms();
        tx.execute(
            "UPDATE instructions SET status = 'abandoned', updated_at = ?1, finished_at = ?1 WHERE id = ?2",
            params![now, existing.id],
        )?;
        tx.execute(
            "UPDATE instruction_inputs SET status = 'cancelled', finished_at = ?1 \
             WHERE instruction_id = ?2 AND status = 'queued'",
            params![now, existing.id],
        )?;
    }
    let id = new_id();
    let now = now_ms();
    tx.execute(
        "INSERT INTO instructions (id, project_id, source_message_id, kind, target_agent_id, phase, status, \
         discuss_rounds, turn_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', 0, 0, ?7, ?7)",
        params![id, project_id, source_message_id, kind, target_agent_id, phase, now],
    )?;
    tx.commit()?;
    get(conn, &id)
}

/// 运行中的补充或点名入队，不打断当前回合。
pub fn enqueue_input(
    conn: &Connection,
    instruction_id: &str,
    message_id: i64,
    kind: &str,
    target_agent_id: Option<&str>,
) -> AppResult<InstructionInput> {
    if !matches!(kind, "supplement" | "direct") {
        return Err(invalid("输入类型无效"));
    }
    let tx = conn.unchecked_transaction()?;
    let instruction = get(&tx, instruction_id)?;
    if instruction.status != "running" {
        return Err(invalid("只有进行中的指令可以接收插话"));
    }
    let message = messages::get(&tx, message_id)?;
    if message.project_id != instruction.project_id {
        return Err(invalid("消息不属于该项目"));
    }
    if let Some(agent_id) = target_agent_id {
        let agent = agents::get(&tx, agent_id)?;
        if agent.project_id != instruction.project_id {
            return Err(invalid("点名成员不属于该项目"));
        }
    } else if kind == "direct" {
        return Err(invalid("点名必须指定成员"));
    }
    let id = new_id();
    let now = now_ms();
    tx.execute(
        "INSERT INTO instruction_inputs (id, instruction_id, message_id, kind, target_agent_id, status, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6)",
        params![id, instruction_id, message_id, kind, target_agent_id, now],
    )?;
    tx.commit()?;
    conn.query_row(
        &format!("SELECT {INPUT_COLUMNS} FROM instruction_inputs WHERE id = ?1"),
        [&id],
        input_from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("指令输入 {id}")))
}

/// 在一个事务里写入决议消息、全部任务和新批次。任一任务无效则整批回滚。
pub fn insert_batch(
    conn: &Connection,
    instruction_id: &str,
    leader_agent_id: Option<&str>,
    decision: &str,
    tasks: &[NewTask],
) -> AppResult<InsertedBatch> {
    if tasks.is_empty() || tasks.len() > 8 {
        return Err(invalid("任务数必须在 1 到 8 之间"));
    }
    let decision_text = decision.trim();
    if decision_text.is_empty() {
        return Err(invalid("decision 不能为空"));
    }

    let tx = conn.unchecked_transaction()?;
    let instruction = get(&tx, instruction_id)?;
    if let Some(leader_id) = leader_agent_id {
        let leader = agents::get(&tx, leader_id)?;
        if leader.project_id != instruction.project_id {
            return Err(invalid("决议成员不属于该项目"));
        }
    }
    let mut prepared = Vec::with_capacity(tasks.len());
    for task in tasks {
        let title = task.title.trim();
        let detail = task.detail.trim();
        if title.is_empty() {
            return Err(invalid("title 不能为空"));
        }
        if detail.is_empty() {
            return Err(invalid("detail 不能为空"));
        }
        let assignee = match agents::get(&tx, &task.assignee_agent_id) {
            Ok(agent) => agent,
            Err(AppError::NotFound(_)) => return Err(invalid(format!("成员「{}」无效", task.assignee_agent_id))),
            Err(error) => return Err(error),
        };
        if assignee.project_id != instruction.project_id {
            return Err(invalid(format!("成员「{}」不属于该项目", assignee.name)));
        }
        prepared.push((title.to_string(), detail.to_string(), assignee.id, task.replaces_task_id.clone()));
    }

    let decision_message = messages::insert(
        &tx,
        &instruction.project_id,
        if leader_agent_id.is_some() { "agent" } else { "system" },
        leader_agent_id,
        "decision",
        decision_text,
    )?;
    let batch_id = new_id();
    let now = now_ms();
    let mut task_ids = Vec::with_capacity(prepared.len());
    for (index, (title, detail, assignee_id, _)) in prepared.iter().enumerate() {
        let id = new_id();
        tx.execute(
            "INSERT INTO tasks (id, project_id, instruction_message_id, assignee_agent_id, title, detail, \
             order_index, status, rework_count, created_at, instruction_id, batch_id) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'pending', 0, ?8, ?9, ?10)",
            params![id, instruction.project_id, decision_message.id, assignee_id, title, detail, index as i64, now, instruction.id, batch_id],
        )?;
        task_ids.push(id);
    }
    for (id, (_, _, _, replaces)) in task_ids.iter().zip(prepared.iter()) {
        if let Some(replaced_id) = replaces {
            supersede_in(&tx, replaced_id, id)?;
        }
    }
    tx.execute(
        "UPDATE instructions SET current_batch = ?1, updated_at = ?2 WHERE id = ?3",
        params![batch_id, now, instruction.id],
    )?;
    tx.commit()?;

    let mut stored = Vec::with_capacity(task_ids.len());
    for id in &task_ids {
        stored.push(get_task(conn, id)?);
    }
    Ok(InsertedBatch { batch_id, decision_message_id: decision_message.id, tasks: stored })
}

/// 结束一个任务。失败时把同批尚未开始的任务改为取消，不删除任何行。
/// 受阻不算失败：保留同批未开始的任务，用户授权重试后还能继续这一批。
pub fn finish_task(
    conn: &Connection,
    task_id: &str,
    status: &str,
    report_message_id: Option<i64>,
) -> AppResult<TaskRecord> {
    if !matches!(status, "done" | "failed" | "cancelled" | "blocked") {
        return Err(invalid("任务终态无效"));
    }
    let tx = conn.unchecked_transaction()?;
    let task = get_task(&tx, task_id)?;
    if !matches!(task.status.as_str(), "pending" | "running") {
        return Err(invalid("任务已结束"));
    }
    let now = now_ms();
    tx.execute(
        "UPDATE tasks SET status = ?1, report_message_id = ?2, finished_at = ?3 WHERE id = ?4",
        params![status, report_message_id, now, task_id],
    )?;
    if status == "failed" {
        if let Some(batch_id) = task.batch_id.as_deref() {
            tx.execute(
                "UPDATE tasks SET status = 'cancelled', finished_at = ?1 \
                 WHERE instruction_id = ?2 AND batch_id = ?3 AND status = 'pending' AND id <> ?4",
                params![now, task.instruction_id, batch_id, task_id],
            )?;
        }
    }
    tx.commit()?;
    get_task(conn, task_id)
}

/// 取消一条指令里尚未开始的任务，返回取消数量。
pub fn cancel_unstarted(conn: &Connection, instruction_id: &str) -> AppResult<usize> {
    get(conn, instruction_id)?;
    let changed = conn.execute(
        "UPDATE tasks SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status = 'pending'",
        params![now_ms(), instruction_id],
    )?;
    Ok(changed)
}

/// 取消仍在队列中的插话，返回取消数量。
pub fn cancel_queued(conn: &Connection, instruction_id: &str) -> AppResult<usize> {
    get(conn, instruction_id)?;
    let changed = conn.execute(
        "UPDATE instruction_inputs SET status = 'cancelled', finished_at = ?1 \
         WHERE instruction_id = ?2 AND status = 'queued'",
        params![now_ms(), instruction_id],
    )?;
    Ok(changed)
}

/// 保留旧任务行，并记下替代它的新任务。拒绝自引用、跨指令和成环。
pub fn supersede_task(conn: &Connection, task_id: &str, successor_id: &str) -> AppResult<TaskRecord> {
    let tx = conn.unchecked_transaction()?;
    supersede_in(&tx, task_id, successor_id)?;
    tx.commit()?;
    get_task(conn, task_id)
}

fn supersede_in(conn: &Connection, task_id: &str, successor_id: &str) -> AppResult<()> {
    if task_id == successor_id {
        return Err(invalid("任务不能替代自己"));
    }
    let task = get_task(conn, task_id)?;
    let successor = get_task(conn, successor_id)?;
    match (&task.instruction_id, &successor.instruction_id) {
        (Some(left), Some(right)) if left == right => {}
        _ => return Err(invalid("只能替代同一条指令里的任务")),
    }
    if task.superseded_by_task_id.is_some() {
        return Err(invalid("任务已经被替代"));
    }
    if creates_cycle(conn, task_id, successor_id)? {
        return Err(invalid("任务替代会形成循环"));
    }
    conn.execute(
        "UPDATE tasks SET superseded_by_task_id = ?1 WHERE id = ?2",
        params![successor_id, task_id],
    )?;
    Ok(())
}

fn creates_cycle(conn: &Connection, origin: &str, successor_id: &str) -> AppResult<bool> {
    let mut cursor = successor_id.to_string();
    for _ in 0..64 {
        if cursor == origin {
            return Ok(true);
        }
        let next = conn
            .query_row("SELECT superseded_by_task_id FROM tasks WHERE id = ?1", [&cursor], |row| {
                row.get::<_, Option<String>>(0)
            })
            .optional()?;
        match next {
            None => return Err(AppError::NotFound(format!("任务 {cursor}"))),
            Some(None) => return Ok(false),
            Some(Some(id)) => cursor = id,
        }
    }
    Err(invalid("任务替代链过长"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::agents::NewAgent;
    use crate::store::{agents, messages, open_in_memory, projects};

    const PERSONA: &str = "## 定位\n\n本人定位\n\n## 职责\n\n本人职责\n\n## 边界\n\n本人边界\n\n## 工作风格\n\n本人工作风格\n";

    fn setup() -> (rusqlite::Connection, String, String, String) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "甲", "", "/tmp/甲").unwrap();
        let leader = agents::create(&conn, &project.id, &NewAgent {
            name: "组长".into(), role: "管理".into(), persona: PERSONA.into(),
            cli: "claude".into(), model: None, is_leader: true,
        }).unwrap();
        let member = agents::create(&conn, &project.id, &NewAgent {
            name: "前端".into(), role: "开发".into(), persona: PERSONA.into(),
            cli: "codex".into(), model: None, is_leader: false,
        }).unwrap();
        (conn, project.id, leader.id, member.id)
    }

    fn task(assignee: &str, title: &str) -> NewTask {
        NewTask { assignee_agent_id: assignee.into(), title: title.into(), detail: format!("{title}的细节"), replaces_task_id: None }
    }

    #[test]
    fn second_active_instruction_is_rejected_and_waiting_one_is_abandoned() {
        let (conn, project_id, _, _) = setup();
        let first_message = messages::insert(&conn, &project_id, "user", None, "instruction", "第一条").unwrap();
        let first = insert_instruction(&conn, &project_id, first_message.id, "team", None, "triage").unwrap();
        assert_eq!(first.status, "running");

        let second_message = messages::insert(&conn, &project_id, "user", None, "instruction", "第二条").unwrap();
        assert!(insert_instruction(&conn, &project_id, second_message.id, "team", None, "triage").is_err());
        assert_eq!(active_instruction(&conn, &project_id).unwrap().unwrap().id, first.id);

        conn.execute("UPDATE instructions SET status = 'waiting_user' WHERE id = ?1", [&first.id]).unwrap();
        assert!(insert_instruction(&conn, &project_id, first_message.id, "team", None, "triage").is_err());
        assert_eq!(get(&conn, &first.id).unwrap().status, "waiting_user");

        let third_message = messages::insert(&conn, &project_id, "user", None, "instruction", "第三条").unwrap();
        let third = insert_instruction(&conn, &project_id, third_message.id, "team", None, "triage").unwrap();
        assert_eq!(get(&conn, &first.id).unwrap().status, "abandoned");
        assert_eq!(third.status, "running");
        let active = active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(active.id, third.id);
        assert_eq!(active.status, "running");
    }

    #[test]
    fn failed_task_cancels_unstarted_siblings_and_keeps_rows() {
        let (conn, project_id, leader_id, member_id) = setup();
        let message = messages::insert(&conn, &project_id, "user", None, "instruction", "做三件事").unwrap();
        let instruction = insert_instruction(&conn, &project_id, message.id, "team", None, "triage").unwrap();
        let inserted = insert_batch(&conn, &instruction.id, Some(&leader_id), "先做接口", &[
            task(&member_id, "第一个"), task(&member_id, "第二个"), task(&member_id, "第三个"),
        ]).unwrap();
        assert_eq!(inserted.tasks.len(), 3);
        assert_eq!(inserted.tasks[0].order_index, 0);
        assert_eq!(inserted.tasks[0].batch_id, inserted.tasks[1].batch_id);
        assert_eq!(inserted.batch_id, inserted.tasks[0].batch_id.clone().unwrap());
        assert_eq!(get(&conn, &instruction.id).unwrap().current_batch.as_deref(), Some(inserted.batch_id.as_str()));
        let decision = messages::get(&conn, inserted.decision_message_id).unwrap();
        assert_eq!(decision.kind, "decision");
        assert_eq!(decision.content, "先做接口");

        conn.execute("UPDATE tasks SET status = 'running' WHERE id = ?1", [&inserted.tasks[0].id]).unwrap();
        let failed = finish_task(&conn, &inserted.tasks[1].id, "failed", None).unwrap();
        assert_eq!(failed.status, "failed");
        let tasks = list_tasks(&conn, &instruction.id).unwrap();
        assert_eq!(tasks.len(), 3);
        assert_eq!(tasks[0].status, "running");
        assert_eq!(tasks[1].status, "failed");
        assert_eq!(tasks[1].title, "第二个");
        assert_eq!(tasks[2].status, "cancelled");
    }

    #[test]
    fn invalid_task_rolls_back_the_whole_batch() {
        let (conn, project_id, leader_id, member_id) = setup();
        let message = messages::insert(&conn, &project_id, "user", None, "instruction", "派工").unwrap();
        let instruction = insert_instruction(&conn, &project_id, message.id, "team", None, "triage").unwrap();
        let mut broken = task(&member_id, "好任务");
        broken.assignee_agent_id = "missing".into();
        assert!(insert_batch(&conn, &instruction.id, Some(&leader_id), "不会落库", &[task(&member_id, "好任务"), broken]).is_err());
        assert!(list_tasks(&conn, &instruction.id).unwrap().is_empty());
        assert!(get(&conn, &instruction.id).unwrap().current_batch.is_none());
        let stored = messages::list(&conn, &project_id, None, 20).unwrap();
        assert!(stored.iter().all(|message| message.kind != "decision"));
    }

    #[test]
    fn supersede_keeps_the_row_and_rejects_self_cross_instruction_and_cycles() {
        let (conn, project_id, leader_id, member_id) = setup();
        let message = messages::insert(&conn, &project_id, "user", None, "instruction", "原指令").unwrap();
        let instruction = insert_instruction(&conn, &project_id, message.id, "team", None, "triage").unwrap();
        let inserted = insert_batch(&conn, &instruction.id, Some(&leader_id), "三步", &[
            task(&member_id, "第一个"), task(&member_id, "第二个"), task(&member_id, "第三个"),
        ]).unwrap();
        let (first, second, third) = (&inserted.tasks[0].id, &inserted.tasks[1].id, &inserted.tasks[2].id);

        let kept = supersede_task(&conn, first, second).unwrap();
        assert_eq!(kept.title, "第一个");
        assert_eq!(kept.superseded_by_task_id.as_deref(), Some(second.as_str()));
        assert_eq!(list_tasks(&conn, &instruction.id).unwrap().len(), 3);

        assert!(supersede_task(&conn, second, second).is_err());
        assert!(supersede_task(&conn, second, first).is_err());
        supersede_task(&conn, second, third).unwrap();
        assert!(supersede_task(&conn, third, first).is_err());
        assert_eq!(list_tasks(&conn, &instruction.id).unwrap()[0].title, "第一个");

        let other_project = projects::create(&conn, "乙", "", "/tmp/乙").unwrap();
        let other_leader = agents::create(&conn, &other_project.id, &NewAgent {
            name: "组长".into(), role: "管理".into(), persona: PERSONA.into(),
            cli: "claude".into(), model: None, is_leader: true,
        }).unwrap();
        let other_message = messages::insert(&conn, &other_project.id, "user", None, "instruction", "另一条").unwrap();
        let other = insert_instruction(&conn, &other_project.id, other_message.id, "team", None, "triage").unwrap();
        let other_batch = insert_batch(&conn, &other.id, Some(&other_leader.id), "别处", &[task(&other_leader.id, "外部")]).unwrap();
        assert!(supersede_task(&conn, first, &other_batch.tasks[0].id).is_err());
    }

    #[test]
    fn queued_input_stops_when_waiting_and_cancel_unstarted_skips_started_tasks() {
        let (conn, project_id, leader_id, member_id) = setup();
        let message = messages::insert(&conn, &project_id, "user", None, "instruction", "原指令").unwrap();
        let instruction = insert_instruction(&conn, &project_id, message.id, "team", None, "triage").unwrap();
        let extra = messages::insert(&conn, &project_id, "user", None, "chat", "补充一下").unwrap();
        let queued = enqueue_input(&conn, &instruction.id, extra.id, "supplement", None).unwrap();
        assert_eq!(queued.status, "queued");
        assert_eq!(queued.kind, "supplement");

        let inserted = insert_batch(&conn, &instruction.id, Some(&leader_id), "批次", &[
            task(&member_id, "已完成"), task(&member_id, "正在做"), task(&member_id, "还没做"),
        ]).unwrap();
        finish_task(&conn, &inserted.tasks[0].id, "done", None).unwrap();
        conn.execute("UPDATE tasks SET status = 'running' WHERE id = ?1", [&inserted.tasks[1].id]).unwrap();
        assert_eq!(cancel_unstarted(&conn, &instruction.id).unwrap(), 1);
        let tasks = list_tasks(&conn, &instruction.id).unwrap();
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].status, "running");
        assert_eq!(tasks[2].status, "cancelled");

        conn.execute("UPDATE instructions SET status = 'waiting_user' WHERE id = ?1", [&instruction.id]).unwrap();
        let later = messages::insert(&conn, &project_id, "user", None, "chat", "再补充").unwrap();
        assert!(enqueue_input(&conn, &instruction.id, later.id, "supplement", None).is_err());
    }
}
