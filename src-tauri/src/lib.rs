mod commands;
mod config;
mod google;
mod sites;

use commands::{
  check_auth, check_site_deploy, clear_credential, create_site, delete_site_cmd, get_config,
  health_check_site_cmd,
  get_site, init_state, list_sites, open_data_dir, preview_site_cmd, resume_site_cmd, runtime_info,
  save_config, start_google_auth,
};
use std::fs;
use std::path::PathBuf;
use tauri::Manager;

fn error_log_path() -> PathBuf {
  dirs_fallback()
    .join("app.beacon.desktop")
    .join("last-error.txt")
}

fn dirs_fallback() -> PathBuf {
  std::env::var_os("APPDATA")
    .map(PathBuf::from)
    .unwrap_or_else(|| PathBuf::from("."))
}

fn write_error_log(message: &str) {
  let path = error_log_path();
  if let Some(parent) = path.parent() {
    let _ = fs::create_dir_all(parent);
  }
  let body = format!(
    "{}\n时间：{}\n",
    message,
    chrono::Local::now().to_rfc3339()
  );
  let _ = fs::write(&path, body);
}

#[cfg(windows)]
fn show_error_dialog(title: &str, message: &str) {
  use std::os::windows::ffi::OsStrExt;
  #[link(name = "user32")]
  extern "system" {
    fn MessageBoxW(
      h_wnd: *mut core::ffi::c_void,
      lp_text: *const u16,
      lp_caption: *const u16,
      u_type: u32,
    ) -> i32;
  }
  fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s)
      .encode_wide()
      .chain(std::iter::once(0))
      .collect()
  }
  let text = wide(message);
  let caption = wide(title);
  unsafe {
    MessageBoxW(
      std::ptr::null_mut(),
      text.as_ptr(),
      caption.as_ptr(),
      0x00000010, // MB_ICONERROR
    );
  }
}

#[cfg(not(windows))]
fn show_error_dialog(_title: &str, _message: &str) {}

fn report_fatal(context: &str, err: &str) {
  let message = format!("{context}\n\n{err}\n\n详细日志：{}", error_log_path().display());
  write_error_log(&message);
  show_error_dialog("Beacon 启动失败", &message);
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
  std::panic::set_hook(Box::new(|info| {
    let message = format!("程序异常退出：{info}");
    write_error_log(&message);
    // 避免在不可 unwind 的上下文里再弹窗导致二次 abort
  }));

  let result = tauri::Builder::default()
    .plugin(tauri_plugin_opener::init())
    .setup(|app| {
      if cfg!(debug_assertions) {
        app.handle().plugin(
          tauri_plugin_log::Builder::default()
            .level(log::LevelFilter::Info)
            .build(),
        )?;
      }
      match init_state(app.handle()) {
        Ok(state) => {
          app.manage(state);
          Ok(())
        }
        Err(err) => {
          report_fatal("初始化失败（配置或数据库）。", &err);
          Err(err.into())
        }
      }
    })
    .invoke_handler(tauri::generate_handler![
      get_config,
      save_config,
      clear_credential,
      start_google_auth,
      check_auth,
      open_data_dir,
      runtime_info,
      list_sites,
      get_site,
      create_site,
      preview_site_cmd,
      check_site_deploy,
      resume_site_cmd,
      delete_site_cmd,
      health_check_site_cmd
    ])
    .run(tauri::generate_context!());

  if let Err(err) = result {
    report_fatal("启动窗口失败。", &err.to_string());
  }
}
