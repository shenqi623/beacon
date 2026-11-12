use crate::config::{Secrets, Settings};
use crate::google::{ApiError, GoogleClient};
use crate::sites::gtm::{ensure_container_for_site, StepResult};
use crate::sites::store::{Site, SiteOutputs};
use serde_json::{json, Value};

const GTM_API: &str = "https://tagmanager.googleapis.com/tagmanager/v2";
const ALL_PAGES_TRIGGER_ID: &str = "2147479553";
const WORKSPACE_NAME: &str = "Beacon";

const TRIGGER_CLICK: &str = "Beacon - All Clicks";
const TRIGGER_OUTBOUND: &str = "Beacon - Outbound";
const TRIGGER_FORM: &str = "Beacon - Form Submit";
const TAG_CLICK: &str = "Beacon - click";
const TAG_OUTBOUND: &str = "Beacon - apk_download";
const TAG_FORM: &str = "Beacon - form_submit";

const EVENT_CLICK: &str = "click";
const EVENT_OUTBOUND: &str = "apk_download";
const EVENT_FORM: &str = "form_submit";

#[derive(Debug, Clone)]
struct GtmTag {
  path: Option<String>,
  name: String,
  tag_type: String,
  tag_id_param: Option<String>,
  event_name: Option<String>,
  /// 原始 parameter JSON，用于检测无法编译的变量引用
  parameter_blob: String,
}

#[derive(Debug, Clone)]
struct GtmTrigger {
  trigger_id: String,
  name: String,
}

#[derive(Debug, Clone)]
struct GtmWorkspace {
  path: String,
  name: String,
}

#[derive(Debug, Clone)]
struct GtmVersion {
  path: String,
  container_version_id: String,
  #[allow(dead_code)]
  name: Option<String>,
  tags: Vec<GtmTag>,
}

