use std::path::Path;

use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use crate::error::{AppError, AppResult};
use crate::store::{new_id, now_ms};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub folder_path: String,
    /// idle / discussing / executing / waiting_user
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 首页卡片用：项目本身加成员数和文件夹是否还在。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSummary {
    #[serde(flatten)]
    pub project: Project,
    pub agent_count: i64,
    pub folder_exists: bool,
}

const COLUMNS: &str = "id, name, description, folder_path, status, created_at, updated_at";
const ELEVATION_PREFIX: &str = "project_elevation:";

fn from_row(row: &Row) -> rusqlite::Result<Project> {
    Ok(Project {
        id: row.get(0)?,
        name: row.get(1)?,
        description: row.get(2)?,
        folder_path: row.get(3)?,
        status: row.get(4)?,
        created_at: row.get(5)?,
        updated_at: row.get(6)?,
    })
}

fn validate<'a>(name: &'a str, folder_path: &'a str) -> AppResult<(&'a str, &'a str)> {
    let name = name.trim();
    let folder_path = folder_path.trim();
    if name.is_empty() {
        return Err(AppError::Invalid("项目名称不能为空".into()));
    }
    if folder_path.is_empty() {
        return Err(AppError::Invalid("请选择项目文件夹".into()));
    }
    Ok((name, folder_path))
}

pub fn create(
    conn: &Connection,
    name: &str,
    description: &str,
    folder_path: &str,
) -> AppResult<Project> {
    let (name, folder_path) = validate(name, folder_path)?;
    let id = new_id();
    let now = now_ms();
    conn.execute(
        "INSERT INTO projects (id, name, description, folder_path, status, created_at, updated_at) \
         VALUES (?1, ?2, ?3, ?4, 'idle', ?5, ?5)",
        params![id, name, description.trim(), folder_path, now],
    )?;
    get(conn, &id)
}

pub fn get(conn: &Connection, id: &str) -> AppResult<Project> {
    conn.query_row(
        &format!("SELECT {COLUMNS} FROM projects WHERE id = ?1"),
        [id],
        from_row,
    )
    .optional()?
    .ok_or_else(|| AppError::NotFound(format!("项目 {id}")))
}

pub fn list(conn: &Connection) -> AppResult<Vec<ProjectSummary>> {
    let mut stmt = conn.prepare(
        "SELECT p.id, p.name, p.description, p.folder_path, p.status, p.created_at, p.updated_at, \
                (SELECT COUNT(*) FROM agents a WHERE a.project_id = p.id) \
         FROM projects p ORDER BY p.updated_at DESC",
    )?;
    let rows = stmt.query_map([], |row| {
        let project = from_row(row)?;
        let agent_count: i64 = row.get(7)?;
        Ok((project, agent_count))
    })?;
    let mut summaries = Vec::new();
    for row in rows {
        let (project, agent_count) = row?;
        let folder_exists = Path::new(&project.folder_path).is_dir();
        summaries.push(ProjectSummary {
            project,
            agent_count,
            folder_exists,
        });
    }
    Ok(summaries)
}

pub fn update(
    conn: &Connection,
    id: &str,
    name: &str,
    description: &str,
    folder_path: &str,
) -> AppResult<Project> {
    let (name, folder_path) = validate(name, folder_path)?;
    let tx = conn.unchecked_transaction()?;
    let old = get(&tx, id)?;
    let changed = tx.execute(
        "UPDATE projects SET name = ?1, description = ?2, folder_path = ?3, updated_at = ?4 \
         WHERE id = ?5",
        params![name, description.trim(), folder_path, now_ms(), id],
    )?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("项目 {id}")));
    }
    if old.folder_path != folder_path {
        tx.execute(
            "UPDATE agents SET session_id = NULL WHERE project_id = ?1",
            [id],
        )?;
    }
    tx.commit()?;
    get(conn, id)
}

/// 项目默认是否允许执行任务在权限受阻时自动提权重跑。默认关闭。
pub fn elevation_allowed(conn: &Connection, project_id: &str) -> AppResult<bool> {
    let value: Option<String> = conn
        .query_row("SELECT value FROM settings WHERE key = ?1", [format!("{ELEVATION_PREFIX}{project_id}")], |row| row.get(0))
        .optional()?;
    Ok(value.as_deref() == Some("1"))
}

/// 保存项目的高权限执行开关。只影响之后新建的指令。
pub fn set_elevation_allowed(conn: &Connection, project_id: &str, allowed: bool) -> AppResult<()> {
    get(conn, project_id)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES(?1, ?2) ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![format!("{ELEVATION_PREFIX}{project_id}"), if allowed { "1" } else { "0" }],
    )?;
    Ok(())
}

