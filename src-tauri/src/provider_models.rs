//! 按供应商查询 Claude / Codex 的可用模型。
//!
//! 密钥只在本模块内读取和使用：`ProviderEndpoint` 不实现 `Debug`/`Clone`/`Serialize`，
//! 所有错误文本只含失败类别或状态码，不含密钥、请求头、响应体或配置文件片段。

use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub const MAX_MODELS: usize = 200;
pub const MAX_BODY: usize = 1024 * 1024;

const NO_PROVIDER: &str = "未找到自定义供应商配置，请手填模型";
const NO_KEY: &str = "未找到该供应商的密钥";

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Kind { Codex, Claude }

enum Auth { Bearer(String), ApiKey(String) }

/// 供应商的模型列表端点和认证信息。故意不实现 Debug/Clone/Serialize，避免密钥泄露。
pub struct ProviderEndpoint {
    pub url: String,
    /// 只含主机名，用于界面提示发给谁。
    pub host: String,
    auth: Auth,
    kind: Kind,
}

pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

impl ProviderEndpoint {
    /// 含密钥，只在本模块内使用（`fetch_models`）。
    fn headers(&self) -> Vec<(&'static str, String)> {
        let mut headers = vec![("Accept", "application/json".to_string())];
        match &self.auth {
            Auth::Bearer(token) => headers.push(("Authorization", format!("Bearer {token}"))),
            Auth::ApiKey(key) => headers.push(("x-api-key", key.clone())),
        }
        if self.kind == Kind::Claude { headers.push(("anthropic-version", "2023-06-01".to_string())); }
        headers
    }
}

fn non_empty(value: Option<String>) -> Option<String> {
    value.map(|text| text.trim().to_string()).filter(|text| !text.is_empty())
}

fn user_home(env: Env) -> Option<PathBuf> {
    non_empty(env("USERPROFILE")).or_else(|| non_empty(env("HOME"))).map(PathBuf::from)
}

fn codex_home(env: Env) -> Result<PathBuf, String> {
    if let Some(dir) = non_empty(env("CODEX_HOME")) { return Ok(PathBuf::from(dir)); }
    user_home(env).map(|home| home.join(".codex")).ok_or_else(|| "找不到用户主目录，无法定位 Codex 配置".to_string())
}

fn claude_home(env: Env) -> Result<PathBuf, String> {
    if let Some(dir) = non_empty(env("CLAUDE_CONFIG_DIR")) { return Ok(PathBuf::from(dir)); }
    user_home(env).map(|home| home.join(".claude")).ok_or_else(|| "找不到用户主目录，无法定位 Claude 配置".to_string())
}

/// 校验 Base URL：只允许 http/https、必须有主机名、不含账号密码、查询和片段。返回（去掉末尾 / 的地址，主机名）。
fn validate_base_url(raw: &str) -> Result<(String, String), String> {
    let value = raw.trim();
    let url = tauri::Url::parse(value).map_err(|_| "供应商地址格式不对".to_string())?;
    if !matches!(url.scheme(), "http" | "https") { return Err("供应商地址只支持 http 或 https".into()); }
    let host = url.host_str().ok_or_else(|| "供应商地址缺少主机名".to_string())?.to_string();
    if !url.username().is_empty() || url.password().is_some() { return Err("供应商地址不能包含账号或密码".into()); }
    if url.query().is_some() || url.fragment().is_some() { return Err("供应商地址不能包含查询参数或片段".into()); }
    // 用解析后的规整串，保证界面显示的 host 与真正请求的地址一致。
    Ok((url.as_str().trim_end_matches('/').to_string(), host))
}

fn read_auth_json_key(dir: &Path) -> Option<String> {
    let text = std::fs::read_to_string(dir.join("auth.json")).ok()?;
    let value: serde_json::Value = serde_json::from_str(text.trim_start_matches('\u{feff}')).ok()?;
    non_empty(value.get("OPENAI_API_KEY").and_then(|key| key.as_str()).map(str::to_string))
}

