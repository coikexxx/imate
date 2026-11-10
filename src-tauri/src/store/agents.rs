use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::{Deserialize, Serialize};

use crate::cli_registry;
use crate::error::{AppError, AppResult};
use crate::persona;
use crate::store::{new_id, now_ms, projects};

/// 每个项目的成员上限，也是像素办公室的工位数。
pub const MAX_AGENTS: i64 = 8;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Agent {
    pub id: String,
    pub project_id: String,
    pub name: String,
    pub role: String,
    pub persona: String,
    /// 人设被改过、尚未通知到该成员的 CLI 会话
    pub persona_dirty: bool,
    pub cli: String,
    pub model: Option<String>,
    pub is_leader: bool,
    pub seat_index: i64,
    pub created_at: i64,
}

/// 新建和编辑成员共用的输入。
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewAgent {
    pub name: String,
    pub role: String,
    pub persona: String,
    pub cli: String,
    pub model: Option<String>,
    pub is_leader: bool,
}

const COLUMNS: &str = "id, project_id, name, role, persona, persona_dirty, cli, model, \
                       is_leader, seat_index, created_at";

fn from_row(row: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: row.get(0)?,
        project_id: row.get(1)?,
        name: row.get(2)?,
        role: row.get(3)?,
        persona: row.get(4)?,
        persona_dirty: row.get(5)?,
        cli: row.get(6)?,
        model: row.get(7)?,
        is_leader: row.get(8)?,
        seat_index: row.get(9)?,
        created_at: row.get(10)?,
    })
}

/// 校验并整理输入，返回（名字，角色，模型）。
fn normalize(input: &NewAgent) -> AppResult<(&str, &str, Option<&str>)> {
    let name = input.name.trim();
    let role = input.role.trim();
    if name.is_empty() {
        return Err(AppError::Invalid("成员名字不能为空".into()));
    }
    if role.is_empty() {
        return Err(AppError::Invalid("成员角色不能为空".into()));
    }
    if !cli_registry::is_known(&input.cli) {
        return Err(AppError::Invalid(format!("不支持的 CLI：{}", input.cli)));
    }
    persona::validate(&input.persona)?;
    let model = input
        .model
        .as_deref()
        .map(str::trim)
        .filter(|m| !m.is_empty());
    Ok((name, role, model))
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Agent> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM agents WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("成员 {id}")))
}

pub fn list(conn: &Connection, project_id: &str) -> AppResult<Vec<Agent>> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {COLUMNS} FROM agents WHERE project_id = ?1 ORDER BY seat_index"
    ))?;
    let rows = stmt.query_map([project_id], from_row)?;
    let mut agents = Vec::new();
    for row in rows {
        agents.push(row?);
    }
    Ok(agents)
}

/// 新建成员。项目的第一个成员总是组长；新成员指定为组长时，原组长自动卸任。
pub fn create(conn: &Connection, project_id: &str, input: &NewAgent) -> AppResult<Agent> {
    let (name, role, model) = normalize(input)?;
    projects::get(conn, project_id)?;
    let existing = list(conn, project_id)?;
    if existing.len() as i64 >= MAX_AGENTS {
        return Err(AppError::Invalid(format!(
            "每个项目最多 {MAX_AGENTS} 个成员"
        )));
    }
    if existing.iter().any(|a| a.name == name) {
        return Err(AppError::Invalid(format!("成员名「{name}」已存在")));
    }

    let is_leader = input.is_leader || existing.is_empty();
    if is_leader {
        conn.execute(
            "UPDATE agents SET is_leader = 0 WHERE project_id = ?1",
            [project_id],
        )?;
    }
    let seat_index = (0..MAX_AGENTS)
        .find(|seat| !existing.iter().any(|a| a.seat_index == *seat))
        .unwrap_or(0);

    let id = new_id();
    conn.execute(
        "INSERT INTO agents (id, project_id, name, role, persona, cli, model, is_leader, seat_index, created_at) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params![
            id,
            project_id,
            name,
            role,
            input.persona,
            input.cli,
            model,
            is_leader,
            seat_index,
            now_ms()
        ],
    )?;
    projects::touch(conn, project_id)?;
    get(conn, &id)
}

/// 编辑成员。组长不能直接卸任，只能通过把另一位成员设为组长来交接。
pub fn update(conn: &Connection, id: &str, input: &NewAgent) -> AppResult<Agent> {
    let (name, role, model) = normalize(input)?;
    let old = get(conn, id)?;
    let siblings = list(conn, &old.project_id)?;
    if siblings.iter().any(|a| a.id != id && a.name == name) {
        return Err(AppError::Invalid(format!("成员名「{name}」已存在")));
    }
    if old.is_leader && !input.is_leader {
        return Err(AppError::Invalid(
            "每个项目必须有一个组长；请把另一位成员设为组长来交接".into(),
        ));
    }
    if input.is_leader && !old.is_leader {
        conn.execute(
            "UPDATE agents SET is_leader = 0 WHERE project_id = ?1",
            [&old.project_id],
        )?;
    }

    let persona_dirty = old.persona_dirty || old.persona != input.persona;
    conn.execute(
        "UPDATE agents SET name = ?1, role = ?2, persona = ?3, persona_dirty = ?4, \
                           cli = ?5, model = ?6, is_leader = ?7 \
         WHERE id = ?8",
        params![
            name,
            role,
            input.persona,
            persona_dirty,
            input.cli,
            model,
            input.is_leader,
            id
        ],
    )?;
    projects::touch(conn, &old.project_id)?;
    get(conn, id)
}

