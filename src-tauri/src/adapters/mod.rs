pub mod dsh;
pub mod grok;
pub mod codex;
pub mod claude;
use std::ffi::OsString;
use std::path::PathBuf;
use serde::{Deserialize, Serialize};
use crate::cli_registry::CliInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunMode { Discuss, Execute }

#[derive(Debug, Clone)]
pub struct TurnRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub cwd: PathBuf,
    pub mode: RunMode,
    pub model: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: PathBuf,
    pub args: Vec<String>,
    pub path_env: Option<OsString>,
    pub stdin: Option<String>,
    pub prompt_file: Option<PathBuf>,
    pub support_files: Vec<SupportFile>,
}

/// runner 为本次调用创建并清理的辅助脚本或配置，不包含用户凭据。
#[derive(Debug, Clone)]
pub struct SupportFile { pub path: PathBuf, pub content: String }

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_input_tokens: Option<u64>,
    pub cache_creation_input_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub raw_usage: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    SessionStarted { session_id: String },
    /// CLI 在事件流里回报的实际模型名（只有 Claude、Grok 的 init 事件带它）。
    ModelReported { model: String },
    TextDelta { text: String },
    ToolUse { name: String, summary: String },
    ToolState { active: bool },
    TurnCompleted { final_text: String, usage: Option<Usage> },
    Error { message: String },
}

/// 从 CLI 事件对象的 `model` 字段生成 `ModelReported`。
/// 去掉首尾空白后为空、超过 200 个字符或含控制字符的值一律不接受。
pub(crate) fn model_event(value: &serde_json::Value) -> Option<AgentEvent> {
    let model = value["model"].as_str()?.trim();
    if model.is_empty() || model.chars().count() > 200 || model.chars().any(char::is_control) {
        return None;
    }
    Some(AgentEvent::ModelReported { model: model.into() })
}

pub trait CliAdapter: Send + Sync {
    fn id(&self) -> &'static str;
    fn detect(&self) -> Option<CliInfo>;
    fn build_command(&self, req: &TurnRequest) -> CommandSpec;
    /// 只覆盖本次子进程的环境；None 表示移除继承的变量。
    fn environment(&self) -> Vec<(String, Option<String>)> { vec![] }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent>;
}

/// 测试与故障演练使用显式进程，不经过 shell 拼接提示词。
#[cfg(test)]
pub struct FakeAdapter { program: PathBuf, args: Vec<String>, prompt_in_file: bool }
#[cfg(test)]
impl FakeAdapter {
    pub fn new(program: PathBuf, args: Vec<String>) -> Self { Self { program, args, prompt_in_file: false } }
    /// 提示词改为经 `<cwd>/prompt-under-test.md` 传入，用来测 runner 的文件通道。
    pub fn with_prompt_file(mut self) -> Self { self.prompt_in_file = true; self }
}
#[cfg(test)]
impl CliAdapter for FakeAdapter {
    fn id(&self) -> &'static str { "fake" }
    fn detect(&self) -> Option<CliInfo> { None }
    fn build_command(&self, req: &TurnRequest) -> CommandSpec {
        if self.prompt_in_file {
            return CommandSpec { program: self.program.clone(), args: self.args.clone(), path_env: None,
                stdin: None, prompt_file: Some(req.cwd.join("prompt-under-test.md")), support_files: vec![] };
        }
        CommandSpec { program: self.program.clone(), args: self.args.clone(), path_env: None,
            stdin: Some(req.prompt.clone()), prompt_file: None, support_files: vec![] }
    }
    fn parse_line(&mut self, line: &str) -> Vec<AgentEvent> {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else { return vec![]; };
        let text = v["text"].as_str().unwrap_or_default().to_string();
        match v["type"].as_str() {
            Some("text") => vec![AgentEvent::TextDelta { text }],
            Some("session") => vec![AgentEvent::SessionStarted { session_id: text }],
            Some("tool") => vec![AgentEvent::ToolState { active: v["active"].as_bool().unwrap_or(false) }],
            Some("complete") => vec![AgentEvent::TurnCompleted { final_text: text, usage: None }],
            Some("error") => vec![AgentEvent::Error { message: text }],
            _ => vec![],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usage_uses_camel_case_and_preserves_unknown_fields_as_null() {
        let usage = Usage { input_tokens: Some(12), output_tokens: Some(3), ..Default::default() };
        let json = serde_json::to_value(&usage).unwrap();
        assert_eq!(json["inputTokens"], 12);
        assert_eq!(json["outputTokens"], 3);
        assert!(json["cacheReadInputTokens"].is_null());
        assert!(json.get("input_tokens").is_none());
        assert_eq!(serde_json::to_value(RunMode::Discuss).unwrap(), "discuss");
        assert_eq!(serde_json::to_value(RunMode::Execute).unwrap(), "execute");
    }

    #[test]
    fn model_event_accepts_plain_names_and_rejects_unsafe_values() {
        let event = |model: serde_json::Value| model_event(&serde_json::json!({ "model": model }));
        assert_eq!(event("claude-sonnet-5-5".into()), Some(AgentEvent::ModelReported { model: "claude-sonnet-5-5".into() }));
        assert_eq!(event("  grok-4.7 ".into()), Some(AgentEvent::ModelReported { model: "grok-4.7".into() }));
        assert_eq!(event("模型一".into()), Some(AgentEvent::ModelReported { model: "模型一".into() }));
        assert_eq!(event("m".repeat(200).into()), Some(AgentEvent::ModelReported { model: "m".repeat(200) }));
        for rejected in [serde_json::Value::Null, 5.into(), "".into(), "   ".into(), "a\u{7}b".into(), "a\nb".into(), "m".repeat(201).into()] {
            assert_eq!(event(rejected.clone()), None, "{rejected}");
        }
        assert_eq!(model_event(&serde_json::json!({})), None);
    }

    #[test]
    fn fake_adapter_passes_prompt_through_stdin_and_keeps_cwd_on_request() {
        let adapter = FakeAdapter::new("/bin/test-helper".into(), vec![]);
        let request = TurnRequest {
            prompt: "- 参数一样的提示词\n下一行".into(),
            session_id: None,
            cwd: "/tmp/当前目录".into(),
            mode: RunMode::Discuss,
            model: None,
        };
        let spec = adapter.build_command(&request);
        assert_eq!(spec.stdin.as_deref(), Some(request.prompt.as_str()));
        assert!(!spec.args.contains(&request.prompt));
        assert_eq!(request.cwd, std::path::PathBuf::from("/tmp/当前目录"));
        assert_eq!(spec.program, std::path::PathBuf::from("/bin/test-helper"));
        assert!(spec.prompt_file.is_none());
    }
}
