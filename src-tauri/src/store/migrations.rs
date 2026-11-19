use rusqlite::Connection;

use crate::error::AppResult;

/// 迁移脚本，按数组顺序执行。只能在末尾追加，不能修改已发布的条目。
const MIGRATIONS: &[&str] = &[r#"
CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    folder_path TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'idle',
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL
);

CREATE TABLE agents (
    id                   TEXT PRIMARY KEY,
    project_id           TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    name                 TEXT NOT NULL,
    role                 TEXT NOT NULL,
    persona              TEXT NOT NULL,
    persona_dirty        INTEGER NOT NULL DEFAULT 0,
    cli                  TEXT NOT NULL,
    model                TEXT,
    is_leader            INTEGER NOT NULL DEFAULT 0,
    session_id           TEXT,
    last_seen_message_id INTEGER,
    seat_index           INTEGER NOT NULL,
    created_at           INTEGER NOT NULL,
    UNIQUE (project_id, name)
);
CREATE UNIQUE INDEX agents_one_leader ON agents(project_id) WHERE is_leader = 1;

CREATE TABLE messages (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    project_id      TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    sender_type     TEXT NOT NULL,
    sender_agent_id TEXT REFERENCES agents(id) ON DELETE SET NULL,
    kind            TEXT NOT NULL,
    content         TEXT NOT NULL,
    round           INTEGER,
    created_at      INTEGER NOT NULL
);
CREATE INDEX messages_by_project ON messages(project_id, id);

CREATE TABLE tasks (
    id                     TEXT PRIMARY KEY,
    project_id             TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    instruction_message_id INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    assignee_agent_id      TEXT REFERENCES agents(id) ON DELETE SET NULL,
    title                  TEXT NOT NULL,
    detail                 TEXT NOT NULL DEFAULT '',
    order_index            INTEGER NOT NULL,
    status                 TEXT NOT NULL DEFAULT 'pending',
    rework_count           INTEGER NOT NULL DEFAULT 0,
    report_message_id      INTEGER REFERENCES messages(id) ON DELETE SET NULL,
    created_at             INTEGER NOT NULL,
    finished_at            INTEGER
);

CREATE TABLE runs (
    id         TEXT PRIMARY KEY,
    agent_id   TEXT REFERENCES agents(id) ON DELETE CASCADE,
    mode       TEXT NOT NULL,
    command    TEXT NOT NULL,
    session_id TEXT,
    started_at INTEGER NOT NULL,
    ended_at   INTEGER,
    exit_code  INTEGER,
    status     TEXT NOT NULL DEFAULT 'running',
    usage_json TEXT,
    error      TEXT
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
"#, r#"
CREATE TABLE instructions (
    id                     TEXT PRIMARY KEY,
    project_id             TEXT NOT NULL REFERENCES projects(id) ON DELETE CASCADE,
    source_message_id      INTEGER NOT NULL UNIQUE REFERENCES messages(id) ON DELETE CASCADE,
    kind                   TEXT NOT NULL,
    target_agent_id        TEXT REFERENCES agents(id) ON DELETE SET NULL,
    phase                  TEXT NOT NULL,
    status                 TEXT NOT NULL,
    discuss_rounds         INTEGER NOT NULL DEFAULT 0,
    turn_count             INTEGER NOT NULL DEFAULT 0,
    current_batch          TEXT,
    resume_phase           TEXT,
    context_snapshot_json  TEXT,
    wait_reason            TEXT,
    created_at             INTEGER NOT NULL,
    updated_at             INTEGER NOT NULL,
    finished_at            INTEGER
);

CREATE TABLE instruction_inputs (
    id               TEXT PRIMARY KEY,
    instruction_id   TEXT NOT NULL REFERENCES instructions(id) ON DELETE CASCADE,
    message_id       INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    kind             TEXT NOT NULL,
    target_agent_id  TEXT REFERENCES agents(id) ON DELETE SET NULL,
    status           TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    finished_at      INTEGER
);
CREATE INDEX instruction_inputs_by_message ON instruction_inputs(instruction_id, message_id);

ALTER TABLE tasks ADD COLUMN instruction_id TEXT REFERENCES instructions(id) ON DELETE SET NULL;
ALTER TABLE tasks ADD COLUMN batch_id TEXT;
ALTER TABLE tasks ADD COLUMN superseded_by_task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;

ALTER TABLE runs ADD COLUMN instruction_id TEXT REFERENCES instructions(id) ON DELETE SET NULL;
ALTER TABLE runs ADD COLUMN task_id TEXT REFERENCES tasks(id) ON DELETE SET NULL;

ALTER TABLE agents ADD COLUMN name_key TEXT;
"#, r#"
ALTER TABLE runs ADD COLUMN requested_model TEXT;
ALTER TABLE runs ADD COLUMN reported_model TEXT;
ALTER TABLE messages ADD COLUMN run_id TEXT;
"#, r#"
ALTER TABLE instructions ADD COLUMN elevated_allowed INTEGER NOT NULL DEFAULT 0;
ALTER TABLE runs ADD COLUMN elevated INTEGER NOT NULL DEFAULT 0;
"#, r#"
ALTER TABLE agents ADD COLUMN reasoning_effort TEXT;
"#, r#"
ALTER TABLE agents ADD COLUMN execution_permission TEXT;
"#];

/// 把数据库升级到最新版本。版本号记在 `PRAGMA user_version`。
pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        if index + 1 == 2 {
            backfill_name_keys(&tx)?;
        }
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}

