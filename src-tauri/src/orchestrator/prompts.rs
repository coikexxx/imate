//! 团队六个阶段的提示词。讨论轮共用一份快照，只纳入消息上界以内的内容。

use crate::error::{AppError, AppResult};
use crate::prompts::build_opening;
use crate::rules::RuleDoc;
use crate::store::agents::Agent;
use crate::store::messages::Message;
use crate::store::projects::Project;

const CONSTRAINTS: &str = "\
# 回合约束\n\
本次调用只是你的一次发言，调度由应用负责。说完就结束，不要等其他成员回复，不要反复查看文件等别人的进展。\n\
以本回合的要求为准。群消息里更早的计划和发言只是参考，不是待办。\n\
不得修改 .imates/，尤其是 .imates/rules/；需要调整规章时在汇报中提出，由用户通过规章编辑入口处理。";

const PROTOCOL: &str = "\
# 指令协议\n\
回复末尾只能有一个闭合的 imates 代码块，之前的文字是发言。\n\
reply 需要 message。discuss 需要 to 和 topic。assign 需要 decision 和 tasks，每项含 to、title、detail，可选 replaces_task_id。rework 需要 task_id 和 feedback。\n\
replaces_task_id 只能引用本条指令内尚未被替代的 failed/cancelled 任务，同一批不能重复替代；跨指令重试请新建任务，不要填写 replaces_task_id。旧消息里的任务编号不代表本条指令的任务。\n\
只有干预研判可以带 disposition：continue、end 或 replace。";

/// 组长或成员正在执行的阶段。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Triage,
    Discuss,
    Decide,
    Execute,
    Review,
    Intervention,
}

/// 验收和恢复包里展示的任务事实。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskBrief<'a> {
    pub id: &'a str,
    pub title: &'a str,
    pub assignee: &'a str,
    pub status: &'a str,
    pub detail: &'a str,
    pub report: Option<&'a str>,
}

/// 一轮开始前固定下来的上下文。后启动的成员也只能看到这个上界之内的消息。
#[derive(Debug, Clone, Copy)]
pub struct TeamSnapshot<'a> {
    pub message_bound: i64,
    pub rules_version: &'a str,
    pub roster_version: &'a str,
    pub unread: &'a [Message],
    pub recent: &'a [Message],
    pub project: &'a Project,
    pub rules: &'a [RuleDoc],
    pub agent: &'a Agent,
    pub roster: &'a [Agent],
    pub goal: &'a str,
    pub topic: &'a str,
    pub round_index: u32,
    pub discuss_rounds: u32,
    /// 指令冻结的讨论轮数上限与单任务返工上限。
    pub discuss_round_limit: u32,
    pub rework_limit: u32,
    pub decision: Option<&'a str>,
    pub tasks: &'a [TaskBrief<'a>],
    pub current_task: Option<&'a TaskBrief<'a>>,
    pub intervention_note: &'a str,
    pub recovery: bool,
    pub fresh_session: bool,
}

pub fn build_leader_prompt(stage: Stage, snapshot: &TeamSnapshot<'_>) -> AppResult<String> {
    match stage {
        Stage::Triage | Stage::Decide | Stage::Review | Stage::Intervention => render(stage, snapshot),
        Stage::Discuss | Stage::Execute => Err(invalid("这个阶段不是组长回合")),
    }
}

pub fn build_member_prompt(stage: Stage, snapshot: &TeamSnapshot<'_>) -> AppResult<String> {
    match stage {
        Stage::Discuss | Stage::Execute => render(stage, snapshot),
        Stage::Triage | Stage::Decide | Stage::Review | Stage::Intervention => Err(invalid("这个阶段不是成员回合")),
    }
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Invalid(message.into())
}

fn render(stage: Stage, snapshot: &TeamSnapshot<'_>) -> AppResult<String> {
    let mut sections = Vec::new();
    if snapshot.fresh_session {
        sections.push(build_opening(snapshot.project, snapshot.rules, snapshot.agent, snapshot.roster));
    } else if snapshot.agent.persona_dirty {
        // 已有会话不重发开场，只把新人设全文放在最前面。
        sections.push(format!(
            "# 已更新的人设（本回合立即生效，取代此前的人设）\n{}",
            snapshot.agent.persona.trim()
        ));
    }
    sections.push(format!(
        "# 本轮快照\n消息上界：{}\n规章版本：{}\n花名册版本：{}",
        snapshot.message_bound, snapshot.rules_version, snapshot.roster_version
    ));
    sections.push(frozen_rules(snapshot));
    sections.push(frozen_roster(snapshot));
    let unread = visible(snapshot.unread, snapshot.message_bound, &snapshot.agent.id);
    if !unread.is_empty() {
        sections.push(format!("# 自上次读取后新增的项目消息\n{}", format_messages(&unread, snapshot.roster)));
    }
    if snapshot.recovery {
        sections.push(recovery_section(snapshot));
        let recent = visible(snapshot.recent, snapshot.message_bound, &snapshot.agent.id);
        if !recent.is_empty() {
            let start = recent.len().saturating_sub(20);
            sections.push(format!(
                "# 最近历史补充（仅供参考，不能代替恢复包）\n{}",
                format_messages(&recent[start..], snapshot.roster)
            ));
        }
    }
    sections.push(stage_body(stage, snapshot)?);
    sections.push(CONSTRAINTS.to_string());
    Ok(sections.join("\n\n"))
}

