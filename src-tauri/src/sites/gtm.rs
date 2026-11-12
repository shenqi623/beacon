use crate::config::{Secrets, Settings};
use crate::google::{ApiError, GoogleClient};
use crate::sites::store::{Site, SiteOutputs};
use serde_json::{json, Value};

const GTM_API: &str = "https://tagmanager.googleapis.com/tagmanager/v2";

#[derive(Debug, Clone)]
pub struct GtmContainer {
  pub path: String,
  pub container_id: String,
  pub name: String,
  pub public_id: String,
  pub usage_context: Vec<String>,
  pub domain_name: Vec<String>,
}

fn parse_container(v: &Value) -> Option<GtmContainer> {
  Some(GtmContainer {
    path: v.get("path")?.as_str()?.to_string(),
    container_id: v.get("containerId")?.as_str()?.to_string(),
    name: v.get("name")?.as_str()?.to_string(),
    public_id: v.get("publicId")?.as_str()?.to_string(),
    usage_context: v
      .get("usageContext")
      .and_then(|x| x.as_array())
      .map(|arr| {
        arr
          .iter()
          .filter_map(|i| i.as_str().map(str::to_string))
          .collect()
      })
      .unwrap_or_default(),
    domain_name: v
      .get("domainName")
      .and_then(|x| x.as_array())
      .map(|arr| {
        arr
          .iter()
          .filter_map(|i| i.as_str().map(str::to_string))
          .collect()
      })
      .unwrap_or_default(),
  })
}

async fn list_containers(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
) -> Result<Vec<GtmContainer>, String> {
  let url = format!("{GTM_API}/accounts/{account_id}/containers");
  let items = google
    .api_list_all(settings, secrets, &url, "container")
    .await?;
  Ok(items.iter().filter_map(parse_container).collect())
}

async fn get_container(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
) -> Result<Option<GtmContainer>, String> {
  let url = format!("{GTM_API}/{container_path}");
  match google
    .api_json(settings, secrets, "GET", &url, None, true)
    .await
  {
    Ok(v) => Ok(parse_container(&v)),
    Err(err) if err.status == 404 => Ok(None),
    Err(err) => Err(err.message),
  }
}

async fn create_container(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
  name: &str,
  domain: &str,
) -> Result<GtmContainer, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/accounts/{account_id}/containers");
  let body = json!({
    "name": name,
    "usageContext": ["web"],
    "domainName": [domain],
  });
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_container(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建容器响应缺少必要字段".into(),
  })
}

async fn find_or_create_container(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
  site: &Site,
) -> Result<(GtmContainer, bool), String> {
  let find = || async {
    let list = list_containers(google, settings, secrets, account_id).await?;
    Ok::<_, String>(list.into_iter().find(|c| {
      c.usage_context.iter().any(|u| u == "web")
        && (c.name == site.name || c.domain_name.iter().any(|d| d.contains(&site.domain)))
    }))
  };

  if let Some(existing) = find().await? {
    return Ok((existing, true));
  }

  let mut attempt = 0u32;
  loop {
    match create_container(
      google,
      settings,
      secrets,
      account_id,
      &site.name,
      &site.domain,
    )
    .await
    {
      Ok(created) => return Ok((created, false)),
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          return Err(err.message);
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if let Some(created) = find().await? {
          return Ok((created, false));
        }
        attempt += 1;
      }
    }
  }
}

pub struct StepResult {
  pub status: &'static str, // done | waiting
  pub message: String,
  pub outputs: SiteOutputs,
}

pub async fn run_gtm_container_step(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<StepResult, String> {
  let account_id = settings.gtm_account_id.trim();
  if account_id.is_empty() {
    return Err("未配置 GTM 账号 ID".into());
  }
  let (container, reused) =
    find_or_create_container(google, settings, secrets, account_id, site).await?;
  // 创建/复用后立刻回读，避免把已失效的 path 写入本地
  let verified = get_container(google, settings, secrets, &container.path)
    .await?
    .ok_or_else(|| {
      format!(
        "容器「{}」创建后立即无法读取（{}），请稍后重试或到 GTM 后台确认账号 {}",
        container.name, container.public_id, account_id
      )
    })?;
  Ok(StepResult {
    status: "done",
    message: if reused {
      format!(
        "复用已有容器「{}」（{}）",
        verified.name, verified.public_id
      )
    } else {
      format!(
        "已创建容器「{}」（{}）",
        verified.name, verified.public_id
      )
    },
    outputs: SiteOutputs {
      gtm_container_path: Some(verified.path),
      gtm_container_id: Some(verified.container_id),
      gtm_public_id: Some(verified.public_id),
      ..Default::default()
    },
  })
}

/// 发布前确认容器仍存在；若已被删除则按名称/域名重新查找或创建。
pub async fn ensure_container_for_site(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<(GtmContainer, Option<&'static str>), String> {
  let account_id = settings.gtm_account_id.trim();
  if account_id.is_empty() {
    return Err("未配置 GTM 账号 ID".into());
  }
  if let Some(path) = site.outputs.gtm_container_path.as_deref() {
    if let Some(existing) = get_container(google, settings, secrets, path).await? {
      return Ok((existing, None));
    }
  }
  let (container, reused) =
    find_or_create_container(google, settings, secrets, account_id, site).await?;
  let verified = get_container(google, settings, secrets, &container.path)
    .await?
    .ok_or_else(|| {
      format!(
        "无法恢复 GTM 容器「{}」，请到 GTM 后台确认账号 {} 下是否可见",
        site.name, account_id
      )
    })?;
  Ok((
    verified,
    Some(if reused {
      "原容器已不存在，已复用同名容器"
    } else {
      "原容器已不存在，已重新创建容器"
    }),
  ))
}
