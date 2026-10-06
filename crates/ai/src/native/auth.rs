use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    net::TcpListener,
    time::Instant,
};
fn random() -> String {
    URL_SAFE_NO_PAD.encode(rand::random::<[u8; 32]>())
}
fn metadata() -> AiResult<Value> {
    json_response(client()?.get("https://auth.openai.com/.well-known/openid-configuration").send().map_err(|_| "无法读取授权配置")?)
}
fn official(url: &str) -> AiResult<&str> {
    if url::Url::parse(url).is_ok_and(|u| {
        u.scheme() == "https" && u.host_str() == Some("auth.openai.com") && u.port().is_none() && u.username().is_empty() && u.password().is_none()
    }) {
        Ok(url)
    } else {
        Err("授权服务地址不匹配".into())
    }
}
/// Loopback callback is local to this attempt, with fresh state, nonce and PKCE.
pub fn login(mut settings: Settings, cancel: &Cancellation) -> AiResult<Settings> {
    let old = if settings.account.is_empty() {
        None
    } else {
        Some(
            serde_json::from_str::<Credential>(&secret(&format!("account:{}", settings.account))?)
                .map_err(|_| "所选账户凭据不可用，请添加账户重新登录。")?,
        )
    };
    if settings.host_id.is_empty() {
        let r = rand::random::<[u8; 16]>();
        settings.host_id = format!(
            "urn:uuid:{:08x}-{:04x}-4{:03x}-a{:03x}-{:012x}",
            u32::from_be_bytes(r[0..4].try_into().unwrap()),
            u16::from_be_bytes(r[4..6].try_into().unwrap()),
            u16::from_be_bytes(r[6..8].try_into().unwrap()) & 0xfff,
            u16::from_be_bytes(r[8..10].try_into().unwrap()) & 0xfff,
            u64::from_be_bytes([0, 0, r[10], r[11], r[12], r[13], r[14], r[15]])
        );
        settings.save(None)?;
    }
    let listener = TcpListener::bind("127.0.0.1:0").map_err(|_| "无法启动本机授权回调")?;
    listener.set_nonblocking(true).map_err(|_| "无法配置授权回调")?;
    let redirect = format!("http://127.0.0.1:{}/auth/callback", listener.local_addr().map_err(|_| "回调地址不可用")?.port());
    let (state, nonce, verifier) = (random(), random(), random());
    let challenge = URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes()));
    let mut url = url::Url::parse("https://auth.openai.com/api/accounts/authorize").unwrap();
    url.query_pairs_mut().extend_pairs([
        ("client_id", old.as_ref().map(|c| c.client_id.as_str()).unwrap_or("dynamic_agent_client")),
        ("agent_name_hint", "DesignCraft"),
        ("ext_agent_host_id", &settings.host_id),
        ("response_type", "code"),
        ("redirect_uri", &redirect),
        ("scope", "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct"),
        ("resource", "https://api.openai.com/v1"),
        ("state", &state),
        ("nonce", &nonce),
        ("code_challenge_method", "S256"),
        ("code_challenge", &challenge),
    ]);
    if let Some(c) = &old {
        url.query_pairs_mut().append_pair("id_token_hint", &c.id_token);
    }
    webbrowser::open(url.as_str()).map_err(|_| "无法打开系统浏览器")?;
    let deadline = Instant::now() + Duration::from_secs(300);
    let params = loop {
        cancel.check()?;
        if Instant::now() > deadline {
            return Err("登录已超时，请重试。".into());
        }
        match listener.accept() {
            Ok((mut socket, _)) => {
                socket.set_read_timeout(Some(Duration::from_secs(2))).map_err(|_| "回调读取失败")?;
                let mut buf = [0; 16384];
                let n = socket.read(&mut buf).unwrap_or(0);
                let request = String::from_utf8_lossy(&buf[..n]);
                let first = request.lines().next().unwrap_or("");
                let path = first.strip_prefix("GET ").and_then(|s| s.split_whitespace().next()).unwrap_or("");
                let callback = url::Url::parse(&format!("http://127.0.0.1{path}"));
                let p = callback.as_ref().ok().map(|u| u.query_pairs().into_owned().collect::<HashMap<_, _>>()).unwrap_or_default();
                let valid = callback.as_ref().is_ok_and(|u| u.path() == "/auth/callback") && p.get("state") == Some(&state);
                let body = if valid { "Authorization received. Return to DesignCraft." } else { "Invalid callback." };
                let _ = write!(
                    socket,
                    "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    if valid { "200 OK" } else { "400 Bad Request" },
                    body.len(),
                    body
                );
                if valid {
                    break p;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(100)),
            Err(_) => return Err("授权回调失败".into()),
        }
    };
    if params.contains_key("error") {
        return Err("未完成授权，请重新登录。".into());
    }
    let cid = params
        .get("client_id")
        .cloned()
        .or_else(|| old.as_ref().map(|c| c.client_id.clone()))
        .filter(|c| c != "dynamic_agent_client")
        .ok_or("缺少注册客户端身份")?;
    if old.as_ref().is_some_and(|o| o.client_id != cid) {
        return Err("账户注册身份不匹配".into());
    }
    let code = params.get("code").ok_or("授权回调缺少 code")?;
    cancel.check()?;
    let v = json_response(
        client()?
            .post("https://auth.openai.com/api/accounts/oauth/token")
            .form(&[
                ("grant_type", "authorization_code"),
                ("client_id", &cid),
                ("code", code),
                ("code_verifier", &verifier),
                ("redirect_uri", &redirect),
                ("resource", "https://api.openai.com/v1"),
            ])
            .send()
            .map_err(|_| "授权交换失败，请重新登录")?,
    )?;
    let token = v["id_token"].as_str().ok_or("缺少身份令牌")?;
    let meta = metadata()?;
    let jwks = json_response(client()?.get(official(meta["jwks_uri"].as_str().ok_or("缺少签名密钥地址")?)?).send().map_err(|_| "无法读取签名密钥")?)?;
    let set: jsonwebtoken::jwk::JwkSet = serde_json::from_value(jwks).map_err(|_| "无效签名密钥")?;
    let header = jsonwebtoken::decode_header(token).map_err(|_| "身份令牌格式无效")?;
    if header.alg != jsonwebtoken::Algorithm::RS256 {
        return Err("不支持的身份签名算法".into());
    }
    let jwk = set.find(header.kid.as_deref().ok_or("缺少签名标识")?).ok_or("找不到签名密钥")?;
    let key = jsonwebtoken::DecodingKey::from_jwk(jwk).map_err(|_| "无效签名密钥")?;
    let mut validation = jsonwebtoken::Validation::new(jsonwebtoken::Algorithm::RS256);
    validation.set_audience(&[&cid]);
    validation.set_issuer(&["https://auth.openai.com"]);
    let claims = jsonwebtoken::decode::<Value>(token, &key, &validation).map_err(|_| "身份令牌验证失败")?.claims;
    if claims["nonce"].as_str() != Some(&nonce) {
        return Err("身份 nonce 不匹配".into());
    }
    let subject = claims["sub"].as_str().ok_or("缺少账户身份")?.to_string();
    if old.as_ref().is_some_and(|o| o.subject != subject) {
        return Err("返回账户与所选账户不匹配".into());
    }
    let mut c = Credential {
        client_id: cid.clone(),
        subject,
        email: claims["email"].as_str().unwrap_or("ChatGPT").into(),
        access_token: String::new(),
        refresh_token: String::new(),
        id_token: String::new(),
        expires_at: 0,
        scopes: vec![],
    };
    update_token(&mut c, &v)?;
    cancel.check()?;
    put_secret(&format!("account:{cid}"), &serde_json::to_string(&c).unwrap())?;
    let account = Account {
        id: cid.clone(),
        label: format!("{} · {}", c.email, &cid[cid.len().saturating_sub(6)..]),
        sharing: c.scopes.iter().any(|s| s == "chatgpt.tokens.use.direct"),
    };
    settings.accounts.retain(|a| a.id != cid);
    settings.accounts.push(account);
    settings.account = cid;
    settings.provider = "chatgpt".into();
    settings.model.clear();
    settings.save(None)?;
    Ok(settings)
}
pub fn logout(mut s: Settings) -> AiResult<Settings> {
    let name = format!("account:{}", s.account);
    if let Ok(c) = serde_json::from_str::<Credential>(&secret(&name)?)
        && let Ok(meta) = metadata()
        && let Some(url) = meta["revocation_endpoint"].as_str()
        && let Ok(url) = official(url)
    {
        let _ = client()?
            .post(url)
            .form(&[("token", c.refresh_token.as_str()), ("token_type_hint", "refresh_token"), ("client_id", c.client_id.as_str())])
            .send();
    }
    match entry(&name)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => {}
        Err(_) => return Err("无法删除本机凭据".into()),
    }
    s.accounts.retain(|a| a.id != s.account);
    s.account.clear();
    s.model.clear();
    s.save(None)?;
    Ok(s)
}
