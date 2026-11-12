use std::collections::HashSet;
use std::path::PathBuf;
use serde_json::Value;
use super::{AgentEvent, CliAdapter, CommandSpec, RunMode, TurnRequest, Usage};
use crate::cli_registry::{self, CliInfo, ResolvedCli};

pub struct ClaudeAdapter {
    program: PathBuf,
    path_env: Option<std::ffi::OsString>,
    mode: RunMode,
    active_tools: HashSet<String>,
    failed: bool,
    completed: bool,
}

impl ClaudeAdapter {
    pub fn new(program: PathBuf, mode: RunMode) -> Self {
        Self { program, path_env: None, mode, active_tools: HashSet::new(), failed: false, completed: false }
    }
    pub fn with_environment(mut self, resolved: &ResolvedCli) -> Self {
        self.program = resolved.program.clone();
        self.path_env = Some(resolved.path_env.clone());
        self
    }
    fn permission(&self) -> &'static str {
        match self.mode { RunMode::Discuss => "plan", RunMode::Execute => "bypassPermissions" }
    }
    fn fail(&mut self, message: String) -> Vec<AgentEvent> {
        self.failed = true;
        vec![AgentEvent::Error { message }]
    }
}

impl CliAdapter for ClaudeAdapter {
    fn id(&self) -> &'static str { "claude" }
    fn detect(&self) -> Option<CliInfo> { cli_registry::resolve_cli(self.id()).ok().map(|cli| cli.info) }
    fn build_command(&self, req: &TurnRequest) -> CommandSpec {
        let mut args = vec!["-p".into(), "--output-format".into(), "stream-json".into(),
            "--verbose".into(), "--permission-mode".into(), self.permission().into()];
        if let Some(id) = &req.session_id { args.extend(["--resume".into(), id.clone()]); }
        if let Some(model) = req.model.as_ref().filter(|model| !model.trim().is_empty()) {
            args.extend(["--model".into(), model.clone()]);
        }
        CommandSpec { program: self.program.clone(), args, path_env: self.path_env.clone(),
            stdin: Some(req.prompt.clone()), prompt_file: None, support_files: vec![] }
    }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![]; };
        if self.failed || self.completed { return vec![]; }
        match v["type"].as_str() {
            Some("system") if v["subtype"] == "init" => {
                if v["permissionMode"].as_str() != Some(self.permission()) {
                    return self.fail(format!("Claude 权限模式不符：期望 {}，收到 {}", self.permission(),
                        v["permissionMode"].as_str().unwrap_or("未回显")));
                }
                let mut events: Vec<AgentEvent> = v["session_id"].as_str()
                    .map(|id| AgentEvent::SessionStarted { session_id: id.into() }).into_iter().collect();
                events.extend(super::model_event(&v));
                events
            }
            Some("assistant" | "user") => {
                let mut events = vec![];
                if let Some(content) = v["message"]["content"].as_array() {
                    for block in content {
                        match block["type"].as_str() {
                            Some("text") if v["type"] == "assistant" => {
                                if let Some(text) = block["text"].as_str() { events.push(AgentEvent::TextDelta { text: text.into() }); }
                            }
                            Some("tool_use") => {
                                if let Some(id) = block["id"].as_str() {
                                    if self.active_tools.insert(id.into()) {
                                        events.push(AgentEvent::ToolUse { name: block["name"].as_str().unwrap_or("工具").into(), summary: "正在运行工具".into() });
                                        events.push(AgentEvent::ToolState { active: true });
                                    }
                                }
                            }
                            Some("tool_result") => {
                                if let Some(id) = block["tool_use_id"].as_str() {
                                    self.active_tools.remove(id);
                                    events.push(AgentEvent::ToolState { active: !self.active_tools.is_empty() });
                                }
                            }
                            _ => {}
                        }
                    }
                }
                events
            }
            Some("result") => {
                if v["is_error"].as_bool().unwrap_or(false) || v["subtype"] != "success" {
                    let message = v["result"].as_str().map(str::to_owned)
                        .or_else(|| v["errors"].as_array().map(|es| es.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("；")))
                        .unwrap_or_else(|| "Claude 回合失败".into());
                    return self.fail(message);
                }
                let usage = v.get("usage").filter(|u| u.is_object()).map(|u| {
                    let cost_usd = v["model_usage_since_last_result"].as_object().filter(|m| !m.is_empty()).and_then(|models| {
                        models.values().map(|m| m["costUSD"].as_f64().filter(|n| n.is_finite() && *n >= 0.0))
                            .collect::<Option<Vec<_>>>().map(|costs| costs.into_iter().sum())
                    });
                    Usage { input_tokens: u["input_tokens"].as_u64(), output_tokens: u["output_tokens"].as_u64(),
                        cache_read_input_tokens: u["cache_read_input_tokens"].as_u64(),
                        cache_creation_input_tokens: u["cache_creation_input_tokens"].as_u64(), cost_usd, raw_usage: Some(u.clone()) }
                });
                self.completed = true;
                vec![AgentEvent::TurnCompleted { final_text: v["result"].as_str().unwrap_or_default().into(), usage }]
            }
            Some("error") => self.fail(v["error"]["message"].as_str().or_else(|| v["message"].as_str()).unwrap_or("Claude 返回错误").into()),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AgentEvent, CliAdapter, RunMode, TurnRequest};

    fn feed(text: &str, mode: RunMode) -> Vec<AgentEvent> {
        let mut adapter = ClaudeAdapter::new("claude".into(), mode);
        text.lines().flat_map(|line| adapter.parse_line(line)).collect()
    }

    #[test]
    fn real_discuss_has_session_text_and_one_completion() {
        let events = feed(include_str!("../../tests/fixtures/claude/discuss.jsonl"), RunMode::Discuss);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "7a83cd2c-cc9d-4f7f-aafb-06ad043be63b")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, .. } if name == "Read")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text.contains("README.md"))));
        let completions: Vec<_> = events.iter().filter_map(|e| match e {
            AgentEvent::TurnCompleted { final_text, usage } => Some((final_text, usage)),
            _ => None,
        }).collect();
        assert_eq!(completions.len(), 1);
        assert!(completions[0].0.contains("imates M0 测试目录"));
        assert_eq!(completions[0].1.as_ref().unwrap().raw_usage.as_ref().unwrap()["output_tokens"], 234);
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
    }

    #[test]
    fn error_subtype_success_does_not_override_is_error() {
        let events = feed(include_str!("../../tests/fixtures/claude/error.jsonl"), RunMode::Discuss);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Error { message } if message.contains("no-such-model"))));
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { .. })));
    }

    #[test]
    fn permission_mismatch_fails_and_prompt_is_only_in_stdin() {
        let mut adapter = ClaudeAdapter::new("claude".into(), RunMode::Discuss);
        let events = adapter.parse_line(r#"{"type":"system","subtype":"init","session_id":"测试","permissionMode":"default"}"#);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
        let request = TurnRequest {
            prompt: "- 第一行\n第二行".into(), session_id: Some("已有会话".into()),
            cwd: ".".into(), mode: RunMode::Discuss, model: Some("测试模型".into()),
        };
        let spec = adapter.build_command(&request);
        assert_eq!(spec.stdin.as_deref(), Some(request.prompt.as_str()));
        assert!(!spec.args.iter().any(|arg| arg == &request.prompt));
        assert!(spec.args.windows(2).any(|args| args == ["--resume", "已有会话"]));
        assert!(spec.args.windows(2).any(|args| args == ["--permission-mode", "plan"]));
    }

    #[test]
    fn execute_fixture_tracks_write_tool_and_session() {
        let events = feed(include_str!("../../tests/fixtures/claude/execute.jsonl"), RunMode::Execute);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "cfba7141-e804-4a67-b6ea-50623844e7c4")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, summary } if name == "Write" && !summary.contains("hello.txt"))));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolState { active: true })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolState { active: false })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { final_text, usage: Some(usage) }
            if final_text.contains("已在当前目录创建") && usage.output_tokens == Some(130)
                && usage.cost_usd == Some(0.07241139999999999)
                && usage.raw_usage.as_ref().is_some_and(|raw| raw["output_tokens"] == 130))));
    }

    #[test]
    fn commands_bind_mode_environment_and_keep_prompt_only_in_stdin() {
        let resolved = crate::cli_registry::ResolvedCli {
            info: crate::cli_registry::CliInfo { id: "claude".into(), label: "Claude Code".into(), installed: true,
                supported: true, path: Some("/opt/bin/claude".into()), version: None },
            program: "/opt/bin/claude".into(), path_env: "/opt/bin:/usr/bin".into(),
            environment_source: crate::cli_registry::EnvironmentSource::LoginShell,
        };
        let request = TurnRequest { prompt: "- 参数
下一行".into(), session_id: Some("session-1".into()),
            cwd: "/tmp/work".into(), mode: RunMode::Execute, model: Some("model-1".into()) };
        let adapter = ClaudeAdapter::new("ignored".into(), RunMode::Execute).with_environment(&resolved);
        let spec = adapter.build_command(&request);
        assert_eq!(spec.program, PathBuf::from("/opt/bin/claude"));
        assert_eq!(spec.path_env.as_deref(), Some(std::ffi::OsStr::new("/opt/bin:/usr/bin")));
        assert_eq!(spec.stdin.as_deref(), Some(request.prompt.as_str()));
        assert!(!spec.args.contains(&request.prompt));
        assert!(spec.args.windows(2).any(|args| args == ["--permission-mode", "bypassPermissions"]));
        assert!(spec.args.windows(2).any(|args| args == ["--resume", "session-1"]));
        assert!(spec.args.windows(2).any(|args| args == ["--model", "model-1"]));
        assert_eq!(spec.args.first().map(String::as_str), Some("-p"));
    }

    #[test]
    fn error_result_is_latched_and_cannot_be_overridden_by_late_success() {
        let mut adapter = ClaudeAdapter::new("claude".into(), RunMode::Discuss);
        let error = adapter.parse_line(r#"{"type":"result","subtype":"success","is_error":true,"result":"请求失败"}"#);
        assert!(matches!(error.as_slice(), [AgentEvent::Error { .. }]));
        assert!(adapter.parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"迟到成功"}"#).is_empty());
    }

    #[test]
    fn resume_usage_and_cost_are_per_turn_not_cumulative() {
        let events = feed(include_str!("../../tests/fixtures/claude/resume.jsonl"), RunMode::Discuss);
        let usage = events.iter().find_map(|event| match event {
            AgentEvent::TurnCompleted { usage, .. } => usage.as_ref(), _ => None,
        }).unwrap();
        assert_eq!(usage.input_tokens, Some(2));
        assert_eq!(usage.output_tokens, Some(239));
        assert_eq!(usage.cost_usd, Some(0.0119602));
        assert!(feed("不是 JSON", RunMode::Discuss).is_empty());
        let events = feed(r#"{"type":"result","subtype":"success","result":"正文","usage":{"output_tokens":3},"total_cost_usd":9}"#, RunMode::Discuss);
        assert!(matches!(&events[0], AgentEvent::TurnCompleted { usage: Some(u), .. } if u.cost_usd.is_none() && u.input_tokens.is_none()));
    }

    fn reported(events: &[AgentEvent]) -> Vec<&str> {
        events.iter().filter_map(|event| match event { AgentEvent::ModelReported { model } => Some(model.as_str()), _ => None }).collect()
    }

    #[test]
    fn init_reports_the_model_the_cli_says_it_used() {
        let discuss = feed(include_str!("../../tests/fixtures/claude/discuss.jsonl"), RunMode::Discuss);
        assert_eq!(reported(&discuss), ["claude-sonnet-5-5"]);
        let execute = feed(include_str!("../../tests/fixtures/claude/execute.jsonl"), RunMode::Execute);
        assert_eq!(reported(&execute), ["claude-sonnet-5-5"]);
        // 会话事件仍在，且先于模型事件。
        let session = discuss.iter().position(|e| matches!(e, AgentEvent::SessionStarted { .. })).unwrap();
        let model = discuss.iter().position(|e| matches!(e, AgentEvent::ModelReported { .. })).unwrap();
        assert!(session < model);
    }

    #[test]
    fn init_reports_whatever_model_the_cli_echoes_even_when_unknown() {
        let events = feed(include_str!("../../tests/fixtures/claude/error.jsonl"), RunMode::Discuss);
        assert_eq!(reported(&events), ["no-such-model"]);
    }

    #[test]
    fn init_without_model_or_with_unsafe_model_reports_nothing() {
        let init = |model: &str| format!(r#"{{"type":"system","subtype":"init","session_id":"s","permissionMode":"plan"{model}}}"#);
        let mut adapter = ClaudeAdapter::new("claude".into(), RunMode::Discuss);
        assert!(matches!(adapter.parse_line(&init("")).as_slice(), [AgentEvent::SessionStarted { .. }]));
        let control = adapter.parse_line(&init(r#","model":"bad\u0007name""#));
        assert!(matches!(control.as_slice(), [AgentEvent::SessionStarted { .. }]));
        let long = adapter.parse_line(&init(&format!(r#","model":"{}""#, "m".repeat(201))));
        assert!(matches!(long.as_slice(), [AgentEvent::SessionStarted { .. }]));
        let limit = adapter.parse_line(&init(&format!(r#","model":"{}""#, "m".repeat(200))));
        assert_eq!(reported(&limit), ["m".repeat(200).as_str()]);
    }
}
