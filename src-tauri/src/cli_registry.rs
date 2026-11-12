use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::error::{AppError, AppResult};

/// 一个已知 CLI：内部标识、界面名称和可执行文件名。
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnvironmentSource {
    Process,
    LoginShell,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliEnvironment {
    pub path: OsString,
    pub source: EnvironmentSource,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CliInfo {
    pub id: String,
    pub label: String,
    pub installed: bool,
    pub supported: bool,
    pub path: Option<String>,
    pub version: Option<String>,
}

/// 同时供适配器构造命令与 runner 启动进程使用的解析结果。
#[derive(Debug, Clone, PartialEq)]
pub struct ResolvedCli {
    pub info: CliInfo,
    pub program: PathBuf,
    pub path_env: OsString,
    pub environment_source: EnvironmentSource,
}

pub fn is_known(id: &str) -> bool {
    KNOWN_CLIS.iter().any(|spec| spec.id == id)
}

pub fn is_supported(id: &str) -> bool {
    matches!(id, "claude" | "codex" | "grok" | "dsh")
}

/// 应用管理里手动指定的 CLI 程序路径。进程内共享，保存配置和启动时写入。
static PATH_OVERRIDES: Mutex<Vec<(String, PathBuf)>> = Mutex::new(Vec::new());

/// 设置或清除（传 `None`）某个 CLI 的手动路径。
pub fn set_path_override(id: &str, path: Option<PathBuf>) {
    let mut overrides = PATH_OVERRIDES.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    overrides.retain(|(key, _)| key != id);
    if let Some(path) = path { overrides.push((id.to_string(), path)); }
}

fn path_override(id: &str) -> Option<PathBuf> {
    PATH_OVERRIDES.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
        .iter().find(|(key, _)| key == id).map(|(_, path)| path.clone())
}

/// 用手动指定的程序构造解析结果。PATH 前面补上程序所在目录，让 npm 垫片能找到同目录的 node。
fn resolved_from_override(id: &str, program: PathBuf, process_path: Option<OsString>) -> Option<ResolvedCli> {
    let spec = KNOWN_CLIS.iter().find(|spec| spec.id == id)?;
    if !program.is_absolute() || !program.is_file() { return None; }
    let mut dirs: Vec<PathBuf> = program.parent().map(|parent| vec![parent.to_path_buf()]).unwrap_or_default();
    if let Some(path) = &process_path { dirs.extend(std::env::split_paths(path)); }
    let path_env = std::env::join_paths(dirs).ok()?;
    Some(ResolvedCli {
        info: CliInfo {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            installed: true,
            supported: is_supported(spec.id),
            path: Some(program.to_string_lossy().into_owned()),
            version: None,
        },
        program,
        path_env,
        environment_source: EnvironmentSource::Process,
    })
}

/// 从 `--version` 输出里取出第一个以数字开头且含点的版本词。
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

/// 用进程 PATH；缺失时使用调用方提供的有界登录 shell 结果。
pub fn resolve_environment_with(
    process_path: Option<OsString>,
    login_shell: impl FnOnce(Duration) -> Option<OsString>,
) -> Option<CliEnvironment> {
    if let Some(path) = process_path.filter(|path| !path.is_empty()) {
        return Some(CliEnvironment { path, source: EnvironmentSource::Process });
    }
    login_shell(Duration::from_secs(3))
        .filter(|path| !path.is_empty())
        .map(|path| CliEnvironment { path, source: EnvironmentSource::LoginShell })
}

/// 输出写到私有临时文件，避免后代持有管道导致 reader 无界等待。
pub(crate) fn probe_output(mut command: Command, timeout: Duration) -> Option<String> {
    use std::fs::OpenOptions;
    use std::io::{Seek, SeekFrom};
    let path = std::env::temp_dir().join(format!("imates-probe-{}", uuid::Uuid::new_v4()));
    let mut options = OpenOptions::new();
    options.create_new(true).read(true).write(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(&path).ok()?;
    let result = (|| {
        command.stdin(Stdio::null()).stdout(file.try_clone().ok()?).stderr(Stdio::null());
        #[cfg(unix)]
        { use std::os::unix::process::CommandExt; command.process_group(0); }
        #[cfg(windows)]
        { use std::os::windows::process::CommandExt; command.creation_flags(0x0800_0000); }
        let mut child = command.spawn().ok()?;
        let started = Instant::now();
        let success = loop {
            match child.try_wait() {
                Ok(Some(status)) => break status.success(),
                Ok(None) if started.elapsed() < timeout => thread::sleep(Duration::from_millis(10)),
                _ => { let _ = child.kill(); let _ = child.wait(); break false; }
            }
        };
        #[cfg(unix)]
        unsafe { libc::kill(-(child.id() as i32), libc::SIGKILL); }
        if !success { return None; }
        file.seek(SeekFrom::Start(0)).ok()?;
        let mut text = String::new();
        file.by_ref().take(64 * 1024).read_to_string(&mut text).ok()?;
        Some(text)
    })();
    drop(file);
    let _ = std::fs::remove_file(path);
    result
}

fn login_shell_path(timeout: Duration) -> Option<OsString> {
    #[cfg(windows)]
    { let _ = timeout; None }
    #[cfg(not(windows))]
    {
        let shell = std::env::var_os("SHELL").map(PathBuf::from)
            .filter(|path| path.is_absolute()).unwrap_or_else(|| "/bin/sh".into());
        let mut command = Command::new(shell);
        command.args(["-l", "-c", "printf '\\n__IMATES_PATH__%s\\n' \"$PATH\""]);
        let output = probe_output(command, timeout)?;
        let (_, path) = output.rsplit_once("__IMATES_PATH__")?;
        let path = path.lines().next()?.trim();
        (!path.is_empty()).then(|| OsString::from(path))
    }
}

fn find_in_path(binary: &str, path: &OsString) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?;
    which::which_in(binary, Some(path), cwd).ok()
}

fn resolve_cli_with(
    id: &str,
    path: Option<OsString>,
    find: impl FnOnce(&str, &OsString) -> Option<PathBuf>,
) -> Option<ResolvedCli> {
    let spec = KNOWN_CLIS.iter().find(|spec| spec.id == id)?;
    let path_env = path.filter(|path| !path.is_empty())?;
    let program = find(spec.binary, &path_env)?;
    if !program.is_absolute() {
        return None;
    }
    Some(ResolvedCli {
        info: CliInfo {
            id: spec.id.to_string(),
            label: spec.label.to_string(),
            installed: true,
            supported: is_supported(spec.id),
            path: Some(program.to_string_lossy().into_owned()),
            version: None,
        },
        program,
        path_env,
        environment_source: EnvironmentSource::Process,
    })
}

fn resolve_with_fallback(
    id: &str,
    process_path: Option<OsString>,
    login_shell: impl FnOnce(Duration) -> Option<OsString>,
    find: impl Fn(&str, &OsString) -> Option<PathBuf>,
) -> Option<ResolvedCli> {
    if let Some(resolved) = resolve_cli_with(id, process_path, &find) { return Some(resolved); }
    let mut resolved = resolve_cli_with(id, login_shell(Duration::from_secs(3)), find)?;
    resolved.environment_source = EnvironmentSource::LoginShell;
    Some(resolved)
}

pub fn resolve_cli(id: &str) -> AppResult<ResolvedCli> {
    let spec = KNOWN_CLIS.iter().find(|spec| spec.id == id)
        .ok_or_else(|| AppError::Invalid(format!("未知 CLI：{id}")))?;
    if let Some(program) = path_override(id) {
        let mut resolved = resolved_from_override(id, program, std::env::var_os("PATH"))
            .ok_or_else(|| AppError::Invalid(format!("手动指定的 {} 路径不可用，请在应用管理中重新指定", spec.label)))?;
        resolved.info.version = run_version(&resolved.program, &resolved.path_env);
        return Ok(resolved);
    }
    let mut resolved = resolve_with_fallback(id, std::env::var_os("PATH"), login_shell_path, find_in_path)
        .ok_or_else(|| AppError::Invalid(format!("未找到 {}，请检查 PATH 或登录 shell 启动配置", spec.label)))?;
    resolved.info.version = run_version(&resolved.program, &resolved.path_env);
    Ok(resolved)
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
                supported: is_supported(spec.id),
                path: path.map(|path| path.to_string_lossy().into_owned()),
                version,
            }
        })
        .collect()
}

