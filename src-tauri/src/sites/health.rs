use crate::commands::AppState;
use crate::config::{Secrets, Settings};
use crate::google::GoogleClient;
use crate::sites::deploy::check_homepage_gtm;
use crate::sites::store::{HealthItem, Site, SiteDetail, SiteOutputs, SiteStore};
use serde_json::Value;
use std::sync::Arc;

const GTM_API: &str = "https://tagmanager.googleapis.com/tagmanager/v2";
const ADMIN_API: &str = "https://analyticsadmin.googleapis.com/v1beta";
const WEBMASTERS_API: &str = "https://www.googleapis.com/webmasters/v3";

const TAG_CLICK: &str = "Beacon - click";
const TAG_OUTBOUND: &str = "Beacon - apk_download";
const TAG_FORM: &str = "Beacon - form_submit";
const EVENT_CLICK: &str = "click";
const EVENT_OUTBOUND: &str = "apk_download";
const EVENT_FORM: &str = "form_submit";

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

fn item(key: &str, label: &str, status: &str, message: impl Into<String>) -> HealthItem {
  HealthItem {
    key: key.into(),
    label: label.into(),
    status: status.into(),
    message: message.into(),
  }
}

fn param_value<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
  v.get("parameter")
    .and_then(|p| p.as_array())
    .and_then(|arr| {
      arr.iter().find_map(|param| {
        if param.get("key").and_then(|k| k.as_str()) == Some(key) {
          param.get("value").and_then(|x| x.as_str())
        } else {
          None
        }
      })
    })
}

struct LiveTag {
  name: String,
  tag_type: String,
  tag_id_param: Option<String>,
  event_name: Option<String>,
}

fn parse_live_tag(v: &Value) -> Option<LiveTag> {
  Some(LiveTag {
    name: v
      .get("name")
      .and_then(|n| n.as_str())
      .unwrap_or("")
      .to_string(),
    tag_type: v.get("type")?.as_str()?.to_string(),
    tag_id_param: param_value(v, "tagId")
      .or_else(|| param_value(v, "measurementIdOverride"))
      .map(str::to_string),
    event_name: param_value(v, "eventName").map(str::to_string),
  })
}

fn has_google_tag(tags: &[LiveTag], measurement_id: &str) -> bool {
  tags.iter().any(|tag| {
    tag.tag_type == "googtag" && tag.tag_id_param.as_deref() == Some(measurement_id)
  })
}

fn has_event_tag(tags: &[LiveTag], tag_name: &str, event_name: &str) -> bool {
  tags.iter().any(|tag| {
    tag.tag_type == "gaawe"
      && (tag.name == tag_name || tag.event_name.as_deref() == Some(event_name))
  })
}

fn summarize(items: &[HealthItem]) -> String {
  let actionable: Vec<_> = items
    .iter()
    .filter(|i| i.status != "skip")
    .collect();
  if actionable.is_empty() {
    return "unknown".into();
  }
  if actionable.iter().any(|i| i.status == "fail") {
    "unhealthy".into()
  } else {
    "healthy".into()
  }
}

async fn check_page(settings: &Settings, site: &Site) -> HealthItem {
  let Some(public_id) = site.outputs.gtm_public_id.as_deref() else {
    return item("page_gtm", "页面 GTM", "skip", "缺少 GTM ID，跳过");
  };
  let check = check_homepage_gtm(settings, &site.domain, public_id).await;
  item(
    "page_gtm",
    "页面 GTM",
    if check.ok { "pass" } else { "fail" },
    check.message,
  )
}