#[derive(Debug, Clone)]
struct GtmVersionHeader {
  #[allow(dead_code)]
  path: String,
  container_version_id: String,
  name: Option<String>,
  deleted: bool,
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

fn parse_tag(v: &Value) -> Option<GtmTag> {
  let tag_type = v.get("type")?.as_str()?.to_string();
  let name = v
    .get("name")
    .and_then(|n| n.as_str())
    .unwrap_or("")
    .to_string();
  let tag_id_param = param_value(v, "tagId")
    .or_else(|| param_value(v, "measurementIdOverride"))
    .map(str::to_string);
  let event_name = param_value(v, "eventName").map(str::to_string);
  let parameter_blob = v
    .get("parameter")
    .map(|p| p.to_string())
    .unwrap_or_default();
  Some(GtmTag {
    path: v.get("path").and_then(|p| p.as_str()).map(str::to_string),
    name,
    tag_type,
    tag_id_param,
    event_name,
    parameter_blob,
  })
}

/// ELEMENT 类内置变量不能写入 GA4 事件参数，否则 create_version 会 compilerError。
fn tag_uses_element_builtins(tag: &GtmTag) -> bool {
  tag.parameter_blob.contains("{{Click Element}}")
    || tag.parameter_blob.contains("{{Form Element}}")
}

fn parse_trigger(v: &Value) -> Option<GtmTrigger> {
  Some(GtmTrigger {
    trigger_id: v.get("triggerId")?.as_str()?.to_string(),
    name: v.get("name")?.as_str()?.to_string(),
  })
}

fn has_google_tag(tags: &[GtmTag], measurement_id: &str) -> bool {
  tags.iter().any(|tag| {
    tag.tag_type == "googtag" && tag.tag_id_param.as_deref() == Some(measurement_id)
  })
}

fn has_event_tag(tags: &[GtmTag], tag_name: &str, event_name: &str) -> bool {
  tags.iter().any(|tag| {
    tag.tag_type == "gaawe"
      && (tag.name == tag_name || tag.event_name.as_deref() == Some(event_name))
  })
}

fn live_has_selected_tracking(tags: &[GtmTag], site: &Site) -> bool {
  if site.outputs.track_click && !has_event_tag(tags, TAG_CLICK, EVENT_CLICK) {
    return false;
  }
  if site.outputs.track_outbound && !has_event_tag(tags, TAG_OUTBOUND, EVENT_OUTBOUND) {
    return false;
  }
  if site.outputs.track_form && !has_event_tag(tags, TAG_FORM, EVENT_FORM) {
    return false;
  }
  true
}

fn parse_workspace(v: &Value) -> Option<GtmWorkspace> {
  Some(GtmWorkspace {
    path: v.get("path")?.as_str()?.to_string(),
    name: v.get("name")?.as_str()?.to_string(),
  })
}

fn parse_version(v: &Value) -> Option<GtmVersion> {
  let tags = v
    .get("tag")
    .and_then(|t| t.as_array())
    .map(|arr| arr.iter().filter_map(parse_tag).collect())
    .unwrap_or_default();
  Some(GtmVersion {
    path: v.get("path")?.as_str()?.to_string(),
    container_version_id: v.get("containerVersionId")?.as_str()?.to_string(),
    name: v
      .get("name")
      .and_then(|n| n.as_str())
      .map(str::to_string),
    tags,
  })
}

fn parse_version_header(v: &Value) -> Option<GtmVersionHeader> {
  Some(GtmVersionHeader {
    path: v.get("path")?.as_str()?.to_string(),
    container_version_id: v.get("containerVersionId")?.as_str()?.to_string(),
    name: v
      .get("name")
      .and_then(|n| n.as_str())
      .map(str::to_string),
    deleted: v
      .get("deleted")
      .and_then(|d| d.as_bool())
      .unwrap_or(false),
  })
}

fn event_settings_table(params: &[(&str, &str)]) -> Value {
  json!({
    "type": "list",
    "key": "eventSettingsTable",
    "list": params.iter().map(|(name, value)| json!({
      "type": "map",
      "map": [
        { "type": "template", "key": "parameter", "value": name },
        { "type": "template", "key": "parameterValue", "value": value },
      ]
    })).collect::<Vec<_>>()
  })
}

fn gaawe_tag_body(name: &str, event_name: &str, measurement_id: &str, trigger_id: &str, params: &[(&str, &str)]) -> Value {
  let mut parameter = vec![
    json!({ "type": "template", "key": "eventName", "value": event_name }),
    json!({ "type": "template", "key": "measurementIdOverride", "value": measurement_id }),
  ];
  if !params.is_empty() {
    parameter.push(event_settings_table(params));
  }
  json!({
    "name": name,
    "type": "gaawe",
    "parameter": parameter,
    "firingTriggerId": [trigger_id],
  })
}

async fn get_live_version(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
) -> Result<Option<GtmVersion>, String> {
  let url = format!("{GTM_API}/{container_path}/versions:live");
  match google
    .api_json(settings, secrets, "GET", &url, None, true)
    .await
  {
    Ok(v) => Ok(parse_version(&v)),
    Err(err) if err.status == 404 => Ok(None),
    Err(err) => Err(err.message),
  }
}

async fn list_version_headers(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
) -> Result<Vec<GtmVersionHeader>, String> {
  let url = format!("{GTM_API}/{container_path}/version_headers");
  let items = google
    .api_list_all(settings, secrets, &url, "containerVersionHeader")
    .await?;
  Ok(items.iter().filter_map(parse_version_header).collect())
}

async fn list_workspaces(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
) -> Result<Vec<GtmWorkspace>, String> {
  let url = format!("{GTM_API}/{container_path}/workspaces");
  let items = google
    .api_list_all(settings, secrets, &url, "workspace")
    .await?;
  Ok(items.iter().filter_map(parse_workspace).collect())
}

async fn create_workspace(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
) -> Result<GtmWorkspace, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{container_path}/workspaces");
  let body = json!({ "name": WORKSPACE_NAME });
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_workspace(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建工作区响应缺少必要字段".into(),
  })
}

async fn list_tags(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
) -> Result<Vec<GtmTag>, String> {
  let url = format!("{GTM_API}/{workspace_path}/tags");
  let items = google.api_list_all(settings, secrets, &url, "tag").await?;
  Ok(items.iter().filter_map(parse_tag).collect())
}

