//! Native services. No credentials cross the UI/control response or document boundary.
mod auth;
mod transport;
pub use auth::{login, logout};
use reqwest::blocking::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
pub use transport::{conversation, models, test_connection};

pub type AiResult<T> = std::result::Result<T, String>;
#[derive(Clone, Default)]
pub struct Cancellation(pub Arc<AtomicBool>);
impl Cancellation {
    pub fn cancel(&self) {
        self.0.store(true, Ordering::SeqCst);
    }
    pub fn check(&self) -> AiResult<()> {
        if self.0.load(Ordering::SeqCst) { Err("已停止".into()) } else { Ok(()) }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Settings {
    pub provider: String,
    pub endpoint: String,
    pub model: String,
    pub account: String,
    pub accounts: Vec<Account>,
    pub host_id: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            provider: "compatible".into(),
            endpoint: "https://api.openai.com/v1".into(),
            model: String::new(),
            account: String::new(),
            accounts: vec![],
            host_id: String::new(),
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Account {
    pub id: String,
    pub label: String,
    pub sharing: bool,
}
#[derive(Clone, Serialize, Deserialize)]
struct Credential {
    client_id: String,
    subject: String,
    email: String,
    access_token: String,
    refresh_token: String,
    id_token: String,
    expires_at: u64,
    scopes: Vec<String>,
}
fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs()
}
fn client() -> AiResult<Client> {
    Client::builder()
        .timeout(Duration::from_secs(60))
        .connect_timeout(Duration::from_secs(15))
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .map_err(|_| "无法创建网络客户端".into())
}
fn entry(name: &str) -> AiResult<keyring::Entry> {
    keyring::Entry::new("DesignCraft AI", name).map_err(|_| "系统凭据存储不可用".into())
}
fn secret(name: &str) -> AiResult<String> {
    match entry(name)?.get_password() {
        Ok(v) => Ok(v),
        Err(keyring::Error::NoEntry) => Ok(String::new()),
        Err(_) => Err("无法读取系统凭据存储，请解锁后重试。".into()),
    }
}
fn put_secret(name: &str, value: &str) -> AiResult<()> {
    entry(name)?.set_password(value).map_err(|_| "无法保存系统凭据；设置未保存。".into())
}
fn config_path() -> AiResult<PathBuf> {
    let base = if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support/DesignCraft"))
    } else if cfg!(windows) {
        std::env::var_os("APPDATA").map(|h| PathBuf::from(h).join("DesignCraft"))
    } else {
        std::env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
            .map(|p| p.join("designcraft"))
    };
    Ok(base.ok_or("无法确定用户设置目录")?.join("ai.json"))
}
impl Settings {
    pub fn load() -> AiResult<Self> {
        let path = config_path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        serde_json::from_slice(&std::fs::read(path).map_err(|_| "无法读取 AI 设置")?).map_err(|_| "AI 设置损坏，请重新保存设置。".into())
    }
    pub fn save(&self, key: Option<&str>) -> AiResult<()> {
        endpoint(&self.endpoint)?;
        if !matches!(self.provider.as_str(), "compatible" | "chatgpt") {
            return Err("未知模型服务".into());
        }
        if let Some(key) = key {
            put_secret(&format!("api:{}", self.endpoint.trim_end_matches('/')), key)?;
        }
        let path = config_path()?;
        std::fs::create_dir_all(path.parent().unwrap()).map_err(|_| "无法创建设置目录")?;
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self).map_err(|_| "无法编码设置")?).map_err(|_| "无法写入 AI 设置")?;
        std::fs::rename(tmp, path).map_err(|_| "无法保存 AI 设置".into())
    }
}
fn endpoint(value: &str) -> AiResult<url::Url> {
    let u = url::Url::parse(value).map_err(|_| "无效的服务地址")?;
    if !matches!(u.scheme(), "http" | "https")
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("服务地址须为不含凭据、查询参数的 HTTP(S) 地址".into());
    }
    Ok(u)
}
fn json_response(response: reqwest::blocking::Response) -> AiResult<Value> {
    let status = response.status();
    if !status.is_success() {
        return Err(format!("服务返回 HTTP {}。请检查账户权限、额度、模型和服务地址。", status.as_u16()));
    }
    let mut bytes = Vec::new();
    use std::io::Read;
    response.take(16 * 1024 * 1024 + 1).read_to_end(&mut bytes).map_err(|_| "读取响应失败")?;
    if bytes.len() > 16 * 1024 * 1024 {
        return Err("响应过大".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "服务未返回有效 JSON".into())
}
fn credentials(settings: &Settings) -> AiResult<(String, String)> {
    if settings.provider == "chatgpt" {
        let mut c: Credential = serde_json::from_str(&secret(&format!("account:{}", settings.account))?).map_err(|_| "请先使用 ChatGPT 登录")?;
        if c.expires_at <= now() + 120 {
            let v = json_response(
                client()?
                    .post("https://auth.openai.com/api/accounts/oauth/token")
                    .form(&[
                        ("grant_type", "refresh_token"),
                        ("client_id", c.client_id.as_str()),
                        ("refresh_token", c.refresh_token.as_str()),
                        ("resource", "https://api.openai.com/v1"),
                    ])
                    .send()
                    .map_err(|_| "令牌刷新失败，请重新登录")?,
            )?;
            update_token(&mut c, &v)?;
            put_secret(&format!("account:{}", settings.account), &serde_json::to_string(&c).unwrap())?;
        }
        if !c.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct") {
            return Err("已登录，但没有授予 ChatGPT 订阅推理权限。".into());
        }
        Ok(("https://api.openai.com/v1".into(), c.access_token))
    } else {
        endpoint(&settings.endpoint)?;
        Ok((settings.endpoint.trim_end_matches('/').into(), secret(&format!("api:{}", settings.endpoint.trim_end_matches('/')))?))
    }
}
fn update_token(c: &mut Credential, v: &Value) -> AiResult<()> {
    c.access_token = v["access_token"].as_str().filter(|s| !s.is_empty()).ok_or("授权响应缺少访问令牌")?.into();
    if let Some(t) = v["refresh_token"].as_str() {
        c.refresh_token = t.into();
    }
    if let Some(t) = v["id_token"].as_str() {
        c.id_token = t.into();
    }
    c.expires_at = now() + v["expires_in"].as_u64().unwrap_or(3600);
    if let Some(s) = v["scope"].as_str() {
        c.scopes = s.split_whitespace().map(str::to_owned).collect();
    }
    Ok(())
}

pub fn open_usage() -> AiResult<()> {
    webbrowser::open("https://chatgpt.com/settings/usage").map_err(|_| "无法打开浏览器".into())
}