async fn check_gtm_live(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> HealthItem {
  let Some(container_path) = site.outputs.gtm_container_path.as_deref() else {
    return item("gtm_live", "GTM 线上标签", "skip", "缺少 GTM 容器路径，跳过");
  };
  let Some(measurement_id) = site.outputs.measurement_id.as_deref() else {
    return item("gtm_live", "GTM 线上标签", "skip", "缺少衡量 ID，跳过");
  };

  let url = format!("{GTM_API}/{container_path}/versions:live");
  let live = match google
    .api_json(settings, secrets, "GET", &url, None, true)
    .await
  {
    Ok(v) => v,
    Err(err) if err.status == 404 => {
      return item("gtm_live", "GTM 线上标签", "fail", "容器尚无线上版本");
    }
    Err(err) => {
      return item(
        "gtm_live",
        "GTM 线上标签",
        "fail",
        format!("读取线上版本失败：{}", err.message),
      );
    }
  };

  let tags: Vec<LiveTag> = live
    .get("tag")
    .and_then(|t| t.as_array())
    .map(|arr| arr.iter().filter_map(parse_live_tag).collect())
    .unwrap_or_default();

  if !has_google_tag(&tags, measurement_id) {
    return item(
      "gtm_live",
      "GTM 线上标签",
      "fail",
      format!("线上版本未找到衡量 ID {measurement_id} 的 Google 代码"),
    );
  }

  let mut missing = Vec::new();
  if site.outputs.track_click && !has_event_tag(&tags, TAG_CLICK, EVENT_CLICK) {
    missing.push("click");
  }
  if site.outputs.track_outbound && !has_event_tag(&tags, TAG_OUTBOUND, EVENT_OUTBOUND) {
    missing.push("apk_download");
  }
  if site.outputs.track_form && !has_event_tag(&tags, TAG_FORM, EVENT_FORM) {
    missing.push("form_submit");
  }
  if !missing.is_empty() {
    return item(
      "gtm_live",
      "GTM 线上标签",
      "fail",
      format!("线上版本缺少事件标签：{}", missing.join("、")),
    );
  }

  let version_id = live
    .get("containerVersionId")
    .and_then(|x| x.as_str())
    .unwrap_or("?");
  item(
    "gtm_live",
    "GTM 线上标签",
    "pass",
    format!("线上版本 {version_id} 含 Google 代码与所选监听标签"),
  )
}

async fn check_ga4(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> HealthItem {
  let Some(property_id) = site.outputs.ga_property_id.as_deref() else {
    return item("ga4", "GA4 资源", "skip", "缺少 GA4 属性 ID，跳过");
  };
  let Some(stream_id) = site.outputs.ga_data_stream_id.as_deref() else {
    return item("ga4", "GA4 资源", "skip", "缺少数据流 ID，跳过");
  };
  let Some(measurement_id) = site.outputs.measurement_id.as_deref() else {
    return item("ga4", "GA4 资源", "skip", "缺少衡量 ID，跳过");
  };

  let prop_url = format!("{ADMIN_API}/properties/{property_id}");
  match google
    .api_json(settings, secrets, "GET", &prop_url, None, true)
    .await
  {
    Ok(_) => {}
    Err(err) if err.status == 403 || err.status == 404 => {
      return item(
        "ga4",
        "GA4 资源",
        "fail",
        format!("无法读取属性 {property_id}（可能已删除或无权访问）"),
      );
    }
    Err(err) => {
      return item(
        "ga4",
        "GA4 资源",
        "fail",
        format!("读取属性失败：{}", err.message),
      );
    }
  }

  let stream_url = format!("{ADMIN_API}/properties/{property_id}/dataStreams/{stream_id}");
  let stream = match google
    .api_json(settings, secrets, "GET", &stream_url, None, true)
    .await
  {
    Ok(v) => v,
    Err(err) if err.status == 403 || err.status == 404 => {
      return item(
        "ga4",
        "GA4 资源",
        "fail",
        format!("无法读取数据流 {stream_id}（可能已删除或无权访问）"),
      );
    }
    Err(err) => {
      return item(
        "ga4",
        "GA4 资源",
        "fail",
        format!("读取数据流失败：{}", err.message),
      );
    }
  };

  let live_mid = stream
    .get("webStreamData")
    .and_then(|w| w.get("measurementId"))
    .and_then(|x| x.as_str());
  if live_mid != Some(measurement_id) {
    return item(
      "ga4",
      "GA4 资源",
      "fail",
      format!(
        "数据流衡量 ID 不一致：期望 {measurement_id}，实际 {}",
        live_mid.unwrap_or("（无）")
      ),
    );
  }

  item(
    "ga4",
    "GA4 资源",
    "pass",
    format!("属性 {property_id} 与数据流 {measurement_id} 仍存在"),
  )
}

async fn check_gsc(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> HealthItem {
  // GSC 步骤完成后才有意义；若站点尚未走到 GSC，仍可用域名尝试检查，缺站点则 fail/skip
  let site_url = format!("https://{}/", site.domain);
  let site_api = format!("{WEBMASTERS_API}/sites/{}", encode_component(&site_url));

  let sc_site = match google
    .api_json(settings, secrets, "GET", &site_api, None, true)
    .await
  {
    Ok(v) => v,
    Err(err) if err.status == 403 || err.status == 404 => {
      // 流水线未完成 GSC 时视为跳过更合理
      if site.status == "done" {
        return item(
          "gsc",
          "Search Console",
          "fail",
          format!("Search Console 中未找到或无法访问 {site_url}"),
        );
      }
      return item(
        "gsc",
        "Search Console",
        "skip",
        "站点尚未完成 GSC 加入，跳过",
      );
    }
    Err(err) => {
      return item(
        "gsc",
        "Search Console",
        "fail",
        format!("读取 Search Console 站点失败：{}", err.message),
      );
    }
  };

  let permission = sc_site
    .get("permissionLevel")
    .and_then(|p| p.as_str())
    .unwrap_or("");
  if permission == "siteUnverifiedUser" {
    return item(
      "gsc",
      "Search Console",
      "fail",
      format!("站点 {site_url} 存在但当前账号未验证"),
    );
  }

  let sitemap_api = format!(
    "{}/sitemaps/{}",
    site_api,
    encode_component(&site.sitemap_url)
  );
  match google
    .api_json(settings, secrets, "GET", &sitemap_api, None, true)
    .await
  {
    Ok(_) => item(
      "gsc",
      "Search Console",
      "pass",
      format!("站点可访问，sitemap 仍已提交：{}", site.sitemap_url),
    ),
    Err(err) if err.status == 403 || err.status == 404 => item(
      "gsc",
      "Search Console",
      "fail",
      format!("站点存在，但未找到已提交的 sitemap：{}", site.sitemap_url),
    ),
    Err(err) => item(
      "gsc",
      "Search Console",
      "fail",
      format!("读取 sitemap 失败：{}", err.message),
    ),
  }
}

pub async fn run_health_check(
  state: Arc<AppState>,
  site_id: i64,
) -> Result<SiteDetail, String> {
  if state.runner.is_active(site_id) {
    return Err("站点正在执行中，请稍后再检查".into());
  }

  let site = {
    let store = state.sites.lock();
    store
      .get(site_id)?
      .ok_or_else(|| "站点不存在".to_string())?
  };

  let (settings, secrets) = {
    let cfg = state.config.lock();
    (cfg.settings().clone(), cfg.secrets().clone())
  };

  let page = check_page(&settings, &site).await;
  let gtm = check_gtm_live(&state.google, &settings, &secrets, &site).await;
  let ga4 = check_ga4(&state.google, &settings, &secrets, &site).await;
  let gsc = check_gsc(&state.google, &settings, &secrets, &site).await;

  let items = vec![page, gtm, ga4, gsc];
  let health_status = summarize(&items);
  let checked_at = chrono::Utc::now().to_rfc3339();

  {
    let store = state.sites.lock();
    let current = store
      .get(site_id)?
      .ok_or_else(|| "站点不存在".to_string())?;
    let merged = SiteStore::merge_outputs(
      &current.outputs,
      &SiteOutputs {
        health_status: Some(health_status),
        health_checked_at: Some(checked_at),
        health_items: Some(items),
        ..Default::default()
      },
    );
    store.update(site_id, None, None, None, Some(&merged))?;
  }

  state
    .sites
    .lock()
    .get_detail(site_id)?
    .ok_or_else(|| "站点不存在".into())
}