pub fn codex_endpoint(env: Env) -> Result<ProviderEndpoint, String> {
    let dir = codex_home(env)?;
    let text = std::fs::read_to_string(dir.join("config.toml")).map_err(|_| NO_PROVIDER.to_string())?;
    // toml 的解析错误会带文件片段（可能含密钥），一律丢弃，换成固定文案。
    let table: toml::Table = text.parse().map_err(|_| "Codex 配置文件格式不对".to_string())?;
    let provider_id = table.get("model_provider").and_then(|value| value.as_str()).ok_or(NO_PROVIDER)?;
    let provider = table.get("model_providers").and_then(|value| value.get(provider_id)).and_then(|value| value.as_table()).ok_or(NO_PROVIDER)?;
    let base_url = provider.get("base_url").and_then(|value| value.as_str()).ok_or(NO_PROVIDER)?;
    let (base, host) = validate_base_url(base_url)?;
    let token = if let Some(token) = non_empty(provider.get("experimental_bearer_token").and_then(|value| value.as_str()).map(str::to_string)) {
        token
    } else if let Some(name) = non_empty(provider.get("env_key").and_then(|value| value.as_str()).map(str::to_string)) {
        // 配了 env_key 就只认该变量，不回落到 auth.json，避免把 OpenAI 官方密钥发给第三方网关。
        non_empty(env(&name)).ok_or_else(|| format!("环境变量 {name} 未设置（从图形界面启动的应用可能读不到 shell 里的变量），请手填模型"))?
    } else if provider.get("requires_openai_auth").and_then(|value| value.as_bool()) == Some(true) {
        // Codex 自己也会把 auth.json 里的密钥发给此类供应商。
        read_auth_json_key(&dir).ok_or(NO_KEY)?
    } else {
        return Err(NO_KEY.to_string());
    };
    Ok(ProviderEndpoint { url: format!("{base}/models"), host, auth: Auth::Bearer(token), kind: Kind::Codex })
}

/// 读 settings.json 的 `env` 对象，只取字符串值。文件缺失或损坏时视为空。
fn claude_settings_env(dir: &Path) -> HashMap<String, String> {
    let Ok(text) = std::fs::read_to_string(dir.join("settings.json")) else { return HashMap::new(); };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else { return HashMap::new(); };
    value.get("env").and_then(|env| env.as_object()).map(|object| {
        object.iter().filter_map(|(key, value)| value.as_str().map(|text| (key.clone(), text.to_string()))).collect()
    }).unwrap_or_default()
}

