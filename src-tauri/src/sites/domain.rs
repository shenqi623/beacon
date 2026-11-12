use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

fn default_true() -> bool {
  true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewSiteInput {
  pub domain: String,
  pub name: String,
  #[serde(default)]
  pub sitemap_url: Option<String>,
  #[serde(default = "default_true")]
  pub track_click: bool,
  #[serde(default = "default_true")]
  pub track_outbound: bool,
  #[serde(default = "default_true")]
  pub track_form: bool,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct NewSiteErrors {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub domain: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub name: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub sitemap_url: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ParsedSite {
  pub domain: String,
  pub name: String,
  pub sitemap_url: String,
  pub track_click: bool,
  pub track_outbound: bool,
  pub track_form: bool,
}

#[derive(Debug)]
pub enum ParseSiteError {
  Invalid { status: u16, errors: NewSiteErrors },
}

impl ParseSiteError {
  pub fn to_json(&self) -> String {
    match self {
      Self::Invalid { status, errors } => serde_json::json!({
        "status": status,
        "message": "输入校验失败",
        "errors": errors,
      })
      .to_string(),
    }
  }
}

fn hostname_re() -> &'static Regex {
  static RE: OnceLock<Regex> = OnceLock::new();
  // Rust `regex` 不支持 look-around，长度在 normalize_domain 里单独校验。
  RE.get_or_init(|| {
    Regex::new(
      r"^(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+(?:[a-z]{2,63}|xn--[a-z0-9-]{1,59})$",
    )
    .expect("hostname regex")
  })
}

pub fn normalize_domain(input: &str) -> Option<String> {
  let trimmed = input.trim().trim_end_matches('/');
  if trimmed.is_empty() {
    return None;
  }
  let with_scheme = if trimmed.to_ascii_lowercase().starts_with("http://")
    || trimmed.to_ascii_lowercase().starts_with("https://")
  {
    trimmed.to_string()
  } else {
    format!("https://{trimmed}")
  };
  let url = url::Url::parse(&with_scheme).ok()?;
  if url.path() != "/" || url.query().is_some() || url.fragment().is_some() {
    return None;
  }
  if url.port().is_some() || !url.username().is_empty() {
    return None;
  }
  let host = url.host_str()?.to_ascii_lowercase();
  if !(1..=253).contains(&host.len()) {
    return None;
  }
  if hostname_re().is_match(&host) {
    Some(host)
  } else {
    None
  }
}

pub fn parse_new_site(
  input: &NewSiteInput,
  domain_exists: impl FnOnce(&str) -> Result<bool, String>,
) -> Result<ParsedSite, ParseSiteError> {
  let mut errors = NewSiteErrors::default();
  let domain_input = input.domain.trim();
  let domain = normalize_domain(domain_input);
  if domain_input.is_empty() {
    errors.domain = Some("请填写域名".into());
  } else if domain.is_none() {
    errors.domain = Some("域名格式不正确，例如 example.com".into());
  }

  let name = input.name.trim();
  if name.is_empty() {
    errors.name = Some("请填写站点名称".into());
  } else if name.len() > 100 {
    errors.name = Some("站点名称不能超过 100 个字符".into());
  }

  let sitemap_input = input
    .sitemap_url
    .as_deref()
    .unwrap_or("")
    .trim()
    .to_string();
  let mut sitemap_url = domain
    .as_ref()
    .map(|d| format!("https://{d}/sitemap.xml"))
    .unwrap_or_default();
  if !sitemap_input.is_empty() {
    sitemap_url = sitemap_input.clone();
    if let Some(d) = &domain {
      let prefix = format!("https://{d}/");
      if !sitemap_input.starts_with(&prefix) {
        errors.sitemap_url = Some(format!("sitemap 地址必须以 {prefix} 开头"));
      } else if url::Url::parse(&sitemap_input).is_err() {
        errors.sitemap_url = Some("sitemap 地址格式不正确".into());
      }
    } else if url::Url::parse(&sitemap_input).is_err() {
      errors.sitemap_url = Some("sitemap 地址格式不正确".into());
    }
  }

  let has_errors = errors.domain.is_some() || errors.name.is_some() || errors.sitemap_url.is_some();
  if has_errors {
    return Err(ParseSiteError::Invalid {
      status: 400,
      errors,
    });
  }

  let domain = domain.expect("validated");
  match domain_exists(&domain) {
    Ok(true) => Err(ParseSiteError::Invalid {
      status: 409,
      errors: NewSiteErrors {
        domain: Some("该域名已有任务，不能重复创建".into()),
        ..Default::default()
      },
    }),
    Ok(false) => Ok(ParsedSite {
      domain,
      name: name.to_string(),
      sitemap_url,
      track_click: input.track_click,
      track_outbound: input.track_outbound,
      track_form: input.track_form,
    }),
    Err(e) => Err(ParseSiteError::Invalid {
      status: 500,
      errors: NewSiteErrors {
        domain: Some(e),
        ..Default::default()
      },
    }),
  }
}
