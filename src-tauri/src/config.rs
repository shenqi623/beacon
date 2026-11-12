use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
  pub ga_account_id: String,
  pub gtm_account_id: String,
  pub time_zone: String,
  pub currency_code: String,
  pub gtm_write_interval_ms: u64,
  pub gsc_verify_retry_interval_sec: u64,
  pub gsc_verify_max_attempts: u32,
  pub proxy_enabled: bool,
  pub proxy_url: String,
}

impl Default for Settings {
  fn default() -> Self {
    Self {
      ga_account_id: String::new(),
      gtm_account_id: String::new(),
      time_zone: "Asia/Shanghai".into(),
      currency_code: "USD".into(),
      gtm_write_interval_ms: 4000,
      gsc_verify_retry_interval_sec: 60,
      gsc_verify_max_attempts: 15,
      proxy_enabled: false,
      proxy_url: "http://127.0.0.1:7897".into(),
    }
  }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Secrets {
  pub client_id: String,
  pub client_secret: String,
  pub refresh_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct StoredConfig {
  settings: Settings,
  secrets: Secrets,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CredentialStatus {
  pub client_id: bool,
  pub client_secret: bool,
  pub refresh_token: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppConfigResponse {
  pub settings: Settings,
  pub credentials: CredentialStatus,
  pub data_dir: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveConfigRequest {
  pub settings: Settings,
  /// 仅在非空时覆盖已保存值
  pub client_id: Option<String>,
  pub client_secret: Option<String>,
}

pub struct ConfigStore {
  path: PathBuf,
  data_dir: PathBuf,
  data: StoredConfig,
}

impl ConfigStore {
  pub fn load(data_dir: PathBuf) -> Result<Self, String> {
    fs::create_dir_all(&data_dir).map_err(|e| format!("无法创建数据目录：{e}"))?;
    let path = data_dir.join("config.json");
    let data = if path.exists() {
      let text = fs::read_to_string(&path).map_err(|e| format!("读取配置失败：{e}"))?;
      serde_json::from_str(&text).unwrap_or_default()
    } else {
      StoredConfig::default()
    };
    Ok(Self {
      path,
      data_dir,
      data,
    })
  }

  fn persist(&self) -> Result<(), String> {
    let text =
      serde_json::to_string_pretty(&self.data).map_err(|e| format!("序列化配置失败：{e}"))?;
    let tmp = self.path.with_extension("json.tmp");
    fs::write(&tmp, text).map_err(|e| format!("写入配置失败：{e}"))?;
    fs::rename(&tmp, &self.path).map_err(|e| format!("保存配置失败：{e}"))?;
    Ok(())
  }

  pub fn data_dir(&self) -> &Path {
    &self.data_dir
  }

  pub fn settings(&self) -> &Settings {
    &self.data.settings
  }

  pub fn secrets(&self) -> &Secrets {
    &self.data.secrets
  }

  pub fn credential_status(&self) -> CredentialStatus {
    let s = &self.data.secrets;
    CredentialStatus {
      client_id: !s.client_id.trim().is_empty(),
      client_secret: !s.client_secret.trim().is_empty(),
      refresh_token: !s.refresh_token.trim().is_empty(),
    }
  }

  pub fn response(&self) -> AppConfigResponse {
    AppConfigResponse {
      settings: self.data.settings.clone(),
      credentials: self.credential_status(),
      data_dir: self.data_dir.to_string_lossy().into(),
    }
  }

  pub fn save_request(&mut self, req: SaveConfigRequest) -> Result<AppConfigResponse, String> {
    validate_settings(&req.settings)?;
    self.data.settings = req.settings;
    if let Some(id) = req.client_id {
      let id = id.trim().to_string();
      if !id.is_empty() {
        self.data.secrets.client_id = id;
      }
    }
    if let Some(secret) = req.client_secret {
      let secret = secret.trim().to_string();
      if !secret.is_empty() {
        self.data.secrets.client_secret = secret;
      }
    }
    self.persist()?;
    Ok(self.response())
  }

  pub fn set_refresh_token(&mut self, token: String) -> Result<(), String> {
    let token = token.trim().to_string();
    if token.is_empty() {
      return Err("Refresh Token 为空".into());
    }
    self.data.secrets.refresh_token = token;
    self.persist()
  }

  pub fn clear_secret(&mut self, field: &str) -> Result<AppConfigResponse, String> {
    match field {
      "clientSecret" => self.data.secrets.client_secret.clear(),
      "refreshToken" => self.data.secrets.refresh_token.clear(),
      "clientId" => self.data.secrets.client_id.clear(),
      _ => return Err(format!("未知字段：{field}")),
    }
    self.persist()?;
    Ok(self.response())
  }
}

fn validate_settings(settings: &Settings) -> Result<(), String> {
  if !settings.ga_account_id.is_empty()
    && !settings.ga_account_id.chars().all(|c| c.is_ascii_digit())
  {
    return Err("GA 账号 ID 只能是数字".into());
  }
  if !settings.gtm_account_id.is_empty()
    && !settings.gtm_account_id.chars().all(|c| c.is_ascii_digit())
  {
    return Err("GTM 账号 ID 只能是数字".into());
  }
  if settings.time_zone.trim().is_empty() {
    return Err("请填写默认时区".into());
  }
  if settings.currency_code.trim().len() != 3 {
    return Err("币种代码应为 3 位，例如 USD".into());
  }
  if !(1000..=600_000).contains(&settings.gtm_write_interval_ms) {
    return Err("GTM 写操作间隔请填写 1000 到 600000 之间的整数".into());
  }
  if !(1..=3600).contains(&settings.gsc_verify_retry_interval_sec) {
    return Err("GSC 验证重试间隔请填写 1 到 3600 之间的整数".into());
  }
  if !(1..=100).contains(&settings.gsc_verify_max_attempts) {
    return Err("GSC 验证最多尝试次数请填写 1 到 100 之间的整数".into());
  }
  if settings.proxy_enabled {
    let url = settings.proxy_url.trim();
    if url.is_empty() {
      return Err("已启用代理时请填写代理地址".into());
    }
    url::Url::parse(url).map_err(|_| "代理地址格式不正确，例如 http://127.0.0.1:7897".to_string())?;
  }
  Ok(())
}
