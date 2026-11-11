use std::collections::HashSet;
use std::path::PathBuf;
use serde_json::{json, Value};
use super::{AgentEvent, CliAdapter, CommandSpec, RunMode, TurnRequest, Usage};
use crate::cli_registry::{self, CliInfo, ResolvedCli};
use crate::dsh_config::{self, CatalogCapture, DshSettings};

/// dsh（DeepSeek Harness）的无头适配器。事件格式依据 Windows 0.2.2-rc.1 的 `--json` 样本。
/// dsh 不回显权限模式，也没有 `--model`。
pub struct DshAdapter {
    program: PathBuf,
    path_env: Option<std::ffi::OsString>,
    mode: RunMode,
    active_tools: HashSet<String>,
    steps: Vec<Value>,
    had_text: bool,
    failed: bool,
    completed: bool,
    settings: DshSettings,
    catalog: Option<CatalogCapture>,
}

/// 回合没有正常完成时给用户看的说明。字段名来自实测记录。
fn reason_message(reason: &Value) -> String {
    let error = &reason["error"];
    match (error["code"].as_str(), error["message"].as_str()) {
        (Some(code), Some(message)) => format!("dsh 回合失败（{code}）：{message}"),
        (None, Some(message)) => format!("dsh 回合失败：{message}"),
        (Some(code), None) => format!("dsh 回合失败（{code}）"),
        (None, None) => format!("dsh 回合没有完成（{}）", reason["kind"].as_str().unwrap_or("未知原因")),
    }
}

impl DshAdapter {
    pub fn new(program: PathBuf, mode: RunMode) -> Self {
        Self { program, path_env: None, mode, active_tools: HashSet::new(), steps: Vec::new(),
            had_text: false, failed: false, completed: false, settings: DshSettings::default(), catalog: None }
    }
    pub fn with_environment(mut self, resolved: &ResolvedCli) -> Self {
        self.program = resolved.program.clone();
        self.path_env = Some(resolved.path_env.clone());
        self
    }
    pub fn with_settings(mut self, settings: DshSettings) -> Self { self.settings = settings; self }
    pub fn for_catalog(mut self, capture: CatalogCapture) -> Self { self.catalog = Some(capture); self }
    fn fail(&mut self, message: String) -> Vec<AgentEvent> {
        self.failed = true;
        vec![AgentEvent::Error { message }]
    }
    /// 逐步相加 `step_end.usage`。某个字段只要有一步缺失就整项留空，不拿零去补。
    fn usage(&self) -> Option<Usage> {
        if self.steps.is_empty() { return None; }
        let sum = |key: &str| self.steps.iter().map(|step| step[key].as_u64()).collect::<Option<Vec<_>>>().map(|values| values.into_iter().sum());
        Some(Usage { input_tokens: sum("inputTokens"), output_tokens: sum("outputTokens"),
            cache_read_input_tokens: sum("cacheReadTokens"), cache_creation_input_tokens: sum("cacheWriteTokens"),
            cost_usd: None, raw_usage: Some(json!({ "steps": self.steps })) })
    }
}