fn stage_body(stage: Stage, snapshot: &TeamSnapshot<'_>) -> AppResult<String> {
    let body = match stage {
        Stage::Triage => format!(
            "# 组长研判\n当前目标：{}\n能直接答复就使用 reply，不要为了显得忙而发起讨论或派工。\n\
派工时每个任务的 detail 要写清要做什么、交付什么、怎样算完成。\n\n{PROTOCOL}",
            snapshot.goal.trim()
        ),
        Stage::Discuss => format!(
            "# 成员讨论\n议题：{}\n请给出立场和理由，可以反驳他人。本回合不得修改文件。",
            snapshot.topic.trim()
        ),
        Stage::Decide => {
            let remaining = snapshot.discuss_round_limit.saturating_sub(snapshot.discuss_rounds);
            let mut text = format!(
                "# 组长裁定\n当前是第 {} 轮，已用 {} 轮，剩余 {} 轮。",
                snapshot.round_index, snapshot.discuss_rounds, remaining
            );
            if remaining == 0 {
                text.push_str(&format!("\n讨论已达 {} 轮，必须使用 assign 或 reply。", snapshot.discuss_round_limit));
            }
            text.push_str("\n\n");
            text.push_str(PROTOCOL);
            text
        }
        Stage::Execute => {
            let Some(task) = snapshot.current_task else {
                return Err(invalid("执行阶段缺少任务"));
            };
            format!(
                "# 成员执行\n任务编号：{}\n标题：{}\n详情：{}\n\
如果某个操作被权限拦截且无法绕过，不要声称它已经完成；在汇报末尾单独一行写：\
受阻：<操作>｜<原文摘要>。\n\
完成后给出简短汇报：做了什么、改了哪些文件、还有什么遗留问题。\n\
你只汇报自己的任务，不能宣布整条指令完成。",
                task.id, task.title, task.detail.trim()
            )
        }
        Stage::Review => {
            let rework_rule = if snapshot.rework_limit == 0 {
                "本条指令不允许返工：不要使用 rework，违反规章或越界时用 assign 重新派工或 reply 说明。".to_string()
            } else {
                format!("违反规章或越界时用 rework 打回，rework 必须写任务编号；每个任务最多返工 {} 次。", snapshot.rework_limit)
            };
            format!(
                "# 组长验收\n对照项目规章和各成员的边界检查。{rework_rule}\n\
状态 blocked 表示任务因权限受阻没有完成，不算完成；它需要用户授权后才能重跑。\n{}\n\n{PROTOCOL}",
                format_tasks(snapshot.tasks)
            )
        }
        Stage::Intervention => format!(
            "# 干预研判\n{}\n本阶段只能使用 reply 加 continue、reply 加 end，或 assign 加 replace。其他组合都是协议错误。\n\
不得清零已用的讨论轮数和总回合数。\n\n{PROTOCOL}",
            snapshot.intervention_note.trim()
        ),
    };
    Ok(body)
}

fn recovery_section(snapshot: &TeamSnapshot<'_>) -> String {
    let decision = snapshot.decision.unwrap_or("尚无有效决议").trim();
    format!("# 恢复包\n当前目标：{}\n有效决议：{}\n任务状态：\n{}", snapshot.goal.trim(), decision, format_tasks(snapshot.tasks))
}

fn format_tasks(tasks: &[TaskBrief<'_>]) -> String {
    if tasks.is_empty() {
        return "当前没有任务。".to_string();
    }
    tasks.iter().map(|task| {
        let report = task.report.unwrap_or("尚无汇报");
        format!("- 编号 {}｜{}｜{}｜状态 {}｜汇报：{}", task.id, task.title, task.assignee, task.status, report)
    }).collect::<Vec<_>>().join("\n")
}

fn frozen_rules(snapshot: &TeamSnapshot<'_>) -> String {
    if snapshot.rules.is_empty() {
        return format!("# 冻结的项目规章（版本 {}）\n当前尚未配置项目规章。", snapshot.rules_version);
    }
    let mut text = format!("# 冻结的项目规章（版本 {}）\n", snapshot.rules_version);
    for rule in snapshot.rules {
        text.push_str(&format!("\n## {}\n{}\n", rule.filename, rule.content.trim()));
    }
    text
}

fn frozen_roster(snapshot: &TeamSnapshot<'_>) -> String {
    let mut text = format!("# 冻结的花名册（版本 {}）\n", snapshot.roster_version);
    for member in snapshot.roster {
        text.push_str(&format!("- {}｜{}\n", member.name, member.role));
    }
    text
}

fn visible<'a>(messages: &'a [Message], bound: i64, agent_id: &str) -> Vec<&'a Message> {
    messages.iter().filter(|message| message.id <= bound && message.sender_agent_id.as_deref() != Some(agent_id)).collect()
}

