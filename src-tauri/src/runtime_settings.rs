//! 运行设置：内置助理 CLI 与成本护栏。两者都放在 settings 表，读写从这里收口。
//! 团队指令在首次启动时把护栏冻结进自己的上下文快照，之后改设置只影响新指令；
//! 起草和空闲点名在每次启动时读取一次当前值。

use std::time::Duration;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::runner::RunnerConfig;

/// 内置助理所用 CLI 的设置键。值是 CLI ID，没有记录表示自动选择。
pub const ASSISTANT_CLI_KEY: &str = "assistant_cli";
/// 护栏对象的设置键。值是 `RuntimeLimits` 的 JSON，缺字段按默认值补齐。
pub const LIMITS_KEY: &str = "runtime_limits";

/// 整条指令的回合与超时护栏。窗口按分钟或回合展示，这里一律是整秒和整回合。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RuntimeLimits {
    pub discuss_rounds: u32,
    pub reworks_per_task: u32,
    pub turns_per_instruction: u32,
    pub model_idle_seconds: u64,
    pub discuss_tool_seconds: u64,
    pub execute_tool_seconds: u64,
    pub discuss_total_seconds: u64,
    pub execute_total_seconds: u64,
}

impl Default for RuntimeLimits {
    fn default() -> Self {
        Self {
            discuss_rounds: 3,
            reworks_per_task: 2,
            turns_per_instruction: 30,
            model_idle_seconds: 300,
            discuss_tool_seconds: 600,
            execute_tool_seconds: 1_800,
            discuss_total_seconds: 600,
            execute_total_seconds: 1_800,
        }
    }
}

impl RuntimeLimits {
    pub const MAX_DISCUSS_ROUNDS: u32 = 10;
    pub const MAX_REWORKS: u32 = 10;
    pub const MAX_TURNS: u32 = 300;
    /// 任何一项超时都不能超过一天。
    pub const MAX_SECONDS: u64 = 24 * 60 * 60;

    /// 取值范围检查。失败时给出可直接展示的说明。
    pub fn validate(&self) -> AppResult<()> {
        if !(1..=Self::MAX_DISCUSS_ROUNDS).contains(&self.discuss_rounds) {
            return Err(invalid(format!("讨论轮数上限须在 1 到 {} 之间", Self::MAX_DISCUSS_ROUNDS)));
        }
        if self.reworks_per_task > Self::MAX_REWORKS {
            return Err(invalid(format!("单任务返工上限须在 0 到 {} 之间", Self::MAX_REWORKS)));
        }
        if !(1..=Self::MAX_TURNS).contains(&self.turns_per_instruction) {
            return Err(invalid(format!("单条指令回合总数上限须在 1 到 {} 之间", Self::MAX_TURNS)));
        }
        let seconds = [
            ("模型空闲阈值", self.model_idle_seconds),
            ("讨论工具运行阈值", self.discuss_tool_seconds),
            ("执行工具运行阈值", self.execute_tool_seconds),
            ("讨论回合总时长", self.discuss_total_seconds),
            ("执行回合总时长", self.execute_total_seconds),
        ];
        for (label, value) in seconds {
            if !(1..=Self::MAX_SECONDS).contains(&value) {
                return Err(invalid(format!("{label}须在 1 秒到 24 小时之间")));
            }
        }
        Ok(())
    }

    /// 转成 runner 的超时配置。与超时无关的内部清理参数保持默认。
    pub fn runner_config(&self) -> RunnerConfig {
        RunnerConfig {
            model_idle_timeout: Duration::from_secs(self.model_idle_seconds),
            discuss_tool_timeout: Duration::from_secs(self.discuss_tool_seconds),
            execute_tool_timeout: Duration::from_secs(self.execute_tool_seconds),
            discuss_timeout: Duration::from_secs(self.discuss_total_seconds),
            execute_timeout: Duration::from_secs(self.execute_total_seconds),
            ..RunnerConfig::default()
        }
    }
}

