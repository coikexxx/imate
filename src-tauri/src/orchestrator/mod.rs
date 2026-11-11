pub mod prompts;
pub mod protocol;

use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::rules::{self, RuleDoc};
use crate::runner::RunOutcome;
use crate::store::agents::{self, Agent};
use crate::store::instructions::{self, Instruction, NewTask, TaskRecord};
use crate::store::messages::{self, Message};
use crate::store::projects::{self, Project};
use crate::store::{new_id, now_ms};
use prompts::{Stage, TaskBrief, TeamSnapshot};
use protocol::{parse_final, ActionKind, LimitContext, Phase, TaskLimit};

const TURN_LIMIT: i64 = 30;

/// 事务提交后才交给调用方启动的回合。启动失败不能退回已经预留的回合数。
#[derive(Debug, Clone, PartialEq)]
pub struct TurnSpec {
    pub run_id: String,
    pub agent_id: String,
    pub instruction_id: String,
    pub task_id: Option<String>,
    pub mode: String,
    pub prompt: String,
    pub message_bound: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredSnapshot {
    message_bound: Option<i64>,
    rules_version: Option<String>,
    roster_version: Option<String>,
    pending: Option<StoredPending>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "lowercase")]
enum StoredPending {
    Discuss { agent_ids: Vec<String>, topic: String, launched: bool },
}

struct RunRow {
    id: String,
    agent_id: String,
    mode: String,
    status: String,
    instruction_id: String,
    task_id: Option<String>,
}

/// 为当前指令准备下一批回合。已有运行中的 run 时不重复启动。
pub fn advance(conn: &Connection, project_id: &str) -> AppResult<Vec<TurnSpec>> {
    let Some(active) = instructions::active_instruction(conn, project_id)? else {
        return Ok(Vec::new());
    };
    if active.status != "running" {
        return Ok(Vec::new());
    }
    let tx = conn.unchecked_transaction()?;
    if running_count(&tx, &active.id)? > 0 {
        tx.commit()?;
        return Ok(Vec::new());
    }
    let instruction = instructions::get(&tx, &active.id)?;
    if instruction.status != "running" {
        tx.commit()?;
        return Ok(Vec::new());
    }
    let specs = dispatch(&tx, &instruction)?;
    tx.commit()?;
    Ok(specs)
}

/// 记录一个已经预留的回合结果，并推进阶段。不在这里启动下一回合。
pub fn apply_outcome(conn: &Connection, run_id: &str, outcome: &RunOutcome) -> AppResult<()> {
    let run = load_run(conn, run_id)?;
    if run.status != "running" {
        return Err(invalid("运行已结束"));
    }
    let instruction = instructions::get(conn, &run.instruction_id)?;
    let agent = agents::get(conn, &run.agent_id)?;
    if run.mode == "execute" {
        apply_execute(conn, &run, &instruction, &agent, outcome)
    } else if agent.is_leader {
        apply_leader(conn, &run, &instruction, &agent, outcome)
    } else {
        apply_member_discuss(conn, &instruction, &agent, run_id, outcome)
    }
}

/// 进程没能启动。只终结这次 run，不退回预留的回合数。
pub fn fail_launch(conn: &Connection, run_id: &str, error: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE runs SET status = 'failed', ended_at = ?1, error = ?2 WHERE id = ?3 AND status = 'running'",
        params![now_ms(), error, run_id],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("运行 {run_id}")));
    }
    Ok(())
}

fn dispatch(conn: &Connection, instruction: &Instruction) -> AppResult<Vec<TurnSpec>> {
    match instruction.phase.as_str() {
        "triage" => schedule_leader(conn, instruction, Stage::Triage),
        "decide" => schedule_leader(conn, instruction, Stage::Decide),
        "review" => schedule_leader(conn, instruction, Stage::Review),
        "intervention" => schedule_leader(conn, instruction, Stage::Intervention),
        "discuss" => schedule_discuss(conn, instruction),
        "execute" => schedule_execute(conn, instruction),
        "direct" => Err(invalid("直接指令不走团队调度")),
        other => Err(invalid(format!("未知阶段 {other}"))),
    }
}

fn schedule_leader(conn: &Connection, instruction: &Instruction, stage: Stage) -> AppResult<Vec<TurnSpec>> {
    if !reserve(conn, instruction, 1)? {
        return Ok(Vec::new());
    }
    let leader = leader(conn, &instruction.project_id)?;
    let (prompt, bound) = compose_prompt(conn, instruction, &leader, stage, None, "")?;
    let run_id = insert_run(conn, &leader.id, "discuss", &instruction.id, None)?;
    set_project_status(conn, &instruction.project_id, "discussing")?;
    Ok(vec![spec(run_id, leader.id, &instruction.id, None, "discuss", prompt, bound)])
}

