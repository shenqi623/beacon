use crate::commands::AppState;
use crate::sites::deploy::run_deploy_check_step;
use crate::sites::domain::{parse_new_site, NewSiteInput, ParseSiteError};
use crate::sites::ga::{run_data_stream_step, run_ga_property_step};
use crate::sites::gsc::{run_gsc_add_step, run_gsc_verify_step};
use crate::sites::gtm::{run_gtm_container_step, StepResult};
use crate::sites::gtm_publish::run_gtm_publish_step;
use crate::sites::plan::{build_site_plan, SitePlan};
use crate::sites::store::{SiteDetail, SiteStore, STEP_KEYS};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::sync::Arc;

pub struct SiteRunner {
  active: Mutex<HashSet<i64>>,
}

impl SiteRunner {
  pub fn new() -> Self {
    Self {
      active: Mutex::new(HashSet::new()),
    }
  }

  pub fn is_active(&self, id: i64) -> bool {
    self.active.lock().contains(&id)
  }

  fn try_begin(&self, id: i64) -> bool {
    let mut guard = self.active.lock();
    if guard.contains(&id) {
      return false;
    }
    guard.insert(id);
    true
  }

  fn end(&self, id: i64) {
    self.active.lock().remove(&id);
  }
}

fn step_label(step: &str) -> String {
  match step {
    "gtm_container" => "创建 GTM 容器".into(),
    "deploy_check" => "等待部署".into(),
    "ga_property" => "创建 GA4 属性".into(),
    "data_stream" => "创建数据流".into(),
    "gtm_publish" => "发布 GTM".into(),
    "gsc_verify" => "验证 GSC".into(),
    "gsc_add" => "加入 GSC".into(),
    other => other.to_string(),
  }
}

fn fail_site(store: &SiteStore, site_id: i64, step: Option<&str>, message: &str) {
  if let Some(step) = step {
    let _ = store.set_step(site_id, step, "failed", message);
  }
  let _ = store.update(site_id, Some("failed"), None, Some(Some(message)), None);
}

async fn run_steps(state: Arc<AppState>, site_id: i64) {
  let (settings, secrets) = {
    let cfg = state.config.lock();
    (cfg.settings().clone(), cfg.secrets().clone())
  };

  let auth = state.google.check_auth(&settings, &secrets).await;
  if !auth.ok {
    let reasons: Vec<String> = auth
      .items
      .iter()
      .filter(|i| !i.ok)
      .map(|i| format!("{}：{}", i.label, i.message))
      .collect();
    let message = format!(
      "授权检查未通过，请处理后重新执行。{}",
      reasons.join("；")
    );
    fail_site(&state.sites.lock(), site_id, None, &message);
    return;
  }

  let start_step = {
    let store = state.sites.lock();
    match store.get(site_id) {
      Ok(Some(site)) => site.current_step.clone(),
      _ => return,
    }
  };

  let start_idx = STEP_KEYS
    .iter()
    .position(|s| *s == start_step.as_str())
    .unwrap_or(0);

  for step in &STEP_KEYS[start_idx..] {
    {
      let store = state.sites.lock();
      let _ = store.update(site_id, Some("running"), Some(step), Some(None), None);
      let _ = store.set_step(site_id, step, "running", "");
    }

    let site = {
      let store = state.sites.lock();
      match store.get(site_id) {
        Ok(Some(site)) => site,
        _ => return,
      }
    };

    let result: Result<StepResult, String> = match *step {
      "gtm_container" => {
        run_gtm_container_step(&state.google, &settings, &secrets, &site).await
      }
      "deploy_check" => run_deploy_check_step(&settings, &site).await,
      "ga_property" => {
        run_ga_property_step(&state.google, &settings, &secrets, &site).await
      }
      "data_stream" => {
        run_data_stream_step(&state.google, &settings, &secrets, &site).await
      }
      "gtm_publish" => {
        run_gtm_publish_step(&state.google, &settings, &secrets, &site).await
      }
      "gsc_verify" => {
        let state_for_progress = state.clone();
        let step_owned = (*step).to_string();
        run_gsc_verify_step(
          &state.google,
          &settings,
          &secrets,
          &site,
          move |message| {
            let _ = state_for_progress.sites.lock().set_step(
              site_id,
              &step_owned,
              "running",
              message,
            );
          },
        )
        .await
      }
      "gsc_add" => run_gsc_add_step(&state.google, &settings, &secrets, &site).await,
      other => Err(format!(
        "「{}」尚未实现，将在后续开发阶段完成",
        step_label(other)
      )),
    };

    match result {
      Ok(result) if result.status == "waiting" => {
        let store = state.sites.lock();
        let _ = store.set_step(site_id, step, "waiting", &result.message);
        let _ = store.update(
          site_id,
          Some("waiting_deploy"),
          Some(step),
          Some(None),
          None,
        );
        return;
      }
      Ok(result) => {
        let store = state.sites.lock();
        let _ = store.set_step(site_id, step, "done", &result.message);
        if let Ok(Some(current)) = store.get(site_id) {
          let merged = SiteStore::merge_outputs(&current.outputs, &result.outputs);
          let _ = store.update(site_id, None, None, None, Some(&merged));
        }
      }
      Err(err) => {
        fail_site(&state.sites.lock(), site_id, Some(step), &err);
        return;
      }
    }
  }

  let _ = state
    .sites
    .lock()
    .update(site_id, Some("done"), None, Some(None), None);
}

