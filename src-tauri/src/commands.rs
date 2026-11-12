use crate::config::{AppConfigResponse, ConfigStore, SaveConfigRequest};
use crate::google::{run_oauth_login, AuthCheckResult, GoogleClient, OAuthStartResult};
use crate::sites::{
  check_deploy, create_and_start, delete_site, preview_site, resume_site, run_health_check,
  NewSiteInput, Site, SiteDetail, SitePlan, SiteRunner, SiteStore,
};
use parking_lot::Mutex;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_opener::OpenerExt;

pub struct AppState {
  pub config: Mutex<ConfigStore>,
  pub google: GoogleClient,
  pub sites: Mutex<SiteStore>,
  pub runner: SiteRunner,
}

#[tauri::command]
pub fn get_config(state: State<'_, Arc<AppState>>) -> AppConfigResponse {
  state.config.lock().response()
}

#[tauri::command]
pub fn save_config(
  state: State<'_, Arc<AppState>>,
  request: SaveConfigRequest,
) -> Result<AppConfigResponse, String> {
  state.config.lock().save_request(request)
}

#[tauri::command]
pub fn clear_credential(
  state: State<'_, Arc<AppState>>,
  field: String,
) -> Result<AppConfigResponse, String> {
  state.config.lock().clear_secret(&field)
}

#[tauri::command]
pub async fn start_google_auth(
  state: State<'_, Arc<AppState>>,
) -> Result<OAuthStartResult, String> {
  let (settings, secrets) = {
    let cfg = state.config.lock();
    (cfg.settings().clone(), cfg.secrets().clone())
  };
  let (result, refresh_token) = run_oauth_login(&state.google, &settings, &secrets).await?;
  state.config.lock().set_refresh_token(refresh_token)?;
  Ok(result)
}

#[tauri::command]
pub async fn check_auth(state: State<'_, Arc<AppState>>) -> Result<AuthCheckResult, String> {
  let (settings, secrets) = {
    let cfg = state.config.lock();
    (cfg.settings().clone(), cfg.secrets().clone())
  };
  Ok(state.google.check_auth(&settings, &secrets).await)
}

#[tauri::command]
pub fn open_data_dir(app: AppHandle, state: State<'_, Arc<AppState>>) -> Result<(), String> {
  let path = state.config.lock().data_dir().to_path_buf();
  app
    .opener()
    .open_path(path.to_string_lossy(), None::<&str>)
    .map_err(|e| format!("无法打开数据目录：{e}"))
}

#[tauri::command]
pub fn runtime_info() -> RuntimeInfo {
  RuntimeInfo {
    mode: "tauri".into(),
    sites_ready: true,
  }
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeInfo {
  pub mode: String,
  pub sites_ready: bool,
}

#[tauri::command]
pub fn list_sites(state: State<'_, Arc<AppState>>) -> Result<Vec<Site>, String> {
  state.sites.lock().list()
}

#[tauri::command]
pub fn get_site(state: State<'_, Arc<AppState>>, id: i64) -> Result<SiteDetail, String> {
  state
    .sites
    .lock()
    .get_detail(id)?
    .ok_or_else(|| "站点不存在".into())
}

#[tauri::command]
pub fn create_site(
  state: State<'_, Arc<AppState>>,
  input: NewSiteInput,
) -> Result<SiteDetail, String> {
  create_and_start(state.inner().clone(), input)
}

#[tauri::command]
pub fn preview_site_cmd(
  state: State<'_, Arc<AppState>>,
  input: NewSiteInput,
) -> Result<SitePlan, String> {
  preview_site(&state, input)
}

#[tauri::command]
pub fn check_site_deploy(
  state: State<'_, Arc<AppState>>,
  id: i64,
) -> Result<SiteDetail, String> {
  check_deploy(state.inner().clone(), id)
}

#[tauri::command]
pub fn resume_site_cmd(
  state: State<'_, Arc<AppState>>,
  id: i64,
) -> Result<SiteDetail, String> {
  resume_site(state.inner().clone(), id)
}

#[tauri::command]
pub fn delete_site_cmd(state: State<'_, Arc<AppState>>, id: i64) -> Result<(), String> {
  delete_site(state.inner().clone(), id)
}

#[tauri::command]
pub async fn health_check_site_cmd(
  state: State<'_, Arc<AppState>>,
  id: i64,
) -> Result<SiteDetail, String> {
  run_health_check(state.inner().clone(), id).await
}

pub fn init_state(app: &AppHandle) -> Result<Arc<AppState>, String> {
  let data_dir = app
    .path()
    .app_data_dir()
    .map_err(|e| format!("无法解析应用数据目录：{e}"))?;
  let config = ConfigStore::load(data_dir.clone())?;
  let sites = SiteStore::open(&data_dir)?;
  sites.mark_interrupted()?;
  Ok(Arc::new(AppState {
    config: Mutex::new(config),
    google: GoogleClient::new(),
    sites: Mutex::new(sites),
    runner: SiteRunner::new(),
  }))
}