fn schedule_discuss(conn: &Connection, instruction: &Instruction) -> AppResult<Vec<TurnSpec>> {
    let mut stored = load_snapshot(&instruction.context_snapshot_json)?;
    let Some(StoredPending::Discuss { agent_ids, topic, launched }) = stored.pending.clone() else {
        return Err(invalid("讨论阶段缺少成员和议题"));
    };
    if launched {
        set_phase(conn, &instruction.id, "decide")?;
        let refreshed = instructions::get(conn, &instruction.id)?;
        return schedule_leader(conn, &refreshed, Stage::Decide);
    }
    if !reserve(conn, instruction, agent_ids.len() as i64)? {
        return Ok(Vec::new());
    }
    let roster = agents::list(conn, &instruction.project_id)?;
    let bound = message_bound(conn, &instruction.project_id)?;
    let mut specs = Vec::with_capacity(agent_ids.len());
    for agent_id in &agent_ids {
        let agent = roster.iter().find(|agent| agent.id == *agent_id).cloned()
            .ok_or_else(|| invalid(format!("成员 {agent_id} 不在花名册中")))?;
        let (prompt, prompt_bound) = compose_prompt(conn, instruction, &agent, Stage::Discuss, None, &topic)?;
        let run_id = insert_run(conn, &agent.id, "discuss", &instruction.id, None)?;
        specs.push(spec(run_id, agent.id, &instruction.id, None, "discuss", prompt, prompt_bound));
    }
    stored.pending = Some(StoredPending::Discuss { agent_ids, topic, launched: true });
    stored.message_bound = Some(bound);
    save_snapshot(conn, &instruction.id, &stored)?;
    set_project_status(conn, &instruction.project_id, "discussing")?;
    Ok(specs)
}

fn schedule_execute(conn: &Connection, instruction: &Instruction) -> AppResult<Vec<TurnSpec>> {
    let Some(batch_id) = instruction.current_batch.clone() else {
        set_phase(conn, &instruction.id, "review")?;
        let refreshed = instructions::get(conn, &instruction.id)?;
        return schedule_leader(conn, &refreshed, Stage::Review);
    };
    let pending = next_pending(conn, &instruction.id, &batch_id)?;
    let Some(task) = pending else {
        set_phase(conn, &instruction.id, "review")?;
        let refreshed = instructions::get(conn, &instruction.id)?;
        return schedule_leader(conn, &refreshed, Stage::Review);
    };
    if !reserve(conn, instruction, 1)? {
        return Ok(Vec::new());
    }
    conn.execute("UPDATE tasks SET status = 'running' WHERE id = ?1", [&task.id])?;
    let assignee = agents::get(conn, task.assignee_agent_id.as_deref().unwrap_or(""))?;
    let (prompt, bound) = compose_prompt(conn, instruction, &assignee, Stage::Execute, Some(&task), "")?;
    let run_id = insert_run(conn, &assignee.id, "execute", &instruction.id, Some(&task.id))?;
    set_project_status(conn, &instruction.project_id, "executing")?;
    Ok(vec![spec(run_id, assignee.id, &instruction.id, Some(task.id), "execute", prompt, bound)])
}

