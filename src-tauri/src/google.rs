use crate::config::{Secrets, Settings};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rand::RngCore;
use reqwest::{Client, Proxy, StatusCode, Version};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::sync::Mutex;
use url::Url;

pub const GOOGLE_SCOPES: &[&str] = &[
  "https://www.googleapis.com/auth/analytics.edit",
  "https://www.googleapis.com/auth/tagmanager.edit.containers",
  "https://www.googleapis.com/auth/tagmanager.edit.containerversions",
  "https://www.googleapis.com/auth/tagmanager.publish",
  "https://www.googleapis.com/auth/siteverification",
  "https://www.googleapis.com/auth/webmasters",
];

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCheckItem {
  pub key: String,
  pub label: String,
  pub ok: bool,
  pub message: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthCheckResult {
  pub ok: bool,
  pub checked_at: String,
  pub items: Vec<AuthCheckItem>,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OAuthStartResult {
  pub message: String,
  pub granted_scopes_ok: bool,
  pub missing_scopes: Vec<String>,
}

struct TokenCache {
  value: String,
  expires_at: Instant,
}

#[derive(Deserialize)]
struct TokenResponse {
  access_token: String,
  expires_in: u64,
  #[serde(default)]
  refresh_token: Option<String>,
  #[serde(default)]
  scope: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ApiError {
  pub status: u16,
  pub message: String,
}

impl std::fmt::Display for ApiError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    write!(f, "{}", self.message)
  }
}

impl From<ApiError> for String {
  fn from(value: ApiError) -> Self {
    value.message
  }
}

impl ApiError {
  pub fn is_server_error(&self) -> bool {
    self.status >= 500
  }
}

pub struct GoogleClient {
  token_cache: Mutex<Option<TokenCache>>,
  gtm_write_lock: Mutex<Instant>,
}

impl GoogleClient {
  pub fn new() -> Self {
    Self {
      token_cache: Mutex::new(None),
      gtm_write_lock: Mutex::new(Instant::now() - Duration::from_secs(60)),
    }
  }

  pub async fn gtm_write_slot(&self, interval_ms: u64) {
    let mut last = self.gtm_write_lock.lock().await;
    let ready_at = *last + Duration::from_millis(interval_ms);
    let now = Instant::now();
    if ready_at > now {
      tokio::time::sleep(ready_at - now).await;
    }
    *last = Instant::now();
  }

  pub fn build_http(settings: &Settings) -> Result<Client, String> {
    Self::build_http_opts(settings, false)
  }

  /// `force_direct`：忽略代理设置，直连 Google（用于对空 body POST 更挑剔的接口）。
  pub fn build_http_opts(settings: &Settings, force_direct: bool) -> Result<Client, String> {
    let mut builder = Client::builder()
      .timeout(Duration::from_secs(30))
      .user_agent("Beacon/0.1")
      // 空 POST 在 HTTP/2 下偶发被 Google 边缘节点当成无 Content-Length → 411
      .http1_only();
    if !force_direct && settings.proxy_enabled {
      let proxy = Proxy::all(settings.proxy_url.trim())
        .map_err(|e| format!("代理配置无效：{e}"))?;
      builder = builder.proxy(proxy);
    } else {
      builder = builder.no_proxy();
    }
    builder
      .build()
      .map_err(|e| format!("创建 HTTP 客户端失败：{e}"))
  }

  async fn send_with_retry(
    mut build: impl FnMut() -> reqwest::RequestBuilder,
    retry_server_errors: bool,
  ) -> Result<String, ApiError> {
    let mut attempt = 0u32;
    loop {
      let res = build().send().await.map_err(|e| ApiError {
        status: 0,
        message: format!("网络请求失败：{e}"),
      })?;
      let status = res.status();
      let text = res.text().await.unwrap_or_default();
      if status.is_success() {
        return Ok(text);
      }
      let retryable =
        status == StatusCode::TOO_MANY_REQUESTS || (retry_server_errors && status.is_server_error());
      if !retryable || attempt >= 5 {
        return Err(ApiError {
          status: status.as_u16(),
          message: format_api_error(status, &text),
        });
      }
      let delay_ms = (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
      tokio::time::sleep(Duration::from_millis(delay_ms)).await;
      attempt += 1;
    }
  }

  pub async fn get_access_token(
    &self,
    settings: &Settings,
    secrets: &Secrets,
  ) -> Result<String, String> {
    {
      let cache = self.token_cache.lock().await;
      if let Some(token) = cache.as_ref() {
        if token.expires_at > Instant::now() + Duration::from_secs(60) {
          return Ok(token.value.clone());
        }
      }
    }

    if secrets.client_id.trim().is_empty()
      || secrets.client_secret.trim().is_empty()
      || secrets.refresh_token.trim().is_empty()
    {
      return Err("Google OAuth 凭证未配置完整".into());
    }

    let client = Self::build_http(settings)?;
    let client_id = secrets.client_id.clone();
    let client_secret = secrets.client_secret.clone();
    let refresh_token = secrets.refresh_token.clone();

    let text = Self::send_with_retry(
      || {
        client.post("https://oauth2.googleapis.com/token").form(&[
          ("client_id", client_id.as_str()),
          ("client_secret", client_secret.as_str()),
          ("refresh_token", refresh_token.as_str()),
          ("grant_type", "refresh_token"),
        ])
      },
      true,
    )
    .await
    .map_err(|e| describe_token_error(&e.message))?;

    let data: TokenResponse =
      serde_json::from_str(&text).map_err(|e| format!("解析 Token 响应失败：{e}"))?;

    let mut cache = self.token_cache.lock().await;
    *cache = Some(TokenCache {
      value: data.access_token.clone(),
      expires_at: Instant::now() + Duration::from_secs(data.expires_in),
    });
    Ok(data.access_token)
  }

  pub async fn api_get(
    &self,
    settings: &Settings,
    secrets: &Secrets,
    url: &str,
  ) -> Result<Value, String> {
    self.api_json(settings, secrets, "GET", url, None, true)
      .await
      .map_err(Into::into)
  }

  pub async fn api_json(
    &self,
    settings: &Settings,
    secrets: &Secrets,
    method: &str,
    url: &str,
    body: Option<Value>,
    retry_server_errors: bool,
  ) -> Result<Value, ApiError> {
    self
      .api_json_opts(settings, secrets, method, url, body, retry_server_errors, false)
      .await
  }

  pub async fn api_json_opts(
    &self,
    settings: &Settings,
    secrets: &Secrets,
    method: &str,
    url: &str,
    body: Option<Value>,
    retry_server_errors: bool,
    force_direct: bool,
  ) -> Result<Value, ApiError> {
    let token = self
      .get_access_token(settings, secrets)
      .await
      .map_err(|message| ApiError { status: 0, message })?;
    let client = Self::build_http_opts(settings, force_direct)
      .map_err(|message| ApiError { status: 0, message })?;
    let auth = format!("Bearer {token}");
    let method_owned = method.to_string();
    let url_owned = url.to_string();
    let body_owned = body.clone();
    let text = Self::send_with_retry(
      || {
        let mut req = match method_owned.as_str() {
          "POST" => client.post(&url_owned),
          "PUT" => client.put(&url_owned),
          "PATCH" => client.patch(&url_owned),
          "DELETE" => client.delete(&url_owned),
          _ => client.get(&url_owned),
        };
        req = req.header("authorization", &auth).version(Version::HTTP_11);
        if let Some(ref b) = body_owned {
          req = req.json(b);
        } else if matches!(method_owned.as_str(), "POST" | "PUT" | "PATCH") {
          // Google 对无实体写请求要求 Content-Length: 0（built_in_variables.create 等）。
          req = req
            .header("content-length", "0")
            .header("content-type", "application/json")
            .body(Vec::<u8>::new());
        }
        req
      },
      retry_server_errors,
    )
    .await;
    let text = match text {
      Ok(t) => t,
      Err(err) if err.status == 411 && body.is_none() && method.eq_ignore_ascii_case("POST") => {
        // reqwest 路径仍 411 时，用系统 curl 发明确的空 POST（Windows 自带 curl.exe）
        Self::curl_empty_post(settings, force_direct, &token, url).await?
      }
      Err(err) => return Err(err),
    };
    if text.trim().is_empty() {
      return Ok(Value::Null);
    }
    serde_json::from_str(&text).map_err(|e| ApiError {
      status: 0,
      message: format!("解析 JSON 失败：{e}"),
    })
  }

  async fn curl_empty_post(
    settings: &Settings,
    force_direct: bool,
    access_token: &str,
    url: &str,
  ) -> Result<String, ApiError> {
    let mut args = vec![
      "-sS".into(),
      "-X".into(),
      "POST".into(),
      "-H".into(),
      format!("Authorization: Bearer {access_token}"),
      "-H".into(),
      "Content-Length: 0".into(),
      "-H".into(),
      "Content-Type: application/json".into(),
      "--http1.1".into(),
      "-w".into(),
      "\n__BEACON_HTTP_STATUS__:%{http_code}".into(),
      url.to_string(),
    ];
    if !force_direct && settings.proxy_enabled {
      let proxy = settings.proxy_url.trim();
      if !proxy.is_empty() {
        args.insert(0, "-x".into());
        args.insert(1, proxy.to_string());
      }
    } else {
      args.insert(0, "--noproxy".into());
      args.insert(1, "*".into());
    }

    let output = tokio::task::spawn_blocking(move || {
      std::process::Command::new("curl").args(&args).output()
    })
    .await
    .map_err(|e| ApiError {
      status: 0,
      message: format!("curl 空 POST 任务失败：{e}"),
    })?
    .map_err(|e| ApiError {
      status: 0,
      message: format!("curl 空 POST 失败（未找到 curl？）：{e}"),
    })?;
    let combined = String::from_utf8_lossy(&output.stdout).to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if !output.status.success() && combined.is_empty() {
      return Err(ApiError {
        status: 0,
        message: format!(
          "curl 空 POST 失败：{}",
          if stderr.is_empty() {
            output.status.to_string()
          } else {
            stderr
          }
        ),
      });
    }
    let (body, status) = match combined.rsplit_once("__BEACON_HTTP_STATUS__:") {
      Some((b, code)) => (b.trim_end().to_string(), code.trim().parse().unwrap_or(0)),
      None => (combined, 0),
    };
    if (200..300).contains(&status) {
      return Ok(body);
    }
    Err(ApiError {
      status,
      message: format_api_error(
        StatusCode::from_u16(status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR),
        &body,
      ),
    })
  }

  pub async fn api_list_all(
    &self,
    settings: &Settings,
    secrets: &Secrets,
    base_url: &str,
    field: &str,
  ) -> Result<Vec<Value>, String> {
    let mut items = Vec::new();
    let mut page_token: Option<String> = None;
    loop {
      let mut url = Url::parse(base_url).map_err(|e| e.to_string())?;
      if let Some(token) = &page_token {
        url.query_pairs_mut().append_pair("pageToken", token);
      }
      let page = self.api_get(settings, secrets, url.as_str()).await?;
      if let Some(arr) = page.get(field).and_then(|v| v.as_array()) {
        items.extend(arr.iter().cloned());
      }
      page_token = page
        .get("nextPageToken")
        .and_then(|v| v.as_str())
        .map(str::to_string);
      if page_token.is_none() {
        break;
      }
    }
    Ok(items)
  }

  pub async fn check_auth(&self, settings: &Settings, secrets: &Secrets) -> AuthCheckResult {
    let mut items = Vec::new();
    let finish = |items: Vec<AuthCheckItem>| AuthCheckResult {
      ok: items.iter().all(|i| i.ok),
      checked_at: chrono::Utc::now().to_rfc3339(),
      items,
    };

    let missing: Vec<&str> = [
      (!secrets.client_id.trim().is_empty(), "Client ID"),
      (!secrets.client_secret.trim().is_empty(), "Client Secret"),
      (!secrets.refresh_token.trim().is_empty(), "Refresh Token"),
    ]
    .into_iter()
    .filter_map(|(ok, name)| if ok { None } else { Some(name) })
    .collect();

    if !missing.is_empty() {
      items.push(AuthCheckItem {
        key: "credentials".into(),
        label: "OAuth 凭证".into(),
        ok: false,
        message: format!("配置中缺少 {}", missing.join("、")),
      });
      return finish(items);
    }
    items.push(AuthCheckItem {
      key: "credentials".into(),
      label: "OAuth 凭证".into(),
      ok: true,
      message: "已配置".into(),
    });

    let access_token = match self.get_access_token(settings, secrets).await {
      Ok(token) => {
        items.push(AuthCheckItem {
          key: "token".into(),
          label: "Access Token".into(),
          ok: true,
          message: "刷新成功".into(),
        });
        token
      }
      Err(err) => {
        items.push(AuthCheckItem {
          key: "token".into(),
          label: "Access Token".into(),
          ok: false,
          message: err,
        });
        return finish(items);
      }
    };

    match get_granted_scopes(settings, &access_token).await {
      Ok(granted) => {
        let missing: Vec<String> = GOOGLE_SCOPES
          .iter()
          .filter(|s| !granted.contains(**s))
          .map(|s| s.replace("https://www.googleapis.com/auth/", ""))
          .collect();
        if missing.is_empty() {
          items.push(AuthCheckItem {
            key: "scopes".into(),
            label: "权限范围".into(),
            ok: true,
            message: "所需权限均已授予".into(),
          });
        } else {
          items.push(AuthCheckItem {
            key: "scopes".into(),
            label: "权限范围".into(),
            ok: false,
            message: format!(
              "缺少权限：{}，请重新点击「授权登录」并勾选全部权限",
              missing.join("、")
            ),
          });
        }
      }
      Err(err) => items.push(AuthCheckItem {
        key: "scopes".into(),
        label: "权限范围".into(),
        ok: false,
        message: err,
      }),
    }

    items.push(check_ga_account(self, settings, secrets).await);
    items.push(check_gtm_account(self, settings, secrets).await);
    finish(items)
  }
}

/// 返回 (展示结果, refresh_token)。refresh_token 仅供后端保存，不得传给前端。
pub async fn run_oauth_login(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
) -> Result<(OAuthStartResult, String), String> {
  if secrets.client_id.trim().is_empty() || secrets.client_secret.trim().is_empty() {
    return Err("请先在配置中填写 Client ID 和 Client Secret 并保存".into());
  }

  let listener = TcpListener::bind("127.0.0.1:0")
    .await
    .map_err(|e| format!("无法启动本机回调服务：{e}"))?;
  let port = listener
    .local_addr()
    .map_err(|e| format!("读取回调端口失败：{e}"))?
    .port();
  let redirect_uri = format!("http://127.0.0.1:{port}");

  let state = random_urlsafe(16);
  let verifier = random_urlsafe(32);
  let challenge = {
    let hash = Sha256::digest(verifier.as_bytes());
    URL_SAFE_NO_PAD.encode(hash)
  };

  let mut auth_url =
    Url::parse("https://accounts.google.com/o/oauth2/v2/auth").map_err(|e| e.to_string())?;
  {
    let mut q = auth_url.query_pairs_mut();
    q.append_pair("client_id", secrets.client_id.trim());
    q.append_pair("redirect_uri", &redirect_uri);
    q.append_pair("response_type", "code");
    q.append_pair("scope", &GOOGLE_SCOPES.join(" "));
    q.append_pair("access_type", "offline");
    q.append_pair("prompt", "consent");
    q.append_pair("state", &state);
    q.append_pair("code_challenge", &challenge);
    q.append_pair("code_challenge_method", "S256");
  }

  open::that(auth_url.as_str()).map_err(|e| format!("无法打开浏览器：{e}"))?;
  let code = wait_for_oauth_code(listener, &state).await?;

  let http = GoogleClient::build_http(settings)?;
  let res = http
    .post("https://oauth2.googleapis.com/token")
    .form(&[
      ("code", code.as_str()),
      ("client_id", secrets.client_id.trim()),
      ("client_secret", secrets.client_secret.trim()),
      ("redirect_uri", redirect_uri.as_str()),
      ("grant_type", "authorization_code"),
      ("code_verifier", verifier.as_str()),
    ])
    .send()
    .await
    .map_err(|e| format!("换取 Token 失败：{e}"))?;

  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    return Err(format_api_error(status, &text));
  }

  let data: TokenResponse = res
    .json()
    .await
    .map_err(|e| format!("解析 Token 响应失败：{e}"))?;
  let refresh = data
    .refresh_token
    .ok_or_else(|| "Google 没有返回 Refresh Token，请重试".to_string())?;

  {
    let mut cache = google.token_cache.lock().await;
    *cache = Some(TokenCache {
      value: data.access_token,
      expires_at: Instant::now() + Duration::from_secs(data.expires_in),
    });
  }

  let granted: HashSet<String> = data
    .scope
    .unwrap_or_default()
    .split_whitespace()
    .map(str::to_string)
    .collect();
  let missing: Vec<String> = GOOGLE_SCOPES
    .iter()
    .filter(|s| !granted.contains(**s))
    .map(|s| s.replace("https://www.googleapis.com/auth/", ""))
    .collect();

  Ok((
    OAuthStartResult {
      message: "授权成功，Refresh Token 已保存到本机".into(),
      granted_scopes_ok: missing.is_empty(),
      missing_scopes: missing,
    },
    refresh,
  ))
}

async fn check_ga_account(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
) -> AuthCheckItem {
  let base = |ok: bool, message: String| AuthCheckItem {
    key: "gaAccount".into(),
    label: "GA 账号".into(),
    ok,
    message,
  };
  if settings.ga_account_id.trim().is_empty() {
    return base(false, "未填写 GA 账号 ID".into());
  }
  let url = format!(
    "https://analyticsadmin.googleapis.com/v1beta/accounts/{}",
    settings.ga_account_id.trim()
  );
  match google.api_get(settings, secrets, &url).await {
    Ok(v) => {
      let name = v
        .get("displayName")
        .and_then(|x| x.as_str())
        .unwrap_or(settings.ga_account_id.as_str());
      base(true, format!("可访问：{name}"))
    }
    Err(err) => base(false, describe_account_error(&err)),
  }
}

async fn check_gtm_account(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
) -> AuthCheckItem {
  let base = |ok: bool, message: String| AuthCheckItem {
    key: "gtmAccount".into(),
    label: "GTM 账号".into(),
    ok,
    message,
  };
  if settings.gtm_account_id.trim().is_empty() {
    return base(false, "未填写 GTM 账号 ID".into());
  }
  let url = format!(
    "https://tagmanager.googleapis.com/tagmanager/v2/accounts/{}",
    settings.gtm_account_id.trim()
  );
  match google.api_get(settings, secrets, &url).await {
    Ok(v) => {
      let name = v
        .get("name")
        .and_then(|x| x.as_str())
        .unwrap_or(settings.gtm_account_id.as_str());
      base(true, format!("可访问：{name}"))
    }
    Err(err) => base(false, describe_account_error(&err)),
  }
}

async fn get_granted_scopes(
  settings: &Settings,
  access_token: &str,
) -> Result<HashSet<String>, String> {
  let client = GoogleClient::build_http(settings)?;
  let res = client
    .get("https://oauth2.googleapis.com/tokeninfo")
    .query(&[("access_token", access_token)])
    .send()
    .await
    .map_err(|e| format!("查询权限失败：{e}"))?;
  if !res.status().is_success() {
    let status = res.status();
    let text = res.text().await.unwrap_or_default();
    return Err(format_api_error(status, &text));
  }
  #[derive(Deserialize)]
  struct Info {
    #[serde(default)]
    scope: String,
  }
  let info: Info = res.json().await.map_err(|e| e.to_string())?;
  Ok(info.scope.split_whitespace().map(str::to_string).collect())
}

async fn wait_for_oauth_code(listener: TcpListener, expected_state: &str) -> Result<String, String> {
  let result = tokio::time::timeout(Duration::from_secs(300), async {
    let (mut socket, _) = listener
      .accept()
      .await
      .map_err(|e| format!("等待授权回调失败：{e}"))?;
    let mut buf = vec![0u8; 4096];
    let n = socket
      .read(&mut buf)
      .await
      .map_err(|e| format!("读取回调失败：{e}"))?;
    let req = String::from_utf8_lossy(&buf[..n]);
    let first_line = req.lines().next().unwrap_or("");
    let path = first_line.split_whitespace().nth(1).unwrap_or("/");
    let url = Url::parse(&format!("http://127.0.0.1{path}"))
      .map_err(|e| format!("解析回调地址失败：{e}"))?;

    let body_ok = |message: &str| -> String {
      let body = format!("<p>{message}</p><p>可以关闭此页面并返回 Beacon。</p>");
      format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(),
        body
      )
    };

    if let Some(err) = url
      .query_pairs()
      .find(|(k, _)| k == "error")
      .map(|(_, v)| v.to_string())
    {
      let _ = socket.write_all(body_ok("授权被拒绝或失败。").as_bytes()).await;
      return Err(format!("授权失败：{err}"));
    }
    let state = url
      .query_pairs()
      .find(|(k, _)| k == "state")
      .map(|(_, v)| v.to_string());
    if state.as_deref() != Some(expected_state) {
      let _ = socket
        .write_all(body_ok("授权失败：state 不匹配。").as_bytes())
        .await;
      return Err("state 不匹配，已中止".into());
    }
    let code = url
      .query_pairs()
      .find(|(k, _)| k == "code")
      .map(|(_, v)| v.to_string())
      .ok_or_else(|| "回调中没有授权码".to_string())?;
    let _ = socket.write_all(body_ok("授权成功。").as_bytes()).await;
    Ok(code)
  })
  .await
  .map_err(|_| "等待授权超时（5 分钟）".to_string())?;
  result
}