async fn list_triggers(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
) -> Result<Vec<GtmTrigger>, String> {
  let url = format!("{GTM_API}/{workspace_path}/triggers");
  let items = google
    .api_list_all(settings, secrets, &url, "trigger")
    .await?;
  Ok(items.iter().filter_map(parse_trigger).collect())
}

async fn create_google_tag(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  measurement_id: &str,
) -> Result<GtmTag, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{workspace_path}/tags");
  let body = json!({
    "name": format!("GA4 - {measurement_id}"),
    "type": "googtag",
    "parameter": [{ "type": "template", "key": "tagId", "value": measurement_id }],
    "firingTriggerId": [ALL_PAGES_TRIGGER_ID],
  });
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_tag(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建标签响应缺少必要字段".into(),
  })
}

async fn create_trigger(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  body: Value,
) -> Result<GtmTrigger, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{workspace_path}/triggers");
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_trigger(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建触发器响应缺少必要字段".into(),
  })
}

async fn create_event_tag(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  body: Value,
) -> Result<GtmTag, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{workspace_path}/tags");
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  parse_tag(&v).ok_or_else(|| ApiError {
    status: 0,
    message: "创建事件标签响应缺少必要字段".into(),
  })
}

async fn delete_tag(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  tag_path: &str,
) -> Result<(), ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{tag_path}");
  google
    .api_json(settings, secrets, "DELETE", &url, None, false)
    .await
    .map(|_| ())
}

fn builtin_already_ok(err: &ApiError) -> bool {
  let msg = err.message.to_ascii_lowercase();
  err.status == 409
    || msg.contains("already")
    || msg.contains("exist")
    || msg.contains("duplicate")
}

fn required_builtins(site: &Site) -> Vec<&'static str> {
  let mut set = std::collections::BTreeSet::new();
  // 不含 clickElement / formElement：它们是 ELEMENT 类型，不能进 GA4 事件参数
  if site.outputs.track_click || site.outputs.track_outbound {
    for t in ["clickClasses", "clickId", "clickTarget", "clickUrl", "clickText"] {
      set.insert(t);
    }
  }
  if site.outputs.track_outbound {
    set.insert("pageHostname");
  }
  if site.outputs.track_form {
    for t in ["formClasses", "formId", "formTarget", "formUrl", "formText"] {
      set.insert(t);
    }
  }
  set.into_iter().collect()
}

async fn list_builtin_types(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
) -> Result<std::collections::HashSet<String>, String> {
  let url = format!("{GTM_API}/{workspace_path}/built_in_variables");
  let items = google
    .api_list_all(settings, secrets, &url, "builtInVariable")
    .await?;
  Ok(
    items
      .iter()
      .filter_map(|v| v.get("type").and_then(|t| t.as_str()).map(str::to_string))
      .collect(),
  )
}

async fn post_builtin_types(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  types: &[&str],
  force_direct: bool,
) -> Result<(), ApiError> {
  if types.is_empty() {
    return Ok(());
  }
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let mut url = format!("{GTM_API}/{workspace_path}/built_in_variables?");
  url.push_str(
    &types
      .iter()
      .map(|t| format!("type={t}"))
      .collect::<Vec<_>>()
      .join("&"),
  );
  google
    .api_json_opts(
      settings,
      secrets,
      "POST",
      &url,
      None,
      false,
      force_direct,
    )
    .await
    .map(|_| ())
}

