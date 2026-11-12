use crate::config::Settings;
use crate::google::GoogleClient;
use crate::sites::gtm::StepResult;
use crate::sites::store::{Site, SiteOutputs};
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct PageGtmCheck {
  pub ok: bool,
  pub message: String,
}

/// 抓取站点首页，检查 HTML 是否包含指定 GTM publicId。
pub async fn check_homepage_gtm(
  settings: &Settings,
  domain: &str,
  public_id: &str,
) -> PageGtmCheck {
  let url = format!("https://{domain}/");
  let client = match GoogleClient::build_http(settings) {
    Ok(c) => c,
    Err(err) => {
      return PageGtmCheck {
        ok: false,
        message: format!("无法创建 HTTP 客户端：{err}"),
      };
    }
  };
  let res = match tokio::time::timeout(
    Duration::from_secs(15),
    client
      .get(&url)
      .header("accept", "text/html")
      .header(
        "user-agent",
        "Mozilla/5.0 (compatible; BeaconDeployCheck/1.0)",
      )
      .send(),
  )
  .await
  {
    Ok(Ok(res)) => res,
    Ok(Err(err)) => {
      return PageGtmCheck {
        ok: false,
        message: format!("无法访问 {url}：{err}"),
      };
    }
    Err(_) => {
      return PageGtmCheck {
        ok: false,
        message: format!("无法访问 {url}：请求超时"),
      };
    }
  };

  if !res.status().is_success() {
    return PageGtmCheck {
      ok: false,
      message: format!("首页返回 HTTP {}，请确认网站可以访问", res.status()),
    };
  }

  let html = match res.text().await {
    Ok(t) => t,
    Err(e) => {
      return PageGtmCheck {
        ok: false,
        message: format!("读取首页失败：{e}"),
      };
    }
  };
  if !html.contains(public_id) {
    return PageGtmCheck {
      ok: false,
      message: format!("首页 HTML 中未找到 {public_id}，请部署 GTM 代码后再检查"),
    };
  }
  PageGtmCheck {
    ok: true,
    message: format!("已在首页检测到 {public_id}"),
  }
}

pub async fn run_deploy_check_step(
  settings: &Settings,
  site: &Site,
) -> Result<StepResult, String> {
  let public_id = site
    .outputs
    .gtm_public_id
    .as_deref()
    .ok_or_else(|| "缺少 GTM ID，请先完成「创建 GTM 容器」".to_string())?;

  let check = check_homepage_gtm(settings, &site.domain, public_id).await;
  if check.ok {
    Ok(StepResult {
      status: "done",
      message: check.message,
      outputs: SiteOutputs::default(),
    })
  } else {
    Ok(StepResult {
      status: "waiting",
      message: check.message,
      outputs: SiteOutputs::default(),
    })
  }
}
