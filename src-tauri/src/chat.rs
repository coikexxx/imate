use rusqlite::Connection;
use crate::adapters::{AgentEvent, RunMode, TurnRequest};
use crate::error::{AppError, AppResult};
use crate::store::agents::{self, Agent};
use crate::store::{messages, projects};
use crate::store::messages::Message;
use crate::prompts::{build_turn_prompt, TurnPromptSnapshot};

pub fn resolve_recipient<'a>(
    text: &'a str,
    agents: &'a [crate::store::agents::Agent],
) -> crate::error::AppResult<(&'a crate::store::agents::Agent, &'a str)> {
    use crate::error::AppError;
    let text = text.trim();
    if text.is_empty() { return Err(AppError::Invalid("消息不能为空".into())); }
    if let Some(mention) = text.strip_prefix('@') {
        let mut matches = Vec::new();
        for end in mention.char_indices().filter(|(_, c)| c.is_whitespace())
            .map(|(i, _)| i).chain(std::iter::once(mention.len())) {
            let key = mention[..end].trim().to_lowercase();
            let found: Vec<_> = agents.iter()
                .filter(|agent| agent.name.trim().to_lowercase() == key).collect();
            if found.len() > 1 {
                return Err(AppError::Invalid("成员名归一后有冲突，请先修改名称".into()));
            }
            if let Some(agent) = found.first() {
                matches.push((*agent, mention[end..].trim(), end));
            }
        }
        matches.sort_by_key(|(_, _, end)| std::cmp::Reverse(*end));
        let Some((agent, prompt, _)) = matches.into_iter().next() else {
            return Err(AppError::Invalid("未找到被点名的成员，请从成员列表选择".into()));
        };
        if prompt.is_empty() { return Err(AppError::Invalid("点名后请填写消息内容".into())); }
        return Ok((agent, prompt));
    }
    let leader = agents.iter().find(|agent| agent.is_leader)
        .ok_or_else(|| AppError::Invalid("项目没有组长，请先配置成员".into()))?;
    Ok((leader, text))
}

pub struct PreparedTurn {
    pub agent: Agent,
    pub prompt: String,
    pub request_text: String,
    pub snapshot_message_id: i64,
    pub unread: Vec<Message>,
    pub recovery: bool,
}

