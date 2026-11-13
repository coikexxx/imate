pub mod platform;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt};
use tokio::process::Command;
use tokio::sync::{mpsc, watch};
use tokio::time::Instant;
use crate::adapters::{AgentEvent, CliAdapter, RunMode, TurnRequest, Usage};

#[derive(Debug, Clone)]
pub struct RunnerConfig {
    pub model_idle_timeout: Duration,
    pub discuss_tool_timeout: Duration,
    pub execute_tool_timeout: Duration,
    pub discuss_timeout: Duration,
    pub execute_timeout: Duration,
    pub stdout_close_grace: Duration,
}
impl Default for RunnerConfig {
    fn default() -> Self { Self { model_idle_timeout: Duration::from_secs(300),
        discuss_tool_timeout: Duration::from_secs(600), execute_tool_timeout: Duration::from_secs(1800),
        discuss_timeout: Duration::from_secs(600), execute_timeout: Duration::from_secs(1800),
        stdout_close_grace: Duration::from_secs(30) } }
}

#[derive(Debug, Clone)]
pub struct RunOutcome {
    pub status: String,
    pub session_id: Option<String>,
    pub final_text: String,
    pub partial_text: String,
    pub usage: Option<Usage>,
    pub exit_code: Option<i32>,
    pub error: Option<String>,
    pub tool_seen: bool,
    pub cleanup_complete: bool,
    /// 这次回合里识别到的第一条权限拒绝。任务据此转为受阻，而不是当成完成。
    pub denial: Option<crate::adapters::Denial>,
}
impl RunOutcome {
    fn failed(message: String) -> Self { Self { status: "failed".into(), session_id: None,
        final_text: String::new(), partial_text: String::new(), usage: None, exit_code: None,
        error: Some(message), tool_seen: false, cleanup_complete: true, denial: None } }
}

enum StreamEvent { Bytes(bool, Vec<u8>), Eof(bool), ReadError(bool), StdinDone(Result<(), std::io::Error>) }
async fn read_stream<R: AsyncRead + Unpin>(mut stream: R, stdout: bool, tx: mpsc::Sender<StreamEvent>) {
    let mut buffer = [0u8; 8192];
    loop {
        match stream.read(&mut buffer).await {
            Ok(0) => { let _ = tx.send(StreamEvent::Eof(stdout)).await; return; }
            Ok(n) => if tx.send(StreamEvent::Bytes(stdout, buffer[..n].to_vec())).await.is_err() { return; },
            Err(_) => { let _ = tx.send(StreamEvent::ReadError(stdout)).await; return; }
        }
    }
}

/// 诊断只记录类型与长度，不落盘原始行、工具参数、stderr 或凭据。
fn log_diagnostic(path: Option<&PathBuf>, line: &[u8]) {
    use std::io::Write;
    let Some(path) = path else { return; };
    let kind = serde_json::from_slice::<serde_json::Value>(line).ok()
        .and_then(|v| v["type"].as_str().map(str::to_owned)).unwrap_or_else(|| "非 JSON".into());
    let kind: String = if kind == "非 JSON" { kind } else {
        kind.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '_').take(64).collect()
    };
    if let Some(parent) = path.parent() { let _ = std::fs::create_dir_all(parent); }
    // 每个 run 最多记录 256 KiB 元信息。
    if std::fs::metadata(path).is_ok_and(|m| m.len() > 256 * 1024) { return; }
    let mut options = std::fs::OpenOptions::new(); options.create(true).append(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    if let Ok(mut file) = options.open(path) { let _ = writeln!(file, "未归一事件 type={kind} bytes={}", line.len()); }
}

/// 提示词文件随作用域删除，任何提前返回都不会把它留在磁盘上。
struct PromptFile(Option<PathBuf>);
impl Drop for PromptFile {
    fn drop(&mut self) {
        if let Some(path) = self.0.take() { let _ = std::fs::remove_file(path); }
    }
}

/// 只新建，不覆盖已有文件。Unix 下只有当前用户可读写。
fn write_prompt_file(path: &std::path::Path, prompt: &str) -> std::io::Result<()> {
    use std::io::Write;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    { use std::os::unix::fs::OpenOptionsExt; options.mode(0o600); }
    let mut file = options.open(path)?;
    let result = file.write_all(prompt.as_bytes()).and_then(|_| file.flush());
    drop(file);
    // 文件已新建但写入失败时，也要删掉不完整的提示词。
    if result.is_err() { let _ = std::fs::remove_file(path); }
    result
}