/// 设置页读写的完整对象。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeSettings {
    /// 空表示按安装顺序自动选择。
    pub assistant_cli: Option<String>,
    pub limits: RuntimeLimits,
}

impl Default for RuntimeSettings {
    fn default() -> Self {
        Self { assistant_cli: None, limits: RuntimeLimits::default() }
    }
}

/// 只读内置助理的 CLI 设置；损坏的护栏不影响这里。
pub fn assistant_cli(conn: &Connection) -> AppResult<Option<String>> {
    Ok(read_key(conn, ASSISTANT_CLI_KEY)?.map(|id| id.trim().to_string()).filter(|id| !id.is_empty()))
}

/// 读取完整设置。没有记录时返回默认值；护栏记录损坏时报错，请用户重新保存。
pub fn load(conn: &Connection) -> AppResult<RuntimeSettings> {
    let limits = match read_key(conn, LIMITS_KEY)? {
        None => RuntimeLimits::default(),
        Some(json) => {
            let limits: RuntimeLimits = serde_json::from_str(&json)
                .map_err(|_| invalid("已保存的护栏设置损坏，请在系统设置中重新保存"))?;
            limits.validate().map_err(|error| invalid(format!("已保存的护栏设置无效（{error}），请在系统设置中重新保存")))?;
            limits
        }
    };
    Ok(RuntimeSettings { assistant_cli: assistant_cli(conn)?, limits })
}

/// 运行时使用的护栏。读取失败或记录损坏时回退默认值，不让损坏的设置挡住回合。
pub fn effective_limits(conn: &Connection) -> RuntimeLimits {
    load(conn).map(|settings| settings.limits).unwrap_or_default()
}

/// 在一个事务里验证并保存两个字段；任一项无效则整体不写。
pub fn save(conn: &Connection, settings: &RuntimeSettings) -> AppResult<RuntimeSettings> {
    settings.limits.validate()?;
    let assistant = match settings.assistant_cli.as_deref().map(str::trim).filter(|id| !id.is_empty()) {
        None => None,
        Some(id) => {
            if !crate::cli_registry::is_known(id) {
                return Err(invalid(format!("未知的助理 CLI：{id}")));
            }
            if !crate::cli_registry::is_supported(id) {
                return Err(invalid(format!("助理 CLI「{id}」尚未接入，不能设为内置助理")));
            }
            Some(id.to_string())
        }
    };
    let json = serde_json::to_string(&settings.limits).map_err(|error| invalid(format!("护栏设置序列化失败：{error}")))?;
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        [LIMITS_KEY, json.as_str()],
    )?;
    match &assistant {
        Some(id) => {
            tx.execute(
                "INSERT INTO settings(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",
                [ASSISTANT_CLI_KEY, id.as_str()],
            )?;
        }
        None => {
            tx.execute("DELETE FROM settings WHERE key = ?1", [ASSISTANT_CLI_KEY])?;
        }
    }
    tx.commit()?;
    Ok(RuntimeSettings { assistant_cli: assistant, limits: settings.limits.clone() })
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::Invalid(message.into())
}