pub struct ChatService;
impl ChatService {
    /// 调用方先在同一事务里写入用户消息，本方法只读取启动时快照。
    pub fn send(conn: &Connection, project_id: &str, user_message_id: i64, recovery: bool) -> AppResult<PreparedTurn> {
        let project = projects::get(conn, project_id)?;
        if matches!(project.status.as_str(), "discussing" | "executing") {
            return Err(AppError::Invalid("项目正在运行，请先停止或等待完成".into()));
        }
        let roster = agents::list(conn, project_id)?;
        let current = messages::get(conn, user_message_id)?;
        if current.project_id != project_id || current.sender_type != "user" { return Err(AppError::Invalid("用户消息不属于当前项目".into())); }
        let (agent, request) = resolve_recipient(&current.content, &roster)?;
        if !crate::cli_registry::is_supported(&agent.cli) {
            return Err(AppError::Invalid(format!("CLI {} 尚未接入，计划在 M5 实现", agent.cli)));
        }
        let recovery = recovery || (agent.session_id.is_some()
            && crate::store::runs::resumable_session(conn, agent, "execute")?.is_none());
        let rules = crate::rules::list(std::path::Path::new(&project.folder_path))?;
        let mut unread = Vec::new();
        let mut page_before = Some(user_message_id);
        loop {
            let page = messages::list(conn, project_id, page_before, 200)?;
            let Some(first) = page.first() else { break; };
            page_before = Some(first.id);
            let reached = first.id <= agent.last_seen_message_id.unwrap_or(0);
            unread.extend(page.into_iter().filter(|message| message.id > agent.last_seen_message_id.unwrap_or(0)
                && message.sender_agent_id.as_deref() != Some(&agent.id)));
            if reached { break; }
        }
        unread.sort_by_key(|message| message.id);
        let recent = if recovery { messages::list(conn, project_id, Some(user_message_id), 20)? } else { vec![] };
        let prompt = build_turn_prompt(&TurnPromptSnapshot { request, agent_id: &agent.id, project: &project,
            rules: &rules, agent, roster: &roster, unread: &unread, recent: &recent, recovery });
        let mut prepared_agent = agent.clone();
        prepared_agent.model = crate::cli_config::effective_model(conn, &agent.cli, agent.model.as_deref())?;
        Ok(PreparedTurn { agent: prepared_agent, prompt, request_text: request.into(), snapshot_message_id: user_message_id, unread, recovery })
    }
    pub fn request(prepared: &PreparedTurn, cwd: std::path::PathBuf) -> TurnRequest {
        TurnRequest { prompt: prepared.prompt.clone(), session_id: if prepared.recovery { None } else { prepared.agent.session_id.clone() }, cwd,
            mode: RunMode::Execute, model: prepared.agent.model.clone() }
    }
    pub fn on_event(conn: &Connection, prepared: &PreparedTurn, run_id: &str, event: &AgentEvent) -> AppResult<()> {
        match event {
            AgentEvent::SessionStarted { session_id } => {
                agents::set_session(conn, &prepared.agent.id, Some(session_id))?;
                conn.execute("UPDATE runs SET session_id=?1 WHERE id=?2 AND status='running'", rusqlite::params![session_id, run_id])?;
            }
            AgentEvent::ModelReported { model } => crate::store::runs::set_reported_model(conn, run_id, model)?,
            _ => {}
        }
        Ok(())
    }
    pub fn finish_success(conn: &Connection, prepared: &PreparedTurn, placeholder: i64, final_text: &str) -> AppResult<()> {
        let tx = conn.unchecked_transaction()?;
        messages::update_content(&tx, placeholder, final_text)?;
        agents::finish_context(&tx, &prepared.agent.id, &prepared.agent.persona, prepared.snapshot_message_id)?;
        tx.commit()?;
        Ok(())
    }
    pub fn finish_failure(conn: &Connection, prepared: &PreparedTurn, placeholder: i64, partial: &str, error: &str) -> AppResult<Message> {
        let tx = conn.unchecked_transaction()?;
        messages::update_content(&tx, placeholder, partial)?;
        let system = messages::insert(&tx, &prepared.agent.project_id, "system", None, "system", error)?;
        tx.commit()?;
        Ok(system)
    }
    pub fn stop_project(sender: &tokio::sync::watch::Sender<bool>) { let _ = sender.send(true); }
}

/// 只有明确的启动会话错误，且没有正文/工具活动时允许重建一次。
pub fn should_rebuild_session(outcome: &crate::runner::RunOutcome, had_session: bool, retried: bool) -> bool {
    if !had_session || retried || outcome.status != "failed" || outcome.tool_seen || !outcome.partial_text.is_empty() || !outcome.cleanup_complete {
        return false;
    }
    mentions_missing_session(outcome.error.as_deref().unwrap_or_default())
}