/// 删除成员。组长不能删除（要先交接，或者删除整个项目）。
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let agent = get(conn, id)?;
    if agent.is_leader {
        return Err(AppError::Invalid(
            "不能删除组长；请先把另一位成员设为组长".into(),
        ));
    }
    conn.execute("DELETE FROM agents WHERE id = ?1", [id])?;
    projects::touch(conn, &agent.project_id)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{open_in_memory, projects};

    const PERSONA: &str =
        "## 定位\n\n测试成员\n\n## 职责\n\n干活\n\n## 边界\n\n不越界\n\n## 工作风格\n\n务实\n";

    fn input(name: &str, is_leader: bool) -> NewAgent {
        NewAgent {
            name: name.to_string(),
            role: "开发".to_string(),
            persona: PERSONA.to_string(),
            cli: "claude".to_string(),
            model: None,
            is_leader,
        }
    }

    fn setup() -> (rusqlite::Connection, String) {
        let conn = open_in_memory().unwrap();
        let project = projects::create(&conn, "测试项目", "", "D:/x").unwrap();
        (conn, project.id)
    }

    #[test]
    fn first_agent_becomes_leader_even_if_not_asked() {
        let (conn, pid) = setup();
        let first = create(&conn, &pid, &input("老王", false)).unwrap();
        assert!(first.is_leader);
        assert_eq!(first.seat_index, 0);
        assert!(!first.persona_dirty);
    }

    #[test]
    fn new_leader_replaces_old_leader() {
        let (conn, pid) = setup();
        let first = create(&conn, &pid, &input("老王", true)).unwrap();
        let second = create(&conn, &pid, &input("小李", true)).unwrap();
        assert!(second.is_leader);
        assert!(!get(&conn, &first.id).unwrap().is_leader);
    }

    #[test]
    fn rejects_duplicate_name_blank_fields_unknown_cli_bad_persona() {
        let (conn, pid) = setup();
        create(&conn, &pid, &input("老王", true)).unwrap();

        assert!(matches!(
            create(&conn, &pid, &input(" 老王 ", false)),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            create(&conn, &pid, &input("  ", false)),
            Err(AppError::Invalid(_))
        ));

        let mut bad_cli = input("小李", false);
        bad_cli.cli = "gemini".to_string();
        assert!(matches!(
            create(&conn, &pid, &bad_cli),
            Err(AppError::Invalid(_))
        ));

        let mut bad_persona = input("小张", false);
        bad_persona.persona = "## 定位\n\n只有一段".to_string();
        assert!(matches!(
            create(&conn, &pid, &bad_persona),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn rejects_ninth_agent() {
        let (conn, pid) = setup();
        for i in 0..8 {
            create(&conn, &pid, &input(&format!("成员{i}"), false)).unwrap();
        }
        assert!(matches!(
            create(&conn, &pid, &input("第九个", false)),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn create_in_missing_project_is_not_found() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(
            create(&conn, "nope", &input("老王", true)),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn list_is_ordered_by_seat_and_freed_seat_is_reused() {
        let (conn, pid) = setup();
        create(&conn, &pid, &input("甲", true)).unwrap();
        let second = create(&conn, &pid, &input("乙", false)).unwrap();
        create(&conn, &pid, &input("丙", false)).unwrap();
        assert_eq!(second.seat_index, 1);

        delete(&conn, &second.id).unwrap();
        let replacement = create(&conn, &pid, &input("丁", false)).unwrap();
        assert_eq!(replacement.seat_index, 1);

        let names: Vec<String> = list(&conn, &pid)
            .unwrap()
            .into_iter()
            .map(|a| a.name)
            .collect();
        assert_eq!(names, ["甲", "丁", "丙"]);
    }

    #[test]
    fn update_marks_persona_dirty_only_when_persona_changes() {
        let (conn, pid) = setup();
        let agent = create(&conn, &pid, &input("老王", true)).unwrap();

        let mut renamed = input("老王头", true);
        renamed.model = Some("  ".to_string());
        let after_rename = update(&conn, &agent.id, &renamed).unwrap();
        assert_eq!(after_rename.name, "老王头");
        assert_eq!(after_rename.model, None);
        assert!(!after_rename.persona_dirty);

        let mut rewritten = input("老王头", true);
        rewritten.persona = PERSONA.replace("务实", "激进");
        let after_rewrite = update(&conn, &agent.id, &rewritten).unwrap();
        assert!(after_rewrite.persona_dirty);
    }

    #[test]
    fn update_can_hand_over_leadership_but_not_drop_it() {
        let (conn, pid) = setup();
        let leader = create(&conn, &pid, &input("老王", true)).unwrap();
        let member = create(&conn, &pid, &input("小李", false)).unwrap();

        assert!(matches!(
            update(&conn, &leader.id, &input("老王", false)),
            Err(AppError::Invalid(_))
        ));

        let promoted = update(&conn, &member.id, &input("小李", true)).unwrap();
        assert!(promoted.is_leader);
        assert!(!get(&conn, &leader.id).unwrap().is_leader);
    }

    #[test]
    fn update_rejects_name_taken_by_sibling() {
        let (conn, pid) = setup();
        create(&conn, &pid, &input("老王", true)).unwrap();
        let member = create(&conn, &pid, &input("小李", false)).unwrap();
        assert!(matches!(
            update(&conn, &member.id, &input("老王", false)),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn leader_cannot_be_deleted() {
        let (conn, pid) = setup();
        let leader = create(&conn, &pid, &input("老王", true)).unwrap();
        assert!(matches!(
            delete(&conn, &leader.id),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(delete(&conn, "nope"), Err(AppError::NotFound(_))));
    }
}
