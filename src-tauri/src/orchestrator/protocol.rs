//! 组长回合最终文本末尾的 `imates` 指令块。
//! 只认唯一、闭合、位于末尾的围栏；围栏前的文字是发言，不参与调度。

use serde::Deserialize;
use serde_json::Value;

use crate::error::{AppError, AppResult};
use crate::store::agents::Agent;

const TITLE_MAX: usize = 200;
const TOPIC_MAX: usize = 4_000;
const LONG_MAX: usize = 16_000;
const BLOCK_MAX_BYTES: usize = 128 * 1024;

/// 指令所处阶段。与 `instructions.phase` 的取值对应。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Triage,
    Discuss,
    Decide,
    Execute,
    Review,
    Intervention,
    Direct,
}

/// 返工校验所需的目标任务。调用方只传入本条指令当前批次中的任务。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskLimit {
    pub id: String,
    pub status: String,
    pub rework_count: u32,
}

/// 调用方只传本条指令的任务，供派工前验证替代关系。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacementLimit {
    pub id: String,
    pub status: String,
    pub superseded: bool,
}

/// 阶段、返工与任务替代护栏。讨论轮数按整条指令累计。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LimitContext {
    pub discuss_rounds: u32,
    pub task: Option<TaskLimit>,
    pub replacement_tasks: Vec<ReplacementLimit>,
}

/// 干预研判才允许的处置。其他阶段出现该字段即拒绝。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Disposition {
    Continue,
    End,
    Replace,
}

/// 已绑定成员 ID 的动作。`to` 是 Agent ID，不再是显示名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActionKind {
    Reply { message: String },
    Discuss { to: Vec<String>, topic: String },
    Assign { decision: String, tasks: Vec<AssignedTask> },
    Rework { task_id: String, feedback: String },
}

/// 派工清单中的一项。`to` 是绑定后的 Agent ID。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssignedTask {
    pub to: String,
    pub title: String,
    pub detail: String,
    pub replaces_task_id: Option<String>,
}

/// 解析后的动作，以及干预阶段的处置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolAction {
    pub kind: ActionKind,
    pub disposition: Option<Disposition>,
}

/// 围栏前的发言与围栏内的动作分开保存。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedLeaderTurn {
    pub speech: String,
    pub action: ProtocolAction,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct RawTask {
    to: String,
    title: String,
    detail: String,
    #[serde(default)]
    replaces_task_id: Option<String>,
}

/// 解析组长最终文本。失败时错误信息点明字段或成员。
pub fn parse_final(
    final_text: &str,
    phase: Phase,
    agents: &[Agent],
    limits: &LimitContext,
) -> AppResult<ParsedLeaderTurn> {
    let (speech, json) = split_trailing_fence(final_text)?;
    if json.len() > BLOCK_MAX_BYTES {
        return Err(invalid("指令块超过 128 KiB"));
    }
    let action = parse_action(&json, agents)?;
    validate_phase(phase, &action, limits)?;
    validate_replacements(&action, limits)?;
    Ok(ParsedLeaderTurn { speech, action })
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Invalid(message.into())
}