fn random_urlsafe(nbytes: usize) -> String {
  let mut buf = vec![0u8; nbytes];
  rand::thread_rng().fill_bytes(&mut buf);
  URL_SAFE_NO_PAD.encode(buf)
}

fn format_api_error(status: StatusCode, text: &str) -> String {
  if let Ok(v) = serde_json::from_str::<Value>(text) {
    if let Some(obj) = v.get("error") {
      if let Some(msg) = obj.get("message").and_then(|m| m.as_str()) {
        return msg.to_string();
      }
      if let Some(s) = obj.as_str() {
        if let Some(desc) = v.get("error_description").and_then(|d| d.as_str()) {
          return format!("{s}: {desc}");
        }
        return s.to_string();
      }
    }
  }
  let trimmed = text.trim();
  if trimmed.is_empty() {
    return format!("HTTP {status}");
  }
  // Google 网关偶发返回 HTML 错误页，避免把整页塞进界面
  if trimmed.starts_with('<') {
    let title = trimmed
      .split_once("<title>")
      .and_then(|(_, rest)| rest.split_once("</title>"))
      .map(|(t, _)| t.trim())
      .filter(|t| !t.is_empty());
    return match title {
      Some(t) => format!("HTTP {status}（{t}）"),
      None => format!("HTTP {status}"),
    };
  }
  trimmed.chars().take(500).collect()
}

fn describe_token_error(message: &str) -> String {
  if message.starts_with("invalid_grant") {
    "Refresh Token 已失效或被撤销，请重新授权登录".into()
  } else if message.starts_with("invalid_client") || message.starts_with("unauthorized_client") {
    "Client ID 或 Client Secret 不正确".into()
  } else {
    format!("刷新 Access Token 失败：{message}")
  }
}

fn describe_account_error(message: &str) -> String {
  if message.contains("403") || message.contains("404") || message.contains("PERMISSION") {
    format!("无法访问该账号，请确认 ID 正确且当前 Google 账号有权限（{message}）")
  } else {
    message.to_string()
  }
}
