use crate::config::{CredentialStatus, Settings};
use crate::sites::domain::ParsedSite;
use serde::Serialize;
use serde_json::{Map, Value};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanOperation {
  pub api: String,
  pub params: Map<String, Value>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanStep {
  pub step: String,
  pub label: String,
  pub operations: Vec<PlanOperation>,
  pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SitePlan {
  pub domain: String,
  pub name: String,
  pub sitemap_url: String,
  pub warnings: Vec<String>,
  pub steps: Vec<PlanStep>,
}

fn op(api: &str, params: Map<String, Value>) -> PlanOperation {
  PlanOperation {
    api: api.into(),
    params,
  }
}

fn params_map(pairs: &[(&str, String)]) -> Map<String, Value> {
  let mut map = Map::new();
  for (k, v) in pairs {
    map.insert((*k).into(), Value::String(v.clone()));
  }
  map
}

pub fn build_site_plan(
  site: &ParsedSite,
  settings: &Settings,
  credentials: &CredentialStatus,
) -> SitePlan {
  let mut warnings = Vec::new();
  if !credentials.client_id || !credentials.client_secret || !credentials.refresh_token {
    warnings.push("Google OAuth 凭证未配置完整，实际执行时会因授权检查不通过而停止".into());
  }
  if settings.gtm_account_id.trim().is_empty() {
    warnings.push("未配置 GTM 账号 ID".into());
  }
  if settings.ga_account_id.trim().is_empty() {
    warnings.push("未配置 GA 账号 ID".into());
  }

  let gtm_account = if settings.gtm_account_id.trim().is_empty() {
    "<未配置>".to_string()
  } else {
    settings.gtm_account_id.clone()
  };
  let site_url = format!("https://{}/", site.domain);

  let mut publish_ops = vec![
    op(
      "Tag Manager workspaces.create",
      params_map(&[("name", "Beacon".into())]),
    ),
    op(
      "Tag Manager workspaces.tags.create",
      params_map(&[
        ("type", "googtag".into()),
        ("tagId", "<衡量 ID>".into()),
        ("firingTriggerId", "2147479553".into()),
      ]),
    ),
  ];
  let mut publish_notes = vec![
    "线上已有 Google 代码且所选监听标签齐全时跳过".into(),
    "GTM 写操作受配置的间隔限制".into(),
  ];
  if site.track_click {
    publish_ops.push(op(
      "Tag Manager triggers.create + tags.create",
      params_map(&[
        ("trigger", "click / Beacon - All Clicks".into()),
        ("tag", "gaawe event=click".into()),
      ]),
    ));
  }
  if site.track_outbound {
    publish_ops.push(op(
      "Tag Manager triggers.create + tags.create",
      params_map(&[
        ("trigger", "linkClick / Beacon - Outbound（Click URL 不含本站域名）".into()),
        ("tag", "gaawe event=apk_download".into()),
      ]),
    ));
  }
  if site.track_form {
    publish_ops.push(op(
      "Tag Manager triggers.create + tags.create",
      params_map(&[
        ("trigger", "formSubmission / Beacon - Form Submit".into()),
        ("tag", "gaawe event=form_submit".into()),
      ]),
    ));
  }
  if site.track_click || site.track_outbound || site.track_form {
    publish_ops.push(op(
      "Tag Manager built_in_variables.create",
      params_map(&[("types", "click* / form* / pageHostname".into())]),
    ));
    publish_notes.push("按添加站点时勾选的监听预设幂等创建触发器与事件标签".into());
  } else {
    publish_notes.push("未勾选点击/出站/表单，仅发布 Google 代码".into());
  }
  publish_ops.push(op(
    "Tag Manager workspaces.create_version + versions.publish",
    params_map(&[("name", "Beacon: GA4 <衡量 ID>".into())]),
  ));

  SitePlan {
    domain: site.domain.clone(),
    name: site.name.clone(),
    sitemap_url: site.sitemap_url.clone(),
    warnings,
    steps: vec![
      PlanStep {
        step: "gtm_container".into(),
        label: "创建 GTM 容器".into(),
        operations: vec![op(
          "Tag Manager accounts.containers.create",
          params_map(&[
            ("parent", format!("accounts/{gtm_account}")),
            ("name", site.name.clone()),
            ("usageContext", "web".into()),
            ("domainName", site.domain.clone()),
          ]),
        )],
        notes: vec!["账号下已有同名或绑定同一域名的 Web 容器时直接复用".into()],
      },
      PlanStep {
        step: "ga_property".into(),
        label: "创建 GA4 属性".into(),
        operations: vec![op(
          "Analytics Admin properties.create",
          params_map(&[
            (
              "parent",
              format!(
                "accounts/{}",
                if settings.ga_account_id.trim().is_empty() {
                  "<未配置>"
                } else {
                  settings.ga_account_id.as_str()
                }
              ),
            ),
            ("displayName", site.name.clone()),
            ("timeZone", settings.time_zone.clone()),
            ("currencyCode", settings.currency_code.clone()),
          ]),
        )],
        notes: vec!["账号下已有同名属性时直接复用".into()],
      },
      PlanStep {
        step: "data_stream".into(),
        label: "创建数据流".into(),
        operations: vec![op(
          "Analytics Admin properties.dataStreams.create",
          params_map(&[
            ("parent", "properties/<创建属性后得到的属性 ID>".into()),
            ("type", "WEB_DATA_STREAM".into()),
            ("displayName", site.name.clone()),
            ("defaultUri", format!("https://{}", site.domain)),
          ]),
        )],
        notes: vec!["同域名 Web 数据流已存在时直接复用".into()],
      },
      PlanStep {
        step: "gtm_publish".into(),
        label: "发布 GTM".into(),
        operations: publish_ops,
        notes: publish_notes,
      },
      PlanStep {
        step: "deploy_check".into(),
        label: "等待部署".into(),
        operations: vec![op(
          "抓取站点首页",
          params_map(&[
            ("url", site_url.clone()),
            ("检查内容", "<GTM ID>".into()),
          ]),
        )],
        notes: vec![
          "GTM 发布完成后暂停；未检测到时请部署代码后点击「检查部署」".into(),
        ],
      },
      PlanStep {
        step: "gsc_verify".into(),
        label: "验证 GSC".into(),
        operations: vec![
          op(
            "Site Verification webResource.getToken",
            params_map(&[
              ("verificationMethod", "TAG_MANAGER".into()),
              ("site.type", "SITE".into()),
              ("site.identifier", site_url.clone()),
            ]),
          ),
          op(
            "Site Verification webResource.insert",
            params_map(&[
              ("verificationMethod", "TAG_MANAGER".into()),
              ("site.identifier", site_url.clone()),
            ]),
          ),
        ],
        notes: vec![format!(
          "验证失败时每 {} 秒重试，最多 {} 次（可在配置中修改）",
          settings.gsc_verify_retry_interval_sec, settings.gsc_verify_max_attempts
        )],
      },
      PlanStep {
        step: "gsc_add".into(),
        label: "加入 GSC".into(),
        operations: vec![
          op(
            "Search Console sites.add",
            params_map(&[("siteUrl", site_url)]),
          ),
          op(
            "Search Console sitemaps.submit",
            params_map(&[("feedpath", site.sitemap_url.clone())]),
          ),
        ],
        notes: vec!["资源或 sitemap 已存在时直接复用".into()],
      },
    ],
  }
}
