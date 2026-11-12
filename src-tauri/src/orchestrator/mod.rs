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
    #[serde(default)]
    message_bound: Option<i64>,
    #[serde(default)]
    rules_version: Option<String>,
    #[serde(default)]
    roster_version: Option<String>,
    #[serde(default)]
    pending: Option<StoredPending>,
    /// 边界上暂存的组长最终文本。继续时再执行，不重复调用这一回合。
    #[serde(default)]
    stashed_text: Option<String>,
    #[serde(default)]
    stashed_phase: Option<String>,
    /// 产生暂存文本的组长运行。继续时用它给决议消息补上模型信息；旧数据没有这一项。
    #[serde(default)]
    stashed_run_id: Option<String>,
    /// 运行中点名。不占用批次任务，做完后进入干预研判。
    #[serde(default)]
    mention: Option<StoredMention>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct StoredMention {
    agent_id: String,
    content: String,
    launched: bool,
}

impl Default for StoredSnapshot {
    fn default() -> Self {
        Self {
            message_bound: None,
            rules_version: None,
            roster_version: None,
            pending: None,
            stashed_text: None,
            stashed_phase: None,
            stashed_run_id: None,
            mention: None,
        }
    }
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
    /// 构建提示词时的最大消息 ID。成功后游标推进到这里。
    message_bound: i64,
    /// 构建提示词时的人设标记，用来判断回合进行中人设有没有被改过。
    persona_mark: String,
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
    let mut instruction = instructions::get(&tx, &active.id)?;
    if instruction.status != "running" {
        tx.commit()?;
        return Ok(Vec::new());
    }
    // 只有没有正在跑的回合时才消化队列，避免插话打断当前进程。
    if instruction.phase != "intervention" && has_queued(&tx, &instruction.id)? {
        arm_next_input(&tx, &instruction)?;
        instruction = instructions::get(&tx, &instruction.id)?;
    }
    let specs = dispatch(&tx, &instruction)?;
    tx.commit()?;
    Ok(specs)
}

/// 运行中的普通消息或 `@成员` 入队。不修改已有 run。
pub fn queue_user_text(conn: &Connection, project_id: &str, content: &str) -> AppResult<Message> {
    let active = instructions::active_instruction(conn, project_id)?.ok_or_else(|| invalid("没有正在运行的指令"))?;
    if active.status != "running" {
        return Err(invalid("只有进行中的指令可以接收插话"));
    }
    let roster = agents::list(conn, project_id)?;
    let (agent, _) = crate::chat::resolve_recipient(content, &roster)?;
    let text = content.trim();
    let message = messages::insert(conn, project_id, "user", None, "chat", text)?;
    if text.starts_with('@') {
        instructions::enqueue_input(conn, &active.id, message.id, "direct", Some(&agent.id))?;
    } else {
        instructions::enqueue_input(conn, &active.id, message.id, "supplement", None)?;
    }
    Ok(message)
}

/// 等待用户时用一条新消息重新开始。放弃旧指令和创建新指令在同一事务里。
pub fn begin_user_instruction(conn: &Connection, project_id: &str, content: &str) -> AppResult<Instruction> {
    let text = content.trim();
    if text.is_empty() {
        return Err(invalid("消息不能为空"));
    }
    let tx = conn.unchecked_transaction()?;
    let roster = agents::list(&tx, project_id)?;
    let (agent, _) = crate::chat::resolve_recipient(text, &roster)?;
    if let Some(active) = instructions::active_instruction(&tx, project_id)? {
        if active.status == "running" {
            return Err(invalid("该项目已有进行中的指令"));
        }
    }
    let mentioned = text.starts_with('@');
    let message = messages::insert(&tx, project_id, "user", None, if mentioned { "chat" } else { "instruction" }, text)?;
    if let Some(active) = instructions::active_instruction(&tx, project_id)? {
        if active.status == "running" {
            return Err(invalid("该项目已有进行中的指令"));
        }
        abandon_in(&tx, &active.id)?;
    }
    let id = new_id();
    let now = now_ms();
    let kind = if mentioned { "direct" } else { "team" };
    let phase = if mentioned { "direct" } else { "triage" };
    let target = if mentioned { Some(agent.id.as_str()) } else { None };
    tx.execute(
        "INSERT INTO instructions (id, project_id, source_message_id, kind, target_agent_id, phase, status, \
         discuss_rounds, turn_count, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'running', 0, 0, ?7, ?7)",
        params![id, project_id, message.id, kind, target, phase, now],
    )?;
    tx.commit()?;
    instructions::get(conn, &id)
}

/// 空闲 `@成员` 的直接指令。不经过组长。
pub fn open_direct_instruction(conn: &Connection, project_id: &str, source_message_id: i64, agent_id: &str) -> AppResult<Instruction> {
    instructions::insert_instruction(conn, project_id, source_message_id, "direct", Some(agent_id), "direct")
}

/// 直接指令的单回合结束。成功则完成，失败则等待用户。
pub fn finish_direct_instruction(conn: &Connection, instruction_id: &str, succeeded: bool) -> AppResult<()> {
    let instruction = instructions::get(conn, instruction_id)?;
    if instruction.kind != "direct" || !matches!(instruction.status.as_str(), "running" | "waiting_user") {
        return Ok(());
    }
    if succeeded {
        // 回合结束前插进来的话留到下一边界，不把直接指令提前标完成。
        if has_queued(conn, &instruction.id)? {
            return Ok(());
        }
        let now = now_ms();
        conn.execute(
            "UPDATE instructions SET status = 'completed', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
            params![now, instruction.id],
        )?;
        set_project_status(conn, &instruction.project_id, "idle")?;
    } else {
        mark_waiting(conn, &instruction, "直接指令没有完成")?;
    }
    Ok(())
}

/// 停止当前代次：运行和未开始的任务、队列都取消，指令等待用户。
pub fn stop_instruction(conn: &Connection, project_id: &str) -> AppResult<Option<Instruction>> {
    let Some(active) = instructions::active_instruction(conn, project_id)? else {
        return Ok(None);
    };
    if active.status != "running" {
        return Ok(Some(active));
    }
    let tx = conn.unchecked_transaction()?;
    let now = now_ms();
    let reason = "已停止。部分文件可能已改动，不会自动撤销。";
    tx.execute(
        "UPDATE tasks SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status IN ('pending', 'running')",
        params![now, active.id],
    )?;
    tx.execute(
        "UPDATE instruction_inputs SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status = 'queued'",
        params![now, active.id],
    )?;
    tx.execute(
        "UPDATE runs SET status = 'cancelled', ended_at = ?1, error = ?2 WHERE instruction_id = ?3 AND status = 'running'",
        params![now, "回合已停止，文件修改不会自动撤销", active.id],
    )?;
    tx.execute(
        "UPDATE instructions SET status = 'waiting_user', wait_reason = ?1, updated_at = ?2 WHERE id = ?3",
        params![reason, now, active.id],
    )?;
    set_project_status(&tx, project_id, "waiting_user")?;
    messages::insert(&tx, project_id, "system", None, "system", reason)?;
    tx.commit()?;
    Ok(Some(instructions::get(conn, &active.id)?))
}

/// `action` 为 `retry`、`supplement` 或 `abandon`。补充内容放在 `note`。
pub fn resume_instruction(conn: &Connection, instruction_id: &str, action: &str, note: Option<&str>) -> AppResult<Instruction> {
    let instruction = instructions::get(conn, instruction_id)?;
    if instruction.status != "waiting_user" {
        return Err(invalid("只有等待用户的指令可以继续"));
    }
    match action {
        "retry" => retry_instruction(conn, &instruction),
        "supplement" => supplement_instruction(conn, &instruction, note.unwrap_or("")),
        "abandon" => abandon_instruction(conn, &instruction),
        _ => Err(invalid("未知的恢复操作")),
    }
}

/// 记录一个已经预留的回合结果，并推进阶段。不在这里启动下一回合。
pub fn apply_outcome(conn: &Connection, run_id: &str, outcome: &RunOutcome) -> AppResult<()> {
    let run = load_run(conn, run_id)?;
    if run.status != "running" {
        return Err(invalid("运行已结束"));
    }
    let instruction = instructions::get(conn, &run.instruction_id)?;
    if instruction.status != "running" {
        return Err(invalid("指令已暂停或结束"));
    }
    let agent = agents::get(conn, &run.agent_id)?;
    if run.mode == "execute" && run.task_id.is_none() {
        apply_mention(conn, &run, &instruction, &agent, outcome)
    } else if run.mode == "execute" {
        apply_execute(conn, &run, &instruction, &agent, outcome)
    } else if agent.is_leader {
        apply_leader(conn, &run, &instruction, &agent, outcome)
    } else {
        apply_member_discuss(conn, &instruction, &agent, run_id, outcome)
    }
}

