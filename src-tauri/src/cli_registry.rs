use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde::Serialize;

/// 一个受支持的 CLI：内部标识、界面上的名字、可执行文件名。
pub struct CliSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub binary: &'static str,
}

/// 顺序即界面上的排列顺序，也是内置助理选默认 CLI 的优先顺序。
pub const KNOWN_CLIS: [CliSpec; 5] = [
    CliSpec { id: "claude", label: "Claude Code", binary: "claude" },
    CliSpec { id: "codex", label: "Codex", binary: "codex" },
    CliSpec { id: "grok", label: "Grok", binary: "grok" },
    CliSpec { id: "dsh", label: "DeepSeek Harness", binary: "dsh" },
    CliSpec { id: "pi", label: "Pi", binary: "pi" },
];

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

pub fn is_known(id: &str) -> bool {
    KNOWN_CLIS.iter().any(|spec| spec.id == id)
}

/// 从 `--version` 的输出里取出版本号：第一个以数字开头且含 `.` 的词。
pub fn parse_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .map(|token| {
            token
                .trim_start_matches('v')
                .trim_end_matches([',', ';', ')'])
        })
        .find(|token| {
            token.contains('.') && token.chars().next().is_some_and(|c| c.is_ascii_digit())
        })
        .map(str::to_string)
}

/// 探测逻辑本体。查找路径和取版本号由调用方注入，便于测试。
pub fn detect_with(
    find: impl Fn(&str) -> Option<PathBuf>,
    get_version: impl Fn(&Path) -> Option<String>,
) -> Vec<CliInfo> {
    KNOWN_CLIS
        .iter()
        .map(|spec| {
            let path = find(spec.binary);
            let version = path.as_deref().and_then(&get_version);
            CliInfo {
                id: spec.id.to_string(),
                label: spec.label.to_string(),
                installed: path.is_some(),
                path: path.map(|p| p.to_string_lossy().into_owned()),
                version,
            }
        })
        .collect()
}

/// 探测本机实际安装的 CLI。`which` 会按 PATHEXT 解析 Windows 的 `.cmd` 垫片。
pub fn detect_all() -> Vec<CliInfo> {
    detect_with(|binary| which::which(binary).ok(), run_version)
}

fn run_version(path: &Path) -> Option<String> {
    let mut command = Command::new(path);
    command.arg("--version").stdin(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        // CREATE_NO_WINDOW：避免每探测一个 CLI 就闪一下黑窗
        command.creation_flags(0x0800_0000);
    }
    let output = command.output().ok()?;
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    parse_version(&text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_version_handles_real_outputs() {
        assert_eq!(
            parse_version("grok 1.0.77 (50ca579fa996)").as_deref(),
            Some("1.0.77")
        );
        assert_eq!(
            parse_version("2.1.319 (Claude Code)").as_deref(),
            Some("2.1.319")
        );
        assert_eq!(
            parse_version("codex-cli 0.167.0").as_deref(),
            Some("0.167.0")
        );
        assert_eq!(parse_version("0.2.2-rc.1\n").as_deref(), Some("0.2.2-rc.1"));
        assert_eq!(parse_version("tool v1.2.3,").as_deref(), Some("1.2.3"));
    }

    #[test]
    fn parse_version_returns_none_without_version() {
        assert_eq!(parse_version("command not found"), None);
        assert_eq!(parse_version(""), None);
    }

    #[test]
    fn is_known_matches_the_five_ids() {
        for id in ["claude", "codex", "grok", "dsh", "pi"] {
            assert!(is_known(id), "{id} 应当是已知 CLI");
        }
        assert!(!is_known("gemini"));
        assert!(!is_known(""));
    }

    #[test]
    fn detect_with_reports_installed_and_missing() {
        let found = detect_with(
            |binary| (binary == "codex").then(|| PathBuf::from("C:/bin/codex.cmd")),
            |_path| Some("0.167.0".to_string()),
        );

        assert_eq!(found.len(), 5);
        let ids: Vec<&str> = found.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "grok", "dsh", "pi"]);

        let codex = &found[1];
        assert!(codex.installed);
        assert_eq!(codex.path.as_deref(), Some("C:/bin/codex.cmd"));
        assert_eq!(codex.version.as_deref(), Some("0.167.0"));

        let claude = &found[0];
        assert!(!claude.installed);
        assert_eq!(claude.path, None);
        assert_eq!(claude.version, None);
    }
}
