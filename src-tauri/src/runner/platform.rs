use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::process::{Child, Command};

/// 只接受可核对的 npm 标准入口；用户参数始终作为 argv 传给 node。
pub fn resolve_program(program: &Path, args: &[String], path_env: Option<&std::ffi::OsStr>) -> Result<(PathBuf, Vec<String>), String> {
    if !program.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd")) {
        return Ok((program.to_path_buf(), args.to_vec()));
    }
    let parent = program.parent().ok_or("CLI 启动器没有父目录")?;
    let entry = match program.file_stem().and_then(|s| s.to_str()) {
        Some("claude") => "node_modules/@anthropic-ai/claude-code/cli.js",
        Some("codex") => "node_modules/@openai/codex/bin/codex.js",
        _ => return Err("无法解析该 .cmd 启动器，请使用已接入 CLI 的标准 npm 安装".into()),
    };
    let shim = std::fs::read_to_string(program).map_err(|_| "无法读取 CLI 启动器")?;
    let script = parent.join(entry);
    if !script.is_file() || !shim.replace('\\', "/").contains(entry) {
        return Err("CLI 启动器不符合已验证的 npm 格式，请检查安装".into());
    }
    let local_node = parent.join("node.exe");
    let node = if local_node.is_file() { local_node } else {
        which::which_in("node", path_env, parent).map_err(|_| "未找到 CLI 所需的 node")?
    };
    let mut node_args = vec![script.to_string_lossy().into_owned()];
    node_args.extend_from_slice(args);
    Ok((node, node_args))
}

pub fn configure(command: &mut Command) {
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; command.as_std_mut().process_group(0); }
    #[cfg(windows)]
    { command.creation_flags(0x0800_0000); }
    command.kill_on_drop(true);
}

/// 即使父进程已经退出，也清理同一进程组中的后代。
pub async fn terminate_tree(child: &mut Child, pid: u32) -> bool {
    #[cfg(unix)]
    {
        let signal = |sig| {
            let rc = unsafe { libc::kill(-(pid as i32), sig) };
            if rc == 0 { true } else {
                std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            }
        };
        let term_ok = signal(libc::SIGTERM);
        // 宽限内持续等待，不因父进程先退出而漏掉忽略 TERM 的后代。
        tokio::time::sleep(Duration::from_millis(100)).await;
        let kill_ok = signal(libc::SIGKILL);
        // macOS 对已经消失的进程组也可能返回 EPERM；用独立的进程表核对。
        let group_gone = if kill_ok { true } else {
            let mut probe = Command::new("/bin/ps");
            probe.args(["-axo", "pgid=,stat="]).stdin(std::process::Stdio::null());
            tokio::time::timeout(Duration::from_secs(1), probe.output()).await
                .is_ok_and(|result| result.is_ok_and(|output| {
                    output.status.success() && !String::from_utf8_lossy(&output.stdout).lines().any(|line| {
                        let mut fields = line.split_whitespace();
                        fields.next().and_then(|value| value.parse::<u32>().ok()) == Some(pid)
                            && fields.next().is_some_and(|state| !state.starts_with('Z'))
                    })
                }))
        };
        let _ = child.start_kill();
        let reaped = tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok();
        (term_ok || group_gone) && group_gone && reaped
    }
    #[cfg(windows)]
    {
        let mut command = Command::new("taskkill");
        command.args(["/PID", &pid.to_string(), "/T", "/F"]).creation_flags(0x0800_0000);
        command.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
        let killed = tokio::time::timeout(Duration::from_secs(3), command.status()).await
            .is_ok_and(|result| result.is_ok_and(|status| status.success() || status.code() == Some(128)));
        let _ = child.start_kill();
        killed && tokio::time::timeout(Duration::from_secs(2), child.wait()).await.is_ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn npm_shim_resolution_never_constructs_shell_arguments() {
        let temp = tempfile::tempdir().unwrap();
        let parent = temp.path();
        let script = parent.join("node_modules/@openai/codex/bin/codex.js");
        std::fs::create_dir_all(script.parent().unwrap()).unwrap();
        std::fs::write(&script, "").unwrap();
        std::fs::write(parent.join("node.exe"), "").unwrap();
        let shim = parent.join("codex.cmd");
        std::fs::write(&shim, "\"%_prog%\" \"%dp0%\\node_modules\\@openai\\codex\\bin\\codex.js\" %*").unwrap();
        let args = vec!["exec".into(), "a & echo secret".into()];
        let (program, argv) = resolve_program(&shim, &args, None).unwrap();
        assert_eq!(program, parent.join("node.exe"));
        assert_eq!(argv[0], script.to_string_lossy());
        assert_eq!(&argv[1..], args);
        std::fs::write(&shim, "echo 不认识的脚本").unwrap();
        assert!(resolve_program(&shim, &args, None).is_err());
    }
}
