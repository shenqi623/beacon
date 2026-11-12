use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const STEP_KEYS: &[&str] = &[
  "gtm_container",
  "ga_property",
  "data_stream",
  "gtm_publish",
  "deploy_check",
  "gsc_verify",
  "gsc_add",
];

pub type StepKey = String;
pub type SiteStatus = String;
pub type StepStatus = String;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HealthItem {
  pub key: String,
  pub label: String,
  /// pass | fail | skip
  pub status: String,
  pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteOutputs {
  #[serde(skip_serializing_if = "Option::is_none")]
  pub gtm_container_path: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub gtm_container_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub gtm_public_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ga_property_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub ga_data_stream_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub measurement_id: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub gtm_version_id: Option<String>,
  /// 添加站点时勾选；旧任务缺字段时反序列化为 false。
  #[serde(default)]
  pub track_click: bool,
  #[serde(default)]
  pub track_outbound: bool,
  #[serde(default)]
  pub track_form: bool,
  /// healthy | unhealthy | unknown
  #[serde(skip_serializing_if = "Option::is_none")]
  pub health_status: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub health_checked_at: Option<String>,
  #[serde(skip_serializing_if = "Option::is_none")]
  pub health_items: Option<Vec<HealthItem>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Site {
  pub id: i64,
  pub domain: String,
  pub name: String,
  pub sitemap_url: String,
  pub status: SiteStatus,
  pub current_step: StepKey,
  pub error: Option<String>,
  pub outputs: SiteOutputs,
  pub created_at: String,
  pub updated_at: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteStep {
  pub step: StepKey,
  pub status: StepStatus,
  pub message: String,
  pub updated_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SiteDetail {
  #[serde(flatten)]
  pub site: Site,
  pub steps: Vec<SiteStep>,
}

pub struct SiteStore {
  conn: Connection,
  #[allow(dead_code)]
  path: PathBuf,
}

impl SiteStore {
  pub fn open(data_dir: &Path) -> Result<Self, String> {
    std::fs::create_dir_all(data_dir).map_err(|e| format!("无法创建数据目录：{e}"))?;
    let path = data_dir.join("beacon.db");
    let conn = Connection::open(&path).map_err(|e| format!("打开数据库失败：{e}"))?;
    conn.busy_timeout(std::time::Duration::from_secs(5))
      .map_err(|e| format!("设置数据库超时失败：{e}"))?;
    conn.execute_batch(
      "
      PRAGMA foreign_keys = ON;
      PRAGMA journal_mode = WAL;
      CREATE TABLE IF NOT EXISTS sites (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        domain TEXT NOT NULL UNIQUE,
        name TEXT NOT NULL,
        sitemap_url TEXT NOT NULL,
        status TEXT NOT NULL,
        current_step TEXT NOT NULL,
        error TEXT,
        outputs TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL,
        updated_at TEXT NOT NULL
      );
      CREATE TABLE IF NOT EXISTS site_steps (
        site_id INTEGER NOT NULL REFERENCES sites(id) ON DELETE CASCADE,
        step TEXT NOT NULL,
        status TEXT NOT NULL,
        message TEXT NOT NULL,
        updated_at TEXT NOT NULL,
        PRIMARY KEY (site_id, step)
      );
      ",
    )
    .map_err(|e| format!("初始化数据库失败：{e}"))?;
    let store = Self { conn, path };
    store.migrate_pipeline_order()?;
    Ok(store)
  }

  /// 旧流程在「创建容器」后立刻等待部署；新流程把部署检查移到「发布 GTM」之后。
  /// 对仍停在旧位置、且尚未完成 GA4 的任务，改从 ga_property 继续。
  fn migrate_pipeline_order(&self) -> Result<(), String> {
    let mut stmt = self
      .conn
      .prepare(
        "SELECT id, outputs FROM sites
         WHERE status = 'waiting_deploy' AND current_step = 'deploy_check'",
      )
      .map_err(|e| e.to_string())?;
    let rows = stmt
      .query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
      })
      .map_err(|e| e.to_string())?;

    let mut to_migrate = Vec::new();
    for row in rows {
      let (id, outputs_json) = row.map_err(|e| e.to_string())?;
      let outputs: SiteOutputs = serde_json::from_str(&outputs_json).unwrap_or_default();
      if outputs.ga_property_id.is_none() || outputs.measurement_id.is_none() {
        to_migrate.push(id);
      }
    }
    drop(stmt);

    let message = "流程已调整：部署检查移到 GTM 发布之后。请点击「从失败处继续」，将先完成 GA4 与发布。";
    let ts = Self::now();
    for id in to_migrate {
      self
        .conn
        .execute(
          "UPDATE sites SET status = 'failed', current_step = 'ga_property', error = ?1, updated_at = ?2 WHERE id = ?3",
          params![message, ts, id],
        )
        .map_err(|e| e.to_string())?;
      self
        .conn
        .execute(
          "INSERT INTO site_steps (site_id, step, status, message, updated_at) VALUES (?1, 'deploy_check', 'pending', '', ?2)
           ON CONFLICT(site_id, step) DO UPDATE SET status = 'pending', message = '', updated_at = excluded.updated_at",
          params![id, ts],
        )
        .map_err(|e| e.to_string())?;
      self
        .conn
        .execute(
          "INSERT INTO site_steps (site_id, step, status, message, updated_at) VALUES (?1, 'ga_property', 'failed', ?2, ?3)
           ON CONFLICT(site_id, step) DO UPDATE SET status = 'failed', message = excluded.message, updated_at = excluded.updated_at",
          params![id, message, ts],
        )
        .map_err(|e| e.to_string())?;
    }
    Ok(())
  }

  fn now() -> String {
    chrono::Utc::now().to_rfc3339()
  }

  fn row_to_site(
    id: i64,
    domain: String,
    name: String,
    sitemap_url: String,
    status: String,
    current_step: String,
    error: Option<String>,
    outputs: String,
    created_at: String,
    updated_at: String,
  ) -> Result<Site, String> {
    Ok(Site {
      id,
      domain,
      name,
      sitemap_url,
      status,
      current_step,
      error,
      outputs: serde_json::from_str(&outputs).unwrap_or_default(),
      created_at,
      updated_at,
    })
  }

  pub fn list(&self) -> Result<Vec<Site>, String> {
    let mut stmt = self
      .conn
      .prepare(
        "SELECT id, domain, name, sitemap_url, status, current_step, error, outputs, created_at, updated_at
         FROM sites ORDER BY updated_at DESC",
      )
      .map_err(|e| e.to_string())?;
    let rows = stmt
      .query_map([], |row| {
        Ok((
          row.get::<_, i64>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
          row.get::<_, String>(4)?,
          row.get::<_, String>(5)?,
          row.get::<_, Option<String>>(6)?,
          row.get::<_, String>(7)?,
          row.get::<_, String>(8)?,
          row.get::<_, String>(9)?,
        ))
      })
      .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
      let r = row.map_err(|e| e.to_string())?;
      out.push(Self::row_to_site(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9)?);
    }
    Ok(out)
  }

  pub fn get(&self, id: i64) -> Result<Option<Site>, String> {
    self
      .conn
      .query_row(
        "SELECT id, domain, name, sitemap_url, status, current_step, error, outputs, created_at, updated_at
         FROM sites WHERE id = ?1",
        params![id],
        |row| {
          Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, String>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
          ))
        },
      )
      .optional()
      .map_err(|e| e.to_string())?
      .map(|r| Self::row_to_site(r.0, r.1, r.2, r.3, r.4, r.5, r.6, r.7, r.8, r.9))
      .transpose()
  }

  pub fn get_detail(&self, id: i64) -> Result<Option<SiteDetail>, String> {
    let Some(site) = self.get(id)? else {
      return Ok(None);
    };
    let mut stmt = self
      .conn
      .prepare("SELECT step, status, message, updated_at FROM site_steps WHERE site_id = ?1")
      .map_err(|e| e.to_string())?;
    let rows = stmt
      .query_map(params![id], |row| {
        Ok((
          row.get::<_, String>(0)?,
          row.get::<_, String>(1)?,
          row.get::<_, String>(2)?,
          row.get::<_, String>(3)?,
        ))
      })
      .map_err(|e| e.to_string())?;
    let mut by_step = std::collections::HashMap::new();
    for row in rows {
      let (step, status, message, updated_at) = row.map_err(|e| e.to_string())?;
      by_step.insert(step, (status, message, updated_at));
    }
    let steps = STEP_KEYS
      .iter()
      .map(|step| {
        if let Some((status, message, updated_at)) = by_step.get(*step) {
          SiteStep {
            step: (*step).into(),
            status: status.clone(),
            message: message.clone(),
            updated_at: Some(updated_at.clone()),
          }
        } else {
          SiteStep {
            step: (*step).into(),
            status: "pending".into(),
            message: String::new(),
            updated_at: None,
          }
        }
      })
      .collect();
    Ok(Some(SiteDetail { site, steps }))
  }

  pub fn delete(&self, id: i64) -> Result<(), String> {
    let existed = self.get(id)?.is_some();
    if !existed {
      return Err("站点不存在".into());
    }
    self
      .conn
      .execute("DELETE FROM site_steps WHERE site_id = ?1", params![id])
      .map_err(|e| e.to_string())?;
    self
      .conn
      .execute("DELETE FROM sites WHERE id = ?1", params![id])
      .map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn domain_exists(&self, domain: &str) -> Result<bool, String> {
    let exists: Option<i64> = self
      .conn
      .query_row(
        "SELECT 1 FROM sites WHERE domain = ?1",
        params![domain],
        |row| row.get(0),
      )
      .optional()
      .map_err(|e| e.to_string())?;
    Ok(exists.is_some())
  }

  pub fn insert(
    &self,
    domain: &str,
    name: &str,
    sitemap_url: &str,
    track_click: bool,
    track_outbound: bool,
    track_form: bool,
  ) -> Result<Site, String> {
    let ts = Self::now();
    let outputs = SiteOutputs {
      track_click,
      track_outbound,
      track_form,
      ..Default::default()
    };
    let outputs_json = serde_json::to_string(&outputs).map_err(|e| e.to_string())?;
    self
      .conn
      .execute(
        "INSERT INTO sites (domain, name, sitemap_url, status, current_step, outputs, created_at, updated_at)
         VALUES (?1, ?2, ?3, 'running', ?4, ?5, ?6, ?7)",
        params![domain, name, sitemap_url, STEP_KEYS[0], outputs_json, ts, ts],
      )
      .map_err(|e| e.to_string())?;
    let id = self.conn.last_insert_rowid();
    self.get(id)?.ok_or_else(|| "创建站点后读取失败".into())
  }

  pub fn update(
    &self,
    id: i64,
    status: Option<&str>,
    current_step: Option<&str>,
    error: Option<Option<&str>>,
    outputs: Option<&SiteOutputs>,
  ) -> Result<(), String> {
    let site = self
      .get(id)?
      .ok_or_else(|| format!("站点 {id} 不存在"))?;
    let status = status.unwrap_or(&site.status);
    let current_step = current_step.unwrap_or(&site.current_step);
    let error_value = match error {
      Some(v) => v.map(|s| s.to_string()),
      None => site.error.clone(),
    };
    let outputs_json = serde_json::to_string(outputs.unwrap_or(&site.outputs))
      .map_err(|e| e.to_string())?;
    self
      .conn
      .execute(
        "UPDATE sites SET status = ?1, current_step = ?2, error = ?3, outputs = ?4, updated_at = ?5 WHERE id = ?6",
        params![status, current_step, error_value, outputs_json, Self::now(), id],
      )
      .map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn set_step(
    &self,
    site_id: i64,
    step: &str,
    status: &str,
    message: &str,
  ) -> Result<(), String> {
    self
      .conn
      .execute(
        "INSERT INTO site_steps (site_id, step, status, message, updated_at) VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT(site_id, step) DO UPDATE SET
           status = excluded.status, message = excluded.message, updated_at = excluded.updated_at",
        params![site_id, step, status, message, Self::now()],
      )
      .map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn mark_interrupted(&self) -> Result<(), String> {
    let message = "应用重启，任务被中断";
    let ts = Self::now();
    self
      .conn
      .execute(
        "UPDATE site_steps SET status = 'failed', message = ?1, updated_at = ?2 WHERE status = 'running'",
        params![message, ts],
      )
      .map_err(|e| e.to_string())?;
    self
      .conn
      .execute(
        "UPDATE sites SET status = 'failed', error = ?1, updated_at = ?2 WHERE status = 'running'",
        params![message, ts],
      )
      .map_err(|e| e.to_string())?;
    Ok(())
  }

  pub fn merge_outputs(base: &SiteOutputs, patch: &SiteOutputs) -> SiteOutputs {
    SiteOutputs {
      gtm_container_path: patch
        .gtm_container_path
        .clone()
        .or_else(|| base.gtm_container_path.clone()),
      gtm_container_id: patch
        .gtm_container_id
        .clone()
        .or_else(|| base.gtm_container_id.clone()),
      gtm_public_id: patch
        .gtm_public_id
        .clone()
        .or_else(|| base.gtm_public_id.clone()),
      ga_property_id: patch
        .ga_property_id
        .clone()
        .or_else(|| base.ga_property_id.clone()),
      ga_data_stream_id: patch
        .ga_data_stream_id
        .clone()
        .or_else(|| base.ga_data_stream_id.clone()),
      measurement_id: patch
        .measurement_id
        .clone()
        .or_else(|| base.measurement_id.clone()),
      gtm_version_id: patch
        .gtm_version_id
        .clone()
        .or_else(|| base.gtm_version_id.clone()),
      // 监听预设仅在创建站点时写入，步骤产出不得覆盖
      track_click: base.track_click,
      track_outbound: base.track_outbound,
      track_form: base.track_form,
      health_status: patch
        .health_status
        .clone()
        .or_else(|| base.health_status.clone()),
      health_checked_at: patch
        .health_checked_at
        .clone()
        .or_else(|| base.health_checked_at.clone()),
      health_items: patch
        .health_items
        .clone()
        .or_else(|| base.health_items.clone()),
    }
  }
}