/// 只填充空白的 `name_key`。同一项目归一化后重名时不建唯一索引，调用方事务会回滚。
fn backfill_name_keys(conn: &Connection) -> AppResult<()> {
    let mut stmt = conn.prepare("SELECT id, project_id, name, name_key FROM agents")?;
    let rows: Vec<(String, String, String, Option<String>)> = stmt
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)))?
        .collect::<Result<_, _>>()?;
    drop(stmt);
    let mut seen = std::collections::HashMap::<(String, String), String>::new();
    for (id, project_id, name, name_key) in &rows {
        let key = name_key.clone().filter(|key| !key.is_empty()).unwrap_or_else(|| name.trim().to_lowercase());
        if let Some(other) = seen.insert((project_id.clone(), key), id.clone()) {
            if other != *id {
                return Err(crate::error::AppError::Invalid("成员名归一后有冲突，请先修改名称".into()));
            }
        }
    }
    for (id, _, name, name_key) in &rows {
        if name_key.as_deref().unwrap_or("").is_empty() {
            conn.execute("UPDATE agents SET name_key = ?1 WHERE id = ?2", rusqlite::params![name.trim().to_lowercase(), id])?;
        }
    }
    conn.execute_batch("CREATE UNIQUE INDEX IF NOT EXISTS agents_name_key ON agents(project_id, name_key);")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v1_with_project(conn: &Connection) {
        conn.execute_batch(MIGRATIONS[0]).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute(
            "INSERT INTO projects (id,name,description,folder_path,status,created_at,updated_at) VALUES ('p','甲','','/tmp','idle',1,1)",
            [],
        ).unwrap();
    }

    #[test]
    fn m3_migration_appends_instruction_tables_and_rejects_ambiguous_names() {
        let mut conn = Connection::open_in_memory().unwrap();
        v1_with_project(&conn);
        conn.execute("INSERT INTO agents (id,project_id,name,role,persona,cli,is_leader,seat_index,created_at) VALUES ('a','p','前端','开发','x','claude',1,0,1)", []).unwrap();
        conn.execute("INSERT INTO agents (id,project_id,name,role,persona,cli,is_leader,seat_index,created_at) VALUES ('b','p',' 前端 ','开发','x','codex',0,1,1)", []).unwrap();
        assert!(migrate(&mut conn).is_err());
        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, 1);
    }

    #[test]
    fn run_model_migration_adds_three_columns_and_leaves_old_rows_empty() {
        let mut conn = Connection::open_in_memory().unwrap();
        // 只应用到本迁移之前的两条迁移（固定为 2，之后再追加迁移也不会漂移）。
        let previous = 2;
        for sql in &MIGRATIONS[..previous] { conn.execute_batch(sql).unwrap(); }
        conn.pragma_update(None, "user_version", previous as i64).unwrap();
        conn.execute("INSERT INTO projects (id,name,description,folder_path,status,created_at,updated_at) VALUES ('p','甲','','/tmp','idle',1,1)", []).unwrap();
        conn.execute("INSERT INTO messages (project_id,sender_type,kind,content,created_at) VALUES ('p','user','chat','旧消息',1)", []).unwrap();
        conn.execute("INSERT INTO runs (id,agent_id,mode,command,started_at,status) VALUES ('r',NULL,'execute','{}',1,'ok')", []).unwrap();

        migrate(&mut conn).unwrap();

        let version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
        assert_eq!(version, MIGRATIONS.len() as i64);
        assert!(version > previous as i64);
        let columns = |table: &str| -> Vec<String> {
            let mut statement = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
            statement.query_map([], |row| row.get::<_, String>(1)).unwrap().map(|name| name.unwrap()).collect()
        };
        let runs = columns("runs");
        assert!(runs.contains(&"requested_model".to_string()) && runs.contains(&"reported_model".to_string()));
        assert!(columns("messages").contains(&"run_id".to_string()));
        let (requested, reported): (Option<String>, Option<String>) = conn
            .query_row("SELECT requested_model, reported_model FROM runs WHERE id='r'", [], |row| Ok((row.get(0)?, row.get(1)?))).unwrap();
        assert_eq!((requested, reported), (None, None));
        let run_id: Option<String> = conn.query_row("SELECT run_id FROM messages", [], |row| row.get(0)).unwrap();
        assert_eq!(run_id, None);
    }

    #[test]
    fn blocked_elevation_migration_adds_columns_and_defaults_to_off() {
        let mut conn = Connection::open_in_memory().unwrap();
        let previous = 3;
        for sql in &MIGRATIONS[..previous] { conn.execute_batch(sql).unwrap(); }
        conn.pragma_update(None, "user_version", previous as i64).unwrap();
        conn.execute("INSERT INTO projects (id,name,description,folder_path,status,created_at,updated_at) VALUES ('p','甲','','/tmp','idle',1,1)", []).unwrap();
        conn.execute("INSERT INTO messages (project_id,sender_type,kind,content,created_at) VALUES ('p','user','instruction','做',1)", []).unwrap();
        let message_id: i64 = conn.query_row("SELECT id FROM messages", [], |row| row.get(0)).unwrap();
        conn.execute("INSERT INTO instructions (id,project_id,source_message_id,kind,phase,status,created_at,updated_at) VALUES ('i','p',?1,'team','triage','running',1,1)", [message_id]).unwrap();
        conn.execute("INSERT INTO runs (id,mode,command,started_at,status) VALUES ('r','execute','{}',1,'ok')", []).unwrap();

        migrate(&mut conn).unwrap();

        let columns = |table: &str| -> Vec<String> {
            let mut statement = conn.prepare(&format!("PRAGMA table_info({table})")).unwrap();
            statement.query_map([], |row| row.get::<_, String>(1)).unwrap().map(|name| name.unwrap()).collect()
        };
        assert!(columns("instructions").contains(&"elevated_allowed".to_string()));
        assert!(columns("runs").contains(&"elevated".to_string()));
        let allowed: bool = conn.query_row("SELECT elevated_allowed FROM instructions WHERE id='i'", [], |row| row.get(0)).unwrap();
        let elevated: bool = conn.query_row("SELECT elevated FROM runs WHERE id='r'", [], |row| row.get(0)).unwrap();
        assert!(!allowed && !elevated, "旧数据默认关闭高权限且未提权");
        assert!(columns("agents").contains(&"reasoning_effort".to_string()));
        let effort: Option<String> = conn.query_row("SELECT reasoning_effort FROM agents LIMIT 1", [], |row| row.get(0)).unwrap_or(None);
        assert_eq!(effort, None, "旧成员没有推理强度，沿用 CLI 自己的配置");
    }

    #[test]
    fn execution_permission_migration_preserves_existing_members_and_sessions() {
        let mut conn = Connection::open_in_memory().unwrap();
        v1_with_project(&conn);
        for sql in &MIGRATIONS[1..5] { conn.execute_batch(sql).unwrap(); }
        conn.pragma_update(None, "user_version", 5).unwrap();
        conn.execute("INSERT INTO agents (id,project_id,name,role,persona,cli,is_leader,seat_index,created_at,session_id) VALUES ('a','p','成员','开发','x','grok',1,0,1,'旧会话')", []).unwrap();
        migrate(&mut conn).unwrap();
        let (permission, session): (Option<String>, String) = conn.query_row(
            "SELECT execution_permission,session_id FROM agents WHERE id='a'", [], |row| Ok((row.get(0)?, row.get(1)?)),
        ).unwrap();
        assert_eq!(permission, None);
        assert_eq!(session, "旧会话");
    }

    #[test]
    fn m3_migration_backfills_name_keys_and_accepts_an_instruction() {
        let mut conn = Connection::open_in_memory().unwrap();
        v1_with_project(&conn);
        conn.execute("INSERT INTO agents (id,project_id,name,role,persona,cli,is_leader,seat_index,created_at) VALUES ('a','p',' 前端 ','开发','x','claude',1,0,1)", []).unwrap();
        conn.execute("INSERT INTO agents (id,project_id,name,role,persona,cli,is_leader,seat_index,created_at) VALUES ('b','p','测试','测试','x','codex',0,1,1)", []).unwrap();
        migrate(&mut conn).unwrap();
        let keys: Vec<(String, String)> = {
            let mut stmt = conn.prepare("SELECT id, name_key FROM agents ORDER BY id").unwrap();
            stmt.query_map([], |row| Ok((row.get(1)?, row.get(0)?))).unwrap().map(|row| row.unwrap()).collect()
        };
        assert_eq!(keys, vec![("前端".into(), "a".into()), ("测试".into(), "b".into())]);
        conn.execute("INSERT INTO messages (project_id,sender_type,kind,content,created_at) VALUES ('p','user','instruction','开始',1)", []).unwrap();
        let message_id: i64 = conn.query_row("SELECT id FROM messages", [], |row| row.get(0)).unwrap();
        conn.execute(
            "INSERT INTO instructions (id,project_id,source_message_id,kind,phase,status,discuss_rounds,turn_count,created_at,updated_at) VALUES ('i','p',?1,'team','triage','running',0,0,1,1)",
            [message_id],
        ).unwrap();
        conn.execute(
            "INSERT INTO tasks (id,project_id,title,order_index,created_at) VALUES ('t','p','旧任务',0,1)",
            [],
        ).unwrap();
        let instruction_id: Option<String> = conn.query_row("SELECT instruction_id FROM tasks WHERE id='t'", [], |row| row.get(0)).unwrap();
        assert_eq!(instruction_id, None);
    }
}