/// 结果处理失败后的兜底；由确认 run 原先仍在运行的调用方使用。
pub fn fail_outcome(conn: &Connection, run_id: &str, outcome: &RunOutcome, error: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let run = load_run(&tx, run_id)?;
    let instruction = instructions::get(&tx, &run.instruction_id)?;
    let reason = format!("回合结果处理失败：{error}");
    if run.status == "running" {
        finish_run_with(&tx, run_id, outcome, "failed", &reason)?;
    }
    if instruction.status == "running" {
        // 执行结果没有可靠落库时保留失败事实，不允许 retry 重放已经改过文件的任务。
        tx.execute(
            "UPDATE tasks SET status = 'failed', finished_at = ?1 WHERE id = ?2 AND status = 'running'",
            params![now_ms(), run.task_id],
        )?;
        instructions::cancel_unstarted(&tx, &instruction.id)?;
        mark_waiting(&tx, &instruction, &reason)?;
        messages::insert(&tx, &instruction.project_id, "system", None, "system",
            &format!("{reason}\n指令已暂停，请检查已有产出后重试、补充说明或放弃；执行过的任务不会自动重做。"))?;
    }
    tx.commit()?;
    Ok(())
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

/// 会话在启动阶段被 CLI 明确拒绝时，清空会话并用开场加恢复包重开同一个回合，最多一次。
/// 返回 None 表示不满足条件，旧 run 原样保留，调用方照常记录结果。
pub fn rebuild_turn(conn: &Connection, run_id: &str, outcome: &RunOutcome) -> AppResult<Option<TurnSpec>> {
    let run = load_run(conn, run_id)?;
    if run.status != "running" {
        return Ok(None);
    }
    let instruction = instructions::get(conn, &run.instruction_id)?;
    if instruction.status != "running" {
        return Ok(None);
    }
    let agent = agents::get(conn, &run.agent_id)?;
    let had_session = crate::store::runs::resumable_session(conn, &agent, &run.mode)?.is_some();
    if !crate::chat::should_rebuild_session(outcome, had_session, false)
        || previous_remedy(conn, &instruction.id, &agent.id, Some(run_id))?.is_some()
    {
        return Ok(None);
    }
    let tx = conn.unchecked_transaction()?;
    // 到了回合上限时 reserve 会把指令置为等待用户；这时不重建，让调用方按普通失败记录。
    if !reserve(&tx, &instruction, 1)? {
        tx.commit()?;
        return Ok(None);
    }
    finish_run_with(&tx, run_id, outcome, "failed", &format!("{REMEDY_PREFIX}会话不可用，已清空并重开一次"))?;
    agents::set_session(&tx, &agent.id, None)?;
    let agent = agents::get(&tx, &agent.id)?;
    let (prompt, bound) = if run.mode == "execute" {
        match run.task_id.as_deref() {
            Some(task_id) => {
                let task = instructions::get_task(&tx, task_id)?;
                compose_prompt(&tx, &instruction, &agent, Stage::Execute, Some(&task), "")?
            }
            None => {
                let content = load_snapshot(&instruction.context_snapshot_json)?.mention
                    .map(|mention| mention.content).ok_or_else(|| invalid("点名内容已经不在了"))?;
                compose_mention(&tx, &instruction, &agent, &content)?
            }
        }
    } else if agent.is_leader {
        let stage = match instruction.phase.as_str() {
            "decide" => Stage::Decide,
            "review" => Stage::Review,
            "intervention" => Stage::Intervention,
            _ => Stage::Triage,
        };
        compose_prompt(&tx, &instruction, &agent, stage, None, "")?
    } else {
        let topic = match load_snapshot(&instruction.context_snapshot_json)?.pending {
            Some(StoredPending::Discuss { topic, .. }) => topic,
            None => return Err(invalid("讨论阶段缺少议题")),
        };
        compose_prompt(&tx, &instruction, &agent, Stage::Discuss, None, &topic)?
    };
    let new_run = insert_run(&tx, &agent, &run.mode, &instruction.id, run.task_id.as_deref(), bound)?;
    tx.commit()?;
    Ok(Some(spec(new_run, agent.id, &instruction.id, run.task_id, &run.mode, prompt, bound)))
}

fn dispatch(conn: &Connection, instruction: &Instruction) -> AppResult<Vec<TurnSpec>> {
    match instruction.phase.as_str() {
        "triage" => schedule_leader(conn, instruction, Stage::Triage),
        "decide" => schedule_leader(conn, instruction, Stage::Decide),
        "review" => schedule_leader(conn, instruction, Stage::Review),
        "intervention" => schedule_leader(conn, instruction, Stage::Intervention),
        "discuss" => schedule_discuss(conn, instruction),
        "execute" => schedule_execute(conn, instruction),
        "direct" => schedule_mention(conn, instruction),
        other => Err(invalid(format!("未知阶段 {other}"))),
    }
}

fn schedule_leader(conn: &Connection, instruction: &Instruction, stage: Stage) -> AppResult<Vec<TurnSpec>> {
    if !reserve(conn, instruction, 1)? {
        return Ok(Vec::new());
    }
    let leader = leader(conn, &instruction.project_id)?;
    let (prompt, bound) = compose_prompt(conn, instruction, &leader, stage, None, "")?;
    let run_id = insert_run(conn, &leader, "discuss", &instruction.id, None, bound)?;
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
        let run_id = insert_run(conn, &agent, "discuss", &instruction.id, None, prompt_bound)?;
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
    let run_id = insert_run(conn, &assignee, "execute", &instruction.id, Some(&task.id), bound)?;
    set_project_status(conn, &instruction.project_id, "executing")?;
    Ok(vec![spec(run_id, assignee.id, &instruction.id, Some(task.id), "execute", prompt, bound)])
}

/// 运行中点名的提示词。会话重建时复用。
fn mention_prompt(content: &str) -> String {
    format!(
        "# 直接执行\n用户点名你处理下面这件事。做完给出简短汇报，不要继续其他任务。\n\
本次调用只是你的一次发言。说完就结束，不要等其他成员。\n\
不得修改 .imates/。\n\n{}",
        content.trim()
    )
}

fn compose_mention(conn: &Connection, instruction: &Instruction, agent: &Agent, content: &str) -> AppResult<(String, i64)> {
    let bound = message_bound(conn, &instruction.project_id)?;
    let prompt = mention_prompt(content);
    if agent.cli != "grok" || crate::store::runs::resumable_session(conn, agent, "execute")?.is_some() {
        return Ok((prompt, bound));
    }
    let project = projects::get(conn, &instruction.project_id)?;
    let roster = agents::list(conn, &instruction.project_id)?;
    let rules = rules::list(Path::new(&project.folder_path))?;
    let opening = crate::prompts::build_opening(&project, &rules, agent, &roster);
    let goal = messages::get(conn, instruction.source_message_id)?;
    let decision = latest_decision(conn, &instruction.project_id)?;
    let tasks = instructions::list_tasks(conn, &instruction.id)?;
    let task_statuses = tasks.iter().map(|task| format!("- {}：{}", task.title, task.status)).collect::<Vec<_>>().join("\n");
    Ok((format!("{opening}\n\n# 恢复包\n原指令：{}\n有效决议：{}\n任务状态：\n{task_statuses}\n\n{prompt}",
        goal.content, decision.as_deref().unwrap_or("尚无有效决议")), bound))
}

fn schedule_mention(conn: &Connection, instruction: &Instruction) -> AppResult<Vec<TurnSpec>> {
    let mut stored = load_snapshot(&instruction.context_snapshot_json)?;
    let Some(mention) = stored.mention.clone() else {
        return Ok(Vec::new());
    };
    if mention.launched {
        return Ok(Vec::new());
    }
    if !reserve(conn, instruction, 1)? {
        return Ok(Vec::new());
    }
    let agent = agents::get(conn, &mention.agent_id)?;
    let (prompt, bound) = compose_mention(conn, instruction, &agent, &mention.content)?;
    let run_id = insert_run(conn, &agent, "execute", &instruction.id, None, bound)?;
    stored.mention = Some(StoredMention { launched: true, ..mention });
    save_snapshot(conn, &instruction.id, &stored)?;
    set_project_status(conn, &instruction.project_id, "executing")?;
    Ok(vec![spec(run_id, agent.id, &instruction.id, None, "execute", prompt, bound)])
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
        Err(error) => return protocol_failed(conn, run, instruction, agent, outcome, &error.to_string()),
    };
    if instruction.phase != "intervention" && has_queued(conn, &instruction.id)? {
        return park_leader(conn, run, instruction, agent, outcome);
    }
    if instruction.phase == "intervention" {
        return apply_intervention(conn, run, instruction, agent, outcome, &parsed);
    }
    match parsed.action.kind {
        ActionKind::Assign { decision, tasks } => {
            let batch: Vec<NewTask> = tasks.into_iter().map(|task| NewTask {
                assignee_agent_id: task.to,
                title: task.title,
                detail: task.detail,
                replaces_task_id: task.replaces_task_id,
            }).collect();
            let inserted = instructions::insert_batch(conn, &instruction.id, Some(&agent.id), &decision, &batch)?;
            messages::set_run(conn, inserted.decision_message_id, &run.id)?;
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &run.id, &parsed.speech)?;
            tx.execute("UPDATE instructions SET phase = 'execute', updated_at = ?1 WHERE id = ?2", params![now_ms(), instruction.id])?;
            set_project_status(&tx, &instruction.project_id, "executing")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, run, agent, outcome)?;
            tx.commit()?;
        }
        ActionKind::Reply { .. } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &run.id, &parsed.speech)?;
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
            remember_agent(&tx, run, agent, outcome)?;
            tx.commit()?;
        }
        ActionKind::Discuss { to, topic } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &run.id, &parsed.speech)?;
            let stored = StoredSnapshot {
                pending: Some(StoredPending::Discuss { agent_ids: to, topic, launched: false }),
                ..StoredSnapshot::default()
            };
            tx.execute(
                "UPDATE instructions SET phase = 'discuss', discuss_rounds = discuss_rounds + 1, context_snapshot_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![serde_json::to_string(&stored).map_err(|error| invalid(error.to_string()))?, now_ms(), instruction.id],
            )?;
            set_project_status(&tx, &instruction.project_id, "discussing")?;
            finish_run(&tx, &run.id, outcome, "ok")?;
            remember_agent(&tx, run, agent, outcome)?;
            tx.commit()?;
        }
        ActionKind::Rework { task_id, .. } => {
            let tx = conn.unchecked_transaction()?;
            save_speech(&tx, instruction, agent, &run.id, &parsed.speech)?;
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
            remember_agent(&tx, run, agent, outcome)?;
            tx.commit()?;
        }
    }
    Ok(())
}

