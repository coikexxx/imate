use std::collections::HashSet;
use std::path::PathBuf;
use serde_json::Value;
use super::{AgentEvent, CliAdapter, CommandSpec, RunMode, TurnRequest, Usage};
use crate::cli_registry::{self, CliInfo, ResolvedCli};

/// grok 的无头适配器。事件格式依据 Windows 1.0.77 的 `streaming-messages-json` 样本。
pub struct GrokAdapter {
    program: PathBuf,
    path_env: Option<std::ffi::OsString>,
    mode: RunMode,
    reasoning_effort: Option<String>,
    active_tools: HashSet<String>,
    failed: bool,
    completed: bool,
}

impl GrokAdapter {
    pub fn new(program: PathBuf, mode: RunMode) -> Self {
        Self { program, path_env: None, mode, reasoning_effort: None, active_tools: HashSet::new(), failed: false, completed: false }
    }
    pub fn with_environment(mut self, resolved: &ResolvedCli) -> Self {
        self.program = resolved.program.clone();
        self.path_env = Some(resolved.path_env.clone());
        self
    }
    /// plan 只是讨论意图：它拦住工作目录里的写，但 grok 仍会写自己的会话目录。
    fn permission(&self) -> &'static str {
        match self.mode { RunMode::Discuss => "plan", RunMode::Execute => "bypassPermissions" }
    }
    fn fail(&mut self, message: String) -> Vec<AgentEvent> {
        self.failed = true;
        vec![AgentEvent::Error { message }]
    }
}

