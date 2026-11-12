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

/// 等价于 `^[A-Za-z_][A-Za-z0-9_]{0,63}$`。
fn is_env_var_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else { return false; };
    (first.is_ascii_alphabetic() || first == '_')
        && name.len() <= 64
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
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
        // 只有合法的变量名才回显；用户误把密钥粘进 env_key 时，不能让它出现在错误里。
        non_empty(env(&name)).ok_or_else(|| if is_env_var_name(&name) {
            format!("环境变量 {name} 未设置（从图形界面启动的应用可能读不到 shell 里的变量），请手填模型")
        } else {
            "该供应商配置的 env_key 不是有效的环境变量名，请手填模型".to_string()
        })?
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

use crate::cli_config::CliModel;

fn valid_model_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 512 && !id.chars().any(char::is_control)
}

/// 读 Codex 的本地模型目录：`config.toml` 顶层 `model_catalog_json` 指向的文件。任何失败都返回空列表。
pub fn codex_catalog(env: Env) -> Vec<CliModel> {
    const MAX_CATALOG_BYTES: u64 = 4 * 1024 * 1024;
    let Ok(dir) = codex_home(env) else { return vec![]; };
    let Ok(text) = std::fs::read_to_string(dir.join("config.toml")) else { return vec![]; };
    let Ok(table) = text.trim_start_matches('\u{feff}').parse::<toml::Table>() else { return vec![]; };
    let Some(path) = table.get("model_catalog_json").and_then(|value| value.as_str()) else { return vec![]; };
    let path = Path::new(path.trim());
    let path = if path.is_absolute() { path.to_path_buf() } else { dir.join(path) };
    match std::fs::metadata(&path) { Ok(meta) if meta.len() <= MAX_CATALOG_BYTES => {} _ => return vec![] }
    let Ok(text) = std::fs::read_to_string(&path) else { return vec![]; };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(text.trim_start_matches('\u{feff}')) else { return vec![]; };
    let Some(items) = value.get("models").and_then(|models| models.as_array()) else { return vec![]; };
    let mut models: Vec<CliModel> = vec![];
    for item in items {
        let Some(slug) = item.get("slug").and_then(|slug| slug.as_str()).map(str::trim) else { continue; };
        if !valid_model_id(slug) || models.iter().any(|model| model.id == slug) { continue; }
        let label = item.get("display_name").and_then(|name| name.as_str()).map(str::trim).filter(|name| !name.is_empty()).unwrap_or(slug);
        models.push(CliModel { id: slug.to_string(), label: label.to_string(), custom: false });
        if models.len() >= MAX_MODELS { break; }
    }
    models
}

/// 解析供应商的 `/models` 响应：取 `data[].id`（Claude 另取 `display_name`）。第二项表示是否因超过上限而截断。
pub fn parse_models_response(kind: Kind, body: &str) -> Result<(Vec<CliModel>, bool), String> {
    let bad = || "响应格式不对，该供应商的模型列表可能不是标准格式".to_string();
    let value: serde_json::Value = serde_json::from_str(body).map_err(|_| bad())?;
    let items = value.get("data").and_then(|data| data.as_array()).ok_or_else(bad)?;
    let mut models: Vec<CliModel> = vec![];
    let mut truncated = false;
    for item in items {
        let Some(id) = item.get("id").and_then(|id| id.as_str()).map(str::trim) else { continue; };
        if !valid_model_id(id) || models.iter().any(|model| model.id == id) { continue; }
        if models.len() >= MAX_MODELS { truncated = true; break; }
        let label = if kind == Kind::Claude {
            item.get("display_name").and_then(|name| name.as_str()).map(str::trim).filter(|name| !name.is_empty()).unwrap_or(id)
        } else { id };
        models.push(CliModel { id: id.to_string(), label: label.to_string(), custom: false });
    }
    Ok((models, truncated))
}

// ---- 代理与请求发送 ----

#[derive(Clone, Debug, PartialEq)]
pub enum ProxyChoice { FromEnvironment, Direct, Url(String) }

/// 把应用的代理设置转成本次请求的代理选择。`None` 表示沿用进程环境；全空表示直连。
pub fn proxy_choice(https_target: bool, environment: Option<&crate::proxy::ProxyEnvironment>) -> ProxyChoice {
    let Some(environment) = environment else { return ProxyChoice::FromEnvironment; };
    let specific = if https_target { &environment.https } else { &environment.http };
    match specific.as_ref().or(environment.all.as_ref()) {
        Some(url) => ProxyChoice::Url(url.clone()),
        None => ProxyChoice::Direct,
    }
}

/// 一次请求。含密钥（headers），故意不实现 Debug。
pub struct HttpRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub proxy: ProxyChoice,
}

pub struct HttpResponse { pub status: u16, pub body: String }