impl CliAdapter for DshAdapter {
    fn id(&self) -> &'static str { "dsh" }
    fn detect(&self) -> Option<CliInfo> { cli_registry::resolve_cli(self.id()).ok().map(|cli| cli.info) }
    fn build_command(&self, req: &TurnRequest) -> CommandSpec {
        let permission = match self.mode { RunMode::Discuss => "read-only", RunMode::Execute => "workspace-write" };
        let mut args: Vec<String> = vec!["headless".into(), "--json".into()];
        if let Some(id) = &req.session_id { args.extend(["--session-id".into(), id.clone()]); }
        args.extend(["--permission-mode".into(), permission.into(), "--approval-policy".into(), "never".into(), "-".into()]);
        let mut spec = CommandSpec { program: self.program.clone(), args, path_env: self.path_env.clone(),
            stdin: Some(req.prompt.clone()), prompt_file: None, support_files: vec![] };
        dsh_config::configure_command(&mut spec, req, &self.settings, self.catalog.is_some()); spec
    }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<Value>(line) else { return vec![]; };
        if self.failed || self.completed { return vec![]; }
        match v["type"].as_str() {
            Some("imates_dsh_catalog") if self.catalog.is_some() => {
                match dsh_config::DshCatalog::from_event(v) {
                    Ok(catalog) => {
                        if let Some(capture) = &self.catalog { if let Ok(mut captured) = capture.lock() { *captured = Some(catalog); } }
                        self.completed = true;
                        vec![AgentEvent::TurnCompleted { final_text: String::new(), usage: None }]
                    },
                    Err(error) => self.fail(error.to_string()),
                }
            }
            Some("session") => v["sessionId"].as_str()
                .map(|id| vec![AgentEvent::SessionStarted { session_id: id.into() }]).unwrap_or_default(),
            Some("text") => {
                let Some(text) = v["text"].as_str().filter(|text| !text.is_empty()) else { return vec![]; };
                let text = if self.had_text { format!("\n\n{text}") } else { text.to_string() };
                self.had_text = true;
                vec![AgentEvent::TextDelta { text }]
            }
            Some("tool_call") => {
                let Some(id) = v["callId"].as_str() else { return vec![]; };
                if !self.active_tools.insert(id.into()) { return vec![]; }
                // 只报工具名，不带参数。
                vec![AgentEvent::ToolUse { name: v["tool"].as_str().unwrap_or("工具").into(), summary: "正在运行工具".into() },
                    AgentEvent::ToolState { active: true }]
            }
            Some("tool_result") => {
                let Some(id) = v["callId"].as_str() else { return vec![]; };
                self.active_tools.remove(id);
                vec![AgentEvent::ToolState { active: !self.active_tools.is_empty() }]
            }
            Some("status") => match v["phase"].as_str() {
                Some("step_end") => {
                    if let Some(usage) = v.get("usage").filter(|usage| usage.is_object()) { self.steps.push(usage.clone()); }
                    vec![]
                }
                Some("turn_end") if v["reason"]["kind"] != "completed" => self.fail(reason_message(&v["reason"])),
                _ => vec![],
            },
            Some("final") => {
                if v["reason"]["kind"] != "completed" { return self.fail(reason_message(&v["reason"])); }
                self.completed = true;
                vec![AgentEvent::TurnCompleted { final_text: v["text"].as_str().unwrap_or_default().into(), usage: self.usage() }]
            }
            Some("error") => self.fail(v["message"].as_str().unwrap_or("dsh 返回错误").into()),
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapters::{AgentEvent, CliAdapter, RunMode, TurnRequest};

    fn feed(text: &str, mode: RunMode) -> Vec<AgentEvent> {
        let mut adapter = DshAdapter::new("dsh".into(), mode);
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
    fn real_catalog_is_captured_without_a_chat_session_or_usage() {
        let capture = std::sync::Arc::new(std::sync::Mutex::new(None));
        let mut adapter = DshAdapter::new("dsh".into(), RunMode::Discuss).for_catalog(capture.clone());
        let event = include_str!("../../tests/fixtures/dsh/models.json");
        let events = adapter.parse_line(event);
        assert_eq!(events, vec![AgentEvent::TurnCompleted { final_text: String::new(), usage: None }]);
        let stored = capture.lock().unwrap();
        let catalog = stored.as_ref().unwrap();
        assert_eq!(catalog.profile, crate::dsh_config::Profile::Web);
        let gpt = catalog.groups.iter().find(|group| group.id == "gpt").unwrap();
        assert!(gpt.models.iter().any(|model| model.id == "gpt-6"));
        assert!(catalog.groups.iter().any(|group| group.id == "deepseek-account"));
        assert!(catalog.failures.is_empty());
        assert!(adapter.parse_line(event).is_empty(), "目录只完成一次");
    }

    #[test]
    fn real_discuss_sums_step_usage_without_counting_final_again() {
        let events = feed(include_str!("../../tests/fixtures/dsh/discuss.jsonl"), RunMode::Discuss);
        assert!(events.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "session-af4b6cd9-c143-4e9a-a36e-2fbfeae47afe")));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::ToolUse { name, summary } if name == "read" && !summary.contains("README"))));
        let states: Vec<bool> = events.iter().filter_map(|e| match e { AgentEvent::ToolState { active } => Some(*active), _ => None }).collect();
        assert_eq!(states, [true, false]);
        let texts: Vec<&String> = events.iter().filter_map(|e| match e { AgentEvent::TextDelta { text } => Some(text), _ => None }).collect();
        assert_eq!(texts.len(), 2, "空的 thinking 不算正文");
        assert!(texts[0].starts_with("我叫 DeepSeek Harness"));
        assert!(texts[1].starts_with("\n\n"), "多步正文之间要分段");
        let (final_text, usage) = completion(&events);
        assert!(final_text.starts_with("README.md 只有一行内容"));
        assert_eq!(usage.input_tokens, Some(8154 + 140));
        assert_eq!(usage.output_tokens, Some(102 + 107));
        assert_eq!(usage.cache_read_input_tokens, Some(8192));
        assert_eq!(usage.cache_creation_input_tokens, Some(0));
        assert_eq!(usage.cost_usd, None);
        assert_eq!(usage.raw_usage.as_ref().unwrap()["steps"].as_array().unwrap().len(), 2);
        assert!(!events.iter().any(|e| matches!(e, AgentEvent::Error { .. })));
    }

    #[test]
    fn execute_and_resume_keep_the_session_and_report_their_own_usage() {
        let execute = feed(include_str!("../../tests/fixtures/dsh/execute.jsonl"), RunMode::Execute);
        assert!(execute.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "session-22cff073-10b6-4e6c-af0f-9aa9c35a230c")));
        let tools: Vec<&str> = execute.iter().filter_map(|e| match e { AgentEvent::ToolUse { name, .. } => Some(name.as_str()), _ => None }).collect();
        assert_eq!(tools, ["write", "read"]);
        let (final_text, usage) = completion(&execute);
        assert!(final_text.contains("hello.txt"));
        assert_eq!(usage.input_tokens, Some(2525 + 240 + 239));
        assert_eq!(usage.output_tokens, Some(94 + 63 + 20));
        assert_eq!(usage.cache_read_input_tokens, Some(5632 + 8064 + 8192));

        let resume = feed(include_str!("../../tests/fixtures/dsh/resume.jsonl"), RunMode::Discuss);
        assert!(resume.iter().any(|e| matches!(e, AgentEvent::SessionStarted { session_id } if session_id == "session-22cff073-10b6-4e6c-af0f-9aa9c35a230c")));
        let (_, usage) = completion(&resume);
        assert_eq!((usage.input_tokens, usage.output_tokens, usage.cache_read_input_tokens), (Some(146), Some(20), Some(8448)));
    }

    #[test]
    fn real_unknown_session_error_fails_once_and_never_completes() {
        let events = feed(include_str!("../../tests/fixtures/dsh/error.jsonl"), RunMode::Discuss);
        assert!(matches!(events.as_slice(), [AgentEvent::Error { message }] if message.contains("does not exist") && message.contains("--session-id")));
    }

    /// 余额不足的原始输出没有入库。下面两行按《CLI 实测记录》dsh 一节记载的字段构造，不是录制样本。
    #[test]
    fn documented_quota_failure_is_a_failed_turn_not_an_empty_reply() {
        let mut adapter = DshAdapter::new("dsh".into(), RunMode::Discuss);
        adapter.parse_line(r#"{"type":"session","sessionId":"session-x","cwd":"/tmp"}"#);
        let events = adapter.parse_line(r#"{"type":"status","phase":"turn_end","turn":1,"reason":{"kind":"error","error":{"code":"QUOTA","status":402,"message":"Insufficient Balance"}}}"#);
        assert!(matches!(events.as_slice(), [AgentEvent::Error { message }] if message.contains("QUOTA") && message.contains("Insufficient Balance")));
        assert!(adapter.parse_line(r#"{"type":"final","sessionId":"session-x","reason":{"kind":"error"},"text":""}"#).is_empty());

        let mut adapter = DshAdapter::new("dsh".into(), RunMode::Discuss);
        let events = adapter.parse_line(r#"{"type":"final","sessionId":"session-x","reason":{"kind":"error"},"text":""}"#);
        assert!(matches!(events.as_slice(), [AgentEvent::Error { .. }]), "没有 turn_end 时 final 的失败也不能算成功");
        assert!(adapter.parse_line(r#"{"type":"final","sessionId":"session-x","reason":{"kind":"completed"},"text":"迟到成功"}"#).is_empty());
    }

    #[test]
    fn failed_tool_result_is_not_a_failed_turn_and_missing_usage_stays_unknown() {
        let mut adapter = DshAdapter::new("dsh".into(), RunMode::Discuss);
        assert!(matches!(adapter.parse_line(r#"{"type":"tool_call","callId":"c1","tool":"write","input":{"file_path":"x"}}"#).as_slice(),
            [AgentEvent::ToolUse { .. }, AgentEvent::ToolState { active: true }]));
        // 只读沙箱拒绝写入是一次工具失败，回合照常继续。
        assert_eq!(adapter.parse_line(r#"{"type":"tool_result","callId":"c1","status":"error","error":{"name":"FsError","code":"FS_SANDBOX_DENIED"}}"#),
            vec![AgentEvent::ToolState { active: false }]);
        adapter.parse_line(r#"{"type":"status","phase":"step_end","turn":1,"step":1,"usage":{"inputTokens":5,"outputTokens":2}}"#);
        let events = adapter.parse_line(r#"{"type":"final","sessionId":"s","reason":{"kind":"completed"},"text":"写不了"}"#);
        assert!(matches!(events.as_slice(), [AgentEvent::TurnCompleted { final_text, usage: Some(usage) }]
            if final_text == "写不了" && usage.input_tokens == Some(5) && usage.output_tokens == Some(2)
                && usage.cache_read_input_tokens.is_none() && usage.cache_creation_input_tokens.is_none()));
        let mut bare = DshAdapter::new("dsh".into(), RunMode::Discuss);
        assert!(matches!(bare.parse_line(r#"{"type":"final","sessionId":"s","reason":{"kind":"completed"},"text":"好"}"#).as_slice(),
            [AgentEvent::TurnCompleted { usage: None, .. }]));
    }

    #[test]
    fn default_command_preserves_headless_profile_and_reads_prompt_from_stdin() {
        let request = TurnRequest { prompt: "- 第一行\n第二行".into(), session_id: Some("session-1".into()),
            cwd: "/tmp/项目".into(), mode: RunMode::Execute, model: None };
        let spec = DshAdapter::new("dsh".into(), RunMode::Execute).build_command(&request);
        assert_eq!(spec.args, ["headless", "--json", "--session-id", "session-1",
            "--permission-mode", "workspace-write", "--approval-policy", "never", "-"]);
        assert_eq!(spec.stdin.as_deref(), Some(request.prompt.as_str()));
        assert!(spec.prompt_file.is_none());
        assert!(spec.support_files.is_empty());
        let spec = DshAdapter::new("dsh".into(), RunMode::Discuss).build_command(&TurnRequest { session_id: None, ..request });
        assert_eq!(spec.args, ["headless", "--json", "--permission-mode", "read-only", "--approval-policy", "never", "-"]);
    }
}