impl CliAdapter for GrokAdapter {
    fn id(&self) -> &'static str { "grok" }
    fn set_reasoning_effort(&mut self, effort: Option<String>) { self.reasoning_effort = effort; }
    fn detect(&self) -> Option<CliInfo> { cli_registry::resolve_cli(self.id()).ok().map(|cli| cli.info) }
    fn build_command(&self, req: &TurnRequest) -> CommandSpec {
        // 只决定路径。runner 在启动前写入提示词，回合结束后删除。
        let prompt_file = std::env::temp_dir().join(format!("imates-prompt-{}.md", uuid::Uuid::new_v4()));
        let mut args = vec!["--prompt-file".into(), prompt_file.to_string_lossy().into_owned(),
            "--output-format".into(), "streaming-messages-json".into(),
            "--permission-mode".into(), self.permission().into()];
        if let Some(id) = &req.session_id { args.extend(["-r".into(), id.clone()]); }
        if let Some(model) = req.model.as_ref().filter(|model| !model.trim().is_empty()) {
            args.extend(["-m".into(), model.clone()]);
        }
        if let Some(effort) = &self.reasoning_effort { args.extend(["--reasoning-effort".into(), effort.clone()]); }
        CommandSpec { program: self.program.clone(), args, path_env: self.path_env.clone(),
            stdin: None, prompt_file: Some(prompt_file), support_files: vec![] }
    }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![]; };
        if self.failed || self.completed { return vec![]; }
        match v["type"].as_str() {
            Some("system") if v["subtype"] == "init" => {
                if v["permissionMode"].as_str() != Some(self.permission()) {
                    return self.fail(format!("Grok 权限模式不符：期望 {}，收到 {}", self.permission(),
                        v["permissionMode"].as_str().unwrap_or("未回显")));
                }
                let mut events: Vec<AgentEvent> = v["session_id"].as_str()
                    .map(|id| AgentEvent::SessionStarted { session_id: id.into() }).into_iter().collect();
                events.extend(super::model_event(&v));
                events
            }
            Some("assistant" | "user") => {
                let mut events = vec![];
                let Some(content) = v["message"]["content"].as_array() else { return events; };
                for block in content {
                    match block["type"].as_str() {
                        Some("text") if v["type"] == "assistant" => {
                            if let Some(text) = block["text"].as_str() { events.push(AgentEvent::TextDelta { text: text.into() }); }
                        }
                        Some("tool_use") => {
                            if let Some(id) = block["id"].as_str() {
                                if self.active_tools.insert(id.into()) {
                                    // 只报工具名，不带参数，避免路径和内容进入界面与日志。
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
                events
            }
            Some("result") => {
                if v["is_error"].as_bool().unwrap_or(false) || v["subtype"] != "success" {
                    let message = v["errors"].as_array()
                        .map(|errors| errors.iter().filter_map(Value::as_str).collect::<Vec<_>>().join("；"))
                        .filter(|text| !text.is_empty())
                        .or_else(|| v["result"].as_str().filter(|text| !text.is_empty()).map(str::to_owned))
                        .unwrap_or_else(|| "Grok 回合失败".into());
                    return self.fail(message);
                }
                let usage = v.get("usage").filter(|u| u.is_object()).map(|u| {
                    // 样本里每个模型一项 costUSD；任何一项缺失或非法就不给费用。
                    let cost_usd = v["modelUsage"].as_object().filter(|models| !models.is_empty()).and_then(|models| {
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
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AgentEvent, CliAdapter, RunMode, TurnRequest};

    fn feed(text: &str, mode: RunMode) -> Vec<AgentEvent> {
        let mut adapter = GrokAdapter::new("grok".into(), mode);
        text.lines().flat_map(|line| adapter.parse_line(line)).collect()
    }
    fn completion(events: &[AgentEvent]) -> (&String, &crate::adapters::Usage) {
        let found: Vec<_> = events.iter().filter_map(|event| match event {
            AgentEvent::TurnCompleted { final_text, usage } => Some((final_text, usage.as_ref().unwrap())),
            _ => None,
        }).collect();
        assert_eq!(found.len(), 1, "应当恰好完成一次");
        found[0]
    }

    #[test]
    fn reasoning_effort_adds_the_effort_flag_only_when_selected() {
        let request = TurnRequest { prompt: "p".into(), session_id: None, cwd: std::env::temp_dir(), mode: RunMode::Discuss, model: None };
        let mut adapter = GrokAdapter::new("grok".into(), RunMode::Discuss);
        assert!(!adapter.build_command(&request).args.iter().any(|arg| arg == "--reasoning-effort"));
        adapter.set_reasoning_effort(Some("xhigh".into()));
        assert!(adapter.build_command(&request).args.windows(2).any(|pair| pair == ["--reasoning-effort", "xhigh"]));
    }

    #[test]
    fn real_discuss_has_session_two_tools_text_and_per_turn_usage() {
        let events = feed(include_str!("../../tests/fixtures/grok/discuss.jsonl"), RunMode::Discuss);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "01a1c3f9-0dcd-7242-ac16-9904ab7a6eec")));
        let tools: Vec<&str> = events.iter().filter_map(|e| match e { AgentEvent::ToolUse { name, .. } => Some(name.as_str()), _ => None }).collect();
        assert_eq!(tools, ["read_file", "list_dir"]);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text.contains("README.md"))));
        // 两个工具都结束后才回到空闲
        let states: Vec<bool> = events.iter().filter_map(|e| match e { AgentEvent::ToolState { active } => Some(*active), _ => None }).collect();
        assert_eq!(states, [true, true, true, false]);
        let (final_text, usage) = completion(&events);
        assert!(final_text.contains("imates M0 测试目录"));
        assert_eq!(usage.input_tokens, Some(37525));
        assert_eq!(usage.output_tokens, Some(521));
        assert_eq!(usage.cache_read_input_tokens, Some(3200));
        assert_eq!(usage.cache_creation_input_tokens, Some(0));
        assert_eq!(usage.cost_usd, Some(0.079776));
        assert_eq!(usage.raw_usage.as_ref().unwrap()["output_tokens"], 521);
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
    }

    #[test]
    fn execute_and_resume_share_a_session_and_report_per_turn_numbers() {
        let execute = feed(include_str!("../../tests/fixtures/grok/execute.jsonl"), RunMode::Execute);
        assert!(execute.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "01a1c3fa-67ee-7753-b052-bd4b77b54600")));
        assert!(execute.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, summary } if name == "write" && !summary.contains("hello.txt"))));
        let (final_text, usage) = completion(&execute);
        assert!(final_text.contains("hello.txt"));
        assert_eq!((usage.input_tokens, usage.output_tokens, usage.cost_usd), (Some(18531), Some(260), Some(0.049374)));

        let resume = feed(include_str!("../../tests/fixtures/grok/resume.jsonl"), RunMode::Discuss);
        assert!(resume.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "01a1c3fa-67ee-7753-b052-bd4b77b54600")));
        let (_, usage) = completion(&resume);
        // 同一会话的后一回合数值更小，证明不是累计。
        assert_eq!((usage.input_tokens, usage.output_tokens, usage.cost_usd), (Some(316), Some(257), Some(0.012222)));
    }

    #[test]
    fn real_error_fails_with_the_cli_message_and_never_completes() {
        let events = feed(include_str!("../../tests/fixtures/grok/error.jsonl"), RunMode::Discuss);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::Error { message } if message.contains("unknown model id"))));
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { .. })));
        assert_eq!(events.iter().filter(|e| matches!(e, AgentEvent::Error { .. })).count(), 1, "stderr 的纯文本行不再报一次");
    }

    #[test]
    fn permission_echo_is_checked_and_failures_latch() {
        let mut adapter = GrokAdapter::new("grok".into(), RunMode::Discuss);
        let events = adapter.parse_line(r#"{"type":"system","subtype":"init","session_id":"s","permissionMode":"bypassPermissions"}"#);
        assert!(matches!(events.as_slice(), [AgentEvent::Error { message }] if message.contains("权限模式不符")));
        assert!(adapter.parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"迟到成功"}"#).is_empty());

        let mut adapter = GrokAdapter::new("grok".into(), RunMode::Execute);
        assert!(matches!(adapter.parse_line(r#"{"type":"system","subtype":"init","session_id":"s","permissionMode":"bypassPermissions"}"#).as_slice(), [AgentEvent::SessionStarted { .. }]));
        assert!(matches!(adapter.parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"好","usage":{"output_tokens":3}}"#).as_slice(),
            [AgentEvent::TurnCompleted { usage: Some(usage), .. }] if usage.cost_usd.is_none() && usage.input_tokens.is_none() && usage.output_tokens == Some(3)));
        assert!(adapter.parse_line(r#"{"type":"result","subtype":"success","is_error":false,"result":"第二次"}"#).is_empty());
    }

    #[test]
    fn command_passes_the_prompt_only_through_a_temp_file() {
        let resolved = crate::cli_registry::ResolvedCli {
            info: crate::cli_registry::CliInfo { id: "grok".into(), label: "Grok".into(), installed: true,
                supported: true, path: Some("/opt/bin/grok".into()), version: None },
            program: "/opt/bin/grok".into(), path_env: "/opt/bin:/usr/bin".into(),
            environment_source: crate::cli_registry::EnvironmentSource::Process,
        };
        let request = TurnRequest { prompt: "- 第一行\n第二行".into(), session_id: Some("会话一".into()),
            cwd: "/tmp/项目".into(), mode: RunMode::Execute, model: Some("模型一".into()) };
        let adapter = GrokAdapter::new("ignored".into(), RunMode::Execute).with_environment(&resolved);
        let spec = adapter.build_command(&request);
        assert_eq!(spec.program, std::path::PathBuf::from("/opt/bin/grok"));
        assert_eq!(spec.path_env.as_deref(), Some(std::ffi::OsStr::new("/opt/bin:/usr/bin")));
        assert!(spec.stdin.is_none());
        let file = spec.prompt_file.clone().unwrap();
        assert!(file.is_absolute() && !file.exists(), "适配器只给路径，不创建文件");
        assert!(file.file_name().unwrap().to_string_lossy().starts_with("imates-prompt-"));
        assert!(spec.args.windows(2).any(|a| a[0] == "--prompt-file" && a[1] == file.to_string_lossy()));
        assert!(spec.args.windows(2).any(|a| a == ["--output-format", "streaming-messages-json"]));
        assert!(spec.args.windows(2).any(|a| a == ["--permission-mode", "bypassPermissions"]));
        assert!(spec.args.windows(2).any(|a| a == ["-r", "会话一"]));
        assert!(spec.args.windows(2).any(|a| a == ["-m", "模型一"]));
        assert!(!spec.args.iter().any(|arg| arg == &request.prompt || arg == "--cwd"));
        assert_ne!(adapter.build_command(&request).prompt_file, spec.prompt_file, "每次调用用新的文件名");

        let discuss = GrokAdapter::new("grok".into(), RunMode::Discuss);
        let spec = discuss.build_command(&TurnRequest { session_id: None, model: Some("  ".into()), ..request });
        assert!(spec.args.windows(2).any(|a| a == ["--permission-mode", "plan"]));
        assert!(!spec.args.iter().any(|arg| arg == "-r" || arg == "-m"));
    }

    fn reported(events: &[AgentEvent]) -> Vec<&str> {
        events.iter().filter_map(|event| match event { AgentEvent::ModelReported { model } => Some(model.as_str()), _ => None }).collect()
    }

    #[test]
    fn init_reports_the_model_the_cli_says_it_used() {
        let discuss = feed(include_str!("../../tests/fixtures/grok/discuss.jsonl"), RunMode::Discuss);
        assert_eq!(reported(&discuss), ["grok-4.7"]);
        let execute = feed(include_str!("../../tests/fixtures/grok/execute.jsonl"), RunMode::Execute);
        assert_eq!(reported(&execute), ["grok-4.7"]);
        let resume = feed(include_str!("../../tests/fixtures/grok/resume.jsonl"), RunMode::Discuss);
        assert_eq!(reported(&resume), ["grok-4.7"]);
        let session = discuss.iter().position(|e| matches!(e, AgentEvent::SessionStarted { .. })).unwrap();
        let model = discuss.iter().position(|e| matches!(e, AgentEvent::ModelReported { .. })).unwrap();
        assert!(session < model);
    }

    #[test]
    fn init_reports_whatever_model_the_cli_echoes_even_when_unknown() {
        let events = feed(include_str!("../../tests/fixtures/grok/error.jsonl"), RunMode::Discuss);
        assert_eq!(reported(&events), ["no-such-model"]);
    }

    #[test]
    fn init_with_unsafe_model_reports_nothing_but_keeps_the_session() {
        let init = |model: &str| format!(r#"{{"type":"system","subtype":"init","session_id":"s","permissionMode":"plan"{model}}}"#);
        let mut adapter = GrokAdapter::new("grok".into(), RunMode::Discuss);
        let control = adapter.parse_line(&init(r#","model":"bad\u0007name""#));
        assert!(matches!(control.as_slice(), [AgentEvent::SessionStarted { .. }]));
        let long = adapter.parse_line(&init(&format!(r#","model":"{}""#, "m".repeat(201))));
        assert!(matches!(long.as_slice(), [AgentEvent::SessionStarted { .. }]));
        let limit = adapter.parse_line(&init(&format!(r#","model":"{}""#, "m".repeat(200))));
        assert_eq!(reported(&limit), ["m".repeat(200).as_str()]);
    }
}