/// 末尾空白可以忽略。围栏语言必须正好是 `imates`，且全文只能有一个这样的围栏。
fn split_trailing_fence(text: &str) -> AppResult<(String, String)> {
    let trimmed = text.trim_end();
    if trimmed.is_empty() {
        return Err(invalid("缺少末尾 imates 指令块"));
    }

    let mut open_at = None;
    let mut open_count = 0;
    let mut offset = 0;
    for line in trimmed.split_inclusive('\n') {
        let content = line.trim_end_matches(['\n', '\r']).trim();
        if content == "```imates" {
            open_count += 1;
            if open_at.is_none() {
                open_at = Some(offset);
            }
        }
        offset += line.len();
    }
    if open_count != 1 {
        return Err(invalid(if open_count == 0 {
            "缺少末尾 imates 指令块"
        } else {
            "只能有一个 imates 指令块"
        }));
    }
    let open_at = open_at.expect("已确认存在唯一围栏");

    let last_line_start = trimmed.rfind('\n').map(|index| index + 1).unwrap_or(0);
    let last_line = trimmed[last_line_start..].trim_end_matches('\r').trim();
    if last_line != "```" {
        return Err(invalid("imates 指令块未闭合或未位于最终文本末尾"));
    }
    let Some(open_newline) = trimmed[open_at..].find('\n') else {
        return Err(invalid("imates 指令块未闭合"));
    };
    let open_line_end = open_at + open_newline;
    if open_line_end >= last_line_start {
        return Err(invalid("imates 指令块未闭合"));
    }

    let json = trimmed[open_line_end + 1..last_line_start].trim();
    if json.is_empty() {
        return Err(invalid("指令块为空"));
    }
    let speech = trimmed[..open_at].trim_end().to_string();
    Ok((speech, json.to_string()))
}

fn parse_action(json: &str, agents: &[Agent]) -> AppResult<ProtocolAction> {
    let value: Value = serde_json::from_str(json).map_err(|error| invalid(format!("指令 JSON 无法解析：{error}")))?;
    let obj = value.as_object().ok_or_else(|| invalid("指令必须是 JSON 对象"))?;
    let action = match obj.get("action") {
        Some(Value::String(action)) => action.as_str(),
        Some(_) => return Err(invalid("action 类型不符")),
        None => return Err(invalid("缺少 action")),
    };
    let disposition = parse_disposition(obj.get("disposition"))?;
    let kind = match action {
        "reply" => {
            expect_keys(obj, &["action", "message", "disposition"])?;
            ActionKind::Reply { message: require_text(obj.get("message"), "message", LONG_MAX)? }
        }
        "discuss" => {
            expect_keys(obj, &["action", "to", "topic", "disposition"])?;
            ActionKind::Discuss {
                to: bind_members(obj.get("to"), agents)?,
                topic: require_text(obj.get("topic"), "topic", TOPIC_MAX)?,
            }
        }
        "assign" => {
            expect_keys(obj, &["action", "decision", "tasks", "disposition"])?;
            ActionKind::Assign {
                decision: require_text(obj.get("decision"), "decision", LONG_MAX)?,
                tasks: parse_tasks(obj.get("tasks"), agents)?,
            }
        }
        "rework" => {
            expect_keys(obj, &["action", "task_id", "feedback", "disposition"])?;
            ActionKind::Rework {
                task_id: require_text(obj.get("task_id"), "task_id", LONG_MAX)?,
                feedback: require_text(obj.get("feedback"), "feedback", LONG_MAX)?,
            }
        }
        other => return Err(invalid(format!("未知 action：{other}"))),
    };
    Ok(ProtocolAction { kind, disposition })
}

fn expect_keys(obj: &serde_json::Map<String, Value>, allowed: &[&str]) -> AppResult<()> {
    for key in obj.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(invalid(format!("未知字段 {key}")));
        }
    }
    Ok(())
}

fn parse_disposition(value: Option<&Value>) -> AppResult<Option<Disposition>> {
    let Some(value) = value else {
        return Ok(None);
    };
    let text = value.as_str().ok_or_else(|| invalid("disposition 类型不符"))?;
    match text {
        "continue" => Ok(Some(Disposition::Continue)),
        "end" => Ok(Some(Disposition::End)),
        "replace" => Ok(Some(Disposition::Replace)),
        other => Err(invalid(format!("disposition「{other}」无效"))),
    }
}