fn apply_leader(conn: &Connection, run: &RunRow, instruction: &Instruction, agent: &Agent, outcome: &RunOutcome) -> AppResult<()> {
    if outcome.status != "ok" {
        finish_run(conn, &run.id, outcome, "failed")?;
        return mark_waiting(conn, instruction, outcome.error.as_deref().unwrap_or("组长回合失败"));
    }
    let roster = agents::list(conn, &instruction.project_id)?;
    let task = rework_limit(conn, &outcome.final_text)?;
    let limits = LimitContext { discuss_rounds: instruction.discuss_rounds.max(0) as u32, task };
    let parsed = match parse_final(&outcome.final_text, protocol_phase(&instruction.phase), &roster, &limits) {
        Ok(parsed) => parsed,
        Err(error) => {
            finish_run(conn, &run.id, outcome, "failed")?;
            mark_waiting(conn, instruction, &error.to_string())?;
            return Err(error);
        }
    };
    match parsed.action.kind {
        ActionKind::Assign { decision, tasks } => {
            let batch: Vec<NewTask> = tasks.into_iter().map(|task| NewTask {
                assignee_agent_id: task.to,
                title: task.title,
                detail: task.detail,
                replaces_task_id: task.replaces_task_id,
            }).collect();
            instructions::insert_batch(conn, &instruction.id, Some(&agent.id), &decision, &batch)?;
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &parsed.speech)?;
            tx.execute("UPDATE instructions SET phase = 'execute', updated_at = ?1 WHERE id = ?2", params![now_ms(), instruction.id])?;
            set_project_status(&tx, &instruction.project_id, "executing")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, agent, &instruction.project_id, outcome)?;
            tx.commit()?;
        }
        ActionKind::Reply { .. } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &parsed.speech)?;
            if let Some(notice) = leftover_notice(&instructions::list_tasks(&tx, &instruction.id)?) {
                messages::insert(&tx, &instruction.project_id, "system", None, "system", &notice)?;
            }
            let now = now_ms();
            tx.execute(
                "UPDATE instructions SET status = 'completed', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, instruction.id],
            )?;
            set_project_status(&tx, &instruction.project_id, "idle")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, agent, &instruction.project_id, outcome)?;
            tx.commit()?;
        }
        ActionKind::Discuss { to, topic } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &parsed.speech)?;
            let stored = StoredSnapshot {
                message_bound: None,
                rules_version: None,
                roster_version: None,
                pending: Some(StoredPending::Discuss { agent_ids: to, topic, launched: false }),
            };
            tx.execute(
                "UPDATE instructions SET phase = 'discuss', discuss_rounds = discuss_rounds + 1, context_snapshot_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![serde_json::to_string(&stored).map_err(|error| invalid(error.to_string()))?, now_ms(), instruction.id],
            )?;
            set_project_status(&tx, &instruction.project_id, "discussing")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, agent, &instruction.project_id, outcome)?;
            tx.commit()?;
        }
        ActionKind::Rework { task_id, .. } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &parsed.speech)?;
            let changed = tx.execute(
                "UPDATE tasks SET status = 'pending', rework_count = rework_count + 1, finished_at = NULL \
                 WHERE id = ?1 AND instruction_id = ?2",
                params![task_id, instruction.id],
            )?;
            if changed == 0 {
                return Err(AppError::NotFound(format!("任务 {task_id}")));
            }
            tx.execute("UPDATE instructions SET phase = 'execute', updated_at = ?1 WHERE id = ?2", params![now_ms(), instruction.id])?;
            set_project_status(&tx, &instruction.project_id, "executing")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, agent, &instruction.project_id, outcome)?;
            tx.commit()?;
        }
    }
    Ok(())
}

fn apply_execute(conn: &Connection, run: &RunRow, instruction: &Instruction, agent: &Agent, outcome: &RunOutcome) -> AppResult<()> {
    let task_id = run.task_id.clone().ok_or_else(|| invalid("执行回合缺少任务"))?;
    let content = if outcome.final_text.trim().is_empty() {
        outcome.error.clone().unwrap_or_else(|| "执行失败".to_string())
    } else {
        outcome.final_text.clone()
    };
    let report = messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "report", &content)?;
    if outcome.status == "ok" {
        instructions::finish_task(conn, &task_id, "done", Some(report.id))?;
    } else {
        instructions::finish_task(conn, &task_id, "failed", Some(report.id))?;
    }
    finish_run(conn, &run.id, outcome, if outcome.status == "ok" { "ok" } else { "failed" })?;
    remember_agent(conn, agent, &instruction.project_id, outcome)?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE instruction_id = ?1 AND batch_id = ?2 AND status IN ('pending', 'running')",
        params![instruction.id, instruction.current_batch],
        |row| row.get(0),
    )?;
    if pending == 0 || outcome.status != "ok" {
        set_phase(conn, &instruction.id, "review")?;
        set_project_status(conn, &instruction.project_id, "discussing")?;
    }
    Ok(())
}

fn apply_member_discuss(conn: &Connection, instruction: &Instruction, agent: &Agent, run_id: &str, outcome: &RunOutcome) -> AppResult<()> {
    let content = if outcome.final_text.trim().is_empty() { "（没有输出）" } else { outcome.final_text.trim() };
    messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "discussion", content)?;
    let run = load_run(conn, run_id)?;
    finish_run(conn, &run.id, outcome, if outcome.status == "ok" { "ok" } else { "failed" })?;
    remember_agent(conn, agent, &instruction.project_id, outcome)?;
    if outcome.status != "ok" {
        return mark_waiting(conn, instruction, outcome.error.as_deref().unwrap_or("成员讨论失败"));
    }
    if running_count(conn, &instruction.id)? == 0 {
        set_phase(conn, &instruction.id, "decide")?;
    }
    Ok(())
}

