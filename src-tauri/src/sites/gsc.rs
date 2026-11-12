use crate::config::{Secrets, Settings};
use crate::google::{ApiError, GoogleClient};
use crate::sites::gtm::StepResult;
use crate::sites::store::{Site, SiteOutputs};
use serde_json::{json, Value};
use std::time::Duration;

const SITE_VERIFICATION_API: &str = "https://www.googleapis.com/siteVerification/v1";
const WEBMASTERS_API: &str = "https://www.googleapis.com/webmasters/v3";

fn encode_component(s: &str) -> String {
  let mut out = String::with_capacity(s.len() * 3);
  for b in s.bytes() {
    match b {
      b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'!' | b'~' | b'*' | b'\''
      | b'(' | b')' => out.push(b as char),
      _ => out.push_str(&format!("%{b:02X}")),
    }
  }
  out
}

fn site_path(site_url: &str) -> String {
  format!("{WEBMASTERS_API}/sites/{}", encode_component(site_url))
}

async fn is_site_verified(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
) -> Result<bool, String> {
  let url = format!("{SITE_VERIFICATION_API}/webResource");
  let v = google.api_get(settings, secrets, &url).await?;
  let items = v
    .get("items")
    .and_then(|i| i.as_array())
    .cloned()
    .unwrap_or_default();
  Ok(items.iter().any(|item| {
    item
      .get("site")
      .map(|site| {
        site.get("type").and_then(|t| t.as_str()) == Some("SITE")
          && site.get("identifier").and_then(|i| i.as_str()) == Some(site_url)
      })
      .unwrap_or(false)
  }))
}

async fn get_verification_token(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
) -> Result<String, String> {
  let body = json!({
    "verificationMethod": "TAG_MANAGER",
    "site": { "type": "SITE", "identifier": site_url },
  });
  let v = google
    .api_json(
      settings,
      secrets,
      "POST",
      &format!("{SITE_VERIFICATION_API}/token"),
      Some(body),
      true,
    )
    .await
    .map_err(|e| e.message)?;
  v.get("token")
    .and_then(|t| t.as_str())
    .map(str::to_string)
    .ok_or_else(|| "获取验证令牌失败：响应中没有 token".into())
}

async fn verify_site(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
) -> Result<(), ApiError> {
  let body = json!({
    "site": { "type": "SITE", "identifier": site_url },
  });
  google
    .api_json(
      settings,
      secrets,
      "POST",
      &format!("{SITE_VERIFICATION_API}/webResource?verificationMethod=TAG_MANAGER"),
      Some(body),
      true,
    )
    .await
    .map(|_| ())
}

async fn get_optional(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  url: &str,
) -> Result<Option<Value>, String> {
  match google
    .api_json(settings, secrets, "GET", url, None, true)
    .await
  {
    Ok(v) => Ok(Some(v)),
    Err(err) if err.status == 403 || err.status == 404 => Ok(None),
    Err(err) => Err(err.message),
  }
}

async fn get_search_console_site(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
) -> Result<Option<Value>, String> {
  get_optional(google, settings, secrets, &site_path(site_url)).await
}

async fn add_search_console_site(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
) -> Result<(), String> {
  google
    .api_json(settings, secrets, "PUT", &site_path(site_url), None, true)
    .await
    .map(|_| ())
    .map_err(|e| e.message)
}

async fn is_sitemap_submitted(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
  sitemap_url: &str,
) -> Result<bool, String> {
  let url = format!(
    "{}/sitemaps/{}",
    site_path(site_url),
    encode_component(sitemap_url)
  );
  Ok(get_optional(google, settings, secrets, &url).await?.is_some())
}

async fn submit_sitemap(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site_url: &str,
  sitemap_url: &str,
) -> Result<(), String> {
  let url = format!(
    "{}/sitemaps/{}",
    site_path(site_url),
    encode_component(sitemap_url)
  );
  google
    .api_json(settings, secrets, "PUT", &url, None, true)
    .await
    .map(|_| ())
    .map_err(|e| e.message)
}

pub async fn run_gsc_verify_step<F>(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
  mut report_progress: F,
) -> Result<StepResult, String>
where
  F: FnMut(&str),
{
  let site_url = format!("https://{}/", site.domain);
  if is_site_verified(google, settings, secrets, &site_url).await? {
    return Ok(StepResult {
      status: "done",
      message: format!("{site_url} 已验证过所有权，无需重复验证"),
      outputs: SiteOutputs::default(),
    });
  }

  let _token = get_verification_token(google, settings, secrets, &site_url).await?;

  let max_attempts = settings.gsc_verify_max_attempts.max(1);
  let interval_sec = settings.gsc_verify_retry_interval_sec.max(1);
  for attempt in 1..=max_attempts {
    match verify_site(google, settings, secrets, &site_url).await {
      Ok(()) => {
        return Ok(StepResult {
          status: "done",
          message: format!("已通过 Tag Manager 方式验证 {site_url}（第 {attempt} 次尝试）"),
          outputs: SiteOutputs::default(),
        });
      }
      Err(err) if err.status == 400 => {
        if attempt >= max_attempts {
          return Err(format!(
            "已尝试 {max_attempts} 次仍未通过验证，请确认 GTM 代码已部署且版本已发布，稍后从失败处继续（Google 返回：{}）",
            err.message
          ));
        }
        report_progress(&format!(
          "第 {attempt}/{max_attempts} 次验证未通过，GTM 发布生效可能有延迟，{interval_sec} 秒后重试（Google 返回：{}）",
          err.message
        ));
        tokio::time::sleep(Duration::from_secs(interval_sec)).await;
      }
      Err(err) => return Err(err.message),
    }
  }
  Err(format!("已尝试 {max_attempts} 次仍未通过验证"))
}

pub async fn run_gsc_add_step(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<StepResult, String> {
  let site_url = format!("https://{}/", site.domain);
  let mut actions = Vec::new();

  let existing = get_search_console_site(google, settings, secrets, &site_url).await?;
  let permission = existing
    .as_ref()
    .and_then(|v| v.get("permissionLevel"))
    .and_then(|p| p.as_str());
  if let Some(level) = permission {
    if level != "siteUnverifiedUser" {
      actions.push(format!("{site_url} 已在 Search Console 中（{level}）"));
    } else {
      add_search_console_site(google, settings, secrets, &site_url).await?;
      actions.push(format!("已将 {site_url} 加入 Search Console"));
    }
  } else {
    add_search_console_site(google, settings, secrets, &site_url).await?;
    actions.push(format!("已将 {site_url} 加入 Search Console"));
  }

  if is_sitemap_submitted(google, settings, secrets, &site_url, &site.sitemap_url).await? {
    actions.push(format!("sitemap 已提交过（{}）", site.sitemap_url));
  } else {
    submit_sitemap(google, settings, secrets, &site_url, &site.sitemap_url).await?;
    actions.push(format!("已提交 sitemap（{}）", site.sitemap_url));
  }

  Ok(StepResult {
    status: "done",
    message: actions.join("；"),
    outputs: SiteOutputs::default(),
  })
}