pub async fn run_turn(
    adapter: Box<dyn CliAdapter>, request: TurnRequest, cancellation: watch::Receiver<bool>,
    on_event: impl FnMut(AgentEvent),
) -> RunOutcome {
    run_turn_with_config(adapter, request, cancellation, on_event, RunnerConfig::default()).await
}
pub async fn run_turn_with_config(
    adapter: Box<dyn CliAdapter>, request: TurnRequest, cancellation: watch::Receiver<bool>,
    on_event: impl FnMut(AgentEvent), config: RunnerConfig,
) -> RunOutcome {
    run_turn_managed(adapter, request, cancellation, on_event, config, None, |_| Ok(())).await
}

pub async fn run_turn_managed(
    mut adapter: Box<dyn CliAdapter>, request: TurnRequest, cancellation: watch::Receiver<bool>,
    mut on_event: impl FnMut(AgentEvent), config: RunnerConfig, diagnostic_path: Option<PathBuf>,
    mut on_started: impl FnMut(u32) -> Result<(), String>,
) -> RunOutcome {
    if *cancellation.borrow() { let mut out = RunOutcome::failed("回合已停止".into()); out.status = "cancelled".into(); return out; }
    let spec = adapter.build_command(&request);
    let mut _support_files = Vec::new();
    for file in &spec.support_files {
        if let Err(error) = write_prompt_file(&file.path, &file.content) {
            return RunOutcome::failed(format!("无法准备 CLI 辅助文件：{error}"));
        }
        _support_files.push(PromptFile(Some(file.path.clone())));
    }
    let _prompt_file = match &spec.prompt_file {
        Some(path) => match write_prompt_file(path, &request.prompt) {
            Ok(()) => PromptFile(Some(path.clone())),
            Err(e) => return RunOutcome::failed(format!("无法写入提示词文件：{e}")),
        },
        None => PromptFile(None),
    };
    let (program, args) = match platform::resolve_program(&spec.program, &spec.args, spec.path_env.as_deref()) {
        Ok(command) => command, Err(e) => return RunOutcome::failed(e),
    };
    let mut command = Command::new(program);
    command.args(args).current_dir(&request.cwd).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
    if let Some(path) = spec.path_env { command.env("PATH", path); }
    for (key, value) in adapter.environment() {
        if let Some(value) = value { command.env(key, value); } else { command.env_remove(key); }
    }
    platform::configure(&mut command);
    let mut child = match command.spawn() { Ok(child) => child, Err(e) => return RunOutcome::failed(format!("无法启动 CLI：{e}")) };
    let pid = child.id().expect("刚启动的进程有 PID");
    // 必须在写入提示词、子进程创建后代之前挂上进程组。观察在独立线程，不占用输出排空。
    let _tree = platform::track(pid);
    let mut out = RunOutcome::failed(String::new()); out.error = None;
    out.session_id = request.session_id.clone();
    if let Err(e) = on_started(pid) {
        out.error = Some(e); out.cleanup_complete = platform::terminate_tree(&mut child, pid).await; return out;
    }
    let (tx, mut rx) = mpsc::channel(64);
    let stdout = tokio::spawn(read_stream(child.stdout.take().unwrap(), true, tx.clone()));
    let stderr = tokio::spawn(read_stream(child.stderr.take().unwrap(), false, tx.clone()));
    let mut input = child.stdin.take().unwrap();
    let stdin = tokio::spawn(async move {
        let result = async { if let Some(prompt) = spec.stdin { input.write_all(prompt.as_bytes()).await?; } input.shutdown().await }.await;
        drop(input); let _ = tx.send(StreamEvent::StdinDone(result)).await;
    });
    let start = Instant::now(); let mut last_output = start;
    let mut tool_start: Option<Instant> = None;
    let mut stdout_closed: Option<Instant> = None;
    let mut stderr_closed = false;
    let mut exited: Option<Instant> = None;
    let mut completed = false;
    let mut latched: Option<&str> = None;
    let mut line = Vec::new(); let mut dropping_line = false;
    let mut stderr_tail = Vec::new();
    let total_timeout = match request.mode { RunMode::Discuss => config.discuss_timeout, RunMode::Execute => config.execute_timeout };
    let tool_timeout = match request.mode { RunMode::Discuss => config.discuss_tool_timeout, RunMode::Execute => config.execute_tool_timeout };
    loop {
        if *cancellation.borrow() { latched = Some("cancelled"); out.error = Some("回合已停止，文件修改不会自动撤销".into()); }
        if latched.is_some() { break; }
        let now = Instant::now();
        if exited.is_none() {
            match child.try_wait() {
                Ok(Some(status)) => { out.exit_code = status.code(); exited = Some(now); }
                Err(e) => { latched = Some("failed"); out.error = Some(format!("读取进程状态失败：{e}")); break; }
                _ => {}
            }
        }
        if stdout_closed.is_some() && stderr_closed && exited.is_some() { break; }
        if exited.is_some_and(|at| now.duration_since(at) >= config.stdout_close_grace) ||
            (exited.is_none() && stdout_closed.is_some_and(|at| now.duration_since(at) >= config.stdout_close_grace)) {
            latched = Some("failed"); out.error = Some("CLI 退出或输出关闭后仍有进程/管道未结束，已清理进程树".into()); break;
        }
        if now.duration_since(start) >= total_timeout {
            latched = Some("timeout"); out.error = Some("回合超过总时长，已停止".into()); break;
        }
        if let Some(at) = tool_start {
            if now.duration_since(at) >= tool_timeout { latched = Some("timeout"); out.error = Some("工具运行超过时限，已停止".into()); break; }
        } else if exited.is_none() && stdout_closed.is_none() && now.duration_since(last_output) >= config.model_idle_timeout {
            latched = Some("timeout"); out.error = Some("CLI 长时间没有输出，已停止".into()); break;
        }
        let event = tokio::select! { item = rx.recv() => item, _ = tokio::time::sleep(Duration::from_millis(10)) => continue };
        let mut lines = Vec::new();
        match event {
            Some(StreamEvent::Bytes(is_stdout, bytes)) => {
                last_output = Instant::now();
                if is_stdout {
                    for byte in bytes {
                        if byte == b'\n' {
                            if dropping_line { log_diagnostic(diagnostic_path.as_ref(), b"overlong line ignored"); }
                            else { lines.push(std::mem::take(&mut line)); }
                            line.clear(); dropping_line = false;
                        } else if !dropping_line {
                            if line.len() < 8 * 1024 * 1024 { line.push(byte); }
                            else { line.clear(); dropping_line = true; }
                        }
                    }
                } else {
                    stderr_tail.extend(bytes);
                    if stderr_tail.len() > 8192 { stderr_tail.drain(..stderr_tail.len() - 8192); }
                }
            }
            Some(StreamEvent::Eof(true)) => { stdout_closed = Some(Instant::now()); if !line.is_empty() { lines.push(std::mem::take(&mut line)); } }
            Some(StreamEvent::Eof(false)) => stderr_closed = true,
            Some(StreamEvent::ReadError(is_stdout)) => { latched = Some("failed"); out.error = Some(format!("读取 CLI {} 失败", if is_stdout { "stdout" } else { "stderr" })); }
            Some(StreamEvent::StdinDone(Err(e))) => { latched = Some("failed"); out.error = Some(format!("写入 CLI 提示词失败：{e}")); }
            _ => {}
        }
        for bytes in lines {
            if latched.is_some() { break; }
            let events = adapter.parse_line(&String::from_utf8_lossy(&bytes));
            if events.is_empty() { log_diagnostic(diagnostic_path.as_ref(), &bytes); }
            for event in events {
                if latched.is_some() { break; }
                match &event {
                    AgentEvent::SessionStarted { session_id } => out.session_id = Some(session_id.clone()),
                    // 模型名由各调用方按需写入运行记录，不进入回合结果。
                    AgentEvent::ModelReported { .. } => {}
                    AgentEvent::TextDelta { text } => out.partial_text.push_str(text),
                    AgentEvent::ToolUse { .. } => out.tool_seen = true,
                    AgentEvent::ToolState { active } => {
                        if *active { out.tool_seen = true; tool_start.get_or_insert(Instant::now()); }
                        else { tool_start = None; last_output = Instant::now(); }
                    }
                    AgentEvent::ToolDenied { tool, detail } => {
                        out.tool_seen = true;
                        if out.denial.is_none() {
                            out.denial = Some(crate::adapters::Denial { tool: tool.clone(), detail: detail.clone() });
                        }
                    }
                    AgentEvent::TurnCompleted { final_text, usage } => { completed = true; out.final_text = final_text.clone(); out.usage = usage.clone(); }
                    AgentEvent::Error { message } => { latched = Some("failed"); out.error = Some(message.clone()); }
                }
                on_event(event);
            }
        }
    }
    let finished_cleanly = latched.is_none() && completed && out.exit_code == Some(0);
    out.cleanup_complete = if finished_cleanly { true } else { platform::terminate_tree(&mut child, pid).await };
    // 等待取消完成，确保不留下读写任务或持有管道的 future。
    for task in [stdout, stderr, stdin] { task.abort(); let _ = task.await; }
    out.status = latched.unwrap_or(if completed && out.exit_code == Some(0) { "ok" } else { "failed" }).into();
    if !out.cleanup_complete {
        out.status = "failed".into();
        let cleanup = "进程树清理未完成，请人工确认后再运行";
        out.error = Some(out.error.take().map(|error| format!("{error}；{cleanup}")).unwrap_or_else(|| cleanup.into()));
    }
    if out.status != "ok" && out.error.is_none() {
        let exit = out.exit_code.map(|c| c.to_string()).unwrap_or_else(|| "未知".into());
        // stderr 原文可能含隐私，不写入 error；只把“会话不存在”这类可识别的原因换成固定说明，供重建判断使用。
        out.error = Some(if crate::chat::mentions_missing_session(&String::from_utf8_lossy(&stderr_tail)) {
            format!("CLI 报告续接的会话不存在（退出码 {exit}，原文未写入日志）")
        } else {
            format!("CLI 未正常完成（退出码 {exit}，stderr 尾部 {} 字节，诊断内容未写入日志）", stderr_tail.len())
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{FakeAdapter, RunMode, TurnRequest};
    use std::time::Duration;

    #[test]
    #[ignore = "仅供 runner 在隔离子进程中调用"]
    fn subprocess_helper() {
        use std::io::{Read, Write};
        let mut input = String::new();
        std::io::stdin().read_to_string(&mut input).unwrap();
        if input.is_empty() { input = std::fs::read_to_string("prompt-under-test.md").unwrap_or_default(); }
        let Some((scenario, text)) = input.split_once('\n') else { return; };
        let event = |kind: &str, text: &str| println!("{}", serde_json::json!({"type":kind,"text":text}));
        match scenario {
            "proxy_env" => {
                let vars: std::collections::BTreeMap<_, _> = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
                    .into_iter().map(|key| (key, std::env::var(key).ok())).collect();
                event("complete", &serde_json::to_string(&vars).unwrap());
            }
            "echo" => { event("text", text); event("complete", "已完成"); }
            "completed_but_failed" => { event("complete", "已输出"); std::process::exit(7); }
            "error_then_complete" => { event("text", "部分回答"); event("error", "致命错误"); event("complete", "迟到成功"); }
            "silent" => std::thread::sleep(Duration::from_secs(30)),
            "partial" => { event("text", "部分回答"); std::io::stdout().flush().unwrap(); std::thread::sleep(Duration::from_secs(30)); }
            "flood" => { for _ in 0..10000 { eprintln!("诊断持续输出"); } event("complete", "已完成"); }
            "flood_fail" => { for _ in 0..10000 { eprintln!("stderr 中的私密字节"); } event("complete", "完成事件不能掩盖非零退出"); std::process::exit(9); }
            "missing_session" => {
                eprintln!("Error: thread/resume: thread/resume failed: no rollout found for thread id 0000-私密线程 (code -32600)");
                std::process::exit(1);
            }
            "unknown" => { println!("CLI 私密原始输出不应写入诊断日志"); event("complete", "已完成"); }
            "long_line" => {
                let line = serde_json::to_string(&serde_json::json!({"type":"text","text":"长".repeat(150000)})).unwrap();
                for chunk in line.as_bytes().chunks(2000) { std::io::stdout().write_all(chunk).unwrap(); std::io::stdout().flush().unwrap(); std::thread::sleep(Duration::from_millis(2)); }
                println!(); event("complete", "已完成");
            }
            "tool" | "tool_end" | "tool_never_ends" => {
                println!("{{\"type\":\"tool\",\"active\":true}}"); std::io::stdout().flush().unwrap();
                std::thread::sleep(Duration::from_millis(if scenario == "tool_never_ends" { 1500 } else { 250 }));
                if scenario == "tool_end" {
                    println!("{{\"type\":\"tool\",\"active\":false}}"); std::io::stdout().flush().unwrap(); std::thread::sleep(Duration::from_secs(30));
                }
                event("complete", "工具完成");
            }
            #[cfg(unix)]
            "eof" => { unsafe { libc::close(1); } std::thread::sleep(Duration::from_secs(30)); }
            #[cfg(windows)]
            "eof" => {
                unsafe {
                    unsafe extern "system" {
                        fn GetStdHandle(kind: u32) -> *mut std::ffi::c_void;
                        fn CloseHandle(handle: *mut std::ffi::c_void) -> i32;
                    }
                    CloseHandle(GetStdHandle(-11i32 as u32));
                }
                std::thread::sleep(Duration::from_secs(30));
            }
            #[cfg(unix)]
            "tree" | "orphan_pipe" => {
                std::fs::write("parent-pid", std::process::id().to_string()).unwrap();
                let mut child = std::process::Command::new("/bin/sh").args(["-c", "echo $$ >> pids; /bin/sh -c 'echo $$ >> pids; sleep 30' & wait"]).spawn().unwrap();
                if scenario == "orphan_pipe" { event("complete", "父进程退出"); std::process::exit(0); }
                event("text", "进程树已启动"); std::io::stdout().flush().unwrap(); let _ = child.wait();
            }
            #[cfg(windows)]
            "tree" | "orphan_pipe" => {
                std::fs::write("parent-pid", std::process::id().to_string()).unwrap();
                let mut child = std::process::Command::new("cmd.exe");
                child.args(["/D", "/C", "ping -n 40 127.0.0.1 >NUL"]);
                child.stdin(std::process::Stdio::null());
                let mut child = child.spawn().unwrap();
                let child_pid = child.id();
                let started = std::time::Instant::now();
                let mut descendants = Vec::new();
                while started.elapsed() < Duration::from_secs(3) {
                    descendants = descendant_pids(child_pid);
                    if !descendants.is_empty() { break; }
                    std::thread::sleep(Duration::from_millis(30));
                }
                let mut lines = vec![child_pid.to_string()];
                lines.extend(descendants.iter().map(u32::to_string));
                std::fs::write("pids", lines.join("\n")).unwrap();
                if scenario == "orphan_pipe" { event("complete", "父进程退出"); std::process::exit(0); }
                event("text", "进程树已启动"); std::io::stdout().flush().unwrap(); let _ = child.wait();
            }
            _ => panic!("未知测试场景"),
        }
    }

    fn config() -> RunnerConfig {
        RunnerConfig { model_idle_timeout: Duration::from_millis(180), discuss_tool_timeout: Duration::from_millis(800),
            execute_tool_timeout: Duration::from_millis(800), discuss_timeout: Duration::from_secs(3),
            execute_timeout: Duration::from_secs(3), stdout_close_grace: Duration::from_millis(150) }
    }
    async fn run(scenario: &str, cancel_after: Option<Duration>, cfg: RunnerConfig) -> (RunOutcome, tempfile::TempDir) {
        let folder = tempfile::tempdir().unwrap();
        let request = TurnRequest { prompt: format!("{scenario}\n- 多行\n下一行"), session_id: None, cwd: folder.path().into(), mode: RunMode::Execute, model: None };
        let adapter = FakeAdapter::new(std::env::current_exe().unwrap(), vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]);
        let (sender, cancellation) = tokio::sync::watch::channel(false);
        if let Some(delay) = cancel_after { tokio::spawn(async move { tokio::time::sleep(delay).await; let _ = sender.send(true); }); }
        let outcome = run_turn_with_config(Box::new(adapter), request, cancellation, |_| {}, cfg).await;
        (outcome, folder)
    }
    #[tokio::test]
    async fn proxy_overrides_reach_only_child_and_direct_mode_removes_them() {
        let before: Vec<_> = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"].map(std::env::var_os).into();
        for manual in [true, false] {
            let folder = tempfile::tempdir().unwrap();
            let request = TurnRequest { prompt: "proxy_env\n检查子进程环境".into(), session_id: None,
                cwd: folder.path().into(), mode: RunMode::Execute, model: None };
            let inner = FakeAdapter::new(std::env::current_exe().unwrap(), vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]);
            let proxy = if manual { crate::proxy::manual_environment(&crate::proxy::ProxySettings {
                mode: crate::proxy::ProxyMode::Manual, url: "http://127.0.0.1:7890".into(),
            }).unwrap() } else { crate::proxy::ProxyEnvironment::default() };
            let adapter = crate::proxy::ProxyAdapter::new(Box::new(inner), crate::proxy::overrides(&proxy));
            let (_sender, cancellation) = tokio::sync::watch::channel(false);
            let out = run_turn(Box::new(adapter), request, cancellation, |_| {}).await;
            assert_eq!(out.status, "ok");
            let vars: serde_json::Value = serde_json::from_str(&out.final_text).unwrap();
            for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
                if manual { assert_eq!(vars[key], "http://127.0.0.1:7890"); } else { assert!(vars[key].is_null()); }
            }
        }
        assert_eq!(before, ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"].map(std::env::var_os));
    }
    #[tokio::test]
    async fn real_process_preserves_stdin_and_requires_zero_exit_and_completion() {
        let (out, _) = run("echo", None, config()).await;
        assert_eq!(out.status, "ok"); assert_eq!(out.partial_text, "- 多行\n下一行"); assert_eq!(out.final_text, "已完成");
        assert_eq!(run("completed_but_failed", None, config()).await.0.status, "failed");
        let (out, _) = run("error_then_complete", None, config()).await;
        assert_eq!(out.status, "failed"); assert_eq!(out.partial_text, "部分回答"); assert!(out.error.unwrap().contains("致命错误"));
    }
    #[tokio::test]
    async fn cancellation_and_idle_timeout_preserve_partial_and_never_revive() {
        let (out, _) = run("partial", Some(Duration::from_millis(100)), config()).await;
        assert_eq!(out.status, "cancelled"); assert_eq!(out.partial_text, "部分回答");
        assert_eq!(run("silent", None, config()).await.0.status, "timeout");
    }
    #[tokio::test]
    async fn stderr_and_long_lines_do_not_block_or_trigger_idle() {
        let flood = run("flood", None, config()).await.0;
        assert_eq!(flood.status, "ok", "exit={:?} err={:?}", flood.exit_code, flood.error);
        let failed = run("flood_fail", None, config()).await.0;
        assert_eq!(failed.status, "failed");
        assert!(failed.error.unwrap().contains("尾部 8192 字节"));
        let (out, _) = run("long_line", None, config()).await;
        assert_eq!(out.status, "ok"); assert_eq!(out.partial_text.chars().count(), 150000);
    }
    #[tokio::test]
    async fn stderr_missing_session_is_reported_without_raw_text() {
        let (out, _) = run("missing_session", None, config()).await;
        assert_eq!(out.status, "failed");
        let error = out.error.clone().unwrap();
        assert!(!error.contains("私密线程") && !error.contains("rollout"), "不写入 stderr 原文：{error}");
        assert!(crate::chat::should_rebuild_session(&out, true, false), "会话不存在应当允许重建：{error}");
        let failed = run("flood_fail", None, config()).await.0;
        assert!(!crate::chat::should_rebuild_session(&failed, true, false));
    }
    #[tokio::test]
    async fn tools_have_separate_timeout_and_switch_back_to_model_idle() {
        assert_eq!(run("tool", None, config()).await.0.status, "ok");
        assert_eq!(run("tool_end", None, config()).await.0.status, "timeout");
        assert_eq!(run("tool_never_ends", None, config()).await.0.status, "timeout");
        let mut cfg = config(); cfg.execute_timeout = Duration::from_millis(120);
        assert_eq!(run("tool", None, cfg).await.0.status, "timeout");
    }
    #[tokio::test]
    async fn diagnostics_keep_metadata_without_raw_cli_text() {
        let folder = tempfile::tempdir().unwrap();
        let request = TurnRequest { prompt: "unknown\n提示词".into(), session_id: None,
            cwd: folder.path().into(), mode: RunMode::Execute, model: None };
        let adapter = FakeAdapter::new(std::env::current_exe().unwrap(), vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]);
        let (_sender, cancellation) = tokio::sync::watch::channel(false);
        let path = folder.path().join("诊断.log");
        let outcome = run_turn_managed(Box::new(adapter), request, cancellation, |_| {}, config(), Some(path.clone()), |_| Ok(())).await;
        assert_eq!(outcome.status, "ok");
        let log = std::fs::read_to_string(path).unwrap();
        assert!(log.contains("type=非 JSON"));
        assert!(log.contains("bytes="));
        assert!(!log.contains("私密原始输出"));
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn eof_and_inherited_pipes_have_bounded_cleanup() {
        let now = std::time::Instant::now();
        assert_eq!(run("eof", None, config()).await.0.status, "failed");
        let (out, folder) = run("orphan_pipe", None, config()).await;
        assert_eq!(out.status, "failed"); assert!(now.elapsed() < Duration::from_secs(3));
        assert_tree_gone(folder.path());
        let (out, folder) = run("tree", Some(Duration::from_millis(150)), config()).await;
        assert_eq!(out.status, "cancelled"); assert_tree_gone(folder.path());
    }
    #[tokio::test]
    #[cfg(windows)]
    async fn windows_process_tree_cancel_and_orphan_pipe_are_cleaned() {
        let now = std::time::Instant::now();
        assert_eq!(run("eof", None, config()).await.0.status, "failed");
        let (out, folder) = run("orphan_pipe", None, config()).await;
        assert_eq!(out.status, "failed");
        assert!(out.cleanup_complete, "父进程退出后仍须确认后代已被清理");
        assert_tree_gone(folder.path());
        let folder = tempfile::tempdir().unwrap();
        let pids = folder.path().join("pids");
        let request = TurnRequest { prompt: "tree\n- 多行\n下一行".into(), session_id: None, cwd: folder.path().into(), mode: RunMode::Execute, model: None };
        let adapter = FakeAdapter::new(std::env::current_exe().unwrap(), vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]);
        let (sender, cancellation) = tokio::sync::watch::channel(false);
        let signal = pids.clone();
        tokio::spawn(async move {
            for _ in 0..100 {
                if signal.is_file() { break; }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            let _ = sender.send(true);
        });
        let outcome = run_turn_with_config(Box::new(adapter), request, cancellation, |_| {}, config()).await;
        assert_eq!(outcome.status, "cancelled");
        assert!(outcome.cleanup_complete);
        assert_tree_gone(folder.path());
        assert!(now.elapsed() < Duration::from_secs(15), "进程树清理超过有界时间");
    }

    #[cfg(windows)]
    fn descendant_pids(root: u32) -> Vec<u32> {
        use std::ffi::c_void;
        #[repr(C)]
        struct ProcessEntry { size: u32, cnt_usage: u32, process_id: u32, default_heap_id: usize, module_id: u32,
            threads: u32, parent_process_id: u32, pri_class_base: i32, flags: u32, exe_file: [u16; 260] }
        unsafe extern "system" {
            fn CreateToolhelp32Snapshot(flags: u32, pid: u32) -> *mut c_void;
            fn Process32FirstW(snap: *mut c_void, entry: *mut ProcessEntry) -> i32;
            fn Process32NextW(snap: *mut c_void, entry: *mut ProcessEntry) -> i32;
            fn CloseHandle(handle: *mut c_void) -> i32;
        }
        let snap = unsafe { CreateToolhelp32Snapshot(0x00000002, 0) };
        if snap.is_null() || snap as isize == -1 { return Vec::new(); }
        let mut rows = Vec::new();
        unsafe {
            let mut entry = std::mem::zeroed::<ProcessEntry>();
            entry.size = std::mem::size_of::<ProcessEntry>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    rows.push((entry.process_id, entry.parent_process_id));
                    entry.size = std::mem::size_of::<ProcessEntry>() as u32;
                    if Process32NextW(snap, &mut entry) == 0 { break; }
                }
            }
            CloseHandle(snap);
        }
        let mut pending = vec![root];
        let mut found = Vec::new();
        while let Some(parent) = pending.pop() {
            for (pid, ppid) in &rows {
                if *ppid == parent && *pid != parent { found.push(*pid); pending.push(*pid); }
            }
        }
        found
    }
    #[cfg(windows)]
    fn assert_tree_gone(folder: &std::path::Path) {
        let pids = std::fs::read_to_string(folder.join("pids")).unwrap();
        let pids: Vec<u32> = pids.lines().filter_map(|line| line.trim().parse().ok()).collect();
        assert!(pids.len() >= 2, "没有记录到子进程和孙进程");
        for pid in pids {
            let output = std::process::Command::new("tasklist").args(["/FI", &format!("PID eq {pid}"), "/FO", "CSV", "/NH"]).output().unwrap();
            let text = String::from_utf8_lossy(&output.stdout);
            let alive = text.split(',').any(|field| field.trim().trim_matches('"') == pid.to_string());
            assert!(!alive, "后代仍在运行：{pid} {text}");
        }
    }
    #[cfg(unix)]
    fn assert_tree_gone(folder: &std::path::Path) {
        let pids = std::fs::read_to_string(folder.join("pids")).unwrap();
        assert!(pids.lines().count() >= 2);
        for pid in pids.lines() {
            let output = std::process::Command::new("/bin/ps").args(["-o", "stat=", "-p", pid]).output().unwrap();
            let state = String::from_utf8_lossy(&output.stdout);
            assert!(state.trim().is_empty() || state.trim().starts_with('Z'), "后代仍在运行：{pid} {state}");
        }
    }
    #[tokio::test]
    async fn prompt_file_is_written_before_start_and_always_removed() {
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("prompt-under-test.md");
        let helper = || FakeAdapter::new(std::env::current_exe().unwrap(), vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]).with_prompt_file();
        let request = |scenario: &str| TurnRequest { prompt: format!("{scenario}\n- 多行\n下一行"), session_id: None, cwd: folder.path().into(), mode: RunMode::Execute, model: None };

        let (_sender, cancellation) = tokio::sync::watch::channel(false);
        let outcome = run_turn_with_config(Box::new(helper()), request("echo"), cancellation, |_| {}, config()).await;
        assert_eq!(outcome.status, "ok", "{:?}", outcome.error);
        assert_eq!(outcome.partial_text, "- 多行\n下一行");
        assert!(!file.exists(), "成功后要删除提示词文件");

        let (_sender, cancellation) = tokio::sync::watch::channel(false);
        let outcome = run_turn_with_config(Box::new(helper()), request("silent"), cancellation, |_| {}, config()).await;
        assert_eq!(outcome.status, "timeout");
        assert!(!file.exists(), "超时后也要删除提示词文件");

        std::fs::write(&file, "别人的文件").unwrap();
        let (_sender, cancellation) = tokio::sync::watch::channel(false);
        let outcome = run_turn_with_config(Box::new(helper()), request("echo"), cancellation, |_| {}, config()).await;
        assert_eq!(outcome.status, "failed");
        assert!(outcome.error.unwrap().contains("无法写入提示词文件"));
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "别人的文件", "已存在的文件不覆盖也不删除");
    }

    struct SupportAdapter { inner: FakeAdapter, files: Vec<crate::adapters::SupportFile> }
    impl CliAdapter for SupportAdapter {
        fn id(&self) -> &'static str { "fake" }
        fn detect(&self) -> Option<crate::cli_registry::CliInfo> { None }
        fn build_command(&self, req: &TurnRequest) -> crate::adapters::CommandSpec {
            let mut spec = self.inner.build_command(req); spec.support_files = self.files.clone(); spec
        }
        fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> { self.inner.parse_line(line) }
    }

    #[tokio::test]
    async fn support_files_exist_before_spawn_and_are_removed_on_every_exit() {
        use crate::adapters::SupportFile;
        let folder = tempfile::tempdir().unwrap();
        let file = folder.path().join("bridge.mjs");
        for scenario in ["echo", "silent", "cancel", "spawn_error"] {
            let inner = FakeAdapter::new(if scenario == "spawn_error" { folder.path().join("不存在.exe") } else { std::env::current_exe().unwrap() },
                vec!["--ignored".into(), "--exact".into(), "runner::tests::subprocess_helper".into(), "--nocapture".into()]);
            let adapter = SupportAdapter { inner, files: vec![SupportFile { path: file.clone(), content: "应用辅助代码".into() }] };
            let request = TurnRequest { prompt: if scenario == "echo" { "echo\n辅助文件测试".into() } else { "silent\n辅助文件测试".into() },
                session_id: None, cwd: folder.path().into(), mode: RunMode::Execute, model: None };
            let (sender, cancellation) = watch::channel(false);
            if scenario == "cancel" { tokio::spawn(async move { tokio::time::sleep(Duration::from_millis(80)).await; let _ = sender.send(true); }); }
            let outcome = run_turn_managed(Box::new(adapter), request, cancellation, |_| {}, config(), None, |_| {
                assert_eq!(std::fs::read_to_string(&file).unwrap(), "应用辅助代码"); Ok(())
            }).await;
            assert_eq!(outcome.status, match scenario { "echo" => "ok", "silent" => "timeout", "cancel" => "cancelled", _ => "failed" });
            assert!(!file.exists(), "{scenario} 后应删除辅助文件");
        }
    }

    #[tokio::test]
    async fn support_file_preparation_failure_preserves_existing_files_and_cleans_owned_files() {
        use crate::adapters::SupportFile;
        let folder = tempfile::tempdir().unwrap();
        let owned = folder.path().join("新文件"); let existing = folder.path().join("已存在");
        std::fs::write(&existing, "原内容").unwrap();
        let adapter = SupportAdapter { inner: FakeAdapter::new("不会启动".into(), vec![]), files: vec![
            SupportFile { path: owned.clone(), content: "新内容".into() }, SupportFile { path: existing.clone(), content: "不能覆盖".into() }] };
        let request = TurnRequest { prompt: "不会发送".into(), session_id: None, cwd: folder.path().into(), mode: RunMode::Discuss, model: None };
        let (_sender, cancellation) = watch::channel(false);
        let outcome = run_turn(Box::new(adapter), request, cancellation, |_| {}).await;
        assert_eq!(outcome.status, "failed"); assert!(!owned.exists());
        assert_eq!(std::fs::read_to_string(existing).unwrap(), "原内容");
    }

}