fn reserve(conn: &Connection, instruction: &Instruction, needed: i64) -> AppResult<bool> {
    if needed <= 0 {
        return Err(invalid("预留回合数无效"));
    }
    if instruction.turn_count + needed > TURN_LIMIT {
        let now = now_ms();
        conn.execute(
            "UPDATE instructions SET status = 'waiting_user', wait_reason = ?1, updated_at = ?2 WHERE id = ?3",
            params!["单条指令已达到 30 回合上限", now, instruction.id],
        )?;
        set_project_status(conn, &instruction.project_id, "waiting_user")?;
        return Ok(false);
    }
    conn.execute(
        "UPDATE instructions SET turn_count = turn_count + ?1, updated_at = ?2 WHERE id = ?3",
        params![needed, now_ms(), instruction.id],
    )?;
    Ok(true)
}

fn compose_prompt(
    conn: &Connection,
    instruction: &Instruction,
    agent: &Agent,
    stage: Stage,
    current: Option<&TaskRecord>,
    topic: &str,
) -> AppResult<(String, i64)> {
    let project = projects::get(conn, &instruction.project_id)?;
    let rules = rules::list(Path::new(&project.folder_path))?;
    let roster = agents::list(conn, &instruction.project_id)?;
    let goal_message = messages::get(conn, instruction.source_message_id)?;
    let tasks = instructions::list_tasks(conn, &instruction.id)?;
    let reports = task_reports(conn, &tasks)?;
    let names = roster.iter().map(|member| (member.id.clone(), member.name.clone())).collect::<Vec<_>>();
    let owned_tasks: Vec<(String, String, String, String, String, Option<String>)> = tasks.iter().map(|task| {
        let assignee = task.assignee_agent_id.as_ref().and_then(|id| names.iter().find(|(agent_id, _)| agent_id == id)).map(|(_, name)| name.clone()).unwrap_or_else(|| "未知成员".to_string());
        let report = task.report_message_id.and_then(|id| reports.iter().find(|(message_id, _)| *message_id == id).map(|(_, content)| content.clone()));
        (task.id.clone(), task.title.clone(), assignee, task.status.clone(), task.detail.clone(), report)
    }).collect();
    let briefs: Vec<TaskBrief<'_>> = owned_tasks.iter().map(|task| TaskBrief {
        id: &task.0, title: &task.1, assignee: &task.2, status: &task.3, detail: &task.4, report: task.5.as_deref(),
    }).collect();
    let current_owned = current.and_then(|task| owned_tasks.iter().find(|item| item.0 == task.id));
    let current_brief = current_owned.map(|task| TaskBrief {
        id: &task.0, title: &task.1, assignee: &task.2, status: &task.3, detail: &task.4, report: task.5.as_deref(),
    });
    let decision = latest_decision(conn, &instruction.project_id)?;
    let bound = message_bound(conn, &instruction.project_id)?;
    let unread = project_messages(conn, &instruction.project_id)?;
    let rules_version = rules_version(&rules);
    let roster_version = roster_version(&roster);
    let fresh = agent.session_id.is_none();
    let rounds = instruction.discuss_rounds.max(0) as u32;
    let snapshot = TeamSnapshot {
        message_bound: bound,
        rules_version: &rules_version,
        roster_version: &roster_version,
        unread: &unread,
        recent: &unread,
        project: &project,
        rules: &rules,
        agent,
        roster: &roster,
        goal: goal_message.content.trim(),
        topic,
        round_index: rounds,
        discuss_rounds: rounds,
        decision: decision.as_deref(),
        tasks: &briefs,
        current_task: current_brief.as_ref(),
        intervention_note: instruction.wait_reason.as_deref().unwrap_or(""),
        recovery: fresh,
        fresh_session: fresh,
    };
    let prompt = match stage {
        Stage::Discuss | Stage::Execute => prompts::build_member_prompt(stage, &snapshot)?,
        _ => prompts::build_leader_prompt(stage, &snapshot)?,
    };
    Ok((prompt, bound))
}

fn leftover_notice(tasks: &[TaskRecord]) -> Option<String> {
    let lines: Vec<String> = tasks.iter().filter(|task| task.superseded_by_task_id.is_none())
        .filter(|task| matches!(task.status.as_str(), "failed" | "cancelled" | "pending"))
        .map(|task| format!("- {}｜状态 {}", task.title, task.status))
        .collect();
    if lines.is_empty() {
        None
    } else {
        Some(format!("以下任务尚未完成：\n{}", lines.join("\n")))
    }
}