fn require_text(value: Option<&Value>, field: &str, max_chars: usize) -> AppResult<String> {
    let Some(value) = value else {
        return Err(invalid(format!("缺少 {field}")));
    };
    let text = value.as_str().ok_or_else(|| invalid(format!("{field} 类型不符")))?;
    let text = text.trim();
    if text.is_empty() {
        return Err(invalid(format!("{field} 不能为空")));
    }
    if text.chars().count() > max_chars {
        return Err(invalid(format!("{field} 超过长度限制")));
    }
    Ok(text.to_string())
}

fn bind_members(value: Option<&Value>, agents: &[Agent]) -> AppResult<Vec<String>> {
    let Some(value) = value else {
        return Err(invalid("缺少 to"));
    };
    let list = value.as_array().ok_or_else(|| invalid("to 类型不符"))?;
    if list.is_empty() || list.len() > 8 {
        return Err(invalid("to 的成员数必须在 1 到 8 之间"));
    }
    let mut ids = Vec::with_capacity(list.len());
    for item in list {
        let name = item.as_str().ok_or_else(|| invalid("to 类型不符"))?;
        let id = bind_member(name, agents)?;
        if ids.iter().any(|existing| existing == &id) {
            return Err(invalid(format!("成员「{name}」重复")));
        }
        ids.push(id);
    }
    Ok(ids)
}

fn bind_member(raw: &str, agents: &[Agent]) -> AppResult<String> {
    let key = member_key(raw);
    if key.is_empty() {
        return Err(invalid(format!("成员「{raw}」无效")));
    }
    let mut found = None;
    for agent in agents {
        if agent.name.trim().to_lowercase() == key {
            if found.is_some() {
                return Err(invalid(format!("成员「{raw}」不唯一")));
            }
            found = Some(agent.id.clone());
        }
    }
    found.ok_or_else(|| invalid(format!("成员「{raw}」无效")))
}

/// 去掉开头一个 `@`，再按与创建判重相同的规则归一化。
fn member_key(raw: &str) -> String {
    let trimmed = raw.trim();
    let stripped = trimmed.strip_prefix('@').unwrap_or(trimmed);
    stripped.trim().to_lowercase()
}

fn parse_tasks(value: Option<&Value>, agents: &[Agent]) -> AppResult<Vec<AssignedTask>> {
    let Some(value) = value else {
        return Err(invalid("缺少 tasks"));
    };
    let list = value.as_array().ok_or_else(|| invalid("tasks 类型不符"))?;
    if list.is_empty() || list.len() > 8 {
        return Err(invalid("tasks 的数量必须在 1 到 8 之间"));
    }
    let mut tasks = Vec::with_capacity(list.len());
    for item in list {
        let raw: RawTask = serde_json::from_value(item.clone()).map_err(|error| invalid(format!("任务字段无效：{error}")))?;
        let replaces_task_id = match raw.replaces_task_id {
            Some(value) => {
                let trimmed = value.trim();
                if trimmed.is_empty() {
                    return Err(invalid("replaces_task_id 不能为空"));
                }
                Some(trimmed.to_string())
            }
            None => None,
        };
        tasks.push(AssignedTask {
            to: bind_member(&raw.to, agents)?,
            title: require_text(Some(&Value::String(raw.title)), "title", TITLE_MAX)?,
            detail: require_text(Some(&Value::String(raw.detail)), "detail", LONG_MAX)?,
            replaces_task_id,
        });
    }
    Ok(tasks)
}