/// 启用监听所需内置变量；失败则整步失败（不降级）。
async fn ensure_builtin_variables(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  site: &Site,
  actions: &mut Vec<String>,
) -> Result<(), String> {
  let required = required_builtins(site);
  if required.is_empty() {
    return Ok(());
  }

  let existing = list_builtin_types(google, settings, secrets, workspace_path).await?;
  let missing: Vec<&str> = required
    .iter()
    .copied()
    .filter(|t| !existing.contains(*t))
    .collect();
  if missing.is_empty() {
    actions.push(format!("内置变量已就绪（{}）", required.len()));
    return Ok(());
  }

  // 批量启用：先直连再走代理。空 POST 易 411；遇 411 不再逐个类型空转。
  let mut last_err = String::new();
  let mut hit_411 = false;
  for force_direct in [true, false] {
    match post_builtin_types(
      google,
      settings,
      secrets,
      workspace_path,
      &missing,
      force_direct,
    )
    .await
    {
      Ok(()) => {
        last_err.clear();
        hit_411 = false;
        break;
      }
      Err(err) if builtin_already_ok(&err) => {
        last_err.clear();
        hit_411 = false;
        break;
      }
      Err(err) => {
        hit_411 = err.status == 411 || err.message.contains("411");
        last_err = err.message;
        if hit_411 {
          break;
        }
      }
    }
  }

  let still = list_builtin_types(google, settings, secrets, workspace_path).await?;
  let mut remain: Vec<&str> = required
    .iter()
    .copied()
    .filter(|t| !still.contains(*t))
    .collect();

  // 非 411 时再逐个补齐；411 说明空 POST 通道不通，逐个只会拖时间
  if !remain.is_empty() && !hit_411 {
    for typ in remain.clone() {
      let mut typ_ok = false;
      for force_direct in [true, false] {
        match post_builtin_types(
          google,
          settings,
          secrets,
          workspace_path,
          &[typ],
          force_direct,
        )
        .await
        {
          Ok(()) => {
            typ_ok = true;
            break;
          }
          Err(err) if builtin_already_ok(&err) => {
            typ_ok = true;
            break;
          }
          Err(err) => {
            hit_411 = err.status == 411 || err.message.contains("411");
            last_err = err.message;
            if hit_411 {
              break;
            }
          }
        }
      }
      if hit_411 {
        break;
      }
      if typ_ok {
        // 已启用，稍后统一复查
      }
    }
    let still2 = list_builtin_types(google, settings, secrets, workspace_path).await?;
    remain = required
      .iter()
      .copied()
      .filter(|t| !still2.contains(*t))
      .collect();
  }

  if !remain.is_empty() {
    let hint = if settings.proxy_enabled {
      "若持续失败，可暂时关闭代理后重试「从失败处继续」。"
    } else {
      "请确认网络可访问 tagmanager.googleapis.com 后重试。"
    };
    return Err(format!(
      "启用 GTM 内置变量失败（缺少：{}）。监听点击/出站/表单依赖这些变量，无法降级。{}{}",
      remain.join("、"),
      if last_err.is_empty() {
        String::new()
      } else {
        format!("最近错误：{last_err}。")
      },
      hint
    ));
  }

  actions.push(format!("已启用内置变量（{}）", required.len()));
  Ok(())
}

async fn create_version(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  version_name: &str,
  notes: &str,
) -> Result<GtmVersion, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{workspace_path}:create_version");
  let body = json!({ "name": version_name, "notes": notes });
  let v = google
    .api_json(settings, secrets, "POST", &url, Some(body), false)
    .await?;
  if v.get("compilerError").and_then(|x| x.as_bool()) == Some(true) {
    return Err(ApiError {
      status: 0,
      message: format!(
        "创建版本时 GTM 编译出错（常见原因：Google 自动插入的 dest_ga 与 Google 代码冲突，或事件参数引用了无法序列化的变量）。请点「从失败处继续」重试，或到 GTM 后台检查工作区「{WORKSPACE_NAME}」"
      ),
    });
  }
  if let Some(sync) = v.get("syncStatus") {
    if sync.get("mergeConflict").and_then(|x| x.as_bool()) == Some(true)
      || sync.get("syncError").and_then(|x| x.as_bool()) == Some(true)
    {
      return Err(ApiError {
        status: 0,
        message: format!("工作区「{WORKSPACE_NAME}」与线上版本存在冲突，请在 GTM 后台处理后重试"),
      });
    }
  }
  let version = v
    .get("containerVersion")
    .and_then(parse_version)
    .ok_or_else(|| ApiError {
      status: 0,
      message: "创建版本后 GTM 没有返回版本信息".into(),
    })?;
  Ok(version)
}

