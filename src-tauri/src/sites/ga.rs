use crate::config::{Secrets, Settings};
use crate::google::{ApiError, GoogleClient};
use crate::sites::gtm::StepResult;
use crate::sites::store::{Site, SiteOutputs};
use serde_json::{json, Value};

const ADMIN_API: &str = "https://analyticsadmin.googleapis.com/v1beta";

#[derive(Debug, Clone)]
struct GaProperty {
  name: String,
  display_name: String,
}

#[derive(Debug, Clone)]
struct GaDataStream {
  name: String,
  stream_type: String,
  measurement_id: Option<String>,
  default_uri: Option<String>,
}

fn id_from_resource_name(name: &str) -> String {
  name
    .rsplit('/')
    .next()
    .unwrap_or(name)
    .to_string()
}

fn parse_property(v: &Value) -> Option<GaProperty> {
  Some(GaProperty {
    name: v.get("name")?.as_str()?.to_string(),
    display_name: v.get("displayName")?.as_str()?.to_string(),
  })
}

fn parse_stream(v: &Value) -> Option<GaDataStream> {
  let web = v.get("webStreamData");
  Some(GaDataStream {
    name: v.get("name")?.as_str()?.to_string(),
    stream_type: v.get("type")?.as_str()?.to_string(),
    measurement_id: web
      .and_then(|w| w.get("measurementId"))
      .and_then(|x| x.as_str())
      .map(str::to_string),
    default_uri: web
      .and_then(|w| w.get("defaultUri"))
      .and_then(|x| x.as_str())
      .map(str::to_string),
  })
}

fn hostname_of(uri: &str) -> Option<String> {
  let with_scheme = if uri.to_ascii_lowercase().starts_with("http://")
    || uri.to_ascii_lowercase().starts_with("https://")
  {
    uri.to_string()
  } else {
    format!("https://{uri}")
  };
  url::Url::parse(&with_scheme)
    .ok()
    .and_then(|u| u.host_str().map(|h| h.to_ascii_lowercase()))
}

async fn list_properties(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
) -> Result<Vec<GaProperty>, String> {
  let url = format!(
    "{ADMIN_API}/properties?filter=parent:accounts/{account_id}&pageSize=200"
  );
  let items = google
    .api_list_all(settings, secrets, &url, "properties")
    .await?;
  Ok(items.iter().filter_map(parse_property).collect())
}

async fn create_property(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
  display_name: &str,
) -> Result<GaProperty, ApiError> {
  let body = json!({
    "parent": format!("accounts/{account_id}"),
    "displayName": display_name,
    "timeZone": settings.time_zone,
    "currencyCode": settings.currency_code,
  });
  let v = google
    .api_json(
      settings,
      secrets,
      "POST",
      &format!("{ADMIN_API}/properties"),
      Some(body),
      false,
    )
    .await?;
  parse_property(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建属性响应缺少必要字段".into(),
  })
}

async fn list_data_streams(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  property_id: &str,
) -> Result<Vec<GaDataStream>, String> {
  let url = format!("{ADMIN_API}/properties/{property_id}/dataStreams?pageSize=200");
  let items = google
    .api_list_all(settings, secrets, &url, "dataStreams")
    .await?;
  Ok(items.iter().filter_map(parse_stream).collect())
}

async fn create_web_data_stream(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  property_id: &str,
  display_name: &str,
  default_uri: &str,
) -> Result<GaDataStream, ApiError> {
  let body = json!({
    "type": "WEB_DATA_STREAM",
    "displayName": display_name,
    "webStreamData": { "defaultUri": default_uri },
  });
  let url = format!("{ADMIN_API}/properties/{property_id}/dataStreams");
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_stream(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建数据流响应缺少必要字段".into(),
  })
}

async fn find_or_create_property(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  account_id: &str,
  site: &Site,
) -> Result<(GaProperty, bool), String> {
  let find = || async {
    let list = list_properties(google, settings, secrets, account_id).await?;
    Ok::<_, String>(
      list
        .into_iter()
        .find(|p| p.display_name == site.name),
    )
  };

  if let Some(existing) = find().await? {
    return Ok((existing, true));
  }

  let mut attempt = 0u32;
  loop {
    match create_property(google, settings, secrets, account_id, &site.name).await {
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

async fn find_or_create_stream(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  property_id: &str,
  site: &Site,
) -> Result<(GaDataStream, bool), String> {
  let default_uri = format!("https://{}", site.domain);
  let find = || async {
    let list = list_data_streams(google, settings, secrets, property_id).await?;
    Ok::<_, String>(list.into_iter().find(|s| {
      s.stream_type == "WEB_DATA_STREAM"
        && s.default_uri
          .as_deref()
          .and_then(hostname_of)
          .as_deref()
          == Some(site.domain.as_str())
    }))
  };

  if let Some(existing) = find().await? {
    return Ok((existing, true));
  }

  let mut attempt = 0u32;
  loop {
    match create_web_data_stream(
      google,
      settings,
      secrets,
      property_id,
      &site.name,
      &default_uri,
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

pub async fn run_ga_property_step(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<StepResult, String> {
  let account_id = settings.ga_account_id.trim();
  if account_id.is_empty() {
    return Err("未配置 GA 账号 ID".into());
  }
  let (property, reused) =
    find_or_create_property(google, settings, secrets, account_id, site).await?;
  let property_id = id_from_resource_name(&property.name);
  Ok(StepResult {
    status: "done",
    message: if reused {
      format!("复用已有属性「{}」（{}）", property.display_name, property_id)
    } else {
      format!(
        "已创建属性「{}」（{}，{}，{}）",
        property.display_name, property_id, settings.time_zone, settings.currency_code
      )
    },
    outputs: SiteOutputs {
      ga_property_id: Some(property_id),
      ..Default::default()
    },
  })
}

pub async fn run_data_stream_step(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<StepResult, String> {
  let property_id = site
    .outputs
    .ga_property_id
    .as_deref()
    .ok_or_else(|| "缺少 GA4 属性 ID，请先完成「创建 GA4 属性」".to_string())?;

  let (stream, reused) =
    find_or_create_stream(google, settings, secrets, property_id, site).await?;
  let measurement_id = stream
    .measurement_id
    .ok_or_else(|| format!("数据流 {} 没有返回衡量 ID", stream.name))?;
  let stream_id = id_from_resource_name(&stream.name);

  Ok(StepResult {
    status: "done",
    message: if reused {
      format!("复用已有 Web 数据流（{measurement_id}）")
    } else {
      format!("已创建 Web 数据流（{measurement_id}）")
    },
    outputs: SiteOutputs {
      ga_data_stream_id: Some(stream_id),
      measurement_id: Some(measurement_id),
      ..Default::default()
    },
  })
}