/// 文本里是否有“续接的会话不存在/已损坏”的特征。runner 也用它检查 stderr 尾部。
pub fn mentions_missing_session(text: &str) -> bool {
    let error = text.to_lowercase();
    ["session not found", "session does not exist", "session doesn't exist", "no conversation found", "invalid session", "session is corrupt", "thread not found", "thread does not exist", "no rollout found for thread", "conversation not found", "conversation does not exist", "omit --session-id to start a new session", "会话不存在", "会话损坏", "找不到会话"]
        .iter().any(|pattern| error.contains(pattern))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{agents, open_in_memory, projects};
    use crate::store::agents::NewAgent;

    const PERSONA: &str = "## 定位\n\n本人定位\n\n## 职责\n\n本人职责\n\n## 边界\n\n本人边界\n\n## 工作风格\n\n本人工作风格\n";
    fn setup() -> (rusqlite::Connection, String, Vec<Agent>) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "项目", "描述", "/tmp/项目").unwrap();
        let leader = agents::create(&conn, &project.id, &NewAgent { name: "王".into(), role: "组长".into(), persona: PERSONA.into(), cli: "claude".into(), model: None, is_leader: true, reasoning_effort: None }).unwrap();
        let member = agents::create(&conn, &project.id, &NewAgent { name: "王五".into(), role: "开发".into(), persona: PERSONA.replace("本人", "其他成员"), cli: "codex".into(), model: None, is_leader: false, reasoning_effort: None }).unwrap();
        let mut qa = NewAgent { name: "Dev".into(), role: "开发".into(), persona: PERSONA.into(), cli: "claude".into(), model: None, is_leader: false, reasoning_effort: None };
        let dev = agents::create(&conn, &project.id, &qa).unwrap();
        qa.name = "Dev Ops".into();
        let ops = agents::create(&conn, &project.id, &qa).unwrap();
        (conn, project.id, vec![leader, member, dev, ops])
    }
    #[test]
    fn routes_default_to_leader_and_mentions_to_longest_normalized_name() {
        let (_conn, _project, roster) = setup();
        let (agent, prompt) = resolve_recipient("  默认任务  ", &roster).unwrap();
        assert!(agent.is_leader); assert_eq!(prompt, "默认任务");
        let (agent, prompt) = resolve_recipient(" @dev ops  修复构建 ", &roster).unwrap();
        assert_eq!(agent.name, "Dev Ops"); assert_eq!(prompt, "修复构建");
        let (agent, _) = resolve_recipient("@王五 写测试", &roster).unwrap();
        assert_eq!(agent.name, "王五");
    }
    #[test]
    fn rejects_empty_unknown_mention_and_partial_name_boundary() {
        let (_conn, _project, roster) = setup();
        for text in [" ", "@未知 做事", "@王五项目 做事", "@Dev"] {
            assert!(resolve_recipient(text, &roster).is_err(), "{text}");
        }
    }
    #[test]
    fn rejects_legacy_casefold_collision_instead_of_ambiguous_routing() {
        let (conn, project_id, roster) = setup();
        conn.execute("UPDATE agents SET name=' dev ' WHERE project_id=?1 AND name='Dev Ops'", [&project_id]).unwrap();
        let roster = agents::list(&conn, &project_id).unwrap();
        assert!(resolve_recipient("@DEV 修复", &roster).is_err());
        assert_eq!(roster.len(), 4);
    }
    #[test]
    fn send_preparation_snapshots_all_unread_messages_but_excludes_self_and_current_message() {
        let (conn, project_id, roster) = setup();
        let agent = &roster[0];
        let older = crate::store::messages::insert(&conn, &project_id, "assistant", Some(&agent.id), "chat", "本人旧消息").unwrap();
        let first = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "未读约束" ).unwrap();
        let mut last = first.clone();
        for index in 0..205 { last = crate::store::messages::insert(&conn, &project_id, "assistant", Some(&roster[1].id), "chat", &format!("其他成员{index}")).unwrap(); }
        let current = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "当前要求").unwrap();
        let prepared = ChatService::send(&conn, &project_id, current.id, false).unwrap();
        assert!(prepared.prompt.contains("未读约束"));
        assert!(prepared.prompt.contains("其他成员204"));
        assert!(!prepared.prompt.contains("本人旧消息"));
        assert_eq!(prepared.prompt.matches("当前要求").count(), 1);
        assert_eq!(prepared.snapshot_message_id, current.id);
        let request = ChatService::request(&prepared, "/tmp/项目".into());
        assert_eq!(request.mode, RunMode::Execute);
        assert!(prepared.unread.iter().all(|message| message.id > older.id && message.id < current.id));
        assert!(prepared.unread.iter().any(|message| message.id == first.id));
        assert!(prepared.unread.iter().any(|message| message.id == last.id));
    }
    #[test]
    fn recovered_session_uses_latest_context_and_starts_without_old_session_id() {
        let (conn, project_id, roster) = setup();
        agents::set_session(&conn, &roster[0].id, Some("broken-session")).unwrap();
        let user = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "恢复任务").unwrap();
        let prepared = ChatService::send(&conn, &project_id, user.id, true).unwrap();
        assert!(prepared.prompt.contains("# 项目花名册"));
        assert_eq!(ChatService::request(&prepared, "/tmp/项目".into()).session_id, None);
    }
    #[test]
    fn grok_direct_execute_starts_fresh_after_discuss_but_resumes_execute() {
        let (conn, project_id, roster) = setup();
        let agent_id = &roster[0].id;
        conn.execute("UPDATE agents SET cli='grok',session_id='讨论会话' WHERE id=?1", [agent_id]).unwrap();
        let first = crate::store::runs::insert_running(&conn, Some(agent_id), "discuss", "{}", Some("讨论会话")).unwrap();
        crate::store::runs::finish(&conn, &first.id, "ok", Some(0), None, None).unwrap();
        let user = messages::insert(&conn, &project_id, "user", None, "chat", "请处理任务").unwrap();
        let switched = ChatService::send(&conn, &project_id, user.id, false).unwrap();
        assert!(switched.prompt.contains("# 项目花名册"));
        assert!(ChatService::request(&switched, "/tmp/项目".into()).session_id.is_none());
        agents::set_session(&conn, agent_id, Some("执行会话")).unwrap();
        let second = crate::store::runs::insert_running(&conn, Some(agent_id), "execute", "{}", Some("执行会话")).unwrap();
        crate::store::runs::finish(&conn, &second.id, "ok", Some(0), None, None).unwrap();
        let resumed = ChatService::send(&conn, &project_id, user.id, false).unwrap();
        assert_eq!(ChatService::request(&resumed, "/tmp/项目".into()).session_id.as_deref(), Some("执行会话"));
    }
    #[test]
    fn on_event_records_the_reported_model_only_on_a_running_run() {
        let (conn, project_id, roster) = setup();
        let user = messages::insert(&conn, &project_id, "user", None, "chat", "记录模型").unwrap();
        let prepared = ChatService::send(&conn, &project_id, user.id, false).unwrap();
        let run = crate::store::runs::insert_running(&conn, Some(&roster[0].id), "execute", "{}", None).unwrap();
        let reported = |conn: &rusqlite::Connection| -> Option<String> {
            conn.query_row("SELECT reported_model FROM runs WHERE id=?1", [&run.id], |row| row.get(0)).unwrap()
        };
        ChatService::on_event(&conn, &prepared, &run.id, &AgentEvent::ModelReported { model: "claude-sonnet-5-5".into() }).unwrap();
        assert_eq!(reported(&conn).as_deref(), Some("claude-sonnet-5-5"));
        crate::store::runs::finish(&conn, &run.id, "ok", Some(0), None, None).unwrap();
        ChatService::on_event(&conn, &prepared, &run.id, &AgentEvent::ModelReported { model: "迟到".into() }).unwrap();
        assert_eq!(reported(&conn).as_deref(), Some("claude-sonnet-5-5"));
    }
    #[test]
    fn send_rejects_project_marked_as_running() {
        let (conn, project_id, _) = setup();
        let user = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "再次发送").unwrap();
        conn.execute("UPDATE projects SET status='executing' WHERE id=?1", [&project_id]).unwrap();
        assert!(ChatService::send(&conn, &project_id, user.id, false).is_err());
    }
    #[test]
    fn failure_keeps_cursor_and_success_advances_to_snapshot() {
        let (conn, project_id, _) = setup();
        let current = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "处理任务").unwrap();
        let prepared = ChatService::send(&conn, &project_id, current.id, false).unwrap();
        let before = agents::get(&conn, &prepared.agent.id).unwrap().last_seen_message_id;
        let placeholder = crate::store::messages::insert(&conn, &project_id, "assistant", Some(&prepared.agent.id), "chat", "").unwrap();
        ChatService::finish_failure(&conn, &prepared, placeholder.id, "部分回答", "执行失败").unwrap();
        assert_eq!(agents::get(&conn, &prepared.agent.id).unwrap().last_seen_message_id, before);
        ChatService::finish_success(&conn, &prepared, placeholder.id, "最终正文").unwrap();
        assert_eq!(crate::store::messages::get(&conn, placeholder.id).unwrap().content, "最终正文");
        assert_eq!(agents::get(&conn, &prepared.agent.id).unwrap().last_seen_message_id, Some(prepared.snapshot_message_id));
        assert!(!agents::get(&conn, &prepared.agent.id).unwrap().persona_dirty);
    }
    #[test]
    fn session_rebuild_is_limited_to_explicit_startup_failures() {
        let base = crate::runner::RunOutcome { status: "failed".into(), session_id: Some("old".into()),
            final_text: String::new(), partial_text: String::new(), usage: None, exit_code: Some(1),
            error: Some("thread not found".into()), tool_seen: false, cleanup_complete: true, denial: None };
        assert!(should_rebuild_session(&base, true, false));
        assert!(!should_rebuild_session(&base, true, true));
        assert!(!should_rebuild_session(&base, false, false));
        let mut partial = base.clone(); partial.partial_text = "已有输出".into();
        assert!(!should_rebuild_session(&partial, true, false));
        let mut tool = base.clone(); tool.tool_seen = true;
        assert!(!should_rebuild_session(&tool, true, false));
        let mut timeout = base.clone(); timeout.status = "timeout".into();
        assert!(!should_rebuild_session(&timeout, true, false));
        let mut unrelated = base; unrelated.error = Some("model not found".into());
        assert!(!should_rebuild_session(&unrelated, true, false));
    }

    #[test]
    fn codex_missing_rollout_error_allows_one_rebuild() {
        let outcome = crate::runner::RunOutcome { status: "failed".into(), session_id: Some("01a1c3f2-85eb-71d2-a133-6a6d4627d764".into()),
            final_text: String::new(), partial_text: String::new(), usage: None, exit_code: Some(1),
            error: Some("Error: thread/resume: thread/resume failed: no rollout found for thread id 01a1c3f2-85eb-71d2-a133-6a6d4627d764 (code -32600)".into()),
            tool_seen: false, cleanup_complete: true, denial: None };
        assert!(should_rebuild_session(&outcome, true, false));
        assert!(!should_rebuild_session(&outcome, true, true));
    }

    #[test]
    fn dsh_unknown_session_error_allows_one_rebuild() {
        let outcome = crate::runner::RunOutcome { status: "failed".into(), session_id: Some("session-old".into()),
            final_text: String::new(), partial_text: String::new(), usage: None, exit_code: Some(1),
            error: Some("session \"session-old\" does not exist; omit --session-id to start a new Session".into()),
            tool_seen: false, cleanup_complete: true, denial: None };
        assert!(should_rebuild_session(&outcome, true, false));
        assert!(!should_rebuild_session(&outcome, true, true));
    }

    #[test]
    fn prepared_turn_uses_cli_default_model_only_when_member_model_is_blank() {
        let (conn, project_id, roster) = setup();
        let mut config = crate::cli_config::default_config("claude");
        config.models = vec![crate::cli_config::CliModel { id: "opus".into(), label: "opus".into(), custom: false }];
        config.default_model = Some("opus".into());
        crate::cli_config::save(&conn, "claude", &config).unwrap();
        let current = crate::store::messages::insert(&conn, &project_id, "user", None, "chat", "做事").unwrap();
        let prepared = ChatService::send(&conn, &project_id, current.id, false).unwrap();
        assert_eq!(prepared.agent.model.as_deref(), Some("opus"));
        conn.execute("UPDATE agents SET model='sonnet' WHERE id=?1", [&roster[0].id]).unwrap();
        let again = ChatService::send(&conn, &project_id, current.id, false).unwrap();
        assert_eq!(again.agent.model.as_deref(), Some("sonnet"));
    }
}