async fn publish_version(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  version_path: &str,
) -> Result<Value, ApiError> {
  google.gtm_write_slot(settings.gtm_write_interval_ms).await;
  let url = format!("{GTM_API}/{version_path}:publish");
  google
    .api_json(settings, secrets, "POST", &url, None, true)
    .await
}

/// 从未发布过的容器没有 live 版本，此时新建工作区常会 404；优先用 Default Workspace。
async fn resolve_workspace(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
  has_live_version: bool,
) -> Result<(GtmWorkspace, String), String> {
  let workspaces = list_workspaces(google, settings, secrets, container_path)
    .await
    .map_err(|e| format!("列出工作区失败：{e}"))?;

  if let Some(ws) = workspaces.iter().find(|w| w.name == "Default Workspace") {
    return Ok((ws.clone(), "使用默认工作区".into()));
  }
  if let Some(ws) = workspaces.iter().find(|w| w.name == WORKSPACE_NAME) {
    return Ok((ws.clone(), "复用工作区 Beacon".into()));
  }
  if let Some(ws) = workspaces.first() {
    return Ok((ws.clone(), format!("使用工作区「{}」", ws.name)));
  }

  if !has_live_version {
    return Err(
      "容器尚无工作区且从未发布，无法继续。请到 GTM 后台打开该容器一次后再试".into(),
    );
  }

  let mut attempt = 0u32;
  loop {
    match create_workspace(google, settings, secrets, container_path).await {
      Ok(created) => return Ok((created, "新建工作区 Beacon".into())),
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          return Err(format!("创建工作区失败：{}", err.message));
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let again = list_workspaces(google, settings, secrets, container_path).await?;
        if let Some(ws) = again.into_iter().find(|w| w.name == WORKSPACE_NAME) {
          return Ok((ws, "新建工作区 Beacon".into()));
        }
        attempt += 1;
      }
    }
  }
}

fn version_publish_path(container_path: &str, version_id: &str) -> String {
  format!("{container_path}/versions/{version_id}")
}

async fn find_pending_version(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  container_path: &str,
  version_name: &str,
) -> Result<Option<GtmVersionHeader>, String> {
  let latest = list_version_headers(google, settings, secrets, container_path)
    .await?
    .into_iter()
    .filter(|h| !h.deleted)
    .max_by_key(|h| h.container_version_id.parse::<u64>().unwrap_or(0));
  Ok(latest.filter(|h| h.name.as_deref() == Some(version_name)))
}

fn tag_destination_id(parameter_blob: &str) -> Option<String> {
  // parameter_blob 是 JSON 数组字符串
  let v: Value = serde_json::from_str(parameter_blob).ok()?;
  let arr = v.as_array()?;
  for param in arr {
    if param.get("key").and_then(|k| k.as_str()) == Some("destinationId") {
      return param
        .get("value")
        .and_then(|x| x.as_str())
        .map(str::to_string);
    }
  }
  None
}

/// googtag 旁常会自动出现 dest_ga；二者并存时 create_version 会 compilerError，需在出版本前移除。
async fn remove_conflicting_dest_ga(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  measurement_id: &str,
  actions: &mut Vec<String>,
) -> Result<(), String> {
  let url = format!("{GTM_API}/{workspace_path}/tags");
  let items = google
    .api_list_all(settings, secrets, &url, "tag")
    .await?;
  for item in items {
    let Some(tag) = parse_tag(&item) else {
      continue;
    };
    if tag.tag_type != "dest_ga" {
      continue;
    }
    let dest = tag_destination_id(&tag.parameter_blob);
    let name_match = tag.name.contains(measurement_id);
    if dest.as_deref() != Some(measurement_id) && !name_match {
      continue;
    }
    let Some(path) = tag.path.as_deref() else {
      return Err(format!(
        "发现冲突的 Google Analytics 目标标签「{}」，但缺少 path，无法自动删除",
        tag.name
      ));
    };
    delete_tag(google, settings, secrets, path)
      .await
      .map_err(|e| format!("删除冲突的 dest_ga 标签「{}」失败：{}", tag.name, e.message))?;
    actions.push(format!("移除冲突的目标标签 {}", tag.name));
  }
  Ok(())
}

