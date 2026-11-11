use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode { #[default] System, Manual, Direct }

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxySettings { pub mode: ProxyMode, pub url: String }

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProxyEnvironment { pub http: Option<String>, pub https: Option<String>, pub all: Option<String>, pub bypass: Option<String> }

impl ProxySettings {
    pub fn normalize(&self) -> AppResult<Self> {
        let url = if self.mode == ProxyMode::Manual { validate_url(&self.url)? } else { String::new() };
        Ok(Self { mode: self.mode, url })
    }
}

fn invalid(message: &str) -> AppError { AppError::Invalid(message.into()) }

fn validate_url(raw: &str) -> AppResult<String> {
    if raw.chars().any(char::is_control) { return Err(invalid("代理地址不能包含换行或控制字符")); }
    let value = raw.trim();
    let url = tauri::Url::parse(value).map_err(|_| invalid("代理地址格式不正确，请填写协议、主机和端口"))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5" | "socks5h") {
        return Err(invalid("代理仅支持 HTTP、HTTPS 或 SOCKS5"));
    }
    let authority = value.split_once("://").map(|(_, rest)| rest.split('/').next().unwrap_or("")).unwrap_or("");
    if authority.contains('@') || !url.username().is_empty() || url.password().is_some() {
        return Err(invalid("本机代理地址不支持账号或密码"));
    }
    if url.path() != "" && url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
        return Err(invalid("代理地址不能包含路径、查询参数或片段"));
    }
    // Url 会去掉显式的默认端口，因此单独确认用户确实填写了端口。
    let port = authority.rsplit_once(':').and_then(|(_, port)| port.parse::<u16>().ok());
    if url.host_str().is_none() || port.is_none_or(|port| port == 0) {
        return Err(invalid("代理地址必须包含主机和有效端口"));
    }
    Ok(value.trim_end_matches('/').into())
}

pub fn load(conn: &Connection) -> AppResult<ProxySettings> {
    let stored: Option<String> = conn.query_row("SELECT value FROM settings WHERE key='grok_proxy'", [], |r| r.get(0)).optional()?;
    stored.map(|value| serde_json::from_str::<ProxySettings>(&value)
        .map_err(|_| invalid("Grok 代理设置无法读取，请重新保存"))?.normalize()).unwrap_or_else(|| Ok(ProxySettings::default()))
}