/// 组长的回复不符合指令协议。第一次把具体错误发回去重发；补救额度用过之后展示原文并等待用户。
fn protocol_failed(conn: &Connection, run: &RunRow, instruction: &Instruction, agent: &Agent, outcome: &RunOutcome, reason: &str) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    let used = previous_remedy(&tx, &instruction.id, &agent.id, Some(&run.id))?.is_some();
    // CLI 回合本身是成功的，组长已经在会话里看过这些消息，游标照常推进。
    remember_agent(&tx, run, agent, outcome)?;
    if !used {
        finish_run_with(&tx, &run.id, outcome, "failed", &format!("{PROTOCOL_REMEDY}{reason}"))?;
        tx.commit()?;
        return Ok(());
    }
    finish_run_with(&tx, &run.id, outcome, "failed", &format!("协议错误：{reason}"))?;
    save_speech(&tx, instruction, agent, &run.id, &outcome.final_text)?;
    let notice = format!("组长的回复在重发后仍然不符合指令协议：{reason}\n原文已作为普通发言保留。可以重试、补充说明或放弃。");
    messages::insert(&tx, &instruction.project_id, "system", None, "system", &notice)?;
    mark_waiting(&tx, instruction, &format!("指令协议错误：{reason}"))?;
    tx.commit()?;
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
    messages::set_run(conn, report.id, &run.id)?;
    if outcome.status == "ok" {
        instructions::finish_task(conn, &task_id, "done", Some(report.id))?;
    } else {
        instructions::finish_task(conn, &task_id, "failed", Some(report.id))?;
    }
    finish_run(conn, &run.id, outcome, if outcome.status == "ok" { "ok" } else { "failed" })?;
    remember_agent(conn, run, agent, outcome)?;
    let pending: i64 = conn.query_row(
        "SELECT COUNT(*) FROM tasks WHERE instruction_id = ?1 AND batch_id = ?2 AND status IN ('pending', 'running')",
        params![instruction.id, instruction.current_batch],
        |row| row.get(0),
    )?;
    let next_phase = if pending == 0 || outcome.status != "ok" { "review" } else { "execute" };
    if has_queued(conn, &instruction.id)? {
        ensure_resume_phase(conn, &instruction.id, next_phase)?;
        let fresh = instructions::get(conn, &instruction.id)?;
        arm_next_input(conn, &fresh)?;
        return Ok(());
    }
    if next_phase == "review" {
        set_phase(conn, &instruction.id, "review")?;
        set_project_status(conn, &instruction.project_id, "discussing")?;
    }
    Ok(())
}