async fn ensure_google_tag(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  measurement_id: &str,
  actions: &mut Vec<String>,
) -> Result<(), String> {
  let tags = list_tags(google, settings, secrets, workspace_path).await?;
  if has_google_tag(&tags, measurement_id) {
    actions.push("复用 Google 代码".into());
    return Ok(());
  }
  let mut attempt = 0u32;
  loop {
    match create_google_tag(google, settings, secrets, workspace_path, measurement_id).await {
      Ok(_) => {
        actions.push("添加 Google 代码".into());
        return Ok(());
      }
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          let again = list_tags(google, settings, secrets, workspace_path).await?;
          if has_google_tag(&again, measurement_id) {
            actions.push("复用 Google 代码".into());
            return Ok(());
          }
          return Err(format!("添加 Google 代码失败：{}", err.message));
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let found = list_tags(google, settings, secrets, workspace_path)
          .await?
          .into_iter()
          .any(|tag| has_google_tag(std::slice::from_ref(&tag), measurement_id));
        if found {
          actions.push("复用 Google 代码".into());
          return Ok(());
        }
        attempt += 1;
      }
    }
  }
}

async fn ensure_named_trigger(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  name: &str,
  body: Value,
  actions: &mut Vec<String>,
) -> Result<String, String> {
  let existing = list_triggers(google, settings, secrets, workspace_path)
    .await?
    .into_iter()
    .find(|t| t.name == name);
  if let Some(t) = existing {
    actions.push(format!("复用触发器 {name}"));
    return Ok(t.trigger_id);
  }
  let mut attempt = 0u32;
  loop {
    match create_trigger(google, settings, secrets, workspace_path, body.clone()).await {
      Ok(t) => {
        actions.push(format!("创建触发器 {name}"));
        return Ok(t.trigger_id);
      }
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          if let Some(t) = list_triggers(google, settings, secrets, workspace_path)
            .await?
            .into_iter()
            .find(|t| t.name == name)
          {
            actions.push(format!("复用触发器 {name}"));
            return Ok(t.trigger_id);
          }
          return Err(format!("创建触发器 {name} 失败：{}", err.message));
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        if let Some(t) = list_triggers(google, settings, secrets, workspace_path)
          .await?
          .into_iter()
          .find(|t| t.name == name)
        {
          actions.push(format!("复用触发器 {name}"));
          return Ok(t.trigger_id);
        }
        attempt += 1;
      }
    }
  }
}

async fn ensure_named_event_tag(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  tag_name: &str,
  event_name: &str,
  body: Value,
  actions: &mut Vec<String>,
) -> Result<(), String> {
  let tags = list_tags(google, settings, secrets, workspace_path).await?;
  if let Some(existing) = tags.iter().find(|t| {
    t.tag_type == "gaawe" && (t.name == tag_name || t.event_name.as_deref() == Some(event_name))
  }) {
    if tag_uses_element_builtins(existing) {
      if let Some(path) = existing.path.as_deref() {
        delete_tag(google, settings, secrets, path)
          .await
          .map_err(|e| format!("删除有问题的事件标签 {tag_name} 失败：{}", e.message))?;
        actions.push(format!("删除无法编译的事件标签 {tag_name}"));
      } else {
        return Err(format!(
          "事件标签 {tag_name} 引用了 Click Element / Form Element，且缺少 path，无法自动重建；请在 GTM 后台删除后重试"
        ));
      }
    } else {
      actions.push(format!("复用事件标签 {tag_name}"));
      return Ok(());
    }
  }
  let mut attempt = 0u32;
  loop {
    match create_event_tag(google, settings, secrets, workspace_path, body.clone()).await {
      Ok(_) => {
        actions.push(format!("创建事件标签 {tag_name}"));
        return Ok(());
      }
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          let again = list_tags(google, settings, secrets, workspace_path).await?;
          if let Some(t) = again.iter().find(|t| {
            t.tag_type == "gaawe"
              && (t.name == tag_name || t.event_name.as_deref() == Some(event_name))
          }) {
            if !tag_uses_element_builtins(t) {
              actions.push(format!("复用事件标签 {tag_name}"));
              return Ok(());
            }
          }
          return Err(format!("创建事件标签 {tag_name} 失败：{}", err.message));
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        let again = list_tags(google, settings, secrets, workspace_path).await?;
        if let Some(t) = again.iter().find(|t| {
          t.tag_type == "gaawe"
            && (t.name == tag_name || t.event_name.as_deref() == Some(event_name))
        }) {
          if !tag_uses_element_builtins(t) {
            actions.push(format!("复用事件标签 {tag_name}"));
            return Ok(());
          }
        }
        attempt += 1;
      }
    }
  }
}

