//! 非 Dsh CLI 的推理强度：固定档位、校验与命令行参数。
//! Dsh 的强度跟随模型选择，存在模型 JSON 里（见 `dsh_config`），不走这里。

use crate::error::{AppError, AppResult};

/// 各 CLI 命令行接受的档位，按从弱到强排列。取值来自各 CLI 自己的帮助与报错信息：
/// claude `--effort`、grok `--reasoning-effort`、codex 的 `model_reasoning_effort` 配置项。
pub fn levels(cli: &str) -> &'static [&'static str] {
    match cli {
        "claude" => &["low", "medium", "high", "xhigh", "max"],
        // codex 的档位随模型而异：模型目录里 max、ultra 几乎所有模型都有，none 只有个别模型有，这里不列。
        "codex" => &["low", "medium", "high", "xhigh", "max", "ultra"],
        "grok" => &["low", "medium", "high", "xhigh"],
        _ => &[],
    }
}

/// 校验并整理成员保存的推理强度。空白按「未选择」处理，返回 `None`。
pub fn normalize(cli: &str, effort: Option<&str>) -> AppResult<Option<String>> {
    let Some(effort) = effort.map(str::trim).filter(|value| !value.is_empty()) else { return Ok(None); };
    if cli == "dsh" {
        return Err(AppError::Invalid("Dsh 的推理强度跟随模型选择，请在模型里设置".into()));
    }
    if !levels(cli).contains(&effort) {
        return Err(AppError::Invalid(format!("{cli} 不支持推理强度「{}」，可选：{}", effort.escape_debug(), levels(cli).join("、"))));
    }
    Ok(Some(effort.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_cli_accepts_only_its_own_levels() {
        assert_eq!(normalize("claude", Some("max")).unwrap().as_deref(), Some("max"));
        assert_eq!(normalize("codex", Some("ultra")).unwrap().as_deref(), Some("ultra"));
        assert_eq!(normalize("codex", Some("max")).unwrap().as_deref(), Some("max"));
        assert!(normalize("codex", Some("none")).is_err(), "none 只有个别 codex 模型支持，不提供");
        assert!(normalize("grok", Some("max")).is_err(), "grok 没有 max 档");
        assert!(normalize("grok", Some("ultra")).is_err(), "grok 没有 ultra 档");
        assert_eq!(normalize("grok", Some(" xhigh ")).unwrap().as_deref(), Some("xhigh"));
        assert!(normalize("claude", Some("high; rm")).is_err());
    }

    #[test]
    fn blank_means_not_selected_and_dsh_never_stores_one_here() {
        for blank in [None, Some(""), Some("   ")] { assert_eq!(normalize("claude", blank).unwrap(), None); }
        assert_eq!(normalize("dsh", None).unwrap(), None);
        assert!(normalize("dsh", Some("high")).is_err());
        assert!(levels("dsh").is_empty());
    }
}