/// 探测本机安装情况。路径解析与后续 runner 使用同一搜索规则。
pub fn detect_all() -> Vec<CliInfo> {
    let process_path = std::env::var_os("PATH");
    let needs_fallback = KNOWN_CLIS.iter().any(|spec| {
        process_path.as_ref().and_then(|path| find_in_path(spec.binary, path)).is_none()
    });
    let fallback = needs_fallback.then(|| login_shell_path(Duration::from_secs(3))).flatten();
    KNOWN_CLIS.iter().map(|spec| {
        detect_one_with(
            spec.id,
            path_override(spec.id),
            process_path.clone(),
            || resolve_with_fallback(spec.id, process_path.clone(), |_| fallback.clone(), find_in_path),
            |resolved| run_version(&resolved.program, &resolved.path_env),
        )
    }).collect()
}

/// 单个 CLI 的探测决策。有手动路径时只看该路径，失效就报未安装，不回退 PATH（与 `resolve_cli` 一致）。
fn detect_one_with(
    id: &str,
    override_path: Option<PathBuf>,
    process_path: Option<OsString>,
    fallback: impl FnOnce() -> Option<ResolvedCli>,
    get_version: impl Fn(&ResolvedCli) -> Option<String>,
) -> CliInfo {
    let (label, supported) = KNOWN_CLIS.iter().find(|spec| spec.id == id)
        .map(|spec| (spec.label, is_supported(spec.id))).unwrap_or((id, false));
    let resolved = match &override_path {
        Some(program) => resolved_from_override(id, program.clone(), process_path),
        None => fallback(),
    };
    match resolved {
        Some(resolved) => { let version = get_version(&resolved); CliInfo { version, ..resolved.info } }
        None => CliInfo {
            id: id.into(), label: label.into(), installed: false, supported,
            path: override_path.map(|path| path.to_string_lossy().into_owned()), version: None,
        },
    }
}