fn format_messages(messages: &[&Message], roster: &[Agent]) -> String {
    messages.iter().map(|message| {
        let sender = match message.sender_type.as_str() {
            "user" => "用户".to_owned(),
            "system" => "系统".to_owned(),
            _ => message.sender_agent_id.as_deref()
                .and_then(|id| roster.iter().find(|agent| agent.id == id))
                .map(|agent| agent.name.clone())
                .unwrap_or_else(|| "成员".to_owned()),
        };
        format!("[{sender}] {}", message.content)
    }).collect::<Vec<_>>().join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::RuleDoc;
    use crate::store::agents::{self, Agent, NewAgent};
    use crate::store::messages::Message;
    use crate::store::projects::Project;
    use crate::store::{open_in_memory, projects};

    const PERSONA: &str = "## 定位\n\n本人定位\n\n## 职责\n\n本人职责\n\n## 边界\n\n本人边界\n\n## 工作风格\n\n本人风格\n";

    fn message(id: i64, content: &str, sender: Option<&str>) -> Message {
        Message {
            id, project_id: "p".into(),
            sender_type: if sender.is_some() { "agent" } else { "user" }.into(),
            sender_agent_id: sender.map(str::to_owned),
            kind: "chat".into(), content: content.into(), round: None, created_at: id, model_info: None,
        }
    }

    fn world() -> (Project, Agent, Agent) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "网站", "登录", "/tmp/site").unwrap();
        let leader = agents::create(&conn, &project.id, &NewAgent {
            name: "组长".into(), role: "管理".into(), persona: PERSONA.into(),
            cli: "claude".into(), model: None, is_leader: true, reasoning_effort: None,
        }).unwrap();
        let member = agents::create(&conn, &project.id, &NewAgent {
            name: "前端".into(), role: "开发".into(), persona: PERSONA.into(),
            cli: "codex".into(), model: None, is_leader: false, reasoning_effort: None,
        }).unwrap();
        (project, leader, member)
    }

    fn task_brief<'a>() -> TaskBrief<'a> {
        TaskBrief { id: "task-7", title: "登录校验", assignee: "前端", status: "done", detail: "写清校验规则", report: Some("已经写好校验") }
    }

    fn snapshot<'a>(
        project: &'a Project, agent: &'a Agent, roster: &'a [Agent], rules: &'a [RuleDoc],
        unread: &'a [Message], recent: &'a [Message], tasks: &'a [TaskBrief<'a>], current: Option<&'a TaskBrief<'a>>,
    ) -> TeamSnapshot<'a> {
        TeamSnapshot {
            message_bound: 10, rules_version: "rules-3", roster_version: "roster-2",
            unread, recent, project, rules, agent, roster,
            goal: "当前目标是登录", topic: "登录校验", round_index: 3, discuss_rounds: 3,
            discuss_round_limit: 3, rework_limit: 2,
            decision: Some("有效决议：用手写校验"), tasks, current_task: current,
            intervention_note: "用户补充了验收标准", recovery: false, fresh_session: false,
        }
    }

    #[test]
    fn six_templates_state_turn_limits_and_stage_rules() {
        let (project, leader, member) = world();
        let roster = vec![leader.clone(), member.clone()];
        let rules = vec![RuleDoc { filename: "协作.md".into(), content: "必须写测试".into() }];
        let tasks = vec![task_brief()];
        let current = task_brief();
        let leader_snap = snapshot(&project, &leader, &roster, &rules, &[], &[], &tasks, Some(&current));
        let member_snap = snapshot(&project, &member, &roster, &rules, &[], &[], &tasks, Some(&current));
        let prompts = [
            build_leader_prompt(Stage::Triage, &leader_snap).unwrap(),
            build_leader_prompt(Stage::Decide, &leader_snap).unwrap(),
            build_leader_prompt(Stage::Review, &leader_snap).unwrap(),
            build_leader_prompt(Stage::Intervention, &leader_snap).unwrap(),
            build_member_prompt(Stage::Discuss, &member_snap).unwrap(),
            build_member_prompt(Stage::Execute, &member_snap).unwrap(),
        ];
        for prompt in &prompts {
            assert!(prompt.contains("说完就结束"), "{prompt}");
            assert!(prompt.contains("不得修改 .imates/"), "{prompt}");
        }
        assert!(prompts[0].contains("能直接答复就") && prompts[0].contains("reply"));
        for prompt in &prompts[..4] {
            assert!(prompt.contains("跨指令重试请新建任务，不要填写 replaces_task_id"));
        }
        assert!(prompts[1].contains("必须") && prompts[1].contains("assign") && prompts[1].contains("reply"));
        assert!(prompts[2].contains("task-7") && prompts[2].contains("已经写好校验"));
        assert!(prompts[4].contains("登录校验"));
        assert!(prompts[5].contains("不能宣布整条指令完成") && prompts[5].contains("写清校验规则"));
    }

    #[test]
    fn decide_and_review_prompts_state_the_configured_limits() {
        let (project, leader, _) = world();
        let roster = vec![leader.clone()];
        let tasks = vec![task_brief()];
        let mut snap = snapshot(&project, &leader, &roster, &[], &[], &[], &tasks, None);
        snap.discuss_rounds = 2;
        snap.round_index = 2;
        snap.discuss_round_limit = 5;
        snap.rework_limit = 4;
        let decide = build_leader_prompt(Stage::Decide, &snap).unwrap();
        assert!(decide.contains("已用 2 轮，剩余 3 轮"), "{decide}");
        assert!(!decide.contains("讨论已达"), "{decide}");
        snap.discuss_rounds = 5;
        let capped = build_leader_prompt(Stage::Decide, &snap).unwrap();
        assert!(capped.contains("剩余 0 轮") && capped.contains("讨论已达 5 轮"), "{capped}");
        let review = build_leader_prompt(Stage::Review, &snap).unwrap();
        assert!(review.contains("最多返工 4 次"), "{review}");
        snap.rework_limit = 0;
        let no_rework = build_leader_prompt(Stage::Review, &snap).unwrap();
        assert!(no_rework.contains("不允许返工"), "{no_rework}");
    }

    #[test]
    fn discuss_round_shares_the_snapshot_and_hides_messages_after_the_bound() {
        let (project, leader, member) = world();
        let roster = vec![leader.clone(), member.clone()];
        let rules = vec![RuleDoc { filename: "协作.md".into(), content: "必须写测试".into() }];
        let unread = vec![
            message(5, "上界内的旧发言", None),
            message(11, "本轮刚写的发言", Some(&leader.id)),
        ];
        let tasks = vec![task_brief()];
        let early = snapshot(&project, &leader, &roster, &rules, &unread, &[], &tasks, None);
        let late = snapshot(&project, &member, &roster, &rules, &unread, &[], &tasks, None);
        let first = build_member_prompt(Stage::Discuss, &early).unwrap();
        let second = build_member_prompt(Stage::Discuss, &late).unwrap();
        assert_eq!(first, build_member_prompt(Stage::Discuss, &early).unwrap());
        for prompt in [&first, &second] {
            assert!(prompt.contains("上界内的旧发言"));
            assert!(!prompt.contains("本轮刚写的发言"));
            assert!(prompt.contains("消息上界：10"));
            assert!(prompt.contains("规章版本：rules-3"));
            assert!(prompt.contains("花名册版本：roster-2"));
            assert!(prompt.contains("登录校验"));
            assert!(prompt.contains("必须写测试"));
        }
    }

    #[test]
    fn recovery_keeps_goal_decision_and_task_state_and_limits_recent_history() {
        let (project, leader, _) = world();
        let roster = vec![leader.clone()];
        let tasks = vec![TaskBrief { id: "t1", title: "校验", assignee: "前端", status: "failed", detail: "补齐失败路径", report: Some("写到一半失败") }];
        let mut recent: Vec<Message> = (0..25).map(|index| message(index, &format!("历史{index}"), None)).collect();
        recent.push(message(200, "越界消息不能出现", None));
        let mut snap = snapshot(&project, &leader, &roster, &[], &[], &recent, &tasks, None);
        snap.recovery = true;
        snap.message_bound = 100;
        let prompt = build_leader_prompt(Stage::Triage, &snap).unwrap();
        assert!(prompt.contains("当前目标是登录"));
        assert!(prompt.contains("有效决议：用手写校验"));
        assert!(prompt.contains("t1") && prompt.contains("failed"));
        assert!(prompt.contains("补充"));
        assert!(prompt.contains("历史24") && prompt.contains("历史5"));
        assert!(!prompt.contains("历史4"));
        assert!(!prompt.contains("越界消息不能出现"));
    }
}