async fn ensure_tracking(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  workspace_path: &str,
  site: &Site,
  measurement_id: &str,
  actions: &mut Vec<String>,
) -> Result<(), String> {
  let need_any =
    site.outputs.track_click || site.outputs.track_outbound || site.outputs.track_form;
  if !need_any {
    return Ok(());
  }

  ensure_builtin_variables(google, settings, secrets, workspace_path, site, actions).await?;

  if site.outputs.track_click {
    let trigger_id = ensure_named_trigger(
      google,
      settings,
      secrets,
      workspace_path,
      TRIGGER_CLICK,
      json!({
        "name": TRIGGER_CLICK,
        "type": "click",
      }),
      actions,
    )
    .await?;
    ensure_named_event_tag(
      google,
      settings,
      secrets,
      workspace_path,
      TAG_CLICK,
      EVENT_CLICK,
      gaawe_tag_body(
        TAG_CLICK,
        EVENT_CLICK,
        measurement_id,
        &trigger_id,
        &[
          ("click_text", "{{Click Text}}"),
          ("click_url", "{{Click URL}}"),
          ("click_classes", "{{Click Classes}}"),
        ],
      ),
      actions,
    )
    .await?;
  }

  if site.outputs.track_outbound {
    let trigger_id = ensure_named_trigger(
      google,
      settings,
      secrets,
      workspace_path,
      TRIGGER_OUTBOUND,
      json!({
        "name": TRIGGER_OUTBOUND,
        "type": "linkClick",
        "filter": [{
          "type": "contains",
          "parameter": [
            { "type": "template", "key": "arg0", "value": "{{Click URL}}" },
            { "type": "template", "key": "arg1", "value": site.domain },
            { "type": "boolean", "key": "negate", "value": "true" },
          ]
        }],
      }),
      actions,
    )
    .await?;
    ensure_named_event_tag(
      google,
      settings,
      secrets,
      workspace_path,
      TAG_OUTBOUND,
      EVENT_OUTBOUND,
      gaawe_tag_body(
        TAG_OUTBOUND,
        EVENT_OUTBOUND,
        measurement_id,
        &trigger_id,
        &[
          ("link_url", "{{Click URL}}"),
          ("link_text", "{{Click Text}}"),
        ],
      ),
      actions,
    )
    .await?;
  }

  if site.outputs.track_form {
    let trigger_id = ensure_named_trigger(
      google,
      settings,
      secrets,
      workspace_path,
      TRIGGER_FORM,
      json!({
        "name": TRIGGER_FORM,
        "type": "formSubmission",
      }),
      actions,
    )
    .await?;
    ensure_named_event_tag(
      google,
      settings,
      secrets,
      workspace_path,
      TAG_FORM,
      EVENT_FORM,
      gaawe_tag_body(
        TAG_FORM,
        EVENT_FORM,
        measurement_id,
        &trigger_id,
        &[
          ("form_id", "{{Form ID}}"),
          ("form_classes", "{{Form Classes}}"),
          ("form_url", "{{Form URL}}"),
        ],
      ),
      actions,
    )
    .await?;
  }

  Ok(())
}