fn rework_limit(conn: &Connection, text: &str) -> AppResult<Option<TaskLimit>> {
    let Some(json) = fence_body(text) else { return Ok(None) };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else { return Ok(None) };
    if value.get("action").and_then(|item| item.as_str()) != Some("rework") {
        return Ok(None);
    }
    let Some(task_id) = value.get("task_id").and_then(|item| item.as_str()) else { return Ok(None) };
    let task = match instructions::get_task(conn, task_id) {
        Ok(task) => task,
        Err(AppError::NotFound(_)) => return Ok(None),
        Err(error) => return Err(error),
    };
    Ok(Some(TaskLimit { id: task.id, status: task.status, rework_count: task.rework_count.max(0) as u32 }))
}

fn fence_body(text: &str) -> Option<&str> {
    let marker = "```imates";
    let start = text.rfind(marker)?;
    let after = text[start + marker.len()..].trim_start_matches(['\r', '\n']);
    let end = after.rfind("```")?;
    Some(after[..end].trim())
}

fn save_speech(conn: &Connection, instruction: &Instruction, agent: &Agent, speech: &str) -> AppResult<()> {
    if speech.trim().is_empty() {
        return Ok(());
    }
    messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "discussion", speech.trim())?;
    Ok(())
}

fn remember_agent(conn: &Connection, agent: &Agent, project_id: &str, outcome: &RunOutcome) -> AppResult<()> {
    if let Some(session_id) = outcome.session_id.as_deref() {
        agents::set_session(conn, &agent.id, Some(session_id))?;
    }
    let bound = message_bound(conn, project_id)?;
    agents::finish_context(conn, &agent.id, &agent.persona, bound)?;
    Ok(())
}

fn mark_waiting(conn: &Connection, instruction: &Instruction, reason: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE instructions SET status = 'waiting_user', wait_reason = ?1, updated_at = ?2 WHERE id = ?3",
        params![reason, now_ms(), instruction.id],
    )?;
    set_project_status(conn, &instruction.project_id, "waiting_user")
}

fn protocol_phase(phase: &str) -> Phase {
    match phase {
        "discuss" => Phase::Discuss,
        "decide" => Phase::Decide,
        "execute" => Phase::Execute,
        "review" => Phase::Review,
        "intervention" => Phase::Intervention,
        "direct" => Phase::Direct,
        _ => Phase::Triage,
    }
}

fn leader(conn: &Connection, project_id: &str) -> AppResult<Agent> {
    agents::list(conn, project_id)?.into_iter().find(|agent| agent.is_leader).ok_or_else(|| invalid("项目没有组长"))
}

fn next_pending(conn: &Connection, instruction_id: &str, batch_id: &str) -> AppResult<Option<TaskRecord>> {
    let id: Option<String> = conn.query_row(
        "SELECT id FROM tasks WHERE instruction_id = ?1 AND batch_id = ?2 AND status = 'pending' \
         ORDER BY order_index LIMIT 1",
        params![instruction_id, batch_id],
        |row| row.get(0),
    ).optional()?;
    match id {
        Some(id) => Ok(Some(instructions::get_task(conn, &id)?)),
        None => Ok(None),
    }
}

fn insert_run(conn: &Connection, agent_id: &str, mode: &str, instruction_id: &str, task_id: Option<&str>) -> AppResult<String> {
    let id = new_id();
    let command = format!(r#"{{"source":"orchestrator","mode":"{mode}"}}"#);
    conn.execute(
        "INSERT INTO runs (id, agent_id, mode, command, started_at, status, instruction_id, task_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7)",
        params![id, agent_id, mode, command, now_ms(), instruction_id, task_id],
    )?;
    Ok(id)
}

fn finish_run(conn: &Connection, run_id: &str, outcome: &RunOutcome, status: &str) -> AppResult<()> {
    let changed = conn.execute(
        "UPDATE runs SET status = ?1, ended_at = ?2, exit_code = ?3, error = ?4, session_id = COALESCE(?5, session_id) \
         WHERE id = ?6 AND status = 'running'",
        params![status, now_ms(), outcome.exit_code, outcome.error, outcome.session_id, run_id],
    )?;
    if changed == 0 {
        return Err(invalid("运行已结束"));
    }
    Ok(())
}

fn load_run(conn: &Connection, run_id: &str) -> AppResult<RunRow> {
    conn.query_row(
        "SELECT id, agent_id, mode, status, instruction_id, task_id FROM runs WHERE id = ?1",
        [run_id],
        |row| {
            Ok(RunRow {
                id: row.get(0)?,
                agent_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                mode: row.get(2)?,
                status: row.get(3)?,
                instruction_id: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                task_id: row.get(5)?,
            })
        },
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("运行 {run_id}")))
}

fn running_count(conn: &Connection, instruction_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM runs WHERE instruction_id = ?1 AND status = 'running'",
        [instruction_id],
        |row| row.get(0),
    )?)
}