fn validate_phase(phase: Phase, action: &ProtocolAction, limits: &LimitContext) -> AppResult<()> {
    if phase != Phase::Intervention && action.disposition.is_some() {
        return Err(invalid("当前阶段不能使用 disposition"));
    }
    match phase {
        Phase::Triage => match &action.kind {
            ActionKind::Reply { .. } | ActionKind::Discuss { .. } | ActionKind::Assign { .. } => Ok(()),
            ActionKind::Rework { .. } => Err(invalid("研判阶段不能使用 rework")),
        },
        Phase::Decide => match &action.kind {
            ActionKind::Reply { .. } | ActionKind::Assign { .. } => Ok(()),
            ActionKind::Discuss { .. } if limits.discuss_rounds < 3 => Ok(()),
            ActionKind::Discuss { .. } => Err(invalid("讨论已达 3 轮，裁定必须使用 assign 或 reply")),
            ActionKind::Rework { .. } => Err(invalid("裁定阶段不能使用 rework")),
        },
        Phase::Review => match &action.kind {
            ActionKind::Reply { .. } | ActionKind::Assign { .. } => Ok(()),
            ActionKind::Rework { task_id, .. } => validate_rework(task_id, limits),
            ActionKind::Discuss { .. } => Err(invalid("验收阶段不能使用 discuss")),
        },
        Phase::Intervention => match (&action.kind, action.disposition) {
            (ActionKind::Reply { .. }, Some(Disposition::Continue | Disposition::End)) => Ok(()),
            (ActionKind::Assign { .. }, Some(Disposition::Replace)) => Ok(()),
            _ => Err(invalid("干预阶段只接受 reply+continue、reply+end 或 assign+replace")),
        },
        Phase::Discuss | Phase::Execute | Phase::Direct => Err(invalid("当前阶段不接受组长指令")),
    }
}

fn validate_rework(task_id: &str, limits: &LimitContext) -> AppResult<()> {
    let Some(task) = limits.task.as_ref() else {
        return Err(invalid("rework 缺少可返工的任务"));
    };
    if task.id != task_id {
        return Err(invalid(format!("task_id「{task_id}」不属于当前任务")));
    }
    if task.status != "done" && task.status != "failed" {
        return Err(invalid(format!("任务状态 {} 不能返工", task.status)));
    }
    if task.rework_count >= 2 {
        return Err(invalid("任务已达返工上限"));
    }
    Ok(())
}