fn apply_member_discuss(conn: &Connection, instruction: &Instruction, agent: &Agent, run_id: &str, outcome: &RunOutcome) -> AppResult<()> {
    let run = load_run(conn, run_id)?;
    if outcome.status == "ok" {
        let content = if outcome.final_text.trim().is_empty() { "（没有输出）" } else { outcome.final_text.trim() };
        let speech = messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "discussion", content)?;
        messages::set_run(conn, speech.id, &run.id)?;
        finish_run(conn, &run.id, outcome, "ok")?;
    } else {
        // 记为未发言，本轮照常继续。半截输出不当成发言。
        let reason = outcome.error.as_deref().unwrap_or("回合没有完成");
        messages::insert(conn, &instruction.project_id, "system", None, "system", &format!("{} 本轮未发言：{reason}", agent.name))?;
        finish_run(conn, &run.id, outcome, "failed")?;
    }
    remember_agent(conn, &run, agent, outcome)?;
    if running_count(conn, &instruction.id)? == 0 {
        if has_queued(conn, &instruction.id)? {
            ensure_resume_phase(conn, &instruction.id, "decide")?;
            let fresh = instructions::get(conn, &instruction.id)?;
            arm_next_input(conn, &fresh)?;
        } else {
            set_phase(conn, &instruction.id, "decide")?;
        }
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
    let unread = unread_for(conn, &instruction.project_id, agent, bound)?;
    // 只在新会话里作为恢复包之后的补充展示。
    let recent = messages::list(conn, &instruction.project_id, Some(bound + 1), 20)?;
    let rules_version = rules_version(&rules);
    let roster_version = roster_version(&roster);
    let mode = if stage == Stage::Execute { "execute" } else { "discuss" };
    let fresh = crate::store::runs::resumable_session(conn, agent, mode)?.is_none();
    let rounds = instruction.discuss_rounds.max(0) as u32;
    let snapshot = TeamSnapshot {
        message_bound: bound,
        rules_version: &rules_version,
        roster_version: &roster_version,
        unread: &unread,
        recent: &recent,
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
    let mut prompt = prompt;
    // 只有组长回合会留下协议纠错的标记。
    if let Some(error) = previous_remedy(conn, &instruction.id, &agent.id, None)? {
        if let Some(reason) = error.strip_prefix(PROTOCOL_REMEDY) {
            prompt.push_str(&format!(
                "\n\n# 协议纠错\n你上一条回复没有被执行，原因：{reason}\n\
请重新给出完整的回复：先写发言，末尾带且只带一个闭合的 imates 代码块，内容是当前阶段允许的一个 JSON 指令。\n\
这是唯一一次重发机会。"
            ));
        }
    }
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

/// 保存组长的发言，并把消息链接到产生它的运行（用于显示该次运行使用的模型）。
fn save_speech(conn: &Connection, instruction: &Instruction, agent: &Agent, run_id: &str, speech: &str) -> AppResult<()> {
    if speech.trim().is_empty() {
        return Ok(());
    }
    let message = messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "discussion", speech.trim())?;
    messages::set_run(conn, message.id, run_id)?;
    Ok(())
}

fn remember_agent(conn: &Connection, run: &RunRow, agent: &Agent, outcome: &RunOutcome) -> AppResult<()> {
    // 会话一旦建立就要记住，哪怕这个回合后来失败了。
    if let Some(session_id) = outcome.session_id.as_deref() {
        agents::set_session(conn, &agent.id, Some(session_id))?;
    }
    // 失败回合不推进游标，下一回合会重新带上这些消息。
    if outcome.status != "ok" {
        return Ok(());
    }
    // 人设在回合进行中被改过时传一个不可能相等的值：游标照常推进，待更新标记保留。
    let applied = if persona_mark(&agent.persona) == run.persona_mark { agent.persona.as_str() } else { "\u{0}" };
    agents::finish_context(conn, &agent.id, applied, run.message_bound)
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

/// 人设的长度加 64 位 FNV-1a。只用来比较「是不是同一版人设」。
fn persona_mark(persona: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in persona.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{}:{hash:016x}", persona.len())
}

fn insert_run(conn: &Connection, agent: &Agent, mode: &str, instruction_id: &str, task_id: Option<&str>, bound: i64) -> AppResult<String> {
    let id = new_id();
    // 不记录提示词本身，只记录回合结束时要用的两项事实。
    let command = serde_json::json!({
        "source": "orchestrator", "mode": mode, "messageBound": bound, "personaMark": persona_mark(&agent.persona),
    }).to_string();
    conn.execute(
        "INSERT INTO runs (id, agent_id, mode, command, started_at, status, instruction_id, task_id) \
         VALUES (?1, ?2, ?3, ?4, ?5, 'running', ?6, ?7)",
        params![id, agent.id, mode, command, now_ms(), instruction_id, task_id],
    )?;
    Ok(id)
}

fn finish_run(conn: &Connection, run_id: &str, outcome: &RunOutcome, status: &str) -> AppResult<()> {
    let usage = outcome.usage.as_ref().map(serde_json::to_string).transpose()
        .map_err(|error| invalid(format!("用量序列化失败：{error}")))?;
    let changed = conn.execute(
        "UPDATE runs SET status = ?1, ended_at = ?2, exit_code = ?3, error = ?4, session_id = COALESCE(?5, session_id), \
         usage_json = ?6 WHERE id = ?7 AND status = 'running'",
        params![status, now_ms(), outcome.exit_code, outcome.error, outcome.session_id, usage, run_id],
    )?;
    if changed == 0 {
        return Err(invalid("运行已结束"));
    }
    Ok(())
}

/// 自动补救过的 run 用这个前缀标在 error 里。额度是否用过从上一条 run 判断，不另存状态。
const REMEDY_PREFIX: &str = "自动补救：";
const PROTOCOL_REMEDY: &str = "自动补救：协议错误：";

/// 用指定的说明结束一个 run，其余字段取自回合结果。
fn finish_run_with(conn: &Connection, run_id: &str, outcome: &RunOutcome, status: &str, error: &str) -> AppResult<()> {
    let mut noted = outcome.clone();
    noted.error = Some(error.to_string());
    finish_run(conn, run_id, &noted, status)
}

/// 这个成员在这条指令里最近一条已结束的 run 如果是一次自动补救，返回它的说明。
/// 返回 Some 表示当前这个逻辑回合的补救额度已经用掉。
fn previous_remedy(conn: &Connection, instruction_id: &str, agent_id: &str, except_run: Option<&str>) -> AppResult<Option<String>> {
    let error: Option<Option<String>> = conn.query_row(
        "SELECT error FROM runs WHERE instruction_id = ?1 AND agent_id = ?2 AND status <> 'running' \
         AND (?3 IS NULL OR id <> ?3) ORDER BY rowid DESC LIMIT 1",
        params![instruction_id, agent_id, except_run],
        |row| row.get(0),
    ).optional()?;
    Ok(error.flatten().filter(|text| text.starts_with(REMEDY_PREFIX)))
}

fn load_run(conn: &Connection, run_id: &str) -> AppResult<RunRow> {
    conn.query_row(
        "SELECT id, agent_id, mode, status, instruction_id, task_id, command FROM runs WHERE id = ?1",
        [run_id],
        |row| {
            let command: Option<String> = row.get(6)?;
            let facts: serde_json::Value = command.as_deref().and_then(|text| serde_json::from_str(text).ok()).unwrap_or_default();
            Ok(RunRow {
                id: row.get(0)?,
                agent_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                mode: row.get(2)?,
                status: row.get(3)?,
                instruction_id: row.get::<_, Option<String>>(4)?.unwrap_or_default(),
                task_id: row.get(5)?,
                // 旧记录没有这两项：上界按 0 处理（游标只增不减），人设标记留空（不会清除待更新标记）。
                message_bound: facts["messageBound"].as_i64().unwrap_or(0),
                persona_mark: facts["personaMark"].as_str().unwrap_or_default().to_string(),
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

/// 一次带入提示词的未读消息上限。超过时只带最新的这些。
const UNREAD_LIMIT: i64 = 200;

/// 这个成员还没读过的消息：游标之后、上界以内、不是本人发的。按 ID 升序。
fn unread_for(conn: &Connection, project_id: &str, agent: &Agent, bound: i64) -> AppResult<Vec<Message>> {
    let cursor = agent.last_seen_message_id.unwrap_or(0);
    let mut statement = conn.prepare(
        "SELECT id FROM messages WHERE project_id = ?1 AND id > ?2 AND id <= ?3 \
         AND (sender_agent_id IS NULL OR sender_agent_id <> ?4) ORDER BY id DESC LIMIT ?5",
    )?;
    let mut ids = statement
        .query_map(params![project_id, cursor, bound, agent.id, UNREAD_LIMIT], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    ids.reverse();
    ids.into_iter().map(|id| messages::get(conn, id)).collect()
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
        return Ok(StoredSnapshot::default());
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

fn has_queued(conn: &Connection, instruction_id: &str) -> AppResult<bool> {
    Ok(instructions::list_inputs(conn, instruction_id)?.iter().any(|input| input.status == "queued"))
}

fn ensure_resume_phase(conn: &Connection, instruction_id: &str, phase: &str) -> AppResult<()> {
    let fresh = instructions::get(conn, instruction_id)?;
    if fresh.resume_phase.is_some() {
        return Ok(());
    }
    conn.execute(
        "UPDATE instructions SET resume_phase = ?1, updated_at = ?2 WHERE id = ?3",
        params![phase, now_ms(), instruction_id],
    )?;
    Ok(())
}

fn arm_next_input(conn: &Connection, instruction: &Instruction) -> AppResult<()> {
    let inputs = instructions::list_inputs(conn, &instruction.id)?;
    let Some(input) = inputs.into_iter().find(|item| item.status == "queued") else {
        return Ok(());
    };
    let now = now_ms();
    conn.execute(
        "UPDATE instruction_inputs SET status = 'done', finished_at = ?1 WHERE id = ?2",
        params![now, input.id],
    )?;
    let message = messages::get(conn, input.message_id)?;
    let fresh = instructions::get(conn, &instruction.id)?;
    if input.kind == "direct" {
        let agent_id = input.target_agent_id.clone().ok_or_else(|| invalid("点名必须指定成员"))?;
        let mut snap = load_snapshot(&fresh.context_snapshot_json)?;
        snap.mention = Some(StoredMention { agent_id, content: message.content, launched: false });
        save_snapshot(conn, &instruction.id, &snap)?;
        set_phase(conn, &instruction.id, "direct")?;
        return Ok(());
    }
    let snap = load_snapshot(&fresh.context_snapshot_json)?;
    if snap.stashed_text.is_none() && fresh.resume_phase.is_none() && !matches!(fresh.phase.as_str(), "intervention" | "direct") {
        conn.execute(
            "UPDATE instructions SET resume_phase = ?1, updated_at = ?2 WHERE id = ?3",
            params![fresh.phase, now, instruction.id],
        )?;
    }
    conn.execute(
        "UPDATE instructions SET wait_reason = ?1, updated_at = ?2 WHERE id = ?3",
        params![message.content, now, instruction.id],
    )?;
    set_phase(conn, &instruction.id, "intervention")?;
    set_project_status(conn, &instruction.project_id, "discussing")
}

fn park_leader(conn: &Connection, run: &RunRow, instruction: &Instruction, agent: &Agent, outcome: &RunOutcome) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    save_speech(&tx, instruction, agent, &run.id, speech_of(&outcome.final_text))?;
    finish_run(&tx, &run.id, outcome, "ok")?;
    remember_agent(&tx, run, agent, outcome)?;
    let mut snap = load_snapshot(&instructions::get(&tx, &instruction.id)?.context_snapshot_json)?;
    snap.stashed_text = Some(outcome.final_text.clone());
    snap.stashed_phase = Some(instruction.phase.clone());
    snap.stashed_run_id = Some(run.id.clone());
    save_snapshot(&tx, &instruction.id, &snap)?;
    arm_next_input(&tx, instruction)?;
    tx.commit()?;
    Ok(())
}

fn speech_of(text: &str) -> &str {
    text.split("```imates").next().unwrap_or(text).trim()
}

fn apply_mention(conn: &Connection, run: &RunRow, instruction: &Instruction, agent: &Agent, outcome: &RunOutcome) -> AppResult<()> {
    let content = if outcome.final_text.trim().is_empty() {
        outcome.error.clone().unwrap_or_else(|| "点名没有完成".to_string())
    } else {
        outcome.final_text.clone()
    };
    let report = messages::insert(conn, &instruction.project_id, "agent", Some(&agent.id), "report", &content)?;
    messages::set_run(conn, report.id, &run.id)?;
    finish_run(conn, &run.id, outcome, if outcome.status == "ok" { "ok" } else { "failed" })?;
    remember_agent(conn, run, agent, outcome)?;
    let fresh = instructions::get(conn, &instruction.id)?;
    let mut snap = load_snapshot(&fresh.context_snapshot_json)?;
    snap.mention = None;
    save_snapshot(conn, &instruction.id, &snap)?;
    conn.execute(
        "UPDATE instructions SET phase = 'intervention', wait_reason = ?1, updated_at = ?2 WHERE id = ?3",
        params![format!("点名结果：{content}"), now_ms(), instruction.id],
    )?;
    set_project_status(conn, &instruction.project_id, "discussing")
}

fn apply_intervention(
    conn: &Connection,
    run: &RunRow,
    instruction: &Instruction,
    agent: &Agent,
    outcome: &RunOutcome,
    parsed: &protocol::ParsedLeaderTurn,
) -> AppResult<()> {
    let tx = conn.unchecked_transaction()?;
    save_speech(&tx, instruction, agent, &run.id, &parsed.speech)?;
    finish_run(&tx, &run.id, outcome, "ok")?;
    remember_agent(&tx, run, agent, outcome)?;
    tx.commit()?;
    match (&parsed.action.kind, parsed.action.disposition) {
        (ActionKind::Reply { .. }, Some(protocol::Disposition::Continue)) => continue_instruction(conn, instruction),
        (ActionKind::Reply { .. }, Some(protocol::Disposition::End)) => end_instruction(conn, instruction),
        (ActionKind::Assign { decision, tasks }, Some(protocol::Disposition::Replace)) => {
            replace_batch(conn, instruction, agent, &run.id, decision, tasks)
        }
        _ => {
            mark_waiting(conn, instruction, "干预阶段只接受 reply+continue、reply+end 或 assign+replace")?;
            Err(invalid("干预阶段只接受 reply+continue、reply+end 或 assign+replace"))
        }
    }
}

fn continue_instruction(conn: &Connection, instruction: &Instruction) -> AppResult<()> {
    if has_queued(conn, &instruction.id)? {
        let fresh = instructions::get(conn, &instruction.id)?;
        return arm_next_input(conn, &fresh);
    }
    let fresh = instructions::get(conn, &instruction.id)?;
    let snap = load_snapshot(&fresh.context_snapshot_json)?;
    if let Some(text) = snap.stashed_text.clone() {
        let phase = snap.stashed_phase.clone().unwrap_or_else(|| fresh.phase.clone());
        let stashed_run_id = snap.stashed_run_id.clone();
        clear_stash(conn, &fresh.id)?;
        conn.execute("UPDATE instructions SET resume_phase = NULL, updated_at = ?1 WHERE id = ?2", params![now_ms(), fresh.id])?;
        let roster = agents::list(conn, &fresh.project_id)?;
        let task = rework_limit(conn, &text)?;
        let limits = LimitContext { discuss_rounds: fresh.discuss_rounds.max(0) as u32, task };
        let parsed = parse_final(&text, protocol_phase(&phase), &roster, &limits)?;
        return replay_action(conn, &fresh, parsed, stashed_run_id.as_deref());
    }
    restore_phase(conn, &fresh)
}

fn end_instruction(conn: &Connection, instruction: &Instruction) -> AppResult<()> {
    clear_stash(conn, &instruction.id)?;
    let now = now_ms();
    conn.execute(
        "UPDATE tasks SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status IN ('pending', 'running')",
        params![now, instruction.id],
    )?;
    instructions::cancel_queued(conn, &instruction.id)?;
    if let Some(notice) = leftover_notice(&instructions::list_tasks(conn, &instruction.id)?) {
        messages::insert(conn, &instruction.project_id, "system", None, "system", &notice)?;
    }
    conn.execute(
        "UPDATE instructions SET status = 'completed', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
        params![now, instruction.id],
    )?;
    set_project_status(conn, &instruction.project_id, "idle")
}

fn replace_batch(
    conn: &Connection,
    instruction: &Instruction,
    agent: &Agent,
    run_id: &str,
    decision: &str,
    tasks: &[protocol::AssignedTask],
) -> AppResult<()> {
    clear_stash(conn, &instruction.id)?;
    conn.execute(
        "UPDATE tasks SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status = 'pending'",
        params![now_ms(), instruction.id],
    )?;
    let batch: Vec<NewTask> = tasks.iter().map(|task| NewTask {
        assignee_agent_id: task.to.clone(),
        title: task.title.clone(),
        detail: task.detail.clone(),
        replaces_task_id: task.replaces_task_id.clone(),
    }).collect();
    let inserted = instructions::insert_batch(conn, &instruction.id, Some(&agent.id), decision, &batch)?;
    messages::set_run(conn, inserted.decision_message_id, run_id)?;
    set_phase(conn, &instruction.id, "execute")?;
    set_project_status(conn, &instruction.project_id, "executing")
}

/// 重放暂存的组长动作。`run_id` 是产生该动作的组长运行；旧数据没有记录时为 None，决议消息不链接。
fn replay_action(conn: &Connection, instruction: &Instruction, parsed: protocol::ParsedLeaderTurn, run_id: Option<&str>) -> AppResult<()> {
    let leader = leader(conn, &instruction.project_id)?;
    match parsed.action.kind {
        ActionKind::Assign { decision, tasks } => {
            let batch: Vec<NewTask> = tasks.into_iter().map(|task| NewTask {
                assignee_agent_id: task.to,
                title: task.title,
                detail: task.detail,
                replaces_task_id: task.replaces_task_id,
            }).collect();
            let inserted = instructions::insert_batch(conn, &instruction.id, Some(&leader.id), &decision, &batch)?;
            if let Some(run_id) = run_id {
                messages::set_run(conn, inserted.decision_message_id, run_id)?;
            }
            set_phase(conn, &instruction.id, "execute")?;
            set_project_status(conn, &instruction.project_id, "executing")
        }
        ActionKind::Reply { .. } => {
            if let Some(notice) = leftover_notice(&instructions::list_tasks(conn, &instruction.id)?) {
                messages::insert(conn, &instruction.project_id, "system", None, "system", &notice)?;
            }
            let now = now_ms();
            conn.execute(
                "UPDATE instructions SET status = 'completed', finished_at = ?1, updated_at = ?1 WHERE id = ?2",
                params![now, instruction.id],
            )?;
            set_project_status(conn, &instruction.project_id, "idle")
        }
        ActionKind::Discuss { to, topic } => {
            let stored = StoredSnapshot {
                pending: Some(StoredPending::Discuss { agent_ids: to, topic, launched: false }),
                ..StoredSnapshot::default()
            };
            conn.execute(
                "UPDATE instructions SET phase = 'discuss', discuss_rounds = discuss_rounds + 1, context_snapshot_json = ?1, updated_at = ?2 WHERE id = ?3",
                params![serde_json::to_string(&stored).map_err(|error| invalid(error.to_string()))?, now_ms(), instruction.id],
            )?;
            set_project_status(conn, &instruction.project_id, "discussing")
        }
        ActionKind::Rework { task_id, .. } => {
            let changed = conn.execute(
                "UPDATE tasks SET status = 'pending', rework_count = rework_count + 1, finished_at = NULL WHERE id = ?1 AND instruction_id = ?2",
                params![task_id, instruction.id],
            )?;
            if changed == 0 {
                return Err(AppError::NotFound(format!("任务 {task_id}")));
            }
            set_phase(conn, &instruction.id, "execute")?;
            set_project_status(conn, &instruction.project_id, "executing")
        }
    }
}

fn restore_phase(conn: &Connection, instruction: &Instruction) -> AppResult<()> {
    let fresh = instructions::get(conn, &instruction.id)?;
    let phase = fresh.resume_phase.clone().filter(|phase| phase != "intervention" && phase != "direct").unwrap_or_else(|| {
        if matches!(fresh.phase.as_str(), "intervention" | "direct") { "triage".to_string() } else { fresh.phase.clone() }
    });
    conn.execute(
        "UPDATE instructions SET phase = ?1, resume_phase = NULL, updated_at = ?2 WHERE id = ?3",
        params![phase, now_ms(), instruction.id],
    )?;
    let status = if phase == "execute" { "executing" } else { "discussing" };
    set_project_status(conn, &instruction.project_id, status)
}

fn clear_stash(conn: &Connection, instruction_id: &str) -> AppResult<()> {
    let fresh = instructions::get(conn, instruction_id)?;
    let mut snap = load_snapshot(&fresh.context_snapshot_json)?;
    snap.stashed_text = None;
    snap.stashed_phase = None;
    snap.stashed_run_id = None;
    save_snapshot(conn, instruction_id, &snap)
}

fn abandon_in(conn: &Connection, instruction_id: &str) -> AppResult<()> {
    let now = now_ms();
    conn.execute(
        "UPDATE instructions SET status = 'abandoned', updated_at = ?1, finished_at = ?1 WHERE id = ?2",
        params![now, instruction_id],
    )?;
    conn.execute(
        "UPDATE instruction_inputs SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status = 'queued'",
        params![now, instruction_id],
    )?;
    conn.execute(
        "UPDATE tasks SET status = 'cancelled', finished_at = ?1 WHERE instruction_id = ?2 AND status IN ('pending', 'running')",
        params![now, instruction_id],
    )?;
    Ok(())
}

fn retry_instruction(conn: &Connection, instruction: &Instruction) -> AppResult<Instruction> {
    if instruction.turn_count >= TURN_LIMIT {
        return Err(invalid("已达到 30 回合上限，不能靠重试清零"));
    }
    if matches!(instruction.phase.as_str(), "execute" | "direct") {
        return Err(invalid("执行阶段不能重试，请补充或放弃"));
    }
    if instructions::list_tasks(conn, &instruction.id)?.iter().any(|task| task.status == "failed") {
        return Err(invalid("已有失败任务，请补充说明后续，不能重试已执行的任务"));
    }
    conn.execute(
        "UPDATE instructions SET status = 'running', wait_reason = NULL, finished_at = NULL, updated_at = ?1 WHERE id = ?2",
        params![now_ms(), instruction.id],
    )?;
    set_project_status(conn, &instruction.project_id, "discussing")?;
    instructions::get(conn, &instruction.id)
}

fn supplement_instruction(conn: &Connection, instruction: &Instruction, note: &str) -> AppResult<Instruction> {
    let note = note.trim();
    if note.is_empty() {
        return Err(invalid("补充内容不能为空"));
    }
    let tx = conn.unchecked_transaction()?;
    if instruction.resume_phase.is_none() {
        tx.execute(
            "UPDATE instructions SET resume_phase = ?1, updated_at = ?2 WHERE id = ?3",
            params![instruction.phase, now_ms(), instruction.id],
        )?;
    }
    tx.execute(
        "UPDATE instructions SET status = 'running', wait_reason = NULL, finished_at = NULL, updated_at = ?1 WHERE id = ?2",
        params![now_ms(), instruction.id],
    )?;
    let message = messages::insert(&tx, &instruction.project_id, "user", None, "instruction", note)?;
    tx.execute(
        "INSERT INTO instruction_inputs (id, instruction_id, message_id, kind, target_agent_id, status, created_at) \
         VALUES (?1, ?2, ?3, 'supplement', NULL, 'queued', ?4)",
        params![new_id(), instruction.id, message.id, now_ms()],
    )?;
    tx.commit()?;
    let fresh = instructions::get(conn, &instruction.id)?;
    arm_next_input(conn, &fresh)?;
    instructions::get(conn, &instruction.id)
}

fn abandon_instruction(conn: &Connection, instruction: &Instruction) -> AppResult<Instruction> {
    let tx = conn.unchecked_transaction()?;
    abandon_in(&tx, &instruction.id)?;
    set_project_status(&tx, &instruction.project_id, "idle")?;
    tx.commit()?;
    instructions::get(conn, &instruction.id)
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

    #[test]
    fn grok_team_discuss_uses_recovery_when_last_session_was_execute() {
        let (conn, project_id, members) = setup(&["开发"]);
        let leader_id = &members[0].id;
        conn.execute("UPDATE agents SET cli='grok',session_id='旧执行会话' WHERE id=?1", [leader_id]).unwrap();
        let old = crate::store::runs::insert_running(&conn, Some(leader_id), "execute", "{}", Some("旧执行会话")).unwrap();
        crate::store::runs::finish(&conn, &old.id, "ok", Some(0), None, None).unwrap();
        start_team(&conn, &project_id);
        let turns = advance(&conn, &project_id).unwrap();
        assert_eq!(turns.len(), 1);
        assert_eq!(turns[0].mode, "discuss");
        assert!(turns[0].prompt.contains("# 恢复包"));
        assert!(turns[0].prompt.contains("给登录加上校验"));
        let leader = agents::get(&conn, leader_id).unwrap();
        assert_eq!(crate::store::runs::resumable_session(&conn, &leader, "discuss").unwrap(), None);
        assert_eq!(crate::store::runs::resumable_session(&conn, &leader, "execute").unwrap(), Some("旧执行会话"));
        let mixed = crate::store::runs::insert_running(&conn, Some(leader_id), "discuss", "{}", Some("旧执行会话")).unwrap();
        crate::store::runs::finish(&conn, &mixed.id, "ok", Some(0), None, None).unwrap();
        assert_eq!(crate::store::runs::resumable_session(&conn, &leader, "execute").unwrap(), None);
        agents::set_session(&conn, leader_id, Some("没有运行记录的会话")).unwrap();
        let leader = agents::get(&conn, leader_id).unwrap();
        assert_eq!(crate::store::runs::resumable_session(&conn, &leader, "execute").unwrap(), None);
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

    /// 给运行写入可区分的模型值，返回期望的 model_info。
    fn tag_run(conn: &rusqlite::Connection, run_id: &str, tag: &str) -> Option<messages::ModelInfo> {
        let (requested, reported) = (format!("请求-{tag}"), format!("报告-{tag}"));
        conn.execute("UPDATE runs SET requested_model = ?1, reported_model = ?2 WHERE id = ?3", rusqlite::params![requested, reported, run_id]).unwrap();
        Some(messages::ModelInfo { requested_model: Some(requested), reported_model: Some(reported) })
    }

    fn infos_of(conn: &rusqlite::Connection, project_id: &str, kind: &str) -> Vec<Option<messages::ModelInfo>> {
        messages::list(conn, project_id, None, 100).unwrap().into_iter().filter(|message| message.kind == kind).map(|message| message.model_info).collect()
    }

    #[test]
    fn member_messages_link_to_the_run_that_produced_them_and_others_do_not() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        // 讨论：组长发言与成员发言各链接到自己的运行。
        let leader = advance(&conn, &project_id).unwrap();
        let leader_info = tag_run(&conn, &leader[0].run_id, "组长讨论");
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&fence("先讨论。", r#"{"action":"discuss","to":["前端"],"topic":"校验"}"#))).unwrap();
        let members = advance(&conn, &project_id).unwrap();
        let member_info = tag_run(&conn, &members[0].run_id, "成员讨论");
        apply_outcome(&conn, &members[0].run_id, &ok_text("成员的看法")).unwrap();
        assert_eq!(infos_of(&conn, &project_id, "discussion"), vec![leader_info, member_info]);

        // 派工：决议消息与组长发言链接到本次组长运行；执行报告链接到执行运行。
        let leader = advance(&conn, &project_id).unwrap();
        let decide_info = tag_run(&conn, &leader[0].run_id, "组长派工");
        let assign = fence("开工。", r#"{"action":"assign","decision":"先做校验","tasks":[{"to":"前端","title":"校验","detail":"做校验"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        assert_eq!(infos_of(&conn, &project_id, "decision"), vec![decide_info.clone()]);
        let exec = advance(&conn, &project_id).unwrap();
        let exec_info = tag_run(&conn, &exec[0].run_id, "成员执行");
        apply_outcome(&conn, &exec[0].run_id, &ok_text("校验做完了")).unwrap();
        assert_eq!(infos_of(&conn, &project_id, "report"), vec![exec_info]);

        // 用户消息与系统消息不链接。
        for message in messages::list(&conn, &project_id, None, 100).unwrap() {
            if message.sender_type != "agent" {
                assert_eq!(message.model_info, None, "{} 消息不应链接运行", message.sender_type);
            }
        }
    }

    #[test]
    fn replayed_decision_links_to_the_leader_run_that_was_parked() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let parked_info = tag_run(&conn, &leader[0].run_id, "被暂存的组长回合");
        queue_user_text(&conn, &project_id, "不要派工").unwrap();
        let assign = fence("先派工。", r#"{"action":"assign","decision":"先做接口","tasks":[{"to":"前端","title":"接口","detail":"做接口"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        assert_eq!(infos_of(&conn, &project_id, "discussion"), vec![parked_info.clone()]);
        let review = advance(&conn, &project_id).unwrap();
        let review_info = tag_run(&conn, &review[0].run_id, "干预回合");
        apply_outcome(&conn, &review[0].run_id, &ok_text(&fence(
            "按原计划。",
            r#"{"action":"reply","disposition":"continue","message":"继续"}"#,
        ))).unwrap();
        // 重放出来的决议是被暂存那一回合的产物，不是干预回合的。
        assert_eq!(infos_of(&conn, &project_id, "decision"), vec![parked_info.clone()]);
        assert_eq!(infos_of(&conn, &project_id, "discussion"), vec![parked_info, review_info]);
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

    #[test]
    fn running_messages_queue_without_cancelling_the_current_run() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let specs = advance(&conn, &project_id).unwrap();
        queue_user_text(&conn, &project_id, "补充一句：先看错误文案").unwrap();
        queue_user_text(&conn, &project_id, "@前端 先把按钮文案改掉").unwrap();
        let status: String = conn.query_row("SELECT status FROM runs WHERE id = ?1", [&specs[0].run_id], |row| row.get(0)).unwrap();
        assert_eq!(status, "running");
        assert!(advance(&conn, &project_id).unwrap().is_empty());
        let inputs = instructions::list_inputs(&conn, &specs[0].instruction_id).unwrap();
        assert_eq!(inputs.len(), 2);
        assert_eq!(inputs[0].kind, "supplement");
        assert_eq!(inputs[0].status, "queued");
        assert!(inputs[0].target_agent_id.is_none());
        assert_eq!(inputs[1].kind, "direct");
        assert_eq!(inputs[1].target_agent_id.as_deref(), Some(agents[1].id.as_str()));
    }

    #[test]
    fn boundary_handles_supplement_then_mention_before_resuming_the_batch() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let assign = fence("分两步。", r#"{"action":"assign","decision":"分两步做","tasks":[{"to":"前端","title":"第一步","detail":"做第一步"},{"to":"前端","title":"第二步","detail":"做第二步"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        let first = advance(&conn, &project_id).unwrap();
        queue_user_text(&conn, &project_id, "补充：标题要短").unwrap();
        queue_user_text(&conn, &project_id, "@前端 先改按钮").unwrap();
        assert!(advance(&conn, &project_id).unwrap().is_empty());
        apply_outcome(&conn, &first[0].run_id, &ok_text("第一步完成")).unwrap();

        let intervention = advance(&conn, &project_id).unwrap();
        assert_eq!(intervention.len(), 1);
        assert!(intervention[0].prompt.contains("标题要短"));
        let tasks = instructions::list_tasks(&conn, &first[0].instruction_id).unwrap();
        assert_eq!(tasks[1].status, "pending");
        assert_ne!(intervention[0].task_id.as_deref(), Some(tasks[1].id.as_str()));
        apply_outcome(&conn, &intervention[0].run_id, &ok_text(&fence(
            "先处理插话。",
            r#"{"action":"reply","disposition":"continue","message":"继续原任务"}"#,
        ))).unwrap();

        let mention = advance(&conn, &project_id).unwrap();
        assert_eq!(mention.len(), 1);
        assert_eq!(mention[0].agent_id, agents[1].id);
        assert_eq!(mention[0].mode, "execute");
        assert!(mention[0].task_id.is_none());
        assert!(mention[0].prompt.contains("先改按钮"));
        assert_eq!(instructions::get_task(&conn, &tasks[1].id).unwrap().status, "pending");
        apply_outcome(&conn, &mention[0].run_id, &ok_text("按钮已改")).unwrap();

        let after_mention = advance(&conn, &project_id).unwrap();
        assert!(after_mention[0].prompt.contains("按钮已改"));
        apply_outcome(&conn, &after_mention[0].run_id, &ok_text(&fence(
            "点名已经做完。",
            r#"{"action":"reply","disposition":"continue","message":"回到原批次"}"#,
        ))).unwrap();
        let second = advance(&conn, &project_id).unwrap();
        assert_eq!(second[0].task_id.as_deref(), Some(tasks[1].id.as_str()));
        assert!(second[0].prompt.contains("第二步"));
    }

    #[test]
    fn queued_input_stashes_the_leader_action_until_continue() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        queue_user_text(&conn, &project_id, "不要派工").unwrap();
        let assign = fence("先派工。", r#"{"action":"assign","decision":"先做接口","tasks":[{"to":"前端","title":"接口","detail":"做接口"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        let parked = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(parked.phase, "intervention");
        assert!(instructions::list_tasks(&conn, &parked.id).unwrap().is_empty());
        let review = advance(&conn, &project_id).unwrap();
        assert!(review[0].prompt.contains("不要派工"));
        apply_outcome(&conn, &review[0].run_id, &ok_text(&fence(
            "按原计划。",
            r#"{"action":"reply","disposition":"continue","message":"继续"}"#,
        ))).unwrap();
        let restored = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(restored.phase, "execute");
        assert_eq!(instructions::list_tasks(&conn, &restored.id).unwrap().len(), 1);
    }

    #[test]
    fn stop_cancels_generation_tasks_and_queue() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let assign = fence("分两步。", r#"{"action":"assign","decision":"分两步做","tasks":[{"to":"前端","title":"第一步","detail":"做第一步"},{"to":"前端","title":"第二步","detail":"做第二步"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        let first = advance(&conn, &project_id).unwrap();
        queue_user_text(&conn, &project_id, "先别继续").unwrap();
        let stopped = stop_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(stopped.status, "waiting_user");
        assert!(stopped.wait_reason.unwrap_or_default().contains("部分文件"));
        let tasks = instructions::list_tasks(&conn, &stopped.id).unwrap();
        assert_eq!(tasks[0].status, "cancelled");
        assert_eq!(tasks[1].status, "cancelled");
        assert!(instructions::list_inputs(&conn, &stopped.id).unwrap().iter().all(|input| input.status == "cancelled"));
        let run_status: String = conn.query_row("SELECT status FROM runs WHERE id = ?1", [&first[0].run_id], |row| row.get(0)).unwrap();
        assert_eq!(run_status, "cancelled");
        assert!(apply_outcome(&conn, &first[0].run_id, &ok_text("迟到的完成")).is_err());
        assert!(advance(&conn, &project_id).unwrap().is_empty());
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "waiting_user");
        let systems = messages::list(&conn, &project_id, None, 20).unwrap();
        assert!(systems.iter().any(|message| message.kind == "system" && message.content.contains("部分文件")));
        let _ = agents;
    }

    #[test]
    fn waiting_message_abandons_the_old_instruction_with_the_new_one() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let old = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        conn.execute("UPDATE instructions SET status = 'waiting_user' WHERE id = ?1", [&old.id]).unwrap();
        conn.execute(
            "INSERT INTO instruction_inputs (id, instruction_id, message_id, kind, status, created_at) VALUES ('queued-old', ?1, ?2, 'supplement', 'queued', 1)",
            rusqlite::params![old.id, old.source_message_id],
        ).unwrap();
        let new = begin_user_instruction(&conn, &project_id, "重新做登录校验").unwrap();
        assert_eq!(instructions::get(&conn, &old.id).unwrap().status, "abandoned");
        assert_eq!(new.status, "running");
        assert_eq!(new.kind, "team");
        assert_eq!(new.phase, "triage");
        assert_ne!(new.id, old.id);
        assert_eq!(instructions::active_instruction(&conn, &project_id).unwrap().unwrap().id, new.id);
        let queued: String = conn.query_row("SELECT status FROM instruction_inputs WHERE id = 'queued-old'", [], |row| row.get(0)).unwrap();
        assert_eq!(queued, "cancelled");
        assert!(begin_user_instruction(&conn, &project_id, "不该再开一条").is_err());
        assert_eq!(instructions::get(&conn, &new.id).unwrap().status, "running");
    }

    #[test]
    fn resume_retries_replayable_phase_and_supplement_or_abandon() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        fail_launch(&conn, &first[0].run_id, "进程没有启动").unwrap();
        stop_instruction(&conn, &project_id).unwrap();
        let waiting = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        let turns = waiting.turn_count;
        assert_eq!(waiting.phase, "triage");
        let retried = resume_instruction(&conn, &waiting.id, "retry", None).unwrap();
        assert_eq!(retried.status, "running");
        assert_eq!(retried.turn_count, turns);
        assert_eq!(retried.discuss_rounds, waiting.discuss_rounds);
        conn.execute("UPDATE instructions SET status = 'waiting_user', turn_count = 30 WHERE id = ?1", [&waiting.id]).unwrap();
        let capped = resume_instruction(&conn, &waiting.id, "retry", None).unwrap_err();
        assert!(capped.to_string().contains("30") || capped.to_string().contains("重试"));
        assert_eq!(instructions::get(&conn, &waiting.id).unwrap().turn_count, 30);
        assert_eq!(instructions::get(&conn, &waiting.id).unwrap().status, "waiting_user");

        conn.execute("UPDATE instructions SET phase = 'execute', status = 'waiting_user', turn_count = 4 WHERE id = ?1", [&waiting.id]).unwrap();
        instructions::insert_batch(&conn, &waiting.id, Some(&agents[0].id), "旧决议", &[
            NewTask { assignee_agent_id: agents[1].id.clone(), title: "已失败".into(), detail: "细节".into(), replaces_task_id: None },
            NewTask { assignee_agent_id: agents[1].id.clone(), title: "被取消".into(), detail: "细节".into(), replaces_task_id: None },
        ]).unwrap();
        let tasks = instructions::list_tasks(&conn, &waiting.id).unwrap();
        conn.execute("UPDATE tasks SET status = 'failed' WHERE id = ?1", [&tasks[0].id]).unwrap();
        conn.execute("UPDATE tasks SET status = 'cancelled' WHERE id = ?1", [&tasks[1].id]).unwrap();
        assert!(resume_instruction(&conn, &waiting.id, "retry", None).is_err());
        assert!(resume_instruction(&conn, &waiting.id, "supplement", Some("   ")).is_err());
        let supplemented = resume_instruction(&conn, &waiting.id, "supplement", Some("改用别的方案")).unwrap();
        assert_eq!(supplemented.status, "running");
        assert_eq!(supplemented.phase, "intervention");
        assert_eq!(supplemented.turn_count, 4);
        assert_eq!(instructions::get_task(&conn, &tasks[0].id).unwrap().status, "failed");
        assert_eq!(instructions::get_task(&conn, &tasks[1].id).unwrap().status, "cancelled");
        let prompt = advance(&conn, &project_id).unwrap();
        assert!(prompt[0].prompt.contains("改用别的方案"));

        conn.execute("UPDATE instructions SET status = 'waiting_user' WHERE id = ?1", [&waiting.id]).unwrap();
        let abandoned = resume_instruction(&conn, &waiting.id, "abandon", None).unwrap();
        assert_eq!(abandoned.status, "abandoned");
        assert!(instructions::active_instruction(&conn, &project_id).unwrap().is_none());
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "idle");
        assert_eq!(instructions::get_task(&conn, &tasks[1].id).unwrap().status, "cancelled");
    }

    #[test]
    fn idle_direct_completes_without_the_leader() {
        let (conn, project_id, agents) = setup(&["前端"]);
        let message = messages::insert(&conn, &project_id, "user", None, "chat", "@前端 改按钮文案").unwrap();
        let instruction = open_direct_instruction(&conn, &project_id, message.id, &agents[1].id).unwrap();
        assert_eq!(instruction.kind, "direct");
        assert_eq!(instruction.phase, "direct");
        assert_eq!(instruction.target_agent_id.as_deref(), Some(agents[1].id.as_str()));
        assert!(advance(&conn, &project_id).unwrap().is_empty());
        let runs: i64 = conn.query_row("SELECT COUNT(*) FROM runs", [], |row| row.get(0)).unwrap();
        assert_eq!(runs, 0);
        finish_direct_instruction(&conn, &instruction.id, true).unwrap();
        assert_eq!(instructions::get(&conn, &instruction.id).unwrap().status, "completed");
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "idle");
        assert!(instructions::active_instruction(&conn, &project_id).unwrap().is_none());
    }
    #[test]
    fn team_turn_usage_is_stored_and_serves_as_the_next_baseline() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let specs = advance(&conn, &project_id).unwrap();
        let mut outcome = ok_text(&fence("直接答复。", r#"{"action":"reply","message":"可以"}"#));
        outcome.usage = Some(crate::adapters::Usage {
            input_tokens: Some(12), output_tokens: Some(3),
            raw_usage: Some(serde_json::json!({"input_tokens": 12, "output_tokens": 3})),
            ..Default::default()
        });
        apply_outcome(&conn, &specs[0].run_id, &outcome).unwrap();
        let run = crate::store::runs::get(&conn, &specs[0].run_id).unwrap();
        let stored: serde_json::Value = serde_json::from_str(run.usage_json.as_deref().expect("团队回合应保存用量")).unwrap();
        assert_eq!(stored["inputTokens"], 12);
        assert_eq!(stored["outputTokens"], 3);
        assert!(stored["costUsd"].is_null());
        let baseline = crate::store::runs::latest_usage(&conn, &agents[0].id, "session").unwrap().unwrap();
        assert_eq!(baseline["rawUsage"]["output_tokens"], 3);
    }

    #[test]
    fn team_turn_without_usage_leaves_the_column_empty() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let specs = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &specs[0].run_id, &ok_text(&fence("直接答复。", r#"{"action":"reply","message":"可以"}"#))).unwrap();
        assert_eq!(crate::store::runs::get(&conn, &specs[0].run_id).unwrap().usage_json, None);
    }

    fn next_team(conn: &rusqlite::Connection, project_id: &str, text: &str) {
        let message = messages::insert(conn, project_id, "user", None, "instruction", text).unwrap();
        instructions::insert_instruction(conn, project_id, message.id, "team", None, "triage").unwrap();
    }

    #[test]
    fn failed_turn_keeps_the_cursor_and_success_uses_the_prompt_bound() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let assign = fence("派工。", r#"{"action":"assign","decision":"做一步","tasks":[{"to":"前端","title":"第一步","detail":"做第一步"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        let first = advance(&conn, &project_id).unwrap();
        let before = agents::get(&conn, &agents[1].id).unwrap().last_seen_message_id;
        apply_outcome(&conn, &first[0].run_id, &failed_text("做到一半")).unwrap();
        let member = agents::get(&conn, &agents[1].id).unwrap();
        assert_eq!(member.last_seen_message_id, before, "失败回合不推进游标");
        assert_eq!(member.session_id.as_deref(), Some("session"), "已经建立的会话仍要记住");

        let review = advance(&conn, &project_id).unwrap();
        let task_id = instructions::list_tasks(&conn, &first[0].instruction_id).unwrap()[0].id.clone();
        let rework = format!(r#"{{"action":"rework","task_id":"{task_id}","feedback":"重做"}}"#);
        apply_outcome(&conn, &review[0].run_id, &ok_text(&fence("打回。", &rework))).unwrap();
        let again = advance(&conn, &project_id).unwrap();
        let late = messages::insert(&conn, &project_id, "user", None, "chat", "回合进行中才到的消息").unwrap();
        apply_outcome(&conn, &again[0].run_id, &ok_text("做完了")).unwrap();
        let cursor = agents::get(&conn, &agents[1].id).unwrap().last_seen_message_id.unwrap();
        assert_eq!(cursor, again[0].message_bound, "游标取构建提示词时的上界");
        assert!(cursor < late.id, "回合进行中到达的消息留给下一回合");
    }

    #[test]
    fn edited_persona_reaches_an_existing_session_and_mid_run_edits_stay_pending() {
        let (conn, project_id, agents) = setup(&["前端"]);
        let reply = || ok_text(&fence("直接答复。", r#"{"action":"reply","message":"可以"}"#));
        let edit = |text: &str| agents::update(&conn, &agents[0].id, &NewAgent {
            name: "组长".into(), role: "管理".into(), persona: PERSONA.replace("本人风格", text),
            cli: "claude".into(), model: None, is_leader: true,
        }).unwrap();
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        assert!(first[0].prompt.contains("# 本人完整人设"), "首个回合用开场");
        apply_outcome(&conn, &first[0].run_id, &reply()).unwrap();
        assert!(!agents::get(&conn, &agents[0].id).unwrap().persona_dirty);

        edit("先问清楚再动手");
        next_team(&conn, &project_id, "第二条指令");
        let second = advance(&conn, &project_id).unwrap();
        assert!(second[0].prompt.contains("已更新的人设") && second[0].prompt.contains("先问清楚再动手"));
        assert!(!second[0].prompt.contains("# 本人完整人设"), "已有会话不重发开场");
        edit("回合进行中又改了一次");
        apply_outcome(&conn, &second[0].run_id, &reply()).unwrap();
        assert!(agents::get(&conn, &agents[0].id).unwrap().persona_dirty, "回合进行中的修改不能被这次回合确认掉");

        next_team(&conn, &project_id, "第三条指令");
        let third = advance(&conn, &project_id).unwrap();
        assert!(third[0].prompt.contains("回合进行中又改了一次"));
        apply_outcome(&conn, &third[0].run_id, &reply()).unwrap();
        assert!(!agents::get(&conn, &agents[0].id).unwrap().persona_dirty);
        next_team(&conn, &project_id, "第四条指令");
        assert!(!advance(&conn, &project_id).unwrap()[0].prompt.contains("已更新的人设"), "送达之后不再重复");
    }

    #[test]
    fn prompts_carry_only_messages_after_the_agents_cursor() {
        let (conn, project_id, agents) = setup(&["前端"]);
        messages::insert(&conn, &project_id, "user", None, "chat", "很早的旧消息").unwrap();
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        assert!(first[0].prompt.contains("很早的旧消息"), "游标为空的首个回合能看到历史");
        apply_outcome(&conn, &first[0].run_id, &ok_text(&fence("直接答复。", r#"{"action":"reply","message":"可以"}"#))).unwrap();

        messages::insert(&conn, &project_id, "user", None, "chat", "两条指令之间的新消息").unwrap();
        next_team(&conn, &project_id, "第二条指令");
        let second = advance(&conn, &project_id).unwrap();
        assert!(second[0].prompt.contains("第二条指令"));
        assert!(second[0].prompt.contains("两条指令之间的新消息"));
        assert!(!second[0].prompt.contains("很早的旧消息"), "读过的消息不重复灌入");
        assert!(!second[0].prompt.contains("给登录加上校验"), "上一条指令的原文也已经读过");
        assert!(!second[0].prompt.contains("直接答复。"), "本人发的消息不回灌");

        // 另一位成员还没有参与过，游标为空：第一次参与时看到全部未读，不受组长游标影响。
        let discuss = fence("讨论一下。", r#"{"action":"discuss","to":["前端"],"topic":"怎么做"}"#);
        apply_outcome(&conn, &second[0].run_id, &ok_text(&discuss)).unwrap();
        let member = advance(&conn, &project_id).unwrap();
        assert_eq!(member[0].agent_id, agents[1].id);
        assert!(member[0].prompt.contains("很早的旧消息") && member[0].prompt.contains("讨论一下。"));
    }

    #[test]
    fn unread_is_capped_to_the_newest_two_hundred() {
        let (conn, project_id, agents) = setup(&["前端"]);
        for index in 0..205 {
            messages::insert(&conn, &project_id, "user", None, "chat", &format!("消息{index:03}")).unwrap();
        }
        let bound: i64 = conn.query_row("SELECT MAX(id) FROM messages", [], |row| row.get(0)).unwrap();
        let unread = unread_for(&conn, &project_id, &agents[0], bound).unwrap();
        assert_eq!(unread.len(), 200);
        assert_eq!(unread[0].content, "消息005");
        assert_eq!(unread[199].content, "消息204");
        assert!(unread.windows(2).all(|pair| pair[0].id < pair[1].id));
        assert!(unread_for(&conn, &project_id, &agents[0], 0).unwrap().is_empty());
    }

    #[test]
    fn protocol_error_is_sent_back_once_and_a_corrected_reply_proceeds() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &first[0].run_id, &ok_text("我觉得可以直接做，但忘了写指令块")).unwrap();
        let active = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!((active.status.as_str(), active.phase.as_str(), active.turn_count), ("running", "triage", 1));
        let (status, error): (String, String) = conn.query_row("SELECT status, error FROM runs WHERE id = ?1", [&first[0].run_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(status, "failed");
        assert!(error.starts_with("自动补救：协议错误："), "{error}");
        assert!(messages::list(&conn, &project_id, None, 20).unwrap().iter().all(|m| !m.content.contains("忘了写指令块")), "第一次出错的原文不进群聊");

        let retry = advance(&conn, &project_id).unwrap();
        assert_eq!(retry.len(), 1);
        assert_eq!(turn_count(&conn, &project_id), 2, "纠错回合计入回合总数");
        assert!(retry[0].prompt.contains("# 协议纠错") && retry[0].prompt.contains(error.trim_start_matches("自动补救：协议错误：")));
        apply_outcome(&conn, &retry[0].run_id, &ok_text(&fence("补上了。", r#"{"action":"reply","message":"可以"}"#))).unwrap();
        assert!(instructions::active_instruction(&conn, &project_id).unwrap().is_none());

        // 下一条指令重新有一次纠错额度。
        next_team(&conn, &project_id, "第二条指令");
        let again = advance(&conn, &project_id).unwrap();
        assert!(!again[0].prompt.contains("# 协议纠错"));
        apply_outcome(&conn, &again[0].run_id, &ok_text("又忘了")).unwrap();
        assert_eq!(instructions::active_instruction(&conn, &project_id).unwrap().unwrap().status, "running");
    }

    #[test]
    fn second_protocol_error_shows_the_raw_text_and_waits_for_the_user() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &first[0].run_id, &ok_text("第一次没有指令块")).unwrap();
        let retry = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &retry[0].run_id, &ok_text("第二次还是没有指令块")).unwrap();
        let waiting = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(waiting.status, "waiting_user");
        assert!(waiting.wait_reason.unwrap_or_default().contains("协议"));
        assert_eq!(projects::get(&conn, &project_id).unwrap().status, "waiting_user");
        let stored = messages::list(&conn, &project_id, None, 20).unwrap();
        assert!(stored.iter().any(|m| m.sender_agent_id.as_deref() == Some(agents[0].id.as_str()) && m.content == "第二次还是没有指令块"), "重发后仍错，原文作为普通发言展示");
        assert!(stored.iter().any(|m| m.kind == "system" && m.content.contains("重发后仍然不符合指令协议")));
        assert!(advance(&conn, &project_id).unwrap().is_empty());

        // 用户点重试是新的逻辑回合，重新有一次纠错额度。
        let retried = resume_instruction(&conn, &waiting.id, "retry", None).unwrap();
        assert_eq!(retried.status, "running");
        let manual = advance(&conn, &project_id).unwrap();
        assert!(!manual[0].prompt.contains("# 协议纠错"));
        apply_outcome(&conn, &manual[0].run_id, &ok_text("第三次没有指令块")).unwrap();
        assert_eq!(instructions::get(&conn, &waiting.id).unwrap().status, "running");
    }

    #[test]
    fn failed_discussion_member_is_recorded_as_silent_and_the_round_continues() {
        let (conn, project_id, agents) = setup(&["前端", "测试"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&fence("讨论一下。", r#"{"action":"discuss","to":["前端","测试"],"topic":"登录校验"}"#))).unwrap();
        let members = advance(&conn, &project_id).unwrap();
        assert_eq!(members.len(), 2);
        apply_outcome(&conn, &members[0].run_id, &failed_text("说到一半")).unwrap();
        let active = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!(active.status, "running", "一个成员失败不让整条指令停下");
        assert_eq!(active.phase, "discuss", "还有成员没说完");
        let stored = messages::list(&conn, &project_id, None, 20).unwrap();
        assert!(stored.iter().any(|m| m.kind == "system" && m.content.contains("前端 本轮未发言") && m.content.contains("执行失败")));
        assert!(stored.iter().all(|m| m.content != "说到一半"), "半截输出不当成发言");
        assert_eq!(agents::get(&conn, &agents[1].id).unwrap().last_seen_message_id.unwrap_or(0), 0, "失败不推进游标");

        apply_outcome(&conn, &members[1].run_id, &ok_text("我的看法")).unwrap();
        assert_eq!(instructions::active_instruction(&conn, &project_id).unwrap().unwrap().phase, "decide");
        let decide = advance(&conn, &project_id).unwrap();
        assert_eq!(decide[0].agent_id, agents[0].id);
        assert!(decide[0].prompt.contains("前端 本轮未发言") && decide[0].prompt.contains("我的看法"));
    }

    #[test]
    fn a_round_where_everyone_fails_still_goes_to_the_leader() {
        let (conn, project_id, _) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&fence("讨论一下。", r#"{"action":"discuss","to":["前端"],"topic":"登录校验"}"#))).unwrap();
        let members = advance(&conn, &project_id).unwrap();
        let mut timeout = failed_text("");
        timeout.status = "timeout".into();
        timeout.error = Some("CLI 长时间没有输出，已停止".into());
        apply_outcome(&conn, &members[0].run_id, &timeout).unwrap();
        let active = instructions::active_instruction(&conn, &project_id).unwrap().unwrap();
        assert_eq!((active.status.as_str(), active.phase.as_str()), ("running", "decide"));
        assert!(advance(&conn, &project_id).unwrap()[0].prompt.contains("前端 本轮未发言：CLI 长时间没有输出"));
    }

    fn dead_session(session: &str) -> RunOutcome {
        RunOutcome {
            status: "failed".into(), session_id: Some(session.into()), final_text: String::new(),
            partial_text: String::new(), usage: None, exit_code: Some(1),
            error: Some(format!("No conversation found with session ID: {session}")),
            tool_seen: false, cleanup_complete: true,
        }
    }

    #[test]
    fn dead_session_is_rebuilt_once_with_a_fresh_opening() {
        let (conn, project_id, agents) = setup(&["前端"]);
        agents::set_session(&conn, &agents[0].id, Some("old-session")).unwrap();
        start_team(&conn, &project_id);
        let first = advance(&conn, &project_id).unwrap();
        assert!(!first[0].prompt.contains("# 本人完整人设"), "已有会话的回合不带开场");

        let rebuilt = rebuild_turn(&conn, &first[0].run_id, &dead_session("old-session")).unwrap().expect("应当重建");
        assert_eq!(rebuilt.agent_id, agents[0].id);
        assert_eq!((rebuilt.mode.as_str(), rebuilt.task_id.as_deref()), ("discuss", None));
        assert_ne!(rebuilt.run_id, first[0].run_id);
        assert!(rebuilt.prompt.contains("# 本人完整人设") && rebuilt.prompt.contains("# 恢复包") && rebuilt.prompt.contains("组长研判"));
        assert_eq!(agents::get(&conn, &agents[0].id).unwrap().session_id, None);
        assert_eq!(turn_count(&conn, &project_id), 2, "重建的回合计入回合总数");
        let (status, error): (String, String) = conn.query_row("SELECT status, error FROM runs WHERE id = ?1", [&first[0].run_id], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!(status, "failed");
        assert!(error.starts_with("自动补救："), "{error}");

        // 同一逻辑回合只补救一次：再失败就按普通失败处理，协议出错也不再重发。
        assert!(rebuild_turn(&conn, &rebuilt.run_id, &dead_session("old-session")).unwrap().is_none());
        apply_outcome(&conn, &rebuilt.run_id, &ok_text("重建之后忘了指令块")).unwrap();
        assert_eq!(instructions::active_instruction(&conn, &project_id).unwrap().unwrap().status, "waiting_user");
    }

    #[test]
    fn rebuild_keeps_the_running_task_and_refuses_ordinary_failures() {
        let (conn, project_id, agents) = setup(&["前端"]);
        start_team(&conn, &project_id);
        let leader = advance(&conn, &project_id).unwrap();
        let assign = fence("派工。", r#"{"action":"assign","decision":"做一步","tasks":[{"to":"前端","title":"第一步","detail":"做第一步"}]}"#);
        apply_outcome(&conn, &leader[0].run_id, &ok_text(&assign)).unwrap();
        agents::set_session(&conn, &agents[1].id, Some("member-session")).unwrap();
        let first = advance(&conn, &project_id).unwrap();
        let task_id = first[0].task_id.clone().unwrap();

        let mut wrote = dead_session("member-session");
        wrote.tool_seen = true;
        assert!(rebuild_turn(&conn, &first[0].run_id, &wrote).unwrap().is_none(), "有过工具调用就不重建");
        let mut other = dead_session("member-session");
        other.error = Some("模型不存在".into());
        assert!(rebuild_turn(&conn, &first[0].run_id, &other).unwrap().is_none(), "不是会话错误就不重建");
        assert!(rebuild_turn(&conn, &first[0].run_id, &ok_text("成功")).unwrap().is_none());

        let rebuilt = rebuild_turn(&conn, &first[0].run_id, &dead_session("member-session")).unwrap().expect("应当重建");
        assert_eq!((rebuilt.mode.as_str(), rebuilt.task_id.as_deref()), ("execute", Some(task_id.as_str())));
        assert!(rebuilt.prompt.contains("第一步") && rebuilt.prompt.contains("# 恢复包"));
        assert_eq!(instructions::get_task(&conn, &task_id).unwrap().status, "running", "任务没有被重复创建或改状态");
        apply_outcome(&conn, &rebuilt.run_id, &ok_text("第一步完成")).unwrap();
        assert_eq!(instructions::get_task(&conn, &task_id).unwrap().status, "done");
        assert!(rebuild_turn(&conn, &rebuilt.run_id, &dead_session("member-session")).unwrap().is_none(), "已结束的 run 不重建");
    }
}