pub async fn run_gtm_publish_step(
  google: &GoogleClient,
  settings: &Settings,
  secrets: &Secrets,
  site: &Site,
) -> Result<StepResult, String> {
  let measurement_id = site
    .outputs
    .measurement_id
    .as_deref()
    .ok_or_else(|| "缺少衡量 ID，请先完成「创建数据流」".to_string())?;
  let version_name = format!("Beacon: GA4 {measurement_id}");

  let mut actions = Vec::new();
  let mut outputs = SiteOutputs::default();

  // 本地可能仍记着已删除的容器 path；发布前校验，缺失则重建并写回产出
  let (container, recovered) =
    ensure_container_for_site(google, settings, secrets, site).await?;
  let container_path = container.path.clone();
  if let Some(msg) = recovered {
    actions.push(msg.to_string());
    outputs.gtm_container_path = Some(container.path.clone());
    outputs.gtm_container_id = Some(container.container_id.clone());
    outputs.gtm_public_id = Some(container.public_id.clone());
  }

  let live = get_live_version(google, settings, secrets, &container_path)
    .await
    .map_err(|e| format!("读取线上版本失败：{e}"))?;
  if let Some(live) = &live {
    if has_google_tag(&live.tags, measurement_id)
      && live_has_selected_tracking(&live.tags, site)
    {
      outputs.gtm_version_id = Some(live.container_version_id.clone());
      return Ok(StepResult {
        status: "done",
        message: format!(
          "线上版本 {} 已包含 Google 代码与所选监听标签，无需重复发布",
          live.container_version_id
        ),
        outputs,
      });
    }
  }
  let has_live_version = live.is_some();

  // 若存在同名未发布版本，仍先确保工作区内容完整再决定是否沿用；
  // 缺监听标签时不能直接跳过工作区写入。
  let (workspace, ws_action) = resolve_workspace(
    google,
    settings,
    secrets,
    &container_path,
    has_live_version,
  )
  .await?;
  actions.push(ws_action);

  ensure_google_tag(
    google,
    settings,
    secrets,
    &workspace.path,
    measurement_id,
    &mut actions,
  )
  .await?;

  ensure_tracking(
    google,
    settings,
    secrets,
    &workspace.path,
    site,
    measurement_id,
    &mut actions,
  )
  .await?;

  // googtag 可能触发平台自动插入 dest_ga；与 googtag 并存会导致 create_version compilerError
  remove_conflicting_dest_ga(
    google,
    settings,
    secrets,
    &workspace.path,
    measurement_id,
    &mut actions,
  )
  .await?;

  let notes = format!(
    "Beacon 自动创建：为 {} 添加 GA4 {measurement_id} 与所选监听",
    site.domain
  );
  let mut attempt = 0u32;
  let version = loop {
    if let Some(pending) =
      find_pending_version(google, settings, secrets, &container_path, &version_name).await?
    {
      // 工作区已有改动时 create_version 可能已产生同名版本；直接发布该版本
      actions.push(format!("沿用未发布的版本 {}", pending.container_version_id));
      break GtmVersion {
        path: version_publish_path(&container_path, &pending.container_version_id),
        container_version_id: pending.container_version_id,
        name: pending.name,
        tags: vec![],
      };
    }
    match create_version(
      google,
      settings,
      secrets,
      &workspace.path,
      &version_name,
      &notes,
    )
    .await
    {
      Ok(mut v) => {
        v.path = version_publish_path(&container_path, &v.container_version_id);
        actions.push(format!("创建版本 {}", v.container_version_id));
        break v;
      }
      Err(err) => {
        if !err.is_server_error() || attempt >= 5 {
          return Err(format!("创建版本失败：{}", err.message));
        }
        let delay_ms =
          (1000u64 * 2u64.pow(attempt)).min(32_000) + (rand::random::<u64>() % 1000);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;
        attempt += 1;
      }
    }
  };

  let published = publish_version(google, settings, secrets, &version.path)
    .await
    .map_err(|e| format!("发布版本 {} 失败：{}", version.container_version_id, e.message))?;
  if published.get("compilerError").and_then(|x| x.as_bool()) == Some(true) {
    return Err(format!(
      "发布版本 {} 时 GTM 编译出错",
      version.container_version_id
    ));
  }
  actions.push("发布成功".into());
  outputs.gtm_version_id = Some(version.container_version_id);

  Ok(StepResult {
    status: "done",
    message: actions.join(" → "),
    outputs,
  })
}
