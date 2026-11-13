use crate::rules::RuleDoc;
use crate::store::agents::Agent;
use crate::store::messages::Message;
use crate::store::projects::Project;

pub struct TurnPromptSnapshot<'a> {
    pub request: &'a str,
    pub agent_id: &'a str,
    pub project: &'a Project,
    pub rules: &'a [RuleDoc],
    pub agent: &'a Agent,
    pub roster: &'a [Agent],
    pub unread: &'a [Message],
    pub recent: &'a [Message],
    pub recovery: bool,
}

/// 构造新会话开场，花名册仅暴露定位与边界摘要。
pub fn build_opening(project: &Project, rules: &[RuleDoc], agent: &Agent, roster: &[Agent]) -> String {
    use crate::persona::section_body;
    let mut text = format!("# 项目\n{}\n\n{}\n\n# 当前成员\n名称：{}\n角色：{}\n\n# 本人完整人设\n{}\n\n# 项目规章\n",
        project.name, project.description, agent.name, agent.role, agent.persona.trim());
    if rules.is_empty() {
        text.push_str("当前尚未配置项目规章。\n");
    } else {
        for rule in rules {
            text.push_str(&format!("\n## {}\n{}\n", rule.filename, rule.content.trim()));
        }
    }
    text.push_str("\n# 项目花名册（仅供识别成员）\n");
    for member in roster.iter().filter(|member| member.id != agent.id) {
        let location = section_body(&member.persona, "定位").unwrap_or_default();
        let boundary = section_body(&member.persona, "边界").unwrap_or_default();
        text.push_str(&format!("\n- {}｜{}\n  定位：{}\n  边界：{}\n", member.name, member.role, location, boundary));
    }
    text.push_str("\n你是本项目的一个团队成员，按上文的人设与规章处理本次用户要求。\n\
本次调用只是你的一次发言，调度由应用负责。说完就结束，不要等其他成员回复，不要反复查看文件等别人的进展。\n\
以本回合的要求为准。群消息里更早的计划和发言只是参考，不是待办。\n\
不得修改 .imates/，尤其是 .imates/rules/；规章变更建议在报告中提出，由用户通过规章编辑入口处理。\n\
完成后简要说明做了什么、改了哪些文件、还有什么遗留问题。\n");
    text
}

pub fn build_turn_prompt(snapshot: &TurnPromptSnapshot<'_>) -> String {
    let mut sections = Vec::new();
    if snapshot.recovery || snapshot.agent.session_id.is_none() {
        sections.push(build_opening(snapshot.project, snapshot.rules, snapshot.agent, snapshot.roster));
    } else if snapshot.agent.persona_dirty {
        sections.push(format!("# 已更新的人设（本回合立即生效）\n{}", snapshot.agent.persona.trim()));
    }
    sections.push(format!("# 当前成员\n名称：{}\n角色：{}", snapshot.agent.name, snapshot.agent.role));
    sections.push(format!("# 本回合用户要求\n{}", snapshot.request.trim()));
    if !snapshot.unread.is_empty() {
        sections.push(format!("# 自上次读取后新增的项目消息\n{}", format_messages(snapshot.unread, snapshot.roster, snapshot.agent_id)));
    }
    if snapshot.recovery && !snapshot.recent.is_empty() {
        let start = snapshot.recent.len().saturating_sub(20);
        sections.push(format!("# 最近历史补充（仅供参考；本回合要求和未读消息优先）\n{}",
            format_messages(&snapshot.recent[start..], snapshot.roster, snapshot.agent_id)));
    }
    sections.join("\n\n")
}

