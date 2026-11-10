use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use serde_json::Value;
use super::{AgentEvent, CliAdapter, CommandSpec, RunMode, TurnRequest, Usage};
use crate::cli_registry::{self, CliInfo, ResolvedCli};

pub struct CodexAdapter {
    program: PathBuf,
    path_env: Option<std::ffi::OsString>,
    mode: RunMode,
    usage_baseline: Option<Usage>,
    resumed: AtomicBool,
    active_tools: HashSet<String>,
    final_text: String,
    had_text: bool,
    failed: bool,
    completed: bool,
}

impl CodexAdapter {
    pub fn new(program: PathBuf, mode: RunMode, usage_baseline: Option<Usage>) -> Self {
        Self { program, path_env: None, mode, usage_baseline, resumed: AtomicBool::new(false),
            active_tools: HashSet::new(), final_text: String::new(), had_text: false, failed: false, completed: false }
    }
    pub fn with_environment(mut self, resolved: &ResolvedCli) -> Self {
        self.program = resolved.program.clone(); self.path_env = Some(resolved.path_env.clone()); self
    }
    fn fail(&mut self, message: String) -> Vec<AgentEvent> {
        self.failed = true; vec![AgentEvent::Error { message }]
    }
    fn usage(&self, raw: &Value) -> Usage {
        let baseline = self.usage_baseline.as_ref().and_then(|u| u.raw_usage.as_ref());
        let count = |key: &str| {
            let current = raw[key].as_u64()?;
            match baseline {
                Some(previous) => current.checked_sub(previous[key].as_u64()?),
                None if !self.resumed.load(Ordering::Relaxed) => Some(current),
                None => None,
            }
        };
        // 任一可信累计量下降表示计数重置，整组归一值留空而不猜测。
        let reset = baseline.is_some_and(|b| ["input_tokens", "output_tokens", "cached_input_tokens", "cache_write_input_tokens"].iter()
            .any(|key| raw[*key].as_u64().zip(b[*key].as_u64()).is_some_and(|(now, before)| now < before)));
        if reset { return Usage { raw_usage: Some(raw.clone()), ..Default::default() }; }
        Usage { input_tokens: count("input_tokens"), output_tokens: count("output_tokens"),
            cache_read_input_tokens: count("cached_input_tokens"), cache_creation_input_tokens: count("cache_write_input_tokens"),
            cost_usd: None, raw_usage: Some(raw.clone()) }
    }
}