fn message_bound(conn: &Connection, project_id: &str) -> AppResult<i64> {
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(id), 0) FROM messages WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?)
}

fn project_messages(conn: &Connection, project_id: &str) -> AppResult<Vec<Message>> {
    messages::list(conn, project_id, None, 200)
}

fn latest_decision(conn: &Connection, project_id: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row(
        "SELECT content FROM messages WHERE project_id = ?1 AND kind = 'decision' ORDER BY id DESC LIMIT 1",
        [project_id],
        |row| row.get(0),
    ).optional()?)
}

fn task_reports(conn: &Connection, tasks: &[TaskRecord]) -> AppResult<Vec<(i64, String)>> {
    let mut reports = Vec::new();
    for task in tasks {
        if let Some(message_id) = task.report_message_id {
            if let Ok(message) = messages::get(conn, message_id) {
                reports.push((message_id, message.content));
            }
        }
    }
    Ok(reports)
}

fn rules_version(rules: &[RuleDoc]) -> String {
    if rules.is_empty() {
        return "none".to_string();
    }
    rules.iter().map(|rule| format!("{}:{}", rule.filename, rule.content.len())).collect::<Vec<_>>().join("|")
}

fn roster_version(roster: &[Agent]) -> String {
    roster.iter().map(|agent| format!("{}:{}:{}", agent.id, agent.name, agent.persona.len())).collect::<Vec<_>>().join("|")
}

fn load_snapshot(json: &Option<String>) -> AppResult<StoredSnapshot> {
    let Some(json) = json.as_deref() else {
        return Ok(StoredSnapshot { message_bound: None, rules_version: None, roster_version: None, pending: None });
    };
    serde_json::from_str(json).map_err(|error| invalid(format!("上下文快照损坏：{error}")))
}

fn save_snapshot(conn: &Connection, instruction_id: &str, snapshot: &StoredSnapshot) -> AppResult<()> {
    let json = serde_json::to_string(snapshot).map_err(|error| invalid(error.to_string()))?;
    conn.execute(
        "UPDATE instructions SET context_snapshot_json = ?1, updated_at = ?2 WHERE id = ?3",
        params![json, now_ms(), instruction_id],
    )?;
    Ok(())
}

fn set_phase(conn: &Connection, instruction_id: &str, phase: &str) -> AppResult<()> {
    conn.execute("UPDATE instructions SET phase = ?1, updated_at = ?2 WHERE id = ?3", params![phase, now_ms(), instruction_id])?;
    Ok(())
}

fn set_project_status(conn: &Connection, project_id: &str, status: &str) -> AppResult<()> {
    conn.execute("UPDATE projects SET status = ?1, updated_at = ?2 WHERE id = ?3", params![status, now_ms(), project_id])?;
    Ok(())
}