fn format_messages(messages: &[Message], roster: &[Agent], agent_id: &str) -> String {
    messages.iter().filter(|message| message.sender_agent_id.as_deref() != Some(agent_id))
        .map(|message| {
            let sender = match message.sender_type.as_str() {
                "user" => "用户".to_owned(),
                "system" => "系统".to_owned(),
                _ => message.sender_agent_id.as_deref().and_then(|id| roster.iter().find(|a| a.id == id))
                    .map(|a| a.name.clone()).unwrap_or_else(|| "成员".into()),
            };
            format!("[{sender}] {}", message.content)
        }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::agents::{self, NewAgent};
    use crate::store::{open_in_memory, projects};
    const PERSONA: &str = "## 定位\n\n本人定位\n\n## 职责\n\n本人职责保密标记\n\n## 边界\n\n本人边界\n\n## 工作风格\n\n本人风格保密标记\n";
    #[test]
    fn opening_contains_rules_and_full_persona_but_only_public_roster_fields() {
        let conn = open_in_memory().unwrap(); let project = projects::create(&conn, "网站", "重构官网", "/tmp/site").unwrap();
        let me = agents::create(&conn, &project.id, &NewAgent { name: "组长".into(), role: "管理".into(), persona: PERSONA.into(), cli: "claude".into(), model: None, is_leader: true, reasoning_effort: None }).unwrap();
        let mut input = NewAgent { name: "设计".into(), role: "设计师".into(), persona: PERSONA.replace("本人", "其他成员"), cli: "codex".into(), model: None, is_leader: false, reasoning_effort: None };
        let other = agents::create(&conn, &project.id, &input).unwrap(); input.persona = PERSONA.replace("本人", "第三成员"); input.name = "测试".into();
        let third = agents::create(&conn, &project.id, &input).unwrap();
        let rules = vec![RuleDoc { filename: "协作.md".into(), content: "必须写测试".into() }];
        let opening = build_opening(&project, &rules, &me, &[me.clone(), other.clone(), third]);
        assert!(opening.contains("网站") && opening.contains("重构官网") && opening.contains("必须写测试"));
        assert!(opening.contains("本人职责保密标记") && opening.contains("本人风格保密标记"));
        assert!(opening.contains("设计") && opening.contains("其他成员定位") && opening.contains("其他成员边界"));
        assert!(!opening.contains("其他成员职责保密标记") && !opening.contains("其他成员风格保密标记"));
        assert!(!opening.contains("第三成员职责保密标记"));
        for line in ["本次调用只是你的一次发言", "不得修改 .imates/", "完成后简要说明"] { assert!(opening.contains(line)); }
    }
    #[test]
    fn empty_rules_are_explicit_and_recovery_keeps_unread_plus_last_twenty() {
        let conn = open_in_memory().unwrap(); let project = projects::create(&conn, "空规章", "", "/tmp/empty").unwrap();
        let agent = agents::create(&conn, &project.id, &NewAgent { name: "组长".into(), role: "开发".into(), persona: PERSONA.into(), cli: "claude".into(), model: None, is_leader: true, reasoning_effort: None }).unwrap();
        assert!(build_opening(&project, &[], &agent, std::slice::from_ref(&agent)).contains("尚未配置项目规章"));
        let old_unread = message(2, "尚未读过的早期约束", None); let recent = (0..25).map(|i| message(10+i, &format!("历史{i}"), None)).collect::<Vec<_>>();
        let snapshot = TurnPromptSnapshot { request: "当前请求必须保留", agent_id: &agent.id, project: &project, rules: &[], agent: &agent,
            roster: std::slice::from_ref(&agent), unread: &[old_unread], recent: &recent, recovery: true };
        let prompt = build_turn_prompt(&snapshot);
        assert!(prompt.contains("当前请求必须保留") && prompt.contains("尚未读过的早期约束"));
        assert!(prompt.contains("历史24") && prompt.contains("历史5")); assert!(!prompt.contains("历史4"));
    }
    fn message(id: i64, content: &str, sender: Option<&str>) -> Message {
        Message { id, project_id: "project".into(), sender_type: if sender.is_some() { "assistant" } else { "user" }.into(),
            sender_agent_id: sender.map(str::to_owned), kind: "chat".into(), content: content.into(), round: None, created_at: id, model_info: None }
    }
}