impl CliAdapter for CodexAdapter {
    fn id(&self) -> &'static str { "codex" }
    fn detect(&self) -> Option<CliInfo> { cli_registry::resolve_cli(self.id()).ok().map(|cli| cli.info) }
    fn build_command(&self, req: &TurnRequest) -> CommandSpec {
        self.resumed.store(req.session_id.is_some(), Ordering::Relaxed);
        let mut args = vec!["exec".into()];
        if let Some(id) = &req.session_id { args.extend(["resume".into(), id.clone()]); }
        let sandbox = match self.mode { RunMode::Discuss => "read-only", RunMode::Execute => "workspace-write" };
        args.extend(["--json".into(), "--skip-git-repo-check".into(), "-c".into(),
            format!("sandbox_mode=\"{sandbox}\""), "-c".into(), "approval_policy=\"never\"".into()]);
        if let Some(model) = req.model.as_ref().filter(|m| !m.trim().is_empty()) { args.extend(["--model".into(), model.clone()]); }
        args.push("-".into());
        CommandSpec { program: self.program.clone(), args, path_env: self.path_env.clone(),
            stdin: Some(req.prompt.clone()), prompt_file: None }
    }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![]; };
        if self.failed || self.completed { return vec![]; }
        match v["type"].as_str() {
            Some("thread.started") => v["thread_id"].as_str()
                .map(|id| vec![AgentEvent::SessionStarted { session_id: id.into() }]).unwrap_or_default(),
            Some("turn.failed" | "error") => self.fail(v["error"]["message"].as_str()
                .or_else(|| v["message"].as_str()).unwrap_or("Codex 回合失败").into()),
            Some("item.started" | "item.completed") => {
                let item = &v["item"];
                let completed = v["type"] == "item.completed";
                match item["type"].as_str() {
                    Some("agent_message") if completed => {
                        let Some(text) = item["text"].as_str() else { return vec![]; };
                        self.final_text = text.into();
                        let text = if self.had_text { format!("\n{text}") } else { text.into() };
                        self.had_text = true;
                        vec![AgentEvent::TextDelta { text }]
                    }
                    Some("error") => {
                        let message = item["message"].as_str().unwrap_or("Codex 返回错误");
                        if message.starts_with("Codex is ignoring ") &&
                            (message.contains("unrecognized configuration setting") || message.contains("disable_response_storage is ignored")) {
                            return vec![];
                        }
                        self.fail(message.into())
                    }
                    Some(kind @ ("command_execution" | "mcp_tool_call" | "web_search" | "file_change")) => {
                        let Some(id) = item["id"].as_str() else { return vec![]; };
                        if completed {
                            self.active_tools.remove(id);
                            vec![AgentEvent::ToolState { active: !self.active_tools.is_empty() }]
                        } else if self.active_tools.insert(id.into()) {
                            vec![AgentEvent::ToolUse { name: kind.into(), summary: "正在运行工具".into() }, AgentEvent::ToolState { active: true }]
                        } else { vec![] }
                    }
                    _ => vec![],
                }
            }
            Some("turn.completed") => {
                self.completed = true;
                let usage = v.get("usage").filter(|u| u.is_object()).map(|raw| self.usage(raw));
                vec![AgentEvent::TurnCompleted { final_text: self.final_text.clone(), usage }]
            }
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AgentEvent, CliAdapter, RunMode, TurnRequest};

    #[test]
    fn fixture_keeps_only_last_agent_message_as_final_text() {
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Execute, None);
        let events: Vec<_> = include_str!("../../tests/fixtures/codex/execute.jsonl")
            .lines().flat_map(|line| adapter.parse_line(line)).collect();
        assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "01a1c3f4-575d-7eb3-87e8-6237ed333189")));
        let final_text = events.iter().find_map(|event| match event {
            AgentEvent::TurnCompleted { final_text, .. } => Some(final_text),
            _ => None,
        }).unwrap();
        assert!(final_text.contains("已在当前目录创建"));
        assert!(!final_text.contains("我会在当前工作目录"));
        assert_eq!(events.iter().filter(|e| matches!(e, AgentEvent::TurnCompleted { .. })).count(), 1);
    }

    #[test]
    fn resume_does_not_use_unsupported_sandbox_flag() {
        let adapter = CodexAdapter::new("codex".into(), RunMode::Discuss, None);
        let request = TurnRequest {
            prompt: "多行\n提示词".into(), session_id: Some("已有线程".into()),
            cwd: ".".into(), mode: RunMode::Discuss, model: None,
        };
        let spec = adapter.build_command(&request);
        assert_eq!(&spec.args[..3], ["exec", "resume", "已有线程"]);
        assert!(!spec.args.iter().any(|arg| arg == "-s" || arg == "--sandbox" || arg == "-C"));
        assert!(spec.args.iter().any(|arg| arg == "--skip-git-repo-check"));
        assert!(spec.args.windows(2).any(|args| args == ["-c", "approval_policy=\"never\""]));
        assert!(spec.args.windows(2).any(|args| args == ["-c", "sandbox_mode=\"read-only\""]));
        assert_eq!(spec.args.last().map(String::as_str), Some("-"));
        assert_eq!(spec.stdin, Some(request.prompt));
    }

    #[test]
    fn real_failed_turn_never_completes() {
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Discuss, None);
        let events: Vec<_> = include_str!("../../tests/fixtures/codex/error.jsonl")
            .lines().flat_map(|line| adapter.parse_line(line)).collect();
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Error { message } if message.contains("no-such-model"))));
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { .. })));
    }
    fn request(session_id: Option<&str>) -> TurnRequest {
        TurnRequest { prompt: "- 第一行\n第二行".into(), session_id: session_id.map(str::to_owned),
            cwd: "/tmp/项目".into(), mode: RunMode::Execute, model: Some("测试模型".into()) }
    }

    #[test]
    fn all_fixtures_preserve_tool_state_and_never_expose_command_arguments() {
        for (fixture, mode) in [(include_str!("../../tests/fixtures/codex/discuss.jsonl"), RunMode::Discuss),
            (include_str!("../../tests/fixtures/codex/resume.jsonl"), RunMode::Execute)] {
            let mut adapter = CodexAdapter::new("codex".into(), mode, None);
            let events: Vec<_> = fixture.lines().flat_map(|line| adapter.parse_line(line)).collect();
            assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { .. })));
            assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { .. })));
            assert!(!events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
            for event in events {
                if let AgentEvent::ToolUse { name, summary } = event {
                    assert_eq!(name, "command_execution");
                    assert!(!summary.contains("PowerShell"));
                }
            }
        }
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Execute, None);
        assert!(matches!(adapter.parse_line(r#"{"type":"item.started","item":{"type":"command_execution","id":"a"}}"#).last(), Some(AgentEvent::ToolState { active: true })));
        adapter.parse_line(r#"{"type":"item.started","item":{"type":"command_execution","id":"b"}}"#);
        assert_eq!(adapter.parse_line(r#"{"type":"item.completed","item":{"type":"command_execution","id":"a"}}"#), vec![AgentEvent::ToolState { active: true }]);
        assert_eq!(adapter.parse_line(r#"{"type":"item.completed","item":{"type":"command_execution","id":"b"}}"#), vec![AgentEvent::ToolState { active: false }]);
    }

    #[test]
    fn cumulative_usage_subtracts_only_known_baseline_and_handles_reset() {
        let baseline = Usage { raw_usage: Some(serde_json::json!({"input_tokens":100, "output_tokens":10, "cached_input_tokens":80})), ..Default::default() };
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Execute, Some(baseline));
        adapter.build_command(&request(Some("已有线程")));
        let usage = adapter.usage(&serde_json::json!({"input_tokens":140, "output_tokens":18, "cached_input_tokens":90}));
        assert_eq!(usage.input_tokens, Some(40));
        assert_eq!(usage.output_tokens, Some(8));
        assert_eq!(usage.cache_read_input_tokens, Some(10));
        assert_eq!(usage.cost_usd, None);
        let reset = adapter.usage(&serde_json::json!({"input_tokens":40, "output_tokens":3, "cached_input_tokens":20}));
        assert_eq!(reset.input_tokens, None);
        assert_eq!(reset.output_tokens, None);
        assert!(reset.raw_usage.is_some());
        adapter = CodexAdapter::new("codex".into(), RunMode::Execute, None);
        adapter.build_command(&request(Some("已有线程")));
        assert_eq!(adapter.usage(&serde_json::json!({"input_tokens":40})).input_tokens, None);
        let spec = adapter.build_command(&request(None));
        assert_eq!(adapter.usage(&serde_json::json!({"input_tokens":40})).input_tokens, Some(40));
        assert_eq!(&spec.args[..2], ["exec", "--json"]);
        assert!(spec.args.windows(2).any(|a| a == ["-c", "sandbox_mode=\"workspace-write\""]));
        assert!(spec.args.windows(2).any(|a| a == ["--model", "测试模型"]));
        assert!(!spec.args.contains(&request(None).prompt));
    }

    #[test]
    fn verified_config_diagnostic_is_nonfatal_but_other_errors_latch() {
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Execute, None);
        assert!(adapter.parse_line(r#"{"type":"item.completed","item":{"type":"error","message":"Codex is ignoring the following unrecognized configuration setting: disable_response_storage is ignored"}}"#).is_empty());
        assert!(matches!(adapter.parse_line(r#"{"type":"turn.completed","usage":{}}"#).as_slice(), [AgentEvent::TurnCompleted { .. }]));
        let mut adapter = CodexAdapter::new("codex".into(), RunMode::Execute, None);
        assert!(matches!(adapter.parse_line(r#"{"type":"turn.failed","error":{"message":"启动失败"}}"#).as_slice(), [AgentEvent::Error { .. }]));
        assert!(adapter.parse_line(r#"{"type":"turn.completed","usage":{}}"#).is_empty());
    }

}