fn validate_replacements(action: &ProtocolAction, limits: &LimitContext) -> AppResult<()> {
    let ActionKind::Assign { tasks, .. } = &action.kind else { return Ok(()) };
    let mut seen = std::collections::HashSet::new();
    for id in tasks.iter().filter_map(|task| task.replaces_task_id.as_deref()) {
        let Some(old) = limits.replacement_tasks.iter().find(|task| task.id == id) else {
            return Err(invalid(format!("replaces_task_id「{id}」不属于本条指令或不存在；跨指令重试请新建任务，不要填写 replaces_task_id")));
        };
        if old.superseded {
            return Err(invalid(format!("replaces_task_id「{id}」已经被替代")));
        }
        if !matches!(old.status.as_str(), "failed" | "cancelled") {
            return Err(invalid(format!("replaces_task_id「{id}」的状态 {} 不能替代，只允许 failed/cancelled", old.status)));
        }
        if !seen.insert(id) {
            return Err(invalid(format!("replaces_task_id「{id}」不能在同一批派工中重复替代")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::agents::Agent;

    fn agent(id: &str, name: &str) -> Agent {
        Agent {
            id: id.into(), project_id: "p".into(), name: name.into(), role: "开发".into(),
            persona: "x".into(), persona_dirty: false, cli: "claude".into(), model: None,
            is_leader: id == "a", session_id: None, last_seen_message_id: None, seat_index: 0, created_at: 1,
        }
    }
    fn limits(rounds: u32, task: Option<TaskLimit>) -> LimitContext {
        LimitContext { discuss_rounds: rounds, task, replacement_tasks: Vec::new() }
    }
    #[test]
    fn assign_rejects_replacement_outside_current_instruction() {
        let agents = vec![agent("a", "前端")];
        let action = serde_json::json!({"action":"assign","decision":"重试","tasks":[{
            "to":"前端","title":"重试","detail":"重新执行","replaces_task_id":"旧指令或不存在的任务"
        }]}).to_string();
        let fence = char::from(96).to_string().repeat(3);
        let text = [format!("{fence}imates"), action, fence].join("\n");
        let error = parse_final(&text, Phase::Triage, &agents, &limits(0, None)).unwrap_err().to_string();
        assert!(error.contains("replaces_task_id") && error.contains("本条指令"), "{error}");
    }
    #[test]
    fn replacement_requires_unreplaced_failed_or_cancelled_task_and_unique_target() {
        let agents = vec![agent("a", "前端")];
        let fence = char::from(96).to_string().repeat(3);
        for (status, superseded, count, expected) in [
            ("failed", false, 1, None), ("cancelled", false, 1, None),
            ("failed", true, 1, Some("已经被替代")),
            ("pending", false, 1, Some("不能替代")), ("running", false, 1, Some("不能替代")),
            ("done", false, 1, Some("不能替代")), ("failed", false, 2, Some("重复替代")),
        ] {
            let task = serde_json::json!({"to":"前端","title":"新任务","detail":"完成","replaces_task_id":"old"});
            let action = serde_json::json!({"action":"assign","decision":"重试","tasks":vec![task; count]}).to_string();
            let text = [format!("{fence}imates"), action, fence.clone()].join("\n");
            let mut context = limits(0, None);
            context.replacement_tasks.push(ReplacementLimit { id: "old".into(), status: status.into(), superseded });
            let result = parse_final(&text, Phase::Review, &agents, &context);
            match expected {
                None => assert!(result.is_ok(), "{result:?}"),
                Some(reason) => assert!(result.unwrap_err().to_string().contains(reason)),
            }
        }
    }
    fn task(id: &str, status: &str, rework: u32) -> TaskLimit {
        TaskLimit { id: id.into(), status: status.into(), rework_count: rework }
    }

    #[test]
    fn trailing_discuss_binds_members_and_keeps_the_speech() {
        let agents = vec![agent("a", " 前端 "), agent("b", "测试")];
        let text = "先说结论。\n```imates\n{\"action\":\"discuss\",\"to\":[\"@前端\",\"测试\"],\"topic\":\"登录校验\"}\n```";
        let parsed = parse_final(text, Phase::Triage, &agents, &limits(0, None)).unwrap();
        assert_eq!(parsed.speech, "先说结论。");
        assert!(!parsed.speech.contains("action"));
        match parsed.action.kind {
            ActionKind::Discuss { to, topic } => {
                assert_eq!(to, vec!["a".to_string(), "b".to_string()]);
                assert_eq!(topic, "登录校验");
            }
            other => panic!("意外动作：{other:?}"),
        }
        assert_eq!(parsed.action.disposition, None);
    }

    #[test]
    fn middle_unclosed_and_duplicate_fences_are_rejected() {
        let agents = vec![agent("a", "前端")];
        let limits = limits(0, None);
        let middle = "开头\n```imates\n{\"action\":\"reply\",\"message\":\"好\"}\n```\n结尾";
        assert!(parse_final(middle, Phase::Triage, &agents, &limits).is_err());
        let open = "```imates\n{\"action\":\"reply\",\"message\":\"好\"}";
        assert!(parse_final(open, Phase::Triage, &agents, &limits).is_err());
        let two = "```imates\n{\"action\":\"reply\",\"message\":\"一\"}\n```\n```imates\n{\"action\":\"reply\",\"message\":\"二\"}\n```";
        assert!(parse_final(two, Phase::Triage, &agents, &limits).is_err());
    }

    #[test]
    fn unknown_empty_and_overlong_fields_are_rejected() {
        let agents = vec![agent("a", "前端")];
        let limits = limits(0, None);
        let unknown = "```imates\n{\"action\":\"reply\",\"message\":\"好\",\"extra\":1}\n```";
        assert!(parse_final(unknown, Phase::Triage, &agents, &limits).is_err());
        let empty = "```imates\n{\"action\":\"reply\",\"message\":\"  \"}\n```";
        assert!(parse_final(empty, Phase::Triage, &agents, &limits).unwrap_err().to_string().contains("message"));
        let title = "超".repeat(201);
        let assign = format!("```imates\n{{\"action\":\"assign\",\"decision\":\"做\",\"tasks\":[{{\"to\":\"前端\",\"title\":\"{title}\",\"detail\":\"细节\"}}]}}\n```");
        assert!(parse_final(&assign, Phase::Triage, &agents, &limits).unwrap_err().to_string().contains("title"));
    }

    #[test]
    fn duplicate_and_unknown_members_are_rejected() {
        let agents = vec![agent("a", "前端"), agent("b", "测试")];
        let limits = limits(0, None);
        let dup = "```imates\n{\"action\":\"discuss\",\"to\":[\"前端\",\"前端\"],\"topic\":\"题\"}\n```";
        assert!(parse_final(dup, Phase::Triage, &agents, &limits).is_err());
        let missing = "```imates\n{\"action\":\"discuss\",\"to\":[\"产品\"],\"topic\":\"题\"}\n```";
        assert!(parse_final(missing, Phase::Triage, &agents, &limits).unwrap_err().to_string().contains("产品"));
    }

    #[test]
    fn triage_rejects_rework_and_review_checks_task_state() {
        let agents = vec![agent("a", "前端")];
        let rework = "```imates\n{\"action\":\"rework\",\"task_id\":\"t1\",\"feedback\":\"重做\"}\n```";
        assert!(parse_final(rework, Phase::Triage, &agents, &limits(0, Some(task("t1", "failed", 0)))).is_err());
        assert!(parse_final(rework, Phase::Review, &agents, &limits(0, Some(task("t1", "done", 1)))).is_ok());
        assert!(parse_final(rework, Phase::Review, &agents, &limits(0, Some(task("t1", "failed", 0)))).is_ok());
        assert!(parse_final(rework, Phase::Review, &agents, &limits(0, Some(task("t1", "cancelled", 0)))).is_err());
        assert!(parse_final(rework, Phase::Review, &agents, &limits(0, Some(task("other", "failed", 0)))).is_err());
        assert!(parse_final(rework, Phase::Review, &agents, &limits(0, Some(task("t1", "failed", 2)))).is_err());
    }

    #[test]
    fn intervention_accepts_only_continue_end_and_replace() {
        let agents = vec![agent("a", "前端")];
        let limits = limits(1, None);
        let ok_continue = "```imates\n{\"action\":\"reply\",\"message\":\"继续\",\"disposition\":\"continue\"}\n```";
        let parsed = parse_final(ok_continue, Phase::Intervention, &agents, &limits).unwrap();
        assert_eq!(parsed.action.disposition, Some(Disposition::Continue));
        let ok_end = "```imates\n{\"action\":\"reply\",\"message\":\"结束\",\"disposition\":\"end\"}\n```";
        assert_eq!(parse_final(ok_end, Phase::Intervention, &agents, &limits).unwrap().action.disposition, Some(Disposition::End));
        let ok_replace = "```imates\n{\"action\":\"assign\",\"decision\":\"换一批\",\"disposition\":\"replace\",\"tasks\":[{\"to\":\"前端\",\"title\":\"新任务\",\"detail\":\"重做这一块\"}]}\n```";
        assert_eq!(parse_final(ok_replace, Phase::Intervention, &agents, &limits).unwrap().action.disposition, Some(Disposition::Replace));
        let bad = "```imates\n{\"action\":\"discuss\",\"to\":[\"前端\"],\"topic\":\"题\",\"disposition\":\"continue\"}\n```";
        assert!(parse_final(bad, Phase::Intervention, &agents, &limits).is_err());
        let outside = "```imates\n{\"action\":\"reply\",\"message\":\"好\",\"disposition\":\"continue\"}\n```";
        assert!(parse_final(outside, Phase::Triage, &agents, &limits).is_err());
    }
}
