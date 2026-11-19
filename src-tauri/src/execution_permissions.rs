//! 成员执行回合的原生权限档位。讨论档位由适配器固定。

use serde::Serialize;

use crate::adapters::RunMode;
use crate::error::{AppError, AppResult};

#[derive(Serialize)]
pub struct PermissionOption {
    pub id: &'static str,
    pub label: &'static str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PermissionProfile {
    pub default_execute: &'static str,
    pub discuss: &'static str,
    #[serde(skip)]
    elevated: &'static str,
    pub options: &'static [PermissionOption],
}

const fn option(id: &'static str, label: &'static str) -> PermissionOption {
    PermissionOption { id, label }
}

const BYPASS: &str = "bypassPermissions";
const MAX_SANDBOX: &str = "danger-full-access";

pub static CLAUDE: PermissionProfile = PermissionProfile {
    default_execute: BYPASS,
    discuss: "plan",
    elevated: BYPASS,
    options: &[
        option("plan", "仅规划"),
        option("default", "按 CLI 审批规则"),
        option("dontAsk", "拒绝需审批的操作"),
        option("acceptEdits", "自动接受文件编辑"),
        option("auto", "自动审核"),
        option(BYPASS, "跳过常规审批"),
    ],
};

pub static GROK: PermissionProfile = PermissionProfile {
    default_execute: BYPASS,
    discuss: "plan",
    elevated: BYPASS,
    // Grok 的 auto 在当前无头事件中回显为 default，暂不提供无法核验的档位。
    options: &[
        option("plan", "仅规划"),
        option("default", "按 CLI 审批规则"),
        option("dontAsk", "拒绝需审批的操作"),
        option("acceptEdits", "自动接受文件编辑"),
        option(BYPASS, "跳过常规审批"),
    ],
};

pub static SANDBOX: PermissionProfile = PermissionProfile {
    default_execute: "workspace-write",
    discuss: "read-only",
    elevated: MAX_SANDBOX,
    options: &[
        option("read-only", "只读"),
        option("workspace-write", "工作区写入"),
        option(MAX_SANDBOX, "完全访问"),
    ],
};

pub fn profile(cli: &str) -> Option<&'static PermissionProfile> {
    match cli {
        "claude" => Some(&CLAUDE),
        "grok" => Some(&GROK),
        "codex" | "dsh" => Some(&SANDBOX),
        _ => None,
    }
}

/// 只返回适配器预先列出的值，不能把成员输入直接拼入命令参数。
pub fn normalize(cli: &str, selected: Option<&str>) -> AppResult<Option<&'static str>> {
    let Some(selected) = selected.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    profile(cli)
        .and_then(|profile| profile.options.iter().find(|option| option.id == selected))
        .map(|option| Some(option.id))
        .ok_or_else(|| AppError::Invalid(format!("所选执行权限不适用于 {cli}，请选择该 CLI 提供的档位")))
}

impl PermissionProfile {
    pub fn for_turn(&self, mode: RunMode, selected: Option<&'static str>, elevated: bool) -> &'static str {
        match mode {
            RunMode::Discuss => self.discuss,
            RunMode::Execute if elevated => self.elevated,
            RunMode::Execute => selected.unwrap_or(self.default_execute),
        }
    }
}

/// Grok 和显式使用新档位的 Claude 按讨论、执行模式区分可续接的会话。
pub fn separate_mode_sessions(cli: &str, selected: Option<&str>) -> bool {
    cli == "grok" || (cli == "claude" && selected.is_some_and(|permission| permission != BYPASS))
}

/// Claude/Grok 的工具权限可能绑定到会话。非默认档的单次提权使用新的会话。
pub fn isolated_elevation(cli: &str, selected: Option<&str>, mode: &str, elevated: bool) -> bool {
    elevated && mode == "execute" && matches!(cli, "claude" | "grok")
        && selected.is_some_and(|permission| permission != BYPASS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_permission_choices_match_each_cli_and_reject_other_values() {
        for cli in ["claude", "grok", "codex", "dsh"] {
            let profile = profile(cli).unwrap();
            assert!(profile.options.iter().any(|option| option.id == profile.default_execute));
            for option in profile.options {
                assert_eq!(normalize(cli, Some(option.id)).unwrap(), Some(option.id));
            }
            assert_eq!(normalize(cli, Some("  ")).unwrap(), None);
            assert!(normalize(cli, Some("--help")).is_err());
        }
        assert!(normalize("codex", Some("bypassPermissions")).is_err());
        assert!(normalize("grok", Some("auto")).is_err());
        assert!(normalize("claude", Some("workspace-write")).is_err());
        assert!(normalize("pi", Some("read-only")).is_err());
    }
}