pub fn start_site(state: Arc<AppState>, site_id: i64) -> bool {
  if !state.runner.try_begin(site_id) {
    return false;
  }
  {
    let store = state.sites.lock();
    let _ = store.update(site_id, Some("running"), None, Some(None), None);
  }
  tauri::async_runtime::spawn(async move {
    run_steps(state.clone(), site_id).await;
    state.runner.end(site_id);
  });
  true
}

pub fn create_and_start(
  state: Arc<AppState>,
  input: NewSiteInput,
) -> Result<SiteDetail, String> {
  let parsed = {
    let store = state.sites.lock();
    parse_new_site(&input, |d| store.domain_exists(d)).map_err(|e| match e {
      ParseSiteError::Invalid { .. } => e.to_json(),
    })?
  };
  let site = {
    let store = state.sites.lock();
    store.insert(
      &parsed.domain,
      &parsed.name,
      &parsed.sitemap_url,
      parsed.track_click,
      parsed.track_outbound,
      parsed.track_form,
    )?
  };
  start_site(state.clone(), site.id);
  state
    .sites
    .lock()
    .get_detail(site.id)?
    .ok_or_else(|| "创建后读取站点失败".into())
}

pub fn check_deploy(state: Arc<AppState>, site_id: i64) -> Result<SiteDetail, String> {
  {
    let store = state.sites.lock();
    let site = store
      .get(site_id)?
      .ok_or_else(|| "站点不存在".to_string())?;
    if site.status != "waiting_deploy" || state.runner.is_active(site_id) {
      return Err("站点当前不在等待部署状态".into());
    }
  }
  start_site(state.clone(), site_id);
  state
    .sites
    .lock()
    .get_detail(site_id)?
    .ok_or_else(|| "站点不存在".into())
}

pub fn delete_site(state: Arc<AppState>, site_id: i64) -> Result<(), String> {
  if state.runner.is_active(site_id) {
    return Err("站点正在执行中，请稍后再删除".into());
  }
  state.sites.lock().delete(site_id)
}

pub fn resume_site(state: Arc<AppState>, site_id: i64) -> Result<SiteDetail, String> {
  {
    let store = state.sites.lock();
    let site = store
      .get(site_id)?
      .ok_or_else(|| "站点不存在".to_string())?;
    if site.status != "failed" || state.runner.is_active(site_id) {
      return Err("只有失败的任务可以从失败处继续".into());
    }
  }
  start_site(state.clone(), site_id);
  state
    .sites
    .lock()
    .get_detail(site_id)?
    .ok_or_else(|| "站点不存在".into())
}

pub fn preview_site(state: &AppState, input: NewSiteInput) -> Result<SitePlan, String> {
  let parsed = parse_new_site(&input, |_| Ok(false)).map_err(|e| match e {
    ParseSiteError::Invalid { .. } => e.to_json(),
  })?;
  let cfg = state.config.lock();
  Ok(build_site_plan(
    &parsed,
    cfg.settings(),
    &cfg.credential_status(),
  ))
}