/// 只删除应用内的记录（成员等随外键级联删除），不动项目文件夹。
pub fn delete(conn: &Connection, id: &str) -> AppResult<()> {
    let changed = conn.execute("DELETE FROM projects WHERE id = ?1", [id])?;
    if changed == 0 {
        return Err(AppError::NotFound(format!("项目 {id}")));
    }
    conn.execute("DELETE FROM settings WHERE key = ?1", [format!("{ELEVATION_PREFIX}{id}")])?;
    Ok(())
}

/// 更新"最近活动时间"，成员变动时调用。
pub fn touch(conn: &Connection, id: &str) -> AppResult<()> {
    conn.execute(
        "UPDATE projects SET updated_at = ?1 WHERE id = ?2",
        params![now_ms(), id],
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_in_memory;

    #[test]
    fn create_trims_and_get_returns_same() {
        let conn = open_in_memory().unwrap();
        let project = create(&conn, "  官网改版  ", " 做新官网 ", "D:/work/site").unwrap();
        assert_eq!(project.name, "官网改版");
        assert_eq!(project.description, "做新官网");
        assert_eq!(project.status, "idle");
        assert_eq!(get(&conn, &project.id).unwrap(), project);
    }

    #[test]
    fn create_rejects_blank_name_or_folder() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(
            create(&conn, "  ", "", "D:/x"),
            Err(AppError::Invalid(_))
        ));
        assert!(matches!(
            create(&conn, "a", "", "  "),
            Err(AppError::Invalid(_))
        ));
    }

    #[test]
    fn get_missing_is_not_found() {
        let conn = open_in_memory().unwrap();
        assert!(matches!(get(&conn, "nope"), Err(AppError::NotFound(_))));
    }

    #[test]
    fn list_reports_agent_count_and_folder_exists() {
        let conn = open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let real = create(&conn, "有文件夹", "", dir.path().to_str().unwrap()).unwrap();
        let ghost = create(&conn, "没文件夹", "", "Z:/definitely/not/here").unwrap();
        conn.execute(
            "INSERT INTO agents (id, project_id, name, role, persona, cli, is_leader, seat_index, created_at) \
             VALUES ('a1', ?1, '老王', '组长', 'x', 'claude', 1, 0, 0)",
            [&real.id],
        )
        .unwrap();

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 2);
        let real_row = all.iter().find(|s| s.project.id == real.id).unwrap();
        assert_eq!(real_row.agent_count, 1);
        assert!(real_row.folder_exists);
        let ghost_row = all.iter().find(|s| s.project.id == ghost.id).unwrap();
        assert_eq!(ghost_row.agent_count, 0);
        assert!(!ghost_row.folder_exists);
    }

    #[test]
    fn update_changes_fields() {
        let conn = open_in_memory().unwrap();
        let project = create(&conn, "旧名", "", "D:/old").unwrap();
        let updated = update(&conn, &project.id, "新名", "新描述", "D:/new").unwrap();
        assert_eq!(updated.name, "新名");
        assert_eq!(updated.description, "新描述");
        assert_eq!(updated.folder_path, "D:/new");
        assert!(matches!(
            update(&conn, "nope", "x", "", "D:/x"),
            Err(AppError::NotFound(_))
        ));
    }

    #[test]
    fn elevation_is_off_by_default_and_saved_per_project() {
        let conn = open_in_memory().unwrap();
        let first = create(&conn, "甲", "", "D:/a").unwrap();
        let second = create(&conn, "乙", "", "D:/b").unwrap();
        assert!(!elevation_allowed(&conn, &first.id).unwrap());
        set_elevation_allowed(&conn, &first.id, true).unwrap();
        assert!(elevation_allowed(&conn, &first.id).unwrap());
        assert!(!elevation_allowed(&conn, &second.id).unwrap(), "开关按项目隔离");
        assert!(matches!(set_elevation_allowed(&conn, "nope", true), Err(AppError::NotFound(_))));
        delete(&conn, &first.id).unwrap();
        assert!(!elevation_allowed(&conn, &first.id).unwrap(), "删除项目时清掉开关");
    }

    #[test]
    fn delete_removes_project_and_its_agents() {
        let conn = open_in_memory().unwrap();
        let project = create(&conn, "待删", "", "D:/x").unwrap();
        conn.execute(
            "INSERT INTO agents (id, project_id, name, role, persona, cli, is_leader, seat_index, created_at) \
             VALUES ('a1', ?1, '老王', '组长', 'x', 'claude', 1, 0, 0)",
            [&project.id],
        )
        .unwrap();

        delete(&conn, &project.id).unwrap();

        assert!(matches!(get(&conn, &project.id), Err(AppError::NotFound(_))));
        let agents: i64 = conn
            .query_row("SELECT COUNT(*) FROM agents", [], |r| r.get(0))
            .unwrap();
        assert_eq!(agents, 0);
        assert!(matches!(
            delete(&conn, &project.id),
            Err(AppError::NotFound(_))
        ));
    }
}