pub type HttpFn<'a> = &'a dyn Fn(&HttpRequest) -> Result<HttpResponse, String>;

/// 向供应商查询模型列表。HTTP 发送由调用方注入；错误文本只含状态码或失败类别。
pub fn fetch_models(endpoint: &ProviderEndpoint, proxy: &ProxyChoice, http: HttpFn) -> Result<(Vec<CliModel>, bool), String> {
    let request = HttpRequest {
        url: endpoint.url.clone(),
        headers: endpoint.headers().into_iter().map(|(name, value)| (name.to_string(), value)).collect(),
        proxy: proxy.clone(),
    };
    let response = http(&request)?;
    match response.status {
        200..=299 => {}
        300..=399 => return Err("供应商返回了重定向，为避免把密钥发往别处，已拒绝跟随".into()),
        401 | 403 => return Err(format!("HTTP {}，请检查该 CLI 配置里的密钥", response.status)),
        404 => return Err("HTTP 404，该供应商可能不提供模型列表接口，请手填模型".into()),
        status => return Err(format!("HTTP {status}")),
    }
    if response.body.len() > MAX_BODY { return Err("响应过大，已放弃读取".into()); }
    parse_models_response(endpoint.kind, &response.body)
}

/// 生产用的发送实现：禁止重定向，20 秒超时，响应体最多 1 MB。
pub fn ureq_http(request: &HttpRequest) -> Result<HttpResponse, String> {
    use std::time::Duration;
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(20)))
        .max_redirects(0)
        .http_status_as_error(false);
    match &request.proxy {
        // ureq 默认从环境变量读取代理
        ProxyChoice::FromEnvironment => {}
        ProxyChoice::Direct => { builder = builder.proxy(None); }
        ProxyChoice::Url(url) => {
            let proxy = ureq::Proxy::new(url).map_err(|_| "代理地址无效".to_string())?;
            builder = builder.proxy(Some(proxy));
        }
    }
    let agent: ureq::Agent = builder.build().into();
    let mut call = agent.get(&request.url);
    for (name, value) in &request.headers { call = call.header(name.as_str(), value.as_str()); }
    // 传输层错误只归类，不带 URL 或头，避免泄露任何内容。
    let mut response = call.call().map_err(|error| match error {
        ureq::Error::Timeout(_) => "连接超时".to_string(),
        _ => "无法连接供应商或请求失败".to_string(),
    })?;
    let status = response.status().as_u16();
    let body = response.body_mut().with_config().limit(MAX_BODY as u64 + 1).read_to_string()
        .map_err(|_| "读取响应失败或响应过大".to_string())?;
    Ok(HttpResponse { status, body })
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

    #[test]
    fn codex_env_key_error_never_echoes_a_pasted_secret_but_echoes_valid_names() {
        let dir = tempfile::tempdir().unwrap();
        // 用户误把密钥本身粘进 env_key
        write(dir.path(), "config.toml", &codex_config(&format!("env_key = \"{SECRET}\"\n")));
        let message = err(codex_endpoint(&env_with(dir.path(), &[])));
        assert!(!message.contains(SECRET) && !message.contains("SECRET"), "{message}");
        assert!(message.contains("env_key"), "{message}");
        // 过长的名字也不回显
        let long = "A".repeat(65);
        write(dir.path(), "config.toml", &codex_config(&format!("env_key = \"{long}\"\n")));
        assert!(!err(codex_endpoint(&env_with(dir.path(), &[]))).contains(&long));
        // 合法变量名仍回显
        write(dir.path(), "config.toml", &codex_config("env_key = \"MY_KEY_1\"\n"));
        let message = err(codex_endpoint(&env_with(dir.path(), &[])));
        assert!(message.contains("MY_KEY_1"), "{message}");
    }

    use crate::cli_config::CliModel;
    use std::cell::RefCell;

    fn ids(models: &[CliModel]) -> Vec<&str> { models.iter().map(|m| m.id.as_str()).collect() }

    #[test]
    fn codex_catalog_reads_the_recorded_sample_and_tolerates_bad_files() {
        let dir = tempfile::tempdir().unwrap();
        let catalog = dir.path().join("my-catalog.json");
        std::fs::write(&catalog, include_str!("../tests/fixtures/codex/model_catalog.json")).unwrap();
        // 绝对路径
        write(dir.path(), "config.toml", &format!("model_catalog_json = '{}'\n", catalog.to_string_lossy()));
        let models = codex_catalog(&env_with(dir.path(), &[]));
        assert_eq!(ids(&models), ["gpt-6.2-sol", "codex-auto-review", "gpt-6-astra", "gpt-6.1-sol"]);
        assert_eq!(models[0].label, "GPT-6.2 Sol");
        assert_eq!(models[3].label, "gpt-6.1-sol", "没有 display_name 时用 slug");
        // 相对路径按 Codex 配置目录解析
        write(dir.path(), "config.toml", "model_catalog_json = 'my-catalog.json'\n");
        assert_eq!(codex_catalog(&env_with(dir.path(), &[])).len(), 4);
        // 缺文件、损坏、没有该键：都返回空
        write(dir.path(), "config.toml", "model_catalog_json = 'nope.json'\n");
        assert!(codex_catalog(&env_with(dir.path(), &[])).is_empty());
        std::fs::write(&catalog, "{坏的").unwrap();
        write(dir.path(), "config.toml", "model_catalog_json = 'my-catalog.json'\n");
        assert!(codex_catalog(&env_with(dir.path(), &[])).is_empty());
        write(dir.path(), "config.toml", "model = 'x'\n");
        assert!(codex_catalog(&env_with(dir.path(), &[])).is_empty());
    }

    #[test]
    fn parses_openai_and_anthropic_shaped_responses() {
        let openai = r#"{"object":"list","data":[{"id":"gpt-a"},{"id":"gpt-b","object":"model"}]}"#;
        let (models, truncated) = parse_models_response(Kind::Codex, openai).unwrap();
        assert_eq!(ids(&models), ["gpt-a", "gpt-b"]);
        assert!(!truncated && models.iter().all(|m| !m.custom && m.label == m.id));
        let anthropic = r#"{"data":[{"id":"claude-x","display_name":"Claude X"},{"id":"claude-y"}],"has_more":false}"#;
        let (models, _) = parse_models_response(Kind::Claude, anthropic).unwrap();
        assert_eq!(models[0].label, "Claude X");
        assert_eq!(models[1].label, "claude-y");
        assert!(parse_models_response(Kind::Codex, r#"{"data":[]}"#).unwrap().0.is_empty());
    }

    #[test]
    fn rejects_malformed_responses_and_drops_bad_ids() {
        for bad in ["", "不是 json", "[]", r#"{"models":[]}"#, r#"{"data":"x"}"#] {
            assert!(parse_models_response(Kind::Codex, bad).unwrap_err().contains("响应格式不对"), "{bad}");
        }
        let long = "x".repeat(600);
        let body = format!("{{\"data\":[{{\"id\":\"ok\"}},{{\"id\":\"\"}},{{\"id\":\"a\\nb\"}},{{\"id\":\"{long}\"}},{{\"id\":5}},{{}},{{\"id\":\"ok\"}}]}}");
        let (models, _) = parse_models_response(Kind::Codex, &body).unwrap();
        assert_eq!(ids(&models), ["ok"], "空、含控制字符、过长、非字符串、缺失、重复的 id 都被丢弃");
    }

    #[test]
    fn truncates_to_the_model_limit_and_says_so() {
        let items: Vec<String> = (0..250).map(|n| format!("{{\"id\":\"m{n}\"}}")).collect();
        let body = format!("{{\"data\":[{}]}}", items.join(","));
        let (models, truncated) = parse_models_response(Kind::Codex, &body).unwrap();
        assert_eq!(models.len(), MAX_MODELS);
        assert!(truncated);
    }

    #[test]
    fn proxy_choice_follows_the_app_proxy_settings() {
        use crate::proxy::ProxyEnvironment;
        assert_eq!(proxy_choice(true, None), ProxyChoice::FromEnvironment, "沿用进程环境");
        assert_eq!(proxy_choice(true, Some(&ProxyEnvironment::default())), ProxyChoice::Direct, "直连");
        let both = ProxyEnvironment { http: Some("http://p:1".into()), https: Some("http://s:2".into()), all: None, bypass: None };
        assert_eq!(proxy_choice(true, Some(&both)), ProxyChoice::Url("http://s:2".into()));
        assert_eq!(proxy_choice(false, Some(&both)), ProxyChoice::Url("http://p:1".into()));
        let all_only = ProxyEnvironment { http: None, https: None, all: Some("socks5://a:3".into()), bypass: None };
        assert_eq!(proxy_choice(true, Some(&all_only)), ProxyChoice::Url("socks5://a:3".into()));
        let only_http = ProxyEnvironment { http: Some("http://p:1".into()), https: None, all: None, bypass: None };
        assert_eq!(proxy_choice(true, Some(&only_http)), ProxyChoice::Direct, "https 目标没有对应代理则直连");
    }

    fn sample_endpoint(dir: &Path) -> ProviderEndpoint {
        write(dir, "config.toml", &codex_config(&format!("experimental_bearer_token = \"{SECRET}\"\n")));
        codex_endpoint(&env_with(dir, &[])).unwrap()
    }

    fn fake(status: u16, body: &str) -> impl Fn(&HttpRequest) -> Result<HttpResponse, String> {
        let body = body.to_string();
        move |_| Ok(HttpResponse { status, body: body.clone() })
    }

    #[test]
    fn fetch_sends_the_documented_request_and_parses_the_models() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = sample_endpoint(dir.path());
        let seen = RefCell::new(None);
        let http = |request: &HttpRequest| {
            *seen.borrow_mut() = Some((request.url.clone(), request.headers.clone(), request.proxy.clone()));
            Ok(HttpResponse { status: 200, body: r#"{"data":[{"id":"m1"}]}"#.into() })
        };
        let (models, truncated) = fetch_models(&endpoint, &ProxyChoice::Direct, &http).unwrap();
        assert_eq!(ids(&models), ["m1"]);
        assert!(!truncated);
        let (url, headers, proxy) = seen.into_inner().unwrap();
        assert_eq!(url, "https://gw.example.com/v1/models", "请求只发往端点地址");
        assert!(headers.contains(&("Authorization".to_string(), format!("Bearer {SECRET}"))));
        assert_eq!(proxy, ProxyChoice::Direct);
    }

    #[test]
    fn fetch_errors_are_categories_that_never_contain_secrets_or_bodies() {
        let dir = tempfile::tempdir().unwrap();
        let endpoint = sample_endpoint(dir.path());
        let leaky = format!("{{\"error\":\"bad key {SECRET}\"}}");
        let oversized = "x".repeat(MAX_BODY + 1);
        let cases: Vec<(Box<dyn Fn(&HttpRequest) -> Result<HttpResponse, String>>, &str)> = vec![
            (Box::new(fake(401, &leaky)), "HTTP 401"),
            (Box::new(fake(403, &leaky)), "HTTP 403"),
            (Box::new(fake(404, &leaky)), "HTTP 404"),
            (Box::new(fake(500, &leaky)), "HTTP 500"),
            (Box::new(fake(302, &leaky)), "重定向"),
            (Box::new(fake(200, &leaky)), "响应格式不对"),
            (Box::new(|_: &HttpRequest| Err("连接超时".to_string())), "连接超时"),
            (Box::new(fake(200, &oversized)), "响应过大"),
        ];
        for (http, expected) in cases {
            let message = fetch_models(&endpoint, &ProxyChoice::Direct, &*http).unwrap_err();
            assert!(message.contains(expected), "{expected}: {message}");
            assert!(!message.contains(SECRET) && !message.contains("bad key"), "{message}");
        }
    }

    /// 起一个只服务一次连接的回环 HTTP 服务，返回（端口，收到的请求文本的接收端）。
    fn serve_once(response: String) -> (u16, std::sync::mpsc::Receiver<String>) {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 4096];
            let read = stream.read(&mut buffer).unwrap_or(0);
            let _ = sender.send(String::from_utf8_lossy(&buffer[..read]).into_owned());
            let _ = stream.write_all(response.as_bytes());
        });
        (port, receiver)
    }

    #[test]
    fn ureq_sends_headers_to_loopback_and_reads_the_body() {
        let body = r#"{"data":[{"id":"loop-1"}]}"#;
        let (port, received) = serve_once(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()));
        let request = HttpRequest {
            url: format!("http://127.0.0.1:{port}/v1/models"),
            headers: vec![("Authorization".into(), format!("Bearer {SECRET}")), ("Accept".into(), "application/json".into())],
            proxy: ProxyChoice::Direct,
        };
        let response = ureq_http(&request).unwrap();
        assert_eq!(response.status, 200);
        assert_eq!(response.body, body);
        let text = received.recv_timeout(std::time::Duration::from_secs(5)).unwrap().to_lowercase();
        assert!(text.starts_with("get /v1/models"), "{text}");
        assert!(text.contains(&format!("authorization: bearer {}", SECRET.to_lowercase())), "{text}");
    }

    #[test]
    fn ureq_does_not_follow_redirects_so_the_key_is_never_forwarded() {
        let (port, _received) = serve_once("HTTP/1.1 302 Found\r\nLocation: http://127.0.0.1:1/elsewhere\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_string());
        let request = HttpRequest { url: format!("http://127.0.0.1:{port}/v1/models"), headers: vec![("Authorization".into(), format!("Bearer {SECRET}"))], proxy: ProxyChoice::Direct };
        match ureq_http(&request) {
            Ok(response) => assert_eq!(response.status, 302),
            Err(message) => assert!(!message.contains(SECRET) && !message.is_empty()),
        }
    }

    #[test]
    fn ureq_reports_connection_failures_as_categories() {
        // 端口 1 基本无人监听，连接会被拒绝
        let request = HttpRequest { url: "http://127.0.0.1:1/v1/models".into(), headers: vec![("Authorization".into(), format!("Bearer {SECRET}"))], proxy: ProxyChoice::Direct };
        let message = ureq_http(&request).err().expect("应当失败");
        assert!(!message.contains(SECRET) && !message.contains("127.0.0.1"), "{message}");
    }
}
