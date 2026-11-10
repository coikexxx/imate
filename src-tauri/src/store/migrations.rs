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
"#];

/// 把数据库升级到最新版本。版本号记在 `PRAGMA user_version`。
pub fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(current as usize) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", (index + 1) as i64)?;
        tx.commit()?;
    }
    Ok(())
}