/// 取版本号时实际启动的程序。`.cmd` 启动器先解析成真实入口，不经过 cmd.exe；解析不了就不取版本。
fn version_command(program: &Path, path_env: &OsString) -> Option<(PathBuf, Vec<String>)> {
    crate::runner::platform::resolve_program(program, &["--version".to_string()], Some(path_env.as_os_str())).ok()
}

fn run_version(program: &Path, path_env: &OsString) -> Option<String> {
    let (program, args) = version_command(program, path_env)?;
    let mut command = Command::new(program);
    command.args(args).env("PATH", path_env);
    parse_version(&probe_output(command, Duration::from_secs(3))?)
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
        let ids: Vec<&str> = found.iter().map(|cli| cli.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "grok", "dsh", "pi"]);

        let codex = &found[1];
        assert!(codex.installed);
        assert!(codex.supported);
        assert_eq!(codex.path.as_deref(), Some("C:/bin/codex.cmd"));
        assert_eq!(codex.version.as_deref(), Some("0.167.0"));

        let claude = &found[0];
        assert!(!claude.installed);
        assert!(claude.supported);
        assert_eq!(claude.path, None);
        assert_eq!(claude.version, None);
    }

    #[test]
    fn supported_clis_preserve_display_order_and_leave_only_pi_pending() {
        let clis = detect_with(|_| None, |_| None);
        let ids: Vec<&str> = clis.iter().map(|cli| cli.id.as_str()).collect();
        assert_eq!(ids, ["claude", "codex", "grok", "dsh", "pi"]);
        assert_eq!(
            clis.iter().map(|cli| cli.supported).collect::<Vec<_>>(),
            [true, true, true, true, false]
        );
    }

    #[test]
    fn process_path_wins_and_login_shell_is_only_used_as_fallback() {
        let process = OsString::from("/usr/bin:/bin");
        let environment = resolve_environment_with(Some(process.clone()), |_| {
            panic!("进程 PATH 可用时不应调用登录 shell")
        })
        .unwrap();
        assert_eq!(environment.path, process);
        assert_eq!(environment.source, EnvironmentSource::Process);

        let environment = resolve_environment_with(None, |timeout| {
            assert_eq!(timeout, Duration::from_secs(3));
            Some(OsString::from("/custom/bin:/usr/bin"))
        })
        .unwrap();
        assert_eq!(environment.path, "/custom/bin:/usr/bin");
        assert_eq!(environment.source, EnvironmentSource::LoginShell);

        assert!(resolve_environment_with(Some("".into()), |_| None).is_none());
    }

    #[test]
    fn resolve_cli_uses_the_absolute_executable_from_its_path_snapshot() {
        let program = std::path::PathBuf::from(if cfg!(windows) { r"C:\custom\bin\claude" } else { "/custom/bin/claude" });
        let resolved = resolve_cli_with(
            "claude",
            Some("/custom/bin:/usr/bin".into()),
            |binary, path| {
                assert_eq!(binary, "claude");
                assert_eq!(path, "/custom/bin:/usr/bin");
                Some(program.clone())
            },
        )
        .unwrap();
        assert_eq!(resolved.program, std::path::PathBuf::from(if cfg!(windows) { r"C:\custom\bin\claude" } else { "/custom/bin/claude" }));
        assert_eq!(resolved.path_env, "/custom/bin:/usr/bin");
        assert!(resolved.program.is_absolute());
    }
    #[test]
    fn existing_but_incomplete_path_falls_back_without_changing_global_environment() {
        let resolved = resolve_with_fallback("codex", Some("/usr/bin:/bin".into()), |_| {
            Some("/custom/bin:/usr/bin".into())
        }, |binary, path| {
            assert_eq!(binary, "codex");
            (path == "/custom/bin:/usr/bin").then(|| {
                std::path::PathBuf::from(if cfg!(windows) { r"C:\custom\bin\codex" } else { "/custom/bin/codex" })
            })
        }).unwrap();
        assert_eq!(resolved.environment_source, EnvironmentSource::LoginShell);
        assert_eq!(resolved.path_env, "/custom/bin:/usr/bin");
    }

    #[test]
    #[cfg(unix)]
    fn probe_timeout_and_inherited_output_are_bounded() {
        let start = Instant::now();
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & wait"]);
        assert!(probe_output(command, Duration::from_millis(80)).is_none());
        assert!(start.elapsed() < Duration::from_secs(2));
        let mut command = Command::new("/bin/sh");
        command.args(["-c", "sleep 10 & printf '0.1.2'"]);
        assert_eq!(probe_output(command, Duration::from_secs(1)).as_deref(), Some("0.1.2"));
        assert!(start.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn version_probe_resolves_cmd_shims_instead_of_skipping_them() {
        let dir = tempfile::tempdir().unwrap();
        let path_env = OsString::from(dir.path());
        let plain = dir.path().join("grok.exe");
        assert_eq!(version_command(&plain, &path_env), Some((plain.clone(), vec!["--version".to_string()])));

        let node = dir.path().join("node.exe");
        let script = dir.path().join("bin.js");
        std::fs::write(&node, "").unwrap();
        std::fs::write(&script, "").unwrap();
        let shim = dir.path().join("dsh.cmd");
        std::fs::write(&shim, format!("@echo off\r\n\"{}\" \"{}\" %*\r\n", node.display(), script.display())).unwrap();
        assert_eq!(version_command(&shim, &path_env), Some((node, vec![script.to_string_lossy().into_owned(), "--version".to_string()])));

        std::fs::write(&shim, "echo 不认识的脚本").unwrap();
        assert_eq!(version_command(&shim, &path_env), None);
    }

    #[test]
    fn override_resolves_an_existing_absolute_file_and_prepends_its_directory() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join(if cfg!(windows) { "claude.exe" } else { "claude" });
        std::fs::write(&program, b"").unwrap();
        let resolved = resolved_from_override("claude", program.clone(), Some("/usr/bin".into())).unwrap();
        assert!(resolved.info.installed && resolved.info.supported);
        assert_eq!(resolved.program, program);
        assert_eq!(resolved.info.path.as_deref(), Some(program.to_string_lossy().as_ref()));
        let first = std::env::split_paths(&resolved.path_env).next().unwrap();
        assert_eq!(first, dir.path());
    }

    #[test]
    fn override_rejects_relative_or_missing_files() {
        assert!(resolved_from_override("claude", PathBuf::from("claude"), None).is_none());
        let missing = std::env::temp_dir().join("imates-no-such-cli-binary");
        assert!(resolved_from_override("claude", missing, None).is_none());
        assert!(resolved_from_override("unknown", std::env::temp_dir(), None).is_none());
    }

    #[test]
    fn set_path_override_replaces_and_clears() {
        // pi 没有适配器，其他测试不会用到它的覆盖
        set_path_override("pi", Some(PathBuf::from("/a")));
        set_path_override("pi", Some(PathBuf::from("/b")));
        assert_eq!(path_override("pi"), Some(PathBuf::from("/b")));
        set_path_override("pi", None);
        assert_eq!(path_override("pi"), None);
    }

    #[test]
    fn detect_with_override_uses_an_existing_file() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join(if cfg!(windows) { "claude.exe" } else { "claude" });
        std::fs::write(&program, b"").unwrap();
        let info = detect_one_with("claude", Some(program.clone()), None,
            || panic!("有手动路径时不应回退 PATH"), |_| Some("9.9.9".into()));
        assert!(info.installed);
        assert_eq!(info.path.as_deref(), Some(program.to_string_lossy().as_ref()));
        assert_eq!(info.version.as_deref(), Some("9.9.9"));
    }

    #[test]
    fn detect_with_missing_override_reports_not_installed_without_falling_back() {
        let missing = std::env::temp_dir().join("imates-no-such-cli-binary");
        let calls = std::cell::Cell::new(0);
        let info = detect_one_with("claude", Some(missing.clone()), None, || { calls.set(calls.get() + 1); None },
            |_| panic!("路径失效时不应取版本"));
        assert!(!info.installed && info.supported);
        assert_eq!(info.path.as_deref(), Some(missing.to_string_lossy().as_ref()));
        assert_eq!(info.version, None);
        assert_eq!(calls.get(), 0, "覆盖失效时不能回退 PATH");
    }

    #[test]
    fn detect_without_override_uses_the_fallback() {
        let dir = tempfile::tempdir().unwrap();
        let program = dir.path().join("claude.exe");
        let resolved = ResolvedCli {
            info: CliInfo { id: "claude".into(), label: "Claude Code".into(), installed: true, supported: true,
                path: Some(program.to_string_lossy().into_owned()), version: None },
            program: program.clone(), path_env: OsString::new(), environment_source: EnvironmentSource::Process,
        };
        let info = detect_one_with("claude", None, None, || Some(resolved), |_| Some("1.0.0".into()));
        assert!(info.installed);
        assert_eq!(info.version.as_deref(), Some("1.0.0"));
        let none = detect_one_with("codex", None, None, || None, |_| None);
        assert!(!none.installed && none.path.is_none());
    }
}