fn read_key(conn: &Connection, key: &str) -> AppResult<Option<String>> {
    Ok(conn.query_row("SELECT value FROM settings WHERE key = ?1", [key], |row| row.get(0)).optional()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::open_in_memory;

    fn stored(conn: &Connection, key: &str) -> Option<String> {
        read_key(conn, key).unwrap()
    }

    #[test]
    fn missing_settings_fall_back_to_handbook_defaults() {
        let conn = open_in_memory().unwrap();
        let settings = load(&conn).unwrap();
        assert_eq!(settings.assistant_cli, None);
        assert_eq!(settings.limits, RuntimeLimits {
            discuss_rounds: 3, reworks_per_task: 2, turns_per_instruction: 30,
            model_idle_seconds: 300, discuss_tool_seconds: 600, execute_tool_seconds: 1_800,
            discuss_total_seconds: 600, execute_total_seconds: 1_800,
        });
        assert_eq!(effective_limits(&conn), RuntimeLimits::default());
        assert_eq!(assistant_cli(&conn).unwrap(), None);
    }

    #[test]
    fn partial_or_old_limits_json_keeps_defaults_for_missing_fields() {
        let conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO settings(key,value) VALUES(?1,?2)", [LIMITS_KEY, r#"{"discussRounds":5}"#]).unwrap();
        let settings = load(&conn).unwrap();
        assert_eq!(settings.limits.discuss_rounds, 5);
        assert_eq!(settings.limits.turns_per_instruction, 30);
        assert_eq!(settings.limits.execute_total_seconds, 1_800);
        // 序列化是 camelCase，供前端直接使用。
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["limits"]["discussRounds"], 5);
        assert_eq!(json["limits"]["turnsPerInstruction"], 30);
        assert!(json.get("assistantCli").is_some());
    }

    #[test]
    fn invalid_numbers_are_rejected_and_nothing_is_written() {
        let conn = open_in_memory().unwrap();
        let base = RuntimeSettings::default();
        let cases: Vec<(&str, RuntimeLimits)> = vec![
            ("讨论轮数为 0", RuntimeLimits { discuss_rounds: 0, ..RuntimeLimits::default() }),
            ("讨论轮数过大", RuntimeLimits { discuss_rounds: RuntimeLimits::MAX_DISCUSS_ROUNDS + 1, ..RuntimeLimits::default() }),
            ("返工上限过大", RuntimeLimits { reworks_per_task: RuntimeLimits::MAX_REWORKS + 1, ..RuntimeLimits::default() }),
            ("回合总数为 0", RuntimeLimits { turns_per_instruction: 0, ..RuntimeLimits::default() }),
            ("回合总数过大", RuntimeLimits { turns_per_instruction: RuntimeLimits::MAX_TURNS + 1, ..RuntimeLimits::default() }),
            ("空闲阈值为 0", RuntimeLimits { model_idle_seconds: 0, ..RuntimeLimits::default() }),
            ("讨论工具阈值为 0", RuntimeLimits { discuss_tool_seconds: 0, ..RuntimeLimits::default() }),
            ("执行工具阈值为 0", RuntimeLimits { execute_tool_seconds: 0, ..RuntimeLimits::default() }),
            ("讨论总时长为 0", RuntimeLimits { discuss_total_seconds: 0, ..RuntimeLimits::default() }),
            ("执行总时长超过一天", RuntimeLimits { execute_total_seconds: RuntimeLimits::MAX_SECONDS + 1, ..RuntimeLimits::default() }),
        ];
        for (label, limits) in cases {
            let error = save(&conn, &RuntimeSettings { limits, ..base.clone() }).unwrap_err();
            assert!(matches!(error, AppError::Invalid(_)), "{label} 应当是输入错误：{error}");
            assert_eq!(stored(&conn, LIMITS_KEY), None, "{label} 不应写入");
        }
        // 返工上限允许为 0：组长不能打回任何任务。
        let zero = RuntimeSettings { limits: RuntimeLimits { reworks_per_task: 0, ..RuntimeLimits::default() }, ..base };
        assert_eq!(save(&conn, &zero).unwrap(), zero);
    }

    #[test]
    fn unknown_or_unsupported_assistant_cli_is_rejected() {
        let conn = open_in_memory().unwrap();
        for id in ["gemini", "pi", "  claude-code  "] {
            let error = save(&conn, &RuntimeSettings { assistant_cli: Some(id.into()), limits: RuntimeLimits::default() }).unwrap_err();
            assert!(matches!(error, AppError::Invalid(_)), "{id}：{error}");
            assert_eq!(stored(&conn, ASSISTANT_CLI_KEY), None, "{id} 不应写入");
        }
        // 空白等同于自动选择：删除记录而不是写入空字符串。
        conn.execute("INSERT INTO settings(key,value) VALUES(?1,'codex')", [ASSISTANT_CLI_KEY]).unwrap();
        let cleared = save(&conn, &RuntimeSettings { assistant_cli: Some("   ".into()), limits: RuntimeLimits::default() }).unwrap();
        assert_eq!(cleared.assistant_cli, None);
        assert_eq!(stored(&conn, ASSISTANT_CLI_KEY), None);
        let saved = save(&conn, &RuntimeSettings { assistant_cli: Some(" dsh ".into()), limits: RuntimeLimits::default() }).unwrap();
        assert_eq!(saved.assistant_cli.as_deref(), Some("dsh"));
        assert_eq!(stored(&conn, ASSISTANT_CLI_KEY).as_deref(), Some("dsh"));
        assert_eq!(assistant_cli(&conn).unwrap().as_deref(), Some("dsh"));
    }

    #[test]
    fn failed_save_rolls_back_both_fields() {
        let conn = open_in_memory().unwrap();
        let first = RuntimeSettings { assistant_cli: Some("codex".into()), limits: RuntimeLimits { discuss_rounds: 4, ..RuntimeLimits::default() } };
        assert_eq!(save(&conn, &first).unwrap(), first);
        let bad = RuntimeSettings { assistant_cli: Some("claude".into()), limits: RuntimeLimits { turns_per_instruction: 0, ..RuntimeLimits::default() } };
        assert!(save(&conn, &bad).is_err());
        assert_eq!(load(&conn).unwrap(), first, "助理 CLI 和护栏都应保持上一次的值");
        let bad = RuntimeSettings { assistant_cli: Some("gemini".into()), limits: RuntimeLimits { discuss_rounds: 6, ..RuntimeLimits::default() } };
        assert!(save(&conn, &bad).is_err());
        assert_eq!(load(&conn).unwrap(), first);
    }

    #[test]
    fn corrupted_limits_fail_to_load_but_runtime_falls_back_to_defaults() {
        let conn = open_in_memory().unwrap();
        conn.execute("INSERT INTO settings(key,value) VALUES(?1,'{坏')", [LIMITS_KEY]).unwrap();
        conn.execute("INSERT INTO settings(key,value) VALUES(?1,'grok')", [ASSISTANT_CLI_KEY]).unwrap();
        let error = load(&conn).unwrap_err();
        assert!(error.to_string().contains("重新保存"), "{error}");
        assert_eq!(effective_limits(&conn), RuntimeLimits::default());
        assert_eq!(assistant_cli(&conn).unwrap().as_deref(), Some("grok"), "护栏损坏不影响助理 CLI");
        // 越界的旧记录同样不生效。
        conn.execute("UPDATE settings SET value=?1 WHERE key=?2", [r#"{"turnsPerInstruction":0}"#, LIMITS_KEY]).unwrap();
        assert!(load(&conn).is_err());
        assert_eq!(effective_limits(&conn), RuntimeLimits::default());
        // 重新保存后恢复。
        let fixed = RuntimeSettings { assistant_cli: Some("grok".into()), limits: RuntimeLimits::default() };
        assert_eq!(save(&conn, &fixed).unwrap(), fixed);
        assert_eq!(load(&conn).unwrap(), fixed);
    }

    #[test]
    fn runner_config_uses_seconds_and_keeps_internal_grace() {
        let limits = RuntimeLimits {
            model_idle_seconds: 7, discuss_tool_seconds: 8, execute_tool_seconds: 9,
            discuss_total_seconds: 10, execute_total_seconds: 11, ..RuntimeLimits::default()
        };
        let config = limits.runner_config();
        assert_eq!(config.model_idle_timeout, Duration::from_secs(7));
        assert_eq!(config.discuss_tool_timeout, Duration::from_secs(8));
        assert_eq!(config.execute_tool_timeout, Duration::from_secs(9));
        assert_eq!(config.discuss_timeout, Duration::from_secs(10));
        assert_eq!(config.execute_timeout, Duration::from_secs(11));
        assert_eq!(config.stdout_close_grace, RunnerConfig::default().stdout_close_grace);
        let defaults = RuntimeLimits::default().runner_config();
        assert_eq!(defaults.model_idle_timeout, RunnerConfig::default().model_idle_timeout);
        assert_eq!(defaults.execute_timeout, RunnerConfig::default().execute_timeout);
    }
}
