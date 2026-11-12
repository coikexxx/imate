use std::collections::HashSet;
use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;

use crate::error::{AppError, AppResult};
use crate::rules::{self, RuleDoc};
use crate::store::agents::{self, NewAgent, MAX_AGENTS};
use crate::store::projects::{self, Project};

/// 新建项目向导最后一步提交的全部内容。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewProject {
    pub name: String,
    pub description: String,
    pub folder_path: String,
    pub rules: Vec<RuleDoc>,
    pub agents: Vec<NewAgent>,
}

fn validate(input: &NewProject) -> AppResult<()> {
    if !Path::new(input.folder_path.trim()).is_dir() {
        return Err(AppError::Invalid(format!(
            "项目文件夹不存在：{}",
            input.folder_path
        )));
    }

    if input.rules.is_empty() {
        return Err(AppError::Invalid("至少需要一份规章".into()));
    }
    let mut seen = HashSet::new();
    for rule in &input.rules {
        rules::validate_filename(&rule.filename)?;
        if !seen.insert(rule.filename.as_str()) {
            return Err(AppError::Invalid(format!(
                "规章文件名重复：{}",
                rule.filename
            )));
        }
        if rule.content.trim().is_empty() {
            return Err(AppError::Invalid(format!(
                "规章内容为空：{}",
                rule.filename
            )));
        }
    }

    if input.agents.is_empty() {
        return Err(AppError::Invalid("至少需要一个成员".into()));
    }
    if input.agents.len() as i64 > MAX_AGENTS {
        return Err(AppError::Invalid(format!(
            "每个项目最多 {MAX_AGENTS} 个成员"
        )));
    }
    if input.agents.iter().filter(|a| a.is_leader).count() != 1 {
        return Err(AppError::Invalid("需要且只能有一个组长".into()));
    }
    Ok(())
}

/// 一次性创建项目、成员和规章文件。任何一步失败，数据库不留记录。
pub fn create_full(conn: &Connection, input: &NewProject) -> AppResult<Project> {
    validate(input)?;

    let tx = conn.unchecked_transaction()?;
    let project = projects::create(&tx, &input.name, &input.description, &input.folder_path)?;
    for agent in &input.agents {
        crate::cli_config::ensure_selectable(&tx, &agent.cli)?;
        agents::create(&tx, &project.id, agent)?;
    }
    // 规章文件放在最后写：前面任何一步失败时磁盘上还没有东西
    let folder = Path::new(&project.folder_path);
    for rule in &input.rules {
        rules::save(folder, &rule.filename, &rule.content)?;
    }
    tx.commit()?;
    Ok(project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_in_memory;
    use std::path::Path;

    const PERSONA: &str =
        "## 定位\n\n测试成员\n\n## 职责\n\n干活\n\n## 边界\n\n不越界\n\n## 工作风格\n\n务实\n";

    fn agent(name: &str, is_leader: bool) -> NewAgent {
        NewAgent {
            name: name.to_string(),
            role: "开发".to_string(),
            persona: PERSONA.to_string(),
            cli: "claude".to_string(),
            model: None,
            is_leader,
        }
    }

    fn rule(filename: &str) -> RuleDoc {
        RuleDoc {
            filename: filename.to_string(),
            content: "# 规章\n\n内容\n".to_string(),
        }
    }

    fn valid_input(folder: &Path) -> NewProject {
        NewProject {
            name: "官网改版".to_string(),
            description: "做新官网".to_string(),
            folder_path: folder.to_string_lossy().into_owned(),
            rules: vec![rule("协作规范.md"), rule("开发规范.md")],
            agents: vec![agent("老王", true), agent("小李", false)],
        }
    }

    fn project_count(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM projects", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn creates_project_agents_and_rule_files() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let project = create_full(&conn, &valid_input(dir.path())).unwrap();

        assert_eq!(project.name, "官网改版");
        let members = agents::list(&conn, &project.id).unwrap();
        assert_eq!(members.len(), 2);
        assert!(members[0].is_leader);
        assert!(!members[1].is_leader);
        let docs = rules::list(dir.path()).unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[test]
    fn rejects_missing_folder() {
        let conn = open_in_memory().unwrap();
        let mut input = valid_input(Path::new("Z:/definitely/not/here"));
        input.folder_path = "Z:/definitely/not/here".to_string();
        assert!(matches!(
            create_full(&conn, &input),
            Err(AppError::Invalid(_))
        ));
        assert_eq!(project_count(&conn), 0);
    }

    #[test]
    fn rejects_empty_duplicate_or_blank_rules() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let mut none = valid_input(dir.path());
        none.rules.clear();
        assert!(matches!(create_full(&conn, &none), Err(AppError::Invalid(_))));

        let mut duplicate = valid_input(dir.path());
        duplicate.rules = vec![rule("协作规范.md"), rule("协作规范.md")];
        assert!(matches!(
            create_full(&conn, &duplicate),
            Err(AppError::Invalid(_))
        ));

        let mut blank = valid_input(dir.path());
        blank.rules[0].content = "   \n".to_string();
        assert!(matches!(
            create_full(&conn, &blank),
            Err(AppError::Invalid(_))
        ));

        let mut bad_name = valid_input(dir.path());
        bad_name.rules[0].filename = "../逃逸.md".to_string();
        assert!(matches!(
            create_full(&conn, &bad_name),
            Err(AppError::Invalid(_))
        ));

        assert_eq!(project_count(&conn), 0);
        assert!(rules::list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn rejects_team_without_exactly_one_leader() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();

        let mut empty = valid_input(dir.path());
        empty.agents.clear();
        assert!(matches!(
            create_full(&conn, &empty),
            Err(AppError::Invalid(_))
        ));

        let mut no_leader = valid_input(dir.path());
        no_leader.agents = vec![agent("老王", false), agent("小李", false)];
        assert!(matches!(
            create_full(&conn, &no_leader),
            Err(AppError::Invalid(_))
        ));

        let mut two_leaders = valid_input(dir.path());
        two_leaders.agents = vec![agent("老王", true), agent("小李", true)];
        assert!(matches!(
            create_full(&conn, &two_leaders),
            Err(AppError::Invalid(_))
        ));

        let mut too_many = valid_input(dir.path());
        too_many.agents = (0..9)
            .map(|i| agent(&format!("成员{i}"), i == 0))
            .collect();
        assert!(matches!(
            create_full(&conn, &too_many),
            Err(AppError::Invalid(_))
        ));

        assert_eq!(project_count(&conn), 0);
    }

    #[test]
    fn invalid_agent_rolls_back_everything() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut input = valid_input(dir.path());
        input.agents[1].name = "老王".to_string(); // 与组长重名

        assert!(matches!(
            create_full(&conn, &input),
            Err(AppError::Invalid(_))
        ));
        assert_eq!(project_count(&conn), 0);
        let agent_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(agent_count, 0);
        assert!(rules::list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn rejects_members_whose_cli_was_closed_in_app_management() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let mut closed = crate::cli_config::default_config("claude");
        closed.enabled = false;
        crate::cli_config::save(&conn, "claude", &closed).unwrap();

        let error = create_full(&conn, &valid_input(dir.path())).unwrap_err();

        assert!(error.to_string().contains("已在应用管理中关闭"));
        assert_eq!(project_count(&conn), 0);
    }
}