fn spec(run_id: String, agent_id: String, instruction_id: &str, task_id: Option<String>, mode: &str, prompt: String, message_bound: i64) -> TurnSpec {
    TurnSpec { run_id, agent_id, instruction_id: instruction_id.to_string(), task_id, mode: mode.to_string(), prompt, message_bound }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Invalid(message.into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runner::RunOutcome;
    use crate::store::agents::{self, Agent, NewAgent};
    use crate::store::instructions::{self, NewTask};
    use crate::store::{messages, open_in_memory, projects};

    const PERSONA: &str = "## 定位\n\n本人定位\n\n## 职责\n\n本人职责\n\n## 边界\n\n本人边界\n\n## 工作风格\n\n本人风格\n";

    fn setup(member_names: &[&str]) -> (rusqlite::Connection, String, Vec<Agent>) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "网站", "登录", "/tmp/site").unwrap();
        let mut agents = Vec::new();
        let leader = agents::create(&conn, &project.id, &NewAgent {
            name: "组长".into(), role: "管理".into(), persona: PERSONA.into(),
            cli: "claude".into(), model: None, is_leader: true,
        }).unwrap();
        agents.push(leader);
        for (index, name) in member_names.iter().enumerate() {
            agents.push(agents::create(&conn, &project.id, &NewAgent {
                name: (*name).into(), role: "开发".into(), persona: PERSONA.into(),
                cli: if index == 0 { "claude" } else { "codex" }.into(), model: None, is_leader: false,
            }).unwrap());
        }
        (conn, project.id, agents)
    }

    fn start_team(conn: &rusqlite::Connection, project_id: &str) {
        let message = messages::insert(conn, project_id, "user", None, "instruction", "给登录加上校验").unwrap();
        instructions::insert_instruction(conn, project_id, message.id, "team", None, "triage").unwrap();
    }

    fn ok_text(text: &str) -> RunOutcome {
        RunOutcome {
            status: "ok".into(), session_id: Some("session".into()), final_text: text.into(),
            partial_text: String::new(), usage: None, exit_code: Some(0), error: None,
            tool_seen: false, cleanup_complete: true,
        }
    }

    fn failed_text(text: &str) -> RunOutcome {
        RunOutcome { status: "failed".into(), error: Some("执行失败".into()), ..ok_text(text) }
    }

    fn fence(speech: &str, json: &str) -> String {
        format!("{speech}\n```imates\n{json}\n```")
    }

    fn turn_count(conn: &rusqlite::Connection, project_id: &str) -> i64 {
        instructions::active_instruction(conn, project_id).unwrap().unwrap().turn_count
    }

    #[test]
    fn reply_completes_the_instruction_and_lists_unresolved_tasks() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let specs = advance(&conn, &project_id).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(turn_count(&conn, &project_id), 1);
        apply_outcome(&conn, &specs[0].run_id, &ok_text(&fence("先直接答。", r#"{"action":"reply","message":"可以手写校验"}"#))).unwrap();
        let instruction = instructions::active_instruction(&conn, &project_id).unwrap();
        assert!(instruction.is_none());
        let project = projects::get(&conn, &project_id).unwrap();
        assert_eq!(project.status, "idle");
        let stored = messages::list(&conn, &project_id, None, 20).unwrap();
        assert!(stored.iter().any(|message| message.content.contains("先直接答。")));
        assert!(stored.iter().filter(|message| message.kind == "system").all(|message| !message.content.contains("未完成")));

        let message = messages::insert(&conn, &project_id, "user", None, "instruction", "再做一轮").unwrap();
        let again = instructions::insert_instruction(&conn, &project_id, message.id, "team", None, "review").unwrap();
        instructions::insert_batch(&conn, &again.id, Some(&agents[0].id), "旧决议", &[
            NewTask { assignee_agent_id: agents[1].id.clone(), title: "已失败".into(), detail: "细节".into(), replaces_task_id: None },
            NewTask { assignee_agent_id: agents[1].id.clone(), title: "未执行".into(), detail: "细节".into(), replaces_task_id: None },
        ]).unwrap();
        let tasks = instructions::list_tasks(&conn, &again.id).unwrap();
        conn.execute("UPDATE tasks SET status = 'failed' WHERE id = ?1", [&tasks[0].id]).unwrap();
        conn.execute("UPDATE tasks SET status = 'cancelled' WHERE id = ?1", [&tasks[1].id]).unwrap();
        conn.execute("UPDATE projects SET status = 'discussing' WHERE id = ?1", [&project_id]).unwrap();
        let review = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &review[0].run_id, &ok_text(&fence("交付。", r#"{"action":"reply","message":"先到这里"}"#))).unwrap();
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "idle");
        let systems: Vec<_> = messages::list(&conn, &project_id, None, 20).unwrap().into_iter().filter(|message| message.kind == "system").collect();
        assert!(systems.iter().any(|message| message.content.contains("已失败") && message.content.contains("未执行")));
    }

    #[test]
    fn discuss_reserves_member_turns_and_round_three_must_assign_or_reply() {
        let (conn, project_id, _) = setup(&["前端", "测试"]);
        start_team(&conn, &project_id);
        let mut leader = advance(&conn, &project_id).unwrap();
        for round in 1..=3 {
            apply_outcome(&conn, &leader[0].run_id, &ok_text(&fence("继续讨论。", r#"{"action":"discuss","to":["前端","测试"],"topic":"登录校验"}"#))).unwrap();
            let members = advance(&conn, &project_id).unwrap();
            assert_eq!(members.len(), 2, "第 {round} 轮应同时启动两名成员");
            assert_eq!(members[0].message_bound, members[1].message_bound);
            assert!(members[0].prompt.contains(&format!("消息上界：{}", members[0].message_bound)));
            assert!(members[1].prompt.contains(&format!("消息上界：{}", members[0].message_bound)));
            assert!(members[0].prompt.contains("登录校验") && members[1].prompt.contains("登录校验"));
            assert_eq!(turn_count(&conn, &project_id), round * 3);
            let fresh = format!("第{round}轮成员甲刚写完");
            apply_outcome(&conn, &members[0].run_id, &ok_text(&fresh)).unwrap();
            assert!(!members[1].prompt.contains(&fresh), "后启动的成员不应看到本轮另一位刚写的发言");
            apply_outcome(&conn, &members[1].run_id, &ok_text("成员乙的发言")).unwrap();
            leader = advance(&conn, &project_id).unwrap();
            assert_eq!(leader.len(), 1);
            if round == 3 {
                assert!(leader[0].prompt.contains("必须") && leader[0].prompt.contains("assign") && leader[0].prompt.contains("reply"));
            }
        }
    }

    #[test]
    fn assign_executes_in_order_and_rework_does_not_revive_cancelled_tasks() {
        let (conn, project_id, agents) = setup(&["前端", "测试"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let assign = fence("分三步。", r#"{"action":"assign","decision":"分三步做","tasks":[{"to":"前端","title":"第一步","detail":"做第一步"},{"to":"前端","title":"第二步","detail":"做第二步"},{"to":"测试","title":"第三步","detail":"做第三步"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();

        let first = advance(&conn, &project_id).unwrap();
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].mode, "execute");
        assert!(first[0].prompt.contains("第一步"));
        apply_outcome(&conn, &first[0].run_id, &ok_text("第一步完成")).unwrap();

        let second = advance(&conn, &project_id).unwrap();
        assert!(second[0].prompt.contains("第二步"));
        apply_outcome(&conn, &second[0].run_id, &failed_text("第二步失败")).unwrap();
        let instruction = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        let tasks = instructions::list_tasks(&conn, &instruction.id).unwrap();
        assert_eq!(tasks[0].status, "done");
        assert_eq!(tasks[1].status, "failed");
        assert_eq!(tasks[2].status, "cancelled");

        let review = advance(&conn, &project_id).unwrap();
        assert!(review[0].prompt.contains(&tasks[1].id) && review[0].prompt.contains("第二步"));
        let rework = format!(r#"{{"action":"rework","task_id":"{}","feedback":"重做第二步"}}"#, tasks[1].id);
        apply_outcome(&conn, &review[0].run_id, &ok_text(&fence("打回。", &rework))).unwrap();
        let retried = instructions::list_tasks(&conn, &instruction.id).unwrap();
        assert_eq!(retried[1].rework_count, 1);
        assert_eq!(retried[2].status, "cancelled");
        let again = advance(&conn, &project_id).unwrap();
        assert_eq!(again.len(), 1);
        assert_ne!(again[0].run_id, second[0].run_id);
        assert!(again[0].prompt.contains("第二步"));
        apply_outcome(&conn, &again[0].run_id, &ok_text("第二步改完")).unwrap();

        let review_again = advance(&conn, &project_id).unwrap();
        let replacement = format!(
            r#"{{"action":"assign","decision":"补上第三步","tasks":[{{"to":"测试","title":"替代第三步","detail":"重新做第三步","replaces_task_id":"{}"}}]}}"#,
            tasks[2].id
        );
        apply_outcome(&conn, &review_again[0].run_id, &ok_text(&fence("换一个任务。", &replacement))).unwrap();
        let replaced = instructions::get_task(&conn, &tasks[2].id).unwrap();
        assert_eq!(replaced.status, "cancelled");
        assert!(replaced.superseded_by_task_id.is_some());
        let follow = advance(&conn, &project_id).unwrap();
        assert!(follow[0].prompt.contains("替代第三步"));
        apply_outcome(&conn, &follow[0].run_id, &ok_text("替代完成")).unwrap();
        let closing = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &closing[0].run_id, &ok_text(&fence("可以交付。", r#"{"action":"reply","message":"交付"}"#))).unwrap();
        let systems: Vec<_> = messages::list(&conn, &project_id, None, 50).unwrap().into_iter().filter(|message| message.kind == "system").collect();
        assert!(systems.iter().all(|message| !message.content.contains("第三步")));
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "idle");
    }

    #[test]
    fn the_31st_attempt_waits_and_a_failed_launch_keeps_the_reserved_count() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let specs = advance(&conn, &project_id).unwrap();
        fail_launch(&conn, &specs[0].run_id, "进程没有启动").unwrap();
        assert_eq!(turn_count(&conn, &project_id), 1);
        let status: String = conn.query_row("SELECT status FROM runs WHERE id = ?1", [&specs[0].run_id], |row| row.get(0)).unwrap();
        assert_eq!(status, "failed");
        let next = advance(&conn, &project_id).unwrap();
        assert_eq!(next.len(), 1);
        assert_eq!(turn_count(&conn, &project_id), 2);
        fail_launch(&conn, &next[0].run_id, "第二次也没有启动").unwrap();

        conn.execute("UPDATE instructions SET turn_count = 30 WHERE project_id = ?1", [&project_id]).unwrap();
        let stopped = advance(&conn, &project_id).unwrap();
        assert!(stopped.is_empty());
        let waiting = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(waiting.status, "waiting_user");
        assert_eq!(waiting.turn_count, 30);
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "waiting_user");
    }
}