pub fn claude_endpoint(env: Env) -> Result<ProviderEndpoint, String> {
    let dir = claude_home(env)?;
    let file_env = claude_settings_env(&dir);
    // 进程环境变量覆盖 settings.json
    let get = |name: &str| non_empty(env(name)).or_else(|| non_empty(file_env.get(name).cloned()));
    let base_url = get("ANTHROPIC_BASE_URL").ok_or(NO_PROVIDER)?;
    let (base, host) = validate_base_url(&base_url)?;
    let auth = match (get("ANTHROPIC_AUTH_TOKEN"), get("ANTHROPIC_API_KEY")) {
        (Some(token), _) => Auth::Bearer(token),
        (None, Some(key)) => Auth::ApiKey(key),
        (None, None) => return Err(NO_KEY.to_string()),
    };
    let url = if base.ends_with("/v1") { format!("{base}/models") } else { format!("{base}/v1/models") };
    Ok(ProviderEndpoint { url, host, auth, kind: Kind::Claude })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    const SECRET: &str = "sk-test-SECRET";

    /// 让 CODEX_HOME 与 CLAUDE_CONFIG_DIR 都指向同一个临时目录，其余变量取自 extra。
    fn env_with<'a>(dir: &Path, extra: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        let dir = dir.to_string_lossy().into_owned();
        move |name| match name {
            "CODEX_HOME" | "CLAUDE_CONFIG_DIR" => Some(dir.clone()),
            other => extra.iter().find(|(key, _)| *key == other).map(|(_, value)| (*value).to_string()),
        }
    }

    fn err(result: Result<ProviderEndpoint, String>) -> String {
        match result { Ok(_) => panic!("应当失败"), Err(error) => error }
    }

    fn write(dir: &Path, name: &str, text: &str) { std::fs::write(dir.join(name), text).unwrap(); }

    fn codex_config(extra_provider_lines: &str) -> String {
        format!("model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"https://gw.example.com/v1/\"\n{extra_provider_lines}")
    }

    #[test]
    fn codex_uses_bearer_token_from_the_provider_table() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "config.toml", &codex_config(&format!("experimental_bearer_token = \"{SECRET}\"\n")));
        let endpoint = codex_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert_eq!(endpoint.url, "https://gw.example.com/v1/models");
        assert_eq!(endpoint.host, "gw.example.com");
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}"))));
    }

    #[test]
    fn codex_key_sources_follow_the_documented_order() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "auth.json", &format!("{{\"OPENAI_API_KEY\":\"{SECRET}-auth\"}}"));
        // 只有 auth.json
        write(dir.path(), "config.toml", &codex_config("requires_openai_auth = true\n"));
        let endpoint = codex_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}-auth"))));
        // env_key 优先于 auth.json
        write(dir.path(), "config.toml", &codex_config("env_key = \"MY_KEY\"\n"));
        let extra = [("MY_KEY", "sk-from-env")];
        let endpoint = codex_endpoint(&env_with(dir.path(), &extra)).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", "Bearer sk-from-env".to_string())));
        // experimental_bearer_token 优先于 env_key
        write(dir.path(), "config.toml", &codex_config(&format!("env_key = \"MY_KEY\"\nexperimental_bearer_token = \"{SECRET}\"\n")));
        let endpoint = codex_endpoint(&env_with(dir.path(), &extra)).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}"))));
    }

    #[test]
    fn codex_reports_missing_provider_or_key_without_leaking_anything() {
        let dir = tempfile::tempdir().unwrap();
        // 没有配置文件
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到自定义供应商配置"));
        // 没有 model_provider
        write(dir.path(), "config.toml", "model = \"x\"\n");
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到自定义供应商配置"));
        // 供应商表缺失
        write(dir.path(), "config.toml", "model_provider = \"custom\"\n");
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到自定义供应商配置"));
        // 有 base_url 没密钥
        write(dir.path(), "config.toml", &codex_config(""));
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到该供应商的密钥"));
        // 配置损坏：错误文本固定，不带文件片段
        write(dir.path(), "config.toml", &format!("model_provider = \"custom\"\nexperimental_bearer_token = \"{SECRET}\" = =\n"));
        let message = err(codex_endpoint(&env_with(dir.path(), &[])));
        assert!(message.contains("格式不对") && !message.contains(SECRET), "{message}");
    }

    #[test]
    fn base_url_must_be_plain_http_or_https() {
        let dir = tempfile::tempdir().unwrap();
        for bad in ["ftp://gw.example.com/v1", "gw.example.com", "https://user:pass@gw.example.com/v1", "https://gw.example.com/v1?x=1", "https://gw.example.com/v1#frag", ""] {
            let toml = format!("model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"{bad}\"\nexperimental_bearer_token = \"{SECRET}\"\n");
            write(dir.path(), "config.toml", &toml);
            let message = err(codex_endpoint(&env_with(dir.path(), &[])));
            assert!(!message.contains(SECRET) && !message.contains("pass"), "{bad}: {message}");
        }
        write(dir.path(), "config.toml", &format!("model_provider = \"custom\"\n[model_providers.custom]\nbase_url = \"http://127.0.0.1:8080\"\nexperimental_bearer_token = \"{SECRET}\"\n"));
        assert_eq!(codex_endpoint(&env_with(dir.path(), &[])).unwrap().url, "http://127.0.0.1:8080/models");
    }

    #[test]
    fn codex_home_env_overrides_the_default_directory() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "config.toml", &codex_config(&format!("experimental_bearer_token = \"{SECRET}\"\n")));
        // 不设 CODEX_HOME、也没有主目录变量：找不到目录
        let none = |_: &str| None::<String>;
        assert!(err(codex_endpoint(&none)).contains("主目录"));
        assert!(codex_endpoint(&env_with(dir.path(), &[])).is_ok());
    }

    fn claude_settings(env_json: &str) -> String { format!("{{\"env\":{env_json}}}") }

    #[test]
    fn claude_builds_models_url_and_bearer_headers() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "settings.json", &claude_settings(&format!("{{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com\",\"ANTHROPIC_AUTH_TOKEN\":\"{SECRET}\"}}")));
        let endpoint = claude_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert_eq!(endpoint.url, "https://gw.example.com/v1/models");
        assert_eq!(endpoint.host, "gw.example.com");
        let headers = endpoint.headers();
        assert!(headers.contains(&("Authorization", format!("Bearer {SECRET}"))));
        assert!(headers.contains(&("anthropic-version", "2023-06-01".to_string())));
    }

    #[test]
    fn claude_does_not_double_the_v1_segment_and_supports_api_key() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "settings.json", &claude_settings(&format!("{{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com/v1/\",\"ANTHROPIC_API_KEY\":\"{SECRET}\"}}")));
        let endpoint = claude_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert_eq!(endpoint.url, "https://gw.example.com/v1/models");
        let headers = endpoint.headers();
        assert!(headers.contains(&("x-api-key", SECRET.to_string())));
        assert!(!headers.iter().any(|(name, _)| *name == "Authorization"));
    }

    #[test]
    fn claude_token_beats_api_key_and_process_env_beats_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "settings.json", &claude_settings("{\"ANTHROPIC_BASE_URL\":\"https://file.example.com\",\"ANTHROPIC_AUTH_TOKEN\":\"sk-file-token\",\"ANTHROPIC_API_KEY\":\"sk-file-key\"}"));
        let from_file = claude_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(from_file.headers().contains(&("Authorization", "Bearer sk-file-token".to_string())));
        let extra = [("ANTHROPIC_BASE_URL", "https://proc.example.com"), ("ANTHROPIC_AUTH_TOKEN", "sk-proc-token")];
        let from_env = claude_endpoint(&env_with(dir.path(), &extra)).unwrap();
        assert_eq!(from_env.host, "proc.example.com");
        assert!(from_env.headers().contains(&("Authorization", "Bearer sk-proc-token".to_string())));
    }

    #[test]
    fn claude_reports_missing_provider_or_key_without_leaking_anything() {
        let dir = tempfile::tempdir().unwrap();
        assert!(err(claude_endpoint(&env_with(dir.path(), &[]))).contains("未找到自定义供应商配置"));
        write(dir.path(), "settings.json", "{坏的 json");
        assert!(err(claude_endpoint(&env_with(dir.path(), &[]))).contains("未找到自定义供应商配置"));
        write(dir.path(), "settings.json", &claude_settings("{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com\"}"));
        assert!(err(claude_endpoint(&env_with(dir.path(), &[]))).contains("未找到该供应商的密钥"));
        write(dir.path(), "settings.json", &claude_settings(&format!("{{\"ANTHROPIC_BASE_URL\":\"ftp://x\",\"ANTHROPIC_AUTH_TOKEN\":\"{SECRET}\"}}")));
        assert!(!err(claude_endpoint(&env_with(dir.path(), &[]))).contains(SECRET));
    }

    #[test]
    fn codex_env_key_unset_errors_instead_of_falling_back_to_auth_json() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "auth.json", &format!("{{\"OPENAI_API_KEY\":\"{SECRET}-auth\"}}"));
        // 即使 requires_openai_auth = true，配了 env_key 就不能回落
        write(dir.path(), "config.toml", &codex_config("env_key = \"MY_KEY\"\nrequires_openai_auth = true\n"));
        for extra in [&[][..], &[("MY_KEY", "")][..], &[("MY_KEY", "  ")][..]] {
            let message = err(codex_endpoint(&env_with(dir.path(), extra)));
            assert!(message.contains("环境变量") && message.contains("MY_KEY"), "{message}");
            assert!(!message.contains(SECRET), "{message}");
        }
    }

    #[test]
    fn codex_auth_json_needs_requires_openai_auth_true() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "auth.json", &format!("{{\"OPENAI_API_KEY\":\"{SECRET}-auth\"}}"));
        // 没有 requires_openai_auth
        write(dir.path(), "config.toml", &codex_config(""));
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到该供应商的密钥"));
        // requires_openai_auth = false
        write(dir.path(), "config.toml", &codex_config("requires_openai_auth = false\n"));
        let message = err(codex_endpoint(&env_with(dir.path(), &[])));
        assert!(message.contains("未找到该供应商的密钥") && !message.contains(SECRET), "{message}");
        // 字符串 "true" 不算布尔 true
        write(dir.path(), "config.toml", &codex_config("requires_openai_auth = \"true\"\n"));
        assert!(err(codex_endpoint(&env_with(dir.path(), &[]))).contains("未找到该供应商的密钥"));
        // requires_openai_auth = true
        write(dir.path(), "config.toml", &codex_config("requires_openai_auth = true\n"));
        let endpoint = codex_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}-auth"))));
    }

    #[test]
    fn codex_provider_table_without_base_url_is_reported_as_missing_provider() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "config.toml", &format!("model_provider = \"custom\"\n[model_providers.custom]\nexperimental_bearer_token = \"{SECRET}\"\n"));
        let message = err(codex_endpoint(&env_with(dir.path(), &[])));
        assert!(message.contains("未找到自定义供应商配置") && !message.contains(SECRET), "{message}");
    }

    #[test]
    fn files_with_a_utf8_bom_are_still_parsed() {
        let dir = tempfile::tempdir().unwrap();
        // config.toml 带 BOM
        let config = format!("\u{feff}{}", codex_config(&format!("experimental_bearer_token = \"{SECRET}\"\n")));
        write(dir.path(), "config.toml", &config);
        let endpoint = codex_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}"))));
        // auth.json 带 BOM
        write(dir.path(), "config.toml", &codex_config("requires_openai_auth = true\n"));
        write(dir.path(), "auth.json", &format!("\u{feff}{{\"OPENAI_API_KEY\":\"{SECRET}-auth\"}}"));
        let endpoint = codex_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}-auth"))));
        // settings.json 带 BOM
        let settings = format!("\u{feff}{}", claude_settings(&format!("{{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com\",\"ANTHROPIC_AUTH_TOKEN\":\"{SECRET}\"}}")));
        write(dir.path(), "settings.json", &settings);
        let endpoint = claude_endpoint(&env_with(dir.path(), &[])).unwrap();
        assert!(endpoint.headers().contains(&("Authorization", format!("Bearer {SECRET}"))));
    }

    #[test]
    fn default_directories_come_from_userprofile_or_home() {
        for variable in ["USERPROFILE", "HOME"] {
            let home = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(home.path().join(".codex")).unwrap();
            std::fs::create_dir_all(home.path().join(".claude")).unwrap();
            write(&home.path().join(".codex"), "config.toml", &codex_config(&format!("experimental_bearer_token = \"{SECRET}\"\n")));
            write(&home.path().join(".claude"), "settings.json", &claude_settings(&format!("{{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com\",\"ANTHROPIC_AUTH_TOKEN\":\"{SECRET}\"}}")));
            let path = home.path().to_string_lossy().into_owned();
            let env = |name: &str| if name == variable { Some(path.clone()) } else { None };
            assert_eq!(codex_endpoint(&env).unwrap().host, "gw.example.com", "{variable}");
            assert_eq!(claude_endpoint(&env).unwrap().host, "gw.example.com", "{variable}");
        }
    }

    #[test]
    fn claude_process_env_api_key_beats_settings_file() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), "settings.json", &claude_settings("{\"ANTHROPIC_BASE_URL\":\"https://gw.example.com\",\"ANTHROPIC_API_KEY\":\"sk-file-key\"}"));
        let extra = [("ANTHROPIC_API_KEY", "sk-proc-key")];
        let endpoint = claude_endpoint(&env_with(dir.path(), &extra)).unwrap();
        assert!(endpoint.headers().contains(&("x-api-key", "sk-proc-key".to_string())));
        assert!(!endpoint.headers().contains(&("x-api-key", "sk-file-key".to_string())));
    }
}