pub fn save(conn: &Connection, settings: &ProxySettings) -> AppResult<()> {
    let normalized = settings.normalize()?;
    let json = serde_json::to_string(&normalized).map_err(|_| invalid("无法保存 Grok 代理设置"))?;
    conn.execute("INSERT INTO settings(key,value) VALUES('grok_proxy',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [json])?;
    Ok(())
}

fn static_url(value: &str) -> AppResult<String> {
    validate_url(&if value.contains("://") { value.into() } else { format!("http://{value}") })
}

pub fn windows_proxy(server: &str, bypass: &str) -> AppResult<ProxyEnvironment> {
    let mut proxy = ProxyEnvironment::default();
    if server.contains('=') {
        for entry in server.split(';').map(str::trim).filter(|part| !part.is_empty()) {
            let (protocol, address) = entry.split_once('=').ok_or_else(|| invalid("系统代理格式无法识别，请改用手动代理"))?;
            match protocol.trim().to_ascii_lowercase().as_str() {
                "http" => proxy.http = Some(static_url(address.trim())?),
                "https" => proxy.https = Some(static_url(address.trim())?),
                _ => {} // Windows 的 socks 项不证明协议版本，不猜成 SOCKS5。
            }
        }
        if proxy.http.is_none() && proxy.https.is_none() {
            return Err(invalid("系统未提供 HTTP/HTTPS 代理，请在手动模式填写代理地址"));
        }
    } else {
        let url = static_url(server.trim())?;
        proxy.http = Some(url.clone()); proxy.https = Some(url);
    }
    let mut exceptions = vec!["localhost".to_owned(), "127.0.0.1".into(), "::1".into()];
    exceptions.extend(bypass.split(';').map(str::trim).filter(|part| !part.is_empty() && *part != "<local>")
        .map(|part| part.strip_prefix("*.").map(|host| format!(".{host}")).unwrap_or_else(|| part.into())));
    proxy.bypass = Some(exceptions.join(","));
    Ok(proxy)
}

pub fn manual_environment(settings: &ProxySettings) -> AppResult<ProxyEnvironment> {
    let url = settings.normalize()?.url;
    Ok(ProxyEnvironment { http: Some(url.clone()), https: Some(url.clone()), all: Some(url),
        bypass: Some("localhost,127.0.0.1,::1".into()) })
}

pub fn overrides(proxy: &ProxyEnvironment) -> Vec<(String, Option<String>)> {
    [("HTTP_PROXY", &proxy.http), ("HTTPS_PROXY", &proxy.https), ("ALL_PROXY", &proxy.all)]
        .into_iter().flat_map(|(key, value)| [(key.into(), value.clone()), (key.to_ascii_lowercase(), value.clone())])
        .chain(proxy.bypass.as_ref().into_iter().flat_map(|value| [("NO_PROXY".into(), Some(value.clone())), ("no_proxy".into(), Some(value.clone()))])).collect()
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyState { pub settings: ProxySettings, pub detected_proxy: String, pub detection_error: Option<String> }

/// 返回 None 表示沿用进程环境；Some 则仅覆盖 Grok 子进程。
pub fn resolve(settings: &ProxySettings) -> AppResult<Option<ProxyEnvironment>> {
    match settings.mode {
        ProxyMode::Direct => Ok(Some(ProxyEnvironment::default())),
        ProxyMode::Manual => manual_environment(settings).map(Some),
        ProxyMode::System => {
            let has_environment = ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"]
                .iter().any(|key| std::env::var_os(key).is_some_and(|value| !value.is_empty()));
            if has_environment { return Ok(None); }
            system_proxy()
        }
    }
}

pub fn state(settings: ProxySettings) -> ProxyState {
    let (detected_proxy, detection_error) = match resolve(&ProxySettings::default()) {
        Ok(Some(proxy)) => (format!("HTTP：{}；HTTPS：{}", proxy.http.as_deref().unwrap_or("直连"), proxy.https.as_deref().unwrap_or("直连")), None),
        Ok(None) => ("沿用启动环境中的代理；未设置时直连".into(), None),
        Err(error) => ("无法自动读取本机代理".into(), Some(error.to_string())),
    };
    ProxyState { settings, detected_proxy, detection_error }
}

pub struct ProxyAdapter {
    inner: Box<dyn crate::adapters::CliAdapter>,
    environment: Vec<(String, Option<String>)>,
}
impl ProxyAdapter {
    pub fn new(inner: Box<dyn crate::adapters::CliAdapter>, environment: Vec<(String, Option<String>)>) -> Self {
        Self { inner, environment }
    }
}
impl crate::adapters::CliAdapter for ProxyAdapter {
    fn id(&self) -> &'static str { self.inner.id() }
    fn detect(&self) -> Option<crate::cli_registry::CliInfo> { self.inner.detect() }
    fn build_command(&self, request: &crate::adapters::TurnRequest) -> crate::adapters::CommandSpec { self.inner.build_command(request) }
    fn environment(&self) -> Vec<(String, Option<String>)> { self.environment.clone() }
    fn parse_line(&mut self, line: &str) -> Vec<crate::adapters::AgentEvent> { self.inner.parse_line(line) }
}

#[cfg(not(windows))]
fn system_proxy() -> AppResult<Option<ProxyEnvironment>> { Ok(None) }

#[cfg(windows)]
fn system_proxy() -> AppResult<Option<ProxyEnvironment>> {
    use std::ffi::c_void;
    #[repr(C)]
    struct Config { auto_detect: i32, pac: *mut u16, server: *mut u16, bypass: *mut u16 }
    #[link(name = "winhttp")]
    unsafe extern "system" { fn WinHttpGetIEProxyConfigForCurrentUser(config: *mut Config) -> i32; }
    #[link(name = "kernel32")]
    unsafe extern "system" { fn GlobalFree(memory: *mut c_void) -> *mut c_void; }
    impl Drop for Config {
        fn drop(&mut self) {
            for ptr in [self.pac, self.server, self.bypass] {
                if !ptr.is_null() { unsafe { GlobalFree(ptr.cast()); } }
            }
        }
    }
    fn string(ptr: *const u16) -> String {
        if ptr.is_null() { return String::new(); }
        // 指针来自 WinHTTP，字符串以零结尾，有效期由 Config 管理。
        unsafe {
            let mut len = 0;
            while *ptr.add(len) != 0 { len += 1; }
            String::from_utf16_lossy(std::slice::from_raw_parts(ptr, len))
        }
    }
    let mut config = Config { auto_detect: 0, pac: std::ptr::null_mut(), server: std::ptr::null_mut(), bypass: std::ptr::null_mut() };
    if unsafe { WinHttpGetIEProxyConfigForCurrentUser(&mut config) } == 0 {
        return Err(invalid("无法读取 Windows 系统代理，请改用手动代理"));
    }
    let server = string(config.server);
    if !server.trim().is_empty() { return windows_proxy(&server, &string(config.bypass)).map(Some); }
    if config.auto_detect != 0 || !config.pac.is_null() {
        return Err(invalid("系统使用自动代理脚本或自动检测，请改用手动代理地址"));
    }
    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proxy_defaults_to_system_and_persists_validated_settings() {
        let conn = crate::store::open_in_memory().unwrap();
        assert_eq!(load(&conn).unwrap(), ProxySettings::default());
        let settings = ProxySettings { mode: ProxyMode::Manual, url: "  http://127.0.0.1:7890  ".into() };
        save(&conn, &settings).unwrap();
        assert_eq!(load(&conn).unwrap(), ProxySettings { mode: ProxyMode::Manual, url: "http://127.0.0.1:7890".into() });
        let invalid = ProxySettings { mode: ProxyMode::Manual, url: "ftp://localhost:21".into() };
        assert!(save(&conn, &invalid).is_err());
        assert_eq!(load(&conn).unwrap().url, "http://127.0.0.1:7890");
    }

    #[test]
    fn proxy_rejects_bad_protocol_credentials_and_non_proxy_url_parts() {
        for url in ["", "localhost:7890", "ftp://localhost:21", "http://user:secret@localhost:7890", "http://localhost:7890/path", "http://localhost:7890?key=secret", "http://localhost:7890#fragment", "http://localhost", "http://localhost:0", "http://localhost:7890\n"] {
            let settings = ProxySettings { mode: ProxyMode::Manual, url: url.into() };
            assert!(settings.normalize().is_err(), "不应接受：{url:?}");
        }
        for url in ["http://127.0.0.1:7890", "https://localhost:8443", "socks5://localhost:1080", "socks5h://[::1]:1080"] {
            assert_eq!(ProxySettings { mode: ProxyMode::Manual, url: url.into() }.normalize().unwrap().url, url);
        }
    }

    #[test]
    fn proxy_parses_windows_per_protocol_and_shared_static_addresses() {
        let proxy = windows_proxy("http=127.0.0.1:7890;https=127.0.0.1:7891;socks=127.0.0.1:7890", "<local>;*.example.test").unwrap();
        assert_eq!(proxy.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert_eq!(proxy.https.as_deref(), Some("http://127.0.0.1:7891"));
        assert_eq!(proxy.all, None);
        assert!(proxy.bypass.unwrap().contains(".example.test"));
        let shared = windows_proxy("127.0.0.1:7890", "").unwrap();
        assert_eq!(shared.http, shared.https);
        assert_eq!(shared.http.as_deref(), Some("http://127.0.0.1:7890"));
        assert!(windows_proxy("socks=127.0.0.1:1080", "").is_err());
    }

    #[test]
    fn proxy_manual_and_direct_override_conflicting_inherited_variables() {
        let settings = ProxySettings { mode: ProxyMode::Manual, url: "http://127.0.0.1:7890".into() };
        let env = overrides(&manual_environment(&settings).unwrap());
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            assert_eq!(env.iter().find(|(k,_)| k == key).unwrap().1.as_deref(), Some("http://127.0.0.1:7890"));
        }
        let direct = overrides(&ProxyEnvironment::default());
        for key in ["HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY", "http_proxy", "https_proxy", "all_proxy"] {
            assert!(direct.contains(&(key.into(), None)));
        }
        assert!(direct.iter().all(|(k,_)| k != "PATH"));
    }
}
