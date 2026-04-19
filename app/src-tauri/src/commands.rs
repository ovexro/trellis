use crate::auth;
use crate::connection::ConnectionManager;
use crate::db::{ActivityEntry, Annotation, ApiToken, AlertRule, Database, DeviceGroup, DevicePosition, DeviceTemplate, FirmwareRecord, FloorPlan, FloorPlanRoom, LogEntry, MetricPoint, Rule, SavedDevice, Scene, SceneActionInput, Schedule, Webhook};
use crate::device::Device;
use crate::diagnostics::{self, DiagnosticReport, EligibleRelease, FleetReport};
use crate::discovery::Discovery;
use crate::mqtt::{MqttBridge, MqttConfig, MqttConfigPublic, MqttStatus};
use crate::ota;
use crate::secret_store::{self, SecretStore};
use crate::serial::{SerialManager, SerialPortInfo};
use crate::sinric::{SinricBridge, SinricConfig, SinricConfigPublic, SinricStatus};
use serde::Serialize;
use serde_json::Value;
use std::io::Read as _;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

pub struct AppState {
    pub discovery: Arc<Discovery>,
    pub connection_manager: Arc<ConnectionManager>,
    pub serial_manager: Arc<SerialManager>,
    pub mqtt_bridge: Arc<MqttBridge>,
    pub sinric_bridge: Arc<SinricBridge>,
}

// ─── Device discovery ────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_devices(state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    Ok(state.discovery.get_devices())
}

#[tauri::command]
pub async fn add_device_by_ip(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    ip: String,
    port: u16,
) -> Result<Device, String> {
    let discovery = state.discovery.clone();
    tokio::task::spawn_blocking(move || discovery.add_by_ip(&ip, port, &app_handle))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn send_command(
    state: State<'_, AppState>,
    device_id: String,
    ip: String,
    port: u16,
    command: Value,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;
    let msg = serde_json::to_string(&command).map_err(|e| e.to_string())?;
    tokio::task::spawn_blocking(move || conn_mgr.send_to_device(&device_id, &ip, ws_port, &msg))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

// ─── Device persistence ──────────────────────────────────────────────────────

#[tauri::command]
pub fn set_device_nickname(
    db: State<'_, Database>,
    device_id: String,
    nickname: String,
) -> Result<(), String> {
    db.set_nickname(&device_id, &nickname)
}

#[tauri::command]
pub fn set_device_tags(
    db: State<'_, Database>,
    device_id: String,
    tags: String,
) -> Result<(), String> {
    db.set_tags(&device_id, &tags)
}

#[tauri::command]
pub fn get_saved_devices(db: State<'_, Database>) -> Result<Vec<SavedDevice>, String> {
    db.get_all_saved_devices()
}

#[tauri::command]
pub fn get_saved_device(
    db: State<'_, Database>,
    device_id: String,
) -> Result<Option<SavedDevice>, String> {
    db.get_saved_device(&device_id)
}

// ─── Serial ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<SerialPortInfo>, String> {
    Ok(SerialManager::list_ports())
}

#[tauri::command]
pub fn open_serial(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    port: String,
    baud: u32,
) -> Result<(), String> {
    state.serial_manager.open(&port, baud, app_handle)
}

#[tauri::command]
pub fn close_serial(state: State<'_, AppState>, port: String) -> Result<(), String> {
    state.serial_manager.close(&port)
}

#[tauri::command]
pub fn send_serial(state: State<'_, AppState>, port: String, data: String) -> Result<(), String> {
    state.serial_manager.write(&port, &data)
}

// ─── OTA ─────────────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn start_ota(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    app_handle: AppHandle,
    device_id: String,
    ip: String,
    port: u16,
    firmware_path: String,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;

    // Store firmware copy for rollback
    let fw_dir = app_handle.path().app_data_dir()
        .map_err(|e| format!("No app dir: {}", e))?
        .join("firmware");
    std::fs::create_dir_all(&fw_dir).map_err(|e| format!("Failed to create firmware dir: {}", e))?;

    let src_path = std::path::PathBuf::from(&firmware_path);
    let file_size = std::fs::metadata(&src_path)
        .map_err(|e| format!("Cannot read firmware: {}", e))?.len() as i64;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let dest_name = format!("{}_{}.bin", device_id, timestamp);
    let dest_path = fw_dir.join(&dest_name);
    std::fs::copy(&src_path, &dest_path)
        .map_err(|e| format!("Failed to copy firmware: {}", e))?;

    // Get current firmware version from device
    let version = {
        let devices = state.discovery.get_devices();
        devices.iter()
            .find(|d| d.id == device_id)
            .map(|d| d.firmware.clone())
            .unwrap_or_else(|| "unknown".to_string())
    };

    let dest_str = dest_path.to_string_lossy().to_string();
    db.store_firmware_record(&device_id, &version, &dest_str, file_size)?;

    let serve_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let (url, _stop_flag) =
            ota::serve_firmware(&firmware_path, serve_handle, device_id.clone())?;
        let ota_cmd = serde_json::json!({"command": "ota", "url": url});
        let msg = serde_json::to_string(&ota_cmd).map_err(|e| e.to_string())?;
        conn_mgr.send_to_device(&device_id, &ip, ws_port, &msg)?;
        log::info!("[OTA] Triggered update for device {} from {}", device_id, url);
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ─── Firmware history ───────────────────────────────────────────────────────

#[tauri::command]
pub fn get_firmware_history(
    db: State<'_, Database>, device_id: String,
) -> Result<Vec<FirmwareRecord>, String> {
    db.get_firmware_history(&device_id)
}

#[tauri::command]
pub fn delete_firmware_record(
    db: State<'_, Database>, id: i64,
) -> Result<(), String> {
    let path = db.delete_firmware_record(id)?;
    let _ = std::fs::remove_file(&path);
    Ok(())
}

#[tauri::command]
pub async fn rollback_firmware(
    state: State<'_, AppState>,
    app_handle: AppHandle,
    device_id: String,
    ip: String,
    port: u16,
    firmware_record_path: String,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;
    let serve_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let (url, _stop_flag) =
            ota::serve_firmware(&firmware_record_path, serve_handle, device_id.clone())?;
        let ota_cmd = serde_json::json!({"command": "ota", "url": url});
        let msg = serde_json::to_string(&ota_cmd).map_err(|e| e.to_string())?;
        conn_mgr.send_to_device(&device_id, &ip, ws_port, &msg)?;
        log::info!("[OTA] Rollback triggered for device {}", device_id);
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ─── GitHub OTA ─────────────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct GithubAsset {
    pub name: String,
    pub size: i64,
    pub download_url: String,
}

#[derive(Serialize)]
pub struct GithubRelease {
    pub tag: String,
    pub name: String,
    pub published_at: String,
    pub prerelease: bool,
    pub assets: Vec<GithubAsset>,
}

#[tauri::command]
pub async fn check_github_releases(
    owner: String,
    repo: String,
) -> Result<Vec<GithubRelease>, String> {
    tokio::task::spawn_blocking(move || {
        let url = format!(
            "https://api.github.com/repos/{}/{}/releases",
            owner, repo
        );
        let resp = ureq::get(&url)
            .set("User-Agent", "Trellis-Desktop")
            .set("Accept", "application/vnd.github+json")
            .timeout(std::time::Duration::from_secs(10))
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(404, _) => "Repository not found. Check the owner/repo format. Private repositories are not supported.".to_string(),
                ureq::Error::Status(403, _) => "GitHub API rate limit reached (60 requests/hour). Wait a few minutes and try again.".to_string(),
                ureq::Error::Status(code, _) => format!("GitHub returned HTTP {}.", code),
                ureq::Error::Transport(_) => "Could not reach GitHub. Check your internet connection.".to_string(),
            })?;

        let releases: Vec<serde_json::Value> = resp
            .into_json()
            .map_err(|e| format!("JSON parse error: {}", e))?;

        let mut result = Vec::new();
        for rel in releases.iter().take(20) {
            let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
            let name = rel["name"].as_str().unwrap_or(&tag).to_string();
            let published = rel["published_at"].as_str().unwrap_or("").to_string();
            let prerelease = rel["prerelease"].as_bool().unwrap_or(false);

            let mut assets = Vec::new();
            if let Some(arr) = rel["assets"].as_array() {
                for asset in arr {
                    let aname = asset["name"].as_str().unwrap_or("");
                    if aname.ends_with(".bin") || aname.ends_with(".bin.gz") {
                        assets.push(GithubAsset {
                            name: aname.to_string(),
                            size: asset["size"].as_i64().unwrap_or(0),
                            download_url: asset["browser_download_url"]
                                .as_str()
                                .unwrap_or("")
                                .to_string(),
                        });
                    }
                }
            }

            if !assets.is_empty() {
                result.push(GithubRelease {
                    tag,
                    name,
                    published_at: published,
                    prerelease,
                    assets,
                });
            }
        }
        Ok(result)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn start_github_ota(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    app_handle: AppHandle,
    device_id: String,
    ip: String,
    port: u16,
    download_url: String,
    release_tag: String,
    asset_name: String,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;

    // Prepare firmware storage directory
    let fw_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app dir: {}", e))?
        .join("firmware");
    std::fs::create_dir_all(&fw_dir)
        .map_err(|e| format!("Failed to create firmware dir: {}", e))?;

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let dest_name = format!("{}_gh_{}_{}.bin", device_id, release_tag, timestamp);
    let dest_path = fw_dir.join(&dest_name);
    let dest_str = dest_path.to_string_lossy().to_string();

    // Download firmware from GitHub (auto-decompress .bin.gz)
    let dl_dest = dest_path.clone();
    let dl_url = download_url.clone();
    let is_gzipped = asset_name.ends_with(".bin.gz");
    let progress_handle = app_handle.clone();
    let progress_device_id = device_id.clone();
    let file_size = tokio::task::spawn_blocking(move || {
        log::info!("[OTA] Downloading {} from GitHub...", asset_name);
        let resp = ureq::get(&dl_url)
            .set("User-Agent", "Trellis-Desktop")
            .timeout(std::time::Duration::from_secs(120))
            .call()
            .map_err(|e| match e {
                ureq::Error::Status(404, _) => "Firmware file not found — the asset may have been removed from the release.".to_string(),
                ureq::Error::Status(403, _) => "Download blocked by GitHub — rate limit or authentication required.".to_string(),
                ureq::Error::Status(code, _) => format!("Download failed with HTTP {}.", code),
                ureq::Error::Transport(_) => "Download failed — network error. Check your internet connection.".to_string(),
            })?;

        let content_length = resp
            .header("Content-Length")
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);

        let mut reader = resp.into_reader();
        let mut compressed = Vec::with_capacity(
            if content_length > 0 { content_length as usize } else { 512 * 1024 },
        );
        let mut buf = [0u8; 8192];
        let mut downloaded: u64 = 0;
        let mut last_pct: u64 = 0;

        loop {
            let n = reader
                .read(&mut buf)
                .map_err(|e| format!("Read failed: {}", e))?;
            if n == 0 {
                break;
            }
            compressed.extend_from_slice(&buf[..n]);
            downloaded += n as u64;

            if content_length > 0 {
                let pct = (downloaded * 100 / content_length).min(100);
                if pct >= last_pct + 2 || downloaded >= content_length {
                    let _ = progress_handle.emit(
                        "gh-download-progress",
                        serde_json::json!({
                            "device_id": progress_device_id,
                            "downloaded": downloaded,
                            "total": content_length,
                            "percent": pct,
                        }),
                    );
                    last_pct = pct;
                }
            }
        }

        let data = if is_gzipped {
            log::info!("[OTA] Decompressing .bin.gz ({} bytes compressed)", compressed.len());
            let mut decoder = flate2::read::GzDecoder::new(&compressed[..]);
            let mut decompressed = Vec::new();
            decoder
                .read_to_end(&mut decompressed)
                .map_err(|e| format!("Gzip decompression failed: {}", e))?;
            log::info!("[OTA] Decompressed to {} bytes", decompressed.len());
            decompressed
        } else {
            compressed
        };

        let size = data.len() as i64;
        std::fs::write(&dl_dest, &data)
            .map_err(|e| format!("Failed to save firmware: {}", e))?;

        log::info!("[OTA] Saved {} bytes to {:?}", size, dl_dest);
        Ok::<i64, String>(size)
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    // Store firmware record for history/rollback
    db.store_firmware_record(&device_id, &release_tag, &dest_str, file_size)?;

    // Serve firmware to device via existing OTA flow
    let serve_handle = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let (url, _stop_flag) =
            ota::serve_firmware(&dest_str, serve_handle, device_id.clone())?;
        let ota_cmd = serde_json::json!({"command": "ota", "url": url});
        let msg = serde_json::to_string(&ota_cmd).map_err(|e| e.to_string())?;
        conn_mgr.send_to_device(&device_id, &ip, ws_port, &msg)?;
        log::info!("[OTA] GitHub OTA triggered for device {}", device_id);
        Ok(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ─── Metrics ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn store_metric(
    db: State<'_, Database>,
    device_id: String,
    metric_id: String,
    value: f64,
) -> Result<(), String> {
    db.store_metric(&device_id, &metric_id, value)
}

#[tauri::command]
pub fn get_metrics(
    db: State<'_, Database>,
    device_id: String,
    metric_id: String,
    hours: u32,
) -> Result<Vec<MetricPoint>, String> {
    db.get_metrics(&device_id, &metric_id, hours)
}

/// Return the chart-annotation stream for the React `MetricChart` overlay.
/// Mirrors the `/api/devices/{id}/annotations?hours=N` REST endpoint used by
/// the `:9090` web dashboard so both surfaces render the same markers.
#[tauri::command]
pub fn get_device_annotations(
    db: State<'_, Database>,
    device_id: String,
    hours: u32,
) -> Result<Vec<Annotation>, String> {
    db.get_annotations(&device_id, hours)
}

// ─── Alerts ──────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_alert(
    db: State<'_, Database>,
    device_id: String,
    metric_id: String,
    condition: String,
    threshold: f64,
    label: String,
) -> Result<i64, String> {
    db.create_alert(&device_id, &metric_id, &condition, threshold, &label)
}

#[tauri::command]
pub fn get_alerts(db: State<'_, Database>, device_id: String) -> Result<Vec<AlertRule>, String> {
    db.get_alerts(&device_id)
}

#[tauri::command]
pub fn delete_alert(db: State<'_, Database>, alert_id: i64) -> Result<(), String> {
    db.delete_alert(alert_id)
}

#[tauri::command]
pub fn toggle_alert(
    db: State<'_, Database>,
    alert_id: i64,
    enabled: bool,
) -> Result<(), String> {
    db.toggle_alert(alert_id, enabled)
}

// ─── Activity feed ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_recent_activity(
    db: State<'_, Database>,
    limit: u32,
) -> Result<Vec<ActivityEntry>, String> {
    db.get_recent_activity(limit)
}

// ─── Device diagnostics ──────────────────────────────────────────────────────

/// Run the rule-based diagnostic engine for a single device and return a
/// structured report. Pure read-only: the engine reads metrics, logs, and
/// firmware history from SQLite plus the live `Device` from `Discovery` to
/// decide whether the device is online. When the device has a GitHub repo
/// binding, also fetches the latest release and, if strictly newer than the
/// current firmware version, exposes a one-click `firmware_update` action on
/// the firmware_age finding. Safe to call as a `viewer`.
#[tauri::command]
pub fn diagnose_device(
    db: State<'_, Database>,
    state: State<'_, AppState>,
    device_id: String,
) -> Result<DiagnosticReport, String> {
    let live_devices = state.discovery.get_devices();
    let live = live_devices.iter().find(|d| d.id == device_id);
    let eligible = fetch_eligible_for_device(&*db, &device_id, live);
    diagnostics::diagnose(&*db, &device_id, live, eligible.as_ref())
}

/// Aggregate per-device diagnostics across the saved fleet. Safe for `viewer`.
#[tauri::command]
pub fn diagnose_fleet(
    db: State<'_, Database>,
    state: State<'_, AppState>,
) -> Result<FleetReport, String> {
    let live_devices = state.discovery.get_devices();
    let eligible = fetch_eligible_for_fleet(&*db, &live_devices);
    diagnostics::diagnose_fleet(&*db, &live_devices, &eligible)
}

/// Bind (or clear) the GitHub repo used for firmware auto-remediation on a
/// single device. Passing empty strings for both fields clears the binding.
/// Admin-only via the REST wrapper; Tauri ACL gates the desktop command.
#[tauri::command]
pub fn set_device_github_repo(
    db: State<'_, Database>,
    device_id: String,
    owner: String,
    repo: String,
) -> Result<(), String> {
    db.set_device_github_repo(
        &device_id,
        Some(owner.as_str()),
        Some(repo.as_str()),
    )
}

/// Fetch the newest eligible firmware release for a single device, if any.
/// Returns `None` unless the device has both `github_owner` and `github_repo`
/// bound and a non-prerelease release exists whose tag parses to a strictly
/// newer version than the device's current firmware. Network failure, unparsed
/// versions, and assets without `.bin`/`.bin.gz` all fall back to `None` so
/// the rule engine can stay INFO-only rather than emit a false positive.
pub fn fetch_eligible_for_device(
    db: &Database,
    device_id: &str,
    live: Option<&Device>,
) -> Option<EligibleRelease> {
    let saved = db.get_saved_device(device_id).ok().flatten()?;
    let owner = saved.github_owner.as_ref().filter(|s| !s.is_empty())?;
    let repo = saved.github_repo.as_ref().filter(|s| !s.is_empty())?;
    let current_version = live
        .map(|d| d.firmware.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| saved.firmware.clone().into())
        .filter(|v: &String| !v.is_empty())?;
    fetch_eligible_release_blocking(owner, repo, &current_version)
}

/// Pre-compute eligible updates for every bound device in the fleet. Dedupes
/// by (owner, repo) so a 20-device fleet on one repo makes a single GitHub
/// API call instead of 20.
pub fn fetch_eligible_for_fleet(
    db: &Database,
    live_devices: &[Device],
) -> std::collections::HashMap<String, EligibleRelease> {
    use std::collections::HashMap;
    let saved = match db.get_all_saved_devices() {
        Ok(s) => s,
        Err(_) => return HashMap::new(),
    };
    let mut cache: HashMap<(String, String), Vec<(String, String, String)>> = HashMap::new();
    let mut result: HashMap<String, EligibleRelease> = HashMap::new();
    for sd in &saved {
        let owner = match sd.github_owner.as_ref().filter(|s| !s.is_empty()) {
            Some(o) => o.clone(),
            None => continue,
        };
        let repo = match sd.github_repo.as_ref().filter(|s| !s.is_empty()) {
            Some(r) => r.clone(),
            None => continue,
        };
        let live = live_devices.iter().find(|d| d.id == sd.id);
        let current = live
            .map(|d| d.firmware.clone())
            .filter(|v| !v.is_empty())
            .or_else(|| Some(sd.firmware.clone()))
            .filter(|v| !v.is_empty());
        let current = match current {
            Some(c) => c,
            None => continue,
        };
        let key = (owner.clone(), repo.clone());
        let releases = cache
            .entry(key)
            .or_insert_with(|| fetch_releases_raw(&owner, &repo));
        if let Some(r) = pick_newer(releases, &current) {
            result.insert(sd.id.clone(), r);
        }
    }
    result
}

fn fetch_releases_raw(owner: &str, repo: &str) -> Vec<(String, String, String)> {
    let url = format!("https://api.github.com/repos/{}/{}/releases", owner, repo);
    let resp = match ureq::get(&url)
        .set("User-Agent", "Trellis-Desktop")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
    {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let releases: Vec<serde_json::Value> = match resp.into_json() {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };
    let mut out = Vec::new();
    for rel in releases.iter().take(20) {
        if rel["prerelease"].as_bool().unwrap_or(false) {
            continue;
        }
        let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
        if tag.is_empty() {
            continue;
        }
        if let Some(assets) = rel["assets"].as_array() {
            for asset in assets {
                let aname = asset["name"].as_str().unwrap_or("");
                if aname.ends_with(".bin") || aname.ends_with(".bin.gz") {
                    let url = asset["browser_download_url"].as_str().unwrap_or("").to_string();
                    out.push((tag.clone(), aname.to_string(), url));
                    break;
                }
            }
        }
    }
    out
}

fn pick_newer(
    releases: &[(String, String, String)],
    current_version: &str,
) -> Option<EligibleRelease> {
    releases
        .iter()
        .find(|(tag, _, _)| diagnostics::is_newer_version(tag, current_version))
        .map(|(tag, asset, url)| EligibleRelease {
            release_tag: tag.clone(),
            asset_name: asset.clone(),
            download_url: url.clone(),
        })
}

fn fetch_eligible_release_blocking(
    owner: &str,
    repo: &str,
    current_version: &str,
) -> Option<EligibleRelease> {
    let releases = fetch_releases_raw(owner, repo);
    pick_newer(&releases, current_version)
}

// ─── Device logs ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_device_logs(
    db: State<'_, Database>,
    device_id: String,
    limit: u32,
    severity: Option<String>,
) -> Result<Vec<LogEntry>, String> {
    // `severity` is a comma-separated list (e.g. "state,error,warn") to match
    // the REST API's `?severity=...` query param shape used by the :9090 web
    // dashboard. Empty / missing → unfiltered (same as `get_logs`).
    let sev_list: Vec<String> = severity
        .as_deref()
        .map(|s| {
            s.split(',')
                .map(|part| part.trim().to_string())
                .filter(|part| !part.is_empty())
                .collect()
        })
        .unwrap_or_default();
    if sev_list.is_empty() {
        db.get_logs(&device_id, limit)
    } else {
        db.get_logs_filtered(&device_id, limit, Some(&sev_list))
    }
}

#[tauri::command]
pub fn remove_device(
    db: State<'_, Database>,
    device_id: String,
) -> Result<(), String> {
    db.delete_device(&device_id)
}

#[tauri::command]
pub fn store_log_entry(
    db: State<'_, Database>,
    device_id: String,
    severity: String,
    message: String,
) -> Result<(), String> {
    db.store_log(&device_id, &severity, &message)
}

// ─── Schedules ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_schedule(
    db: State<'_, Database>, device_id: String, capability_id: String,
    value: String, cron: String, label: String, scene_id: Option<i64>,
) -> Result<i64, String> {
    db.create_schedule(&device_id, &capability_id, &value, &cron, &label, scene_id)
}

#[tauri::command]
pub fn get_schedules(db: State<'_, Database>) -> Result<Vec<Schedule>, String> {
    db.get_schedules()
}

#[tauri::command]
pub fn delete_schedule(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_schedule(id)
}

#[tauri::command]
pub fn toggle_schedule(db: State<'_, Database>, id: i64, enabled: bool) -> Result<(), String> {
    db.toggle_schedule(id, enabled)
}

// ─── Conditional rules ──────────────────────────────────────────────────────

#[tauri::command]
pub fn create_rule(
    db: State<'_, Database>, source_device_id: String, source_metric_id: String,
    condition: String, threshold: f64, target_device_id: String,
    target_capability_id: String, target_value: String, label: String,
    logic: Option<String>, conditions: Option<String>,
) -> Result<i64, String> {
    db.create_rule(&source_device_id, &source_metric_id, &condition, threshold,
        &target_device_id, &target_capability_id, &target_value, &label,
        logic.as_deref().unwrap_or("and"), conditions.as_deref())
}

#[tauri::command]
pub fn get_rules(db: State<'_, Database>) -> Result<Vec<Rule>, String> {
    db.get_rules()
}

#[tauri::command]
pub fn delete_rule(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_rule(id)
}

#[tauri::command]
pub fn toggle_rule(db: State<'_, Database>, id: i64, enabled: bool) -> Result<(), String> {
    db.toggle_rule(id, enabled)
}

// ─── Webhooks ───────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_webhook(
    db: State<'_, Database>, event_type: String, device_id: Option<String>,
    url: String, label: String,
) -> Result<i64, String> {
    db.create_webhook(&event_type, device_id.as_deref(), &url, &label)
}

#[tauri::command]
pub fn get_webhooks(db: State<'_, Database>) -> Result<Vec<Webhook>, String> {
    db.get_webhooks()
}

#[tauri::command]
pub fn delete_webhook(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_webhook(id)
}

#[tauri::command]
pub fn toggle_webhook(db: State<'_, Database>, id: i64, enabled: bool) -> Result<(), String> {
    db.toggle_webhook(id, enabled)
}

// ─── Webhook delivery history ────────────────────────────────────────────────

#[tauri::command]
pub fn log_webhook_delivery(
    db: State<'_, Database>, webhook_id: i64, event_type: String,
    status_code: Option<i32>, success: bool, error: Option<String>, attempt: i32,
) -> Result<i64, String> {
    db.log_webhook_delivery(webhook_id, &event_type, status_code, success, error.as_deref(), attempt)
}

#[tauri::command]
pub fn get_webhook_deliveries(
    db: State<'_, Database>, webhook_id: i64, limit: Option<i64>,
) -> Result<Vec<crate::db::WebhookDelivery>, String> {
    db.get_webhook_deliveries(webhook_id, limit.unwrap_or(20))
}

// ─── Device templates ───────────────────────────────────────────────────────

#[tauri::command]
pub fn create_template(
    db: State<'_, Database>, name: String, description: String, capabilities: String,
) -> Result<i64, String> {
    db.create_template(&name, &description, &capabilities)
}

#[tauri::command]
pub fn get_templates(db: State<'_, Database>) -> Result<Vec<DeviceTemplate>, String> {
    db.get_templates()
}

#[tauri::command]
pub fn delete_template(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_template(id)
}

// ─── Device groups ─────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_group(db: State<'_, Database>, name: String, color: String) -> Result<i64, String> {
    db.create_group(&name, &color)
}

#[tauri::command]
pub fn get_groups(db: State<'_, Database>) -> Result<Vec<DeviceGroup>, String> {
    db.get_groups()
}

#[tauri::command]
pub fn update_group(db: State<'_, Database>, id: i64, name: String, color: String) -> Result<(), String> {
    db.update_group(id, &name, &color)
}

#[tauri::command]
pub fn delete_group(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_group(id)
}

#[tauri::command]
pub fn set_device_group(db: State<'_, Database>, device_id: String, group_id: Option<i64>) -> Result<(), String> {
    db.set_device_group(&device_id, group_id)
}

#[tauri::command]
pub fn set_device_favorite(db: State<'_, Database>, device_id: String, favorite: bool) -> Result<(), String> {
    db.set_device_favorite(&device_id, favorite)
}

#[tauri::command]
pub fn toggle_favorite_capability(db: State<'_, Database>, device_id: String, capability_id: String) -> Result<bool, String> {
    db.toggle_favorite_capability(&device_id, &capability_id)
}

#[tauri::command]
pub fn get_favorite_capabilities(db: State<'_, Database>) -> Result<Vec<(String, String)>, String> {
    db.get_favorite_capabilities()
}

// ─── Floor plans ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_floor_plans(db: State<'_, Database>) -> Result<Vec<FloorPlan>, String> {
    db.get_floor_plans()
}

#[tauri::command]
pub fn create_floor_plan(db: State<'_, Database>, name: String) -> Result<i64, String> {
    db.create_floor_plan(&name)
}

#[tauri::command]
pub fn update_floor_plan(db: State<'_, Database>, id: i64, name: Option<String>, background: Option<Option<String>>) -> Result<(), String> {
    let name_ref = name.as_deref();
    let bg_ref = background.as_ref().map(|b| b.as_deref());
    db.update_floor_plan(id, name_ref, bg_ref)
}

#[tauri::command]
pub fn delete_floor_plan(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_floor_plan(id)
}

// ─── Floor plan positions ───────────────────────────────────────────────────

#[tauri::command]
pub fn get_device_positions(db: State<'_, Database>, floor_id: i64) -> Result<Vec<DevicePosition>, String> {
    db.get_device_positions(floor_id)
}

#[tauri::command]
pub fn get_all_device_positions(db: State<'_, Database>) -> Result<Vec<DevicePosition>, String> {
    db.get_all_device_positions()
}

#[tauri::command]
pub fn set_device_position(db: State<'_, Database>, device_id: String, floor_id: i64, x: f64, y: f64) -> Result<(), String> {
    db.set_device_position(&device_id, floor_id, x, y)
}

#[tauri::command]
pub fn remove_device_position(db: State<'_, Database>, device_id: String) -> Result<(), String> {
    db.remove_device_position(&device_id)
}

// ─── Floor plan rooms ───────────────────────────────────────────────────────

#[tauri::command]
pub fn get_rooms(db: State<'_, Database>, floor_id: i64) -> Result<Vec<FloorPlanRoom>, String> {
    db.get_rooms(floor_id)
}

#[tauri::command]
pub fn get_all_rooms(db: State<'_, Database>) -> Result<Vec<FloorPlanRoom>, String> {
    db.get_all_rooms()
}

#[tauri::command]
pub fn create_room(
    db: State<'_, Database>,
    floor_id: i64,
    name: String,
    color: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<i64, String> {
    let color = color.unwrap_or_else(|| "#6366f1".to_string());
    let x = x.unwrap_or(10.0).clamp(0.0, 100.0);
    let y = y.unwrap_or(10.0).clamp(0.0, 100.0);
    let w = w.unwrap_or(30.0).clamp(1.0, 100.0).min(100.0 - x);
    let h = h.unwrap_or(30.0).clamp(1.0, 100.0).min(100.0 - y);
    db.create_room(floor_id, name.trim(), &color, x, y, w, h)
}

#[tauri::command]
pub fn update_room(
    db: State<'_, Database>,
    id: i64,
    name: Option<String>,
    color: Option<String>,
    x: Option<f64>,
    y: Option<f64>,
    w: Option<f64>,
    h: Option<f64>,
) -> Result<(), String> {
    let name_ref = name.as_deref().map(|s| s.trim()).filter(|s| !s.is_empty());
    let color_ref = color.as_deref().filter(|s| !s.is_empty());
    let x = x.map(|v| v.clamp(0.0, 100.0));
    let y = y.map(|v| v.clamp(0.0, 100.0));
    let w = w.map(|v| v.clamp(1.0, 100.0));
    let h = h.map(|v| v.clamp(1.0, 100.0));
    db.update_room(id, name_ref, color_ref, x, y, w, h)
}

#[tauri::command]
pub fn delete_room(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_room(id)
}

// ─── CSV export ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn export_metrics_csv(
    db: State<'_, Database>, device_id: String, metric_id: String, hours: u32,
) -> Result<String, String> {
    db.export_metrics_csv(&device_id, &metric_id, hours)
}

// ─── Settings ──────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_setting(db: State<'_, Database>, key: String) -> Result<Option<String>, String> {
    db.get_setting(&key)
}

#[tauri::command]
pub fn set_setting(db: State<'_, Database>, key: String, value: String) -> Result<(), String> {
    db.set_setting(&key, &value)
}

#[tauri::command]
pub fn delete_setting(db: State<'_, Database>, key: String) -> Result<(), String> {
    db.delete_setting(&key)
}

// ─── ntfy.sh push notifications ────────────────────────────────────────────

#[tauri::command]
pub fn send_ntfy(topic: String, title: String, message: String, priority: u8) -> Result<(), String> {
    let url = format!("https://ntfy.sh/{}", topic);
    let body = serde_json::json!({
        "topic": topic,
        "title": title,
        "message": message,
        "priority": priority.min(5).max(1)
    });
    ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("ntfy send failed: {}", e))?;
    Ok(())
}

// ─── MQTT bridge ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_mqtt_config(state: State<'_, AppState>) -> Result<MqttConfigPublic, String> {
    // Returns the network-safe view (no password). The Settings UI uses the
    // `has_password` flag to render either "(none)" or "(unchanged — type to
    // update)" placeholder text.
    Ok(state.mqtt_bridge.get_config_public())
}

#[tauri::command]
pub fn set_mqtt_config(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    secret_store: State<'_, Arc<SecretStore>>,
    config: MqttConfig,
) -> Result<MqttStatus, String> {
    // Apply via the user-facing path so an empty password in the incoming
    // request preserves the existing stored password instead of blanking it.
    state.mqtt_bridge.apply_config_from_user(config)?;

    // Persist the *merged* config (post-preserve) so a restart picks up the
    // same auth state the live bridge is now using. Reading get_config back
    // out of the bridge gives us the merged result. Encrypt the password
    // field BEFORE serializing so the SQLite blob never holds plaintext.
    let mut merged = state.mqtt_bridge.get_config();
    secret_store::encrypt_mqtt_password(secret_store.inner().as_ref(), &mut merged)?;
    let json = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
    db.set_setting("mqtt_config", &json)?;

    Ok(state.mqtt_bridge.get_status())
}

#[tauri::command]
pub fn clear_mqtt_password(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    secret_store: State<'_, Arc<SecretStore>>,
) -> Result<MqttStatus, String> {
    // Explicit clear path — distinct from "save with empty password" which
    // means preserve. After clearing, persist so the cleared state survives a
    // restart. Encrypt the (now empty) password field — encrypt_mqtt_password
    // is a no-op on empty so we just save the bare JSON.
    state.mqtt_bridge.clear_password()?;
    let mut cleared = state.mqtt_bridge.get_config();
    secret_store::encrypt_mqtt_password(secret_store.inner().as_ref(), &mut cleared)?;
    let json = serde_json::to_string(&cleared).map_err(|e| e.to_string())?;
    db.set_setting("mqtt_config", &json)?;
    Ok(state.mqtt_bridge.get_status())
}

#[tauri::command]
pub fn get_mqtt_status(state: State<'_, AppState>) -> Result<MqttStatus, String> {
    Ok(state.mqtt_bridge.get_status())
}

#[tauri::command]
pub fn test_mqtt_connection(
    state: State<'_, AppState>,
    config: MqttConfig,
) -> Result<(), String> {
    // Same preserve-blank rule as set_mqtt_config: if the user didn't retype
    // the password, exercise the test against the stored one.
    state.mqtt_bridge.test_connection_from_user(config)
}

// ─── Sinric Pro bridge ─────────────────────────────────────────────────────

#[tauri::command]
pub fn get_sinric_config(state: State<'_, AppState>) -> Result<SinricConfigPublic, String> {
    Ok(state.sinric_bridge.get_config_public())
}

#[tauri::command]
pub fn set_sinric_config(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    secret_store: State<'_, Arc<SecretStore>>,
    config: SinricConfig,
) -> Result<SinricStatus, String> {
    state.sinric_bridge.apply_config_from_user(config)?;

    let mut merged = state.sinric_bridge.get_config();
    secret_store::encrypt_sinric_secret(secret_store.inner().as_ref(), &mut merged)?;
    let json = serde_json::to_string(&merged).map_err(|e| e.to_string())?;
    db.set_setting("sinric_config", &json)?;

    Ok(state.sinric_bridge.get_status())
}

#[tauri::command]
pub fn clear_sinric_secret(
    state: State<'_, AppState>,
    db: State<'_, Database>,
    secret_store: State<'_, Arc<SecretStore>>,
) -> Result<SinricStatus, String> {
    state.sinric_bridge.clear_secret()?;
    let mut cleared = state.sinric_bridge.get_config();
    secret_store::encrypt_sinric_secret(secret_store.inner().as_ref(), &mut cleared)?;
    let json = serde_json::to_string(&cleared).map_err(|e| e.to_string())?;
    db.set_setting("sinric_config", &json)?;
    Ok(state.sinric_bridge.get_status())
}

#[tauri::command]
pub fn get_sinric_status(state: State<'_, AppState>) -> Result<SinricStatus, String> {
    Ok(state.sinric_bridge.get_status())
}

#[tauri::command]
pub fn test_sinric_connection(
    state: State<'_, AppState>,
    config: SinricConfig,
) -> Result<(), String> {
    state.sinric_bridge.test_connection_from_user(config)
}

// ─── Device ordering ────────────────────────────────────────────────────────

#[tauri::command]
pub fn reorder_devices(db: State<'_, Database>, order: Vec<(String, i64)>) -> Result<(), String> {
    db.reorder_devices(&order)
}

// ─── API tokens ─────────────────────────────────────────────────────────────

/// Response shape for `create_api_token`. The plaintext `token` field is
/// the only place in the codebase the value is exposed — the UI must
/// surface it immediately and the user must copy it before dismissing the
/// modal. After this returns, only the SHA-256 digest survives in SQLite.
#[derive(Debug, Serialize)]
pub struct CreatedApiToken {
    pub id: i64,
    pub name: String,
    pub token: String,
    pub role: String,
    pub expires_at: Option<String>,
}

#[tauri::command]
pub fn list_api_tokens(db: State<'_, Database>) -> Result<Vec<ApiToken>, String> {
    db.list_api_tokens()
}

#[tauri::command]
pub fn create_api_token(db: State<'_, Database>, name: String, ttl: Option<String>, role: Option<String>) -> Result<CreatedApiToken, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("Token name is required".to_string());
    }
    let role_str = role.as_deref().unwrap_or("admin");
    if role_str != "admin" && role_str != "viewer" {
        return Err("Invalid role. Must be \"admin\" or \"viewer\".".to_string());
    }
    let (plaintext, hash) = auth::generate_token();
    let expires_at = ttl.as_deref().and_then(auth::compute_expires_at);
    let id = db.create_api_token(trimmed, &hash, expires_at.as_deref(), role_str)?;
    Ok(CreatedApiToken {
        id,
        name: trimmed.to_string(),
        token: plaintext,
        role: role_str.to_string(),
        expires_at,
    })
}

#[tauri::command]
pub fn revoke_api_token(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_api_token(id)
}

// ─── Remote access reachability probe ───────────────────────────────────────

/// Result of a single round-trip from the desktop app to a user-supplied
/// public URL (e.g. a Cloudflare Tunnel hostname or a Tailscale Funnel
/// `*.ts.net` URL). Hits `<url>/api/devices` with the supplied token and
/// reports back what happened.
///
/// `category` is a fixed-set string the Settings UI uses to color/style the
/// result without parsing free-form messages: `success`, `auth_failed`,
/// `not_trellis`, `tunnel_down`, `unexpected`, `network_error`, `timeout`.
#[derive(Debug, Serialize)]
pub struct RemoteProbeResult {
    pub ok: bool,
    pub status: u16,
    pub latency_ms: u64,
    pub category: String,
    pub message: String,
}

/// Probe a public URL for remote-access reachability. Used by the Remote
/// Access Settings panel's "Test reachability" button so the user can
/// verify their tunnel + token combo end-to-end without having to copy
/// curl commands. The probe runs entirely from the desktop app's process
/// — it does NOT bounce through the local :9090 server, because the whole
/// point is to verify the path through the *external* network back to
/// :9090.
///
/// Spawned on a blocking task because ureq is sync; the 8-second timeout
/// keeps the UI responsive even on dead URLs.
#[tauri::command]
pub async fn probe_remote_url(url: String, token: String) -> Result<RemoteProbeResult, String> {
    tokio::task::spawn_blocking(move || {
        let trimmed = url.trim().trim_end_matches('/').to_string();
        if trimmed.is_empty() {
            return Err("URL is required.".to_string());
        }
        if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
            return Err("URL must start with http:// or https://.".to_string());
        }
        let token_trimmed = token.trim();
        if token_trimmed.is_empty() {
            return Err(
                "Token is required to test reachability — mint one in API Tokens above first."
                    .to_string(),
            );
        }
        if !token_trimmed.starts_with("trls_") {
            return Err(
                "Token must start with `trls_`. Did you paste the wrong value?".to_string(),
            );
        }
        let probe_url = format!("{}/api/devices", trimmed);
        let started = Instant::now();
        let result = ureq::get(&probe_url)
            .timeout(Duration::from_secs(8))
            .set("Authorization", &format!("Bearer {}", token_trimmed))
            .call();
        let latency_ms = started.elapsed().as_millis() as u64;
        Ok(match result {
            Ok(_resp) => RemoteProbeResult {
                ok: true,
                status: 200,
                latency_ms,
                category: "success".to_string(),
                message: "Reachable. Authentication accepted end-to-end.".to_string(),
            },
            Err(ureq::Error::Status(status, _)) => {
                let (category, message) = match status {
                    401 => (
                        "auth_failed",
                        "Reached the destination, but the token was rejected. Mint a new token in API Tokens above and try again.",
                    ),
                    403 => (
                        "auth_failed",
                        "Reached the destination, but access was forbidden. Verify the URL points at your Trellis instance.",
                    ),
                    404 => (
                        "not_trellis",
                        "Reached an HTTP server, but `/api/devices` returned 404. Make sure the URL points at Trellis on port 9090.",
                    ),
                    502..=504 => (
                        "tunnel_down",
                        "Tunnel responded but could not reach Trellis. Make sure the desktop app is running and the tunnel forwards to localhost:9090.",
                    ),
                    _ => ("unexpected", "Unexpected HTTP status from the destination."),
                };
                RemoteProbeResult {
                    ok: false,
                    status,
                    latency_ms,
                    category: category.to_string(),
                    message: format!("HTTP {} — {}", status, message),
                }
            }
            Err(ureq::Error::Transport(t)) => {
                // Network-level error. ureq's ErrorKind enum discrimination
                // has varied across patch releases, so we classify by
                // formatted-string substring matching — slightly less
                // precise but immune to upstream patch-version churn.
                let raw = format!("{}", t);
                let lower = raw.to_lowercase();
                let (category, message) = if lower.contains("dns") || lower.contains("resolve") {
                    ("network_error", "DNS lookup failed — check the hostname.")
                } else if lower.contains("timed out") || lower.contains("timeout") {
                    ("timeout", "Connection timed out — the tunnel may be slow or unreachable.")
                } else if lower.contains("refused") {
                    ("network_error", "Connection refused — is the tunnel running and forwarding to :9090?")
                } else if lower.contains("tls") || lower.contains("certificate") || lower.contains("handshake") {
                    ("network_error", "TLS handshake failed — check the URL is HTTPS and the cert is valid.")
                } else {
                    ("network_error", "Network error — check the URL and that the tunnel is running.")
                };
                RemoteProbeResult {
                    ok: false,
                    status: 0,
                    latency_ms,
                    category: category.to_string(),
                    message: format!("{} ({})", message, raw),
                }
            }
        })
    })
    .await
    .map_err(|e| format!("Probe task failed: {}", e))?
}

#[tauri::command]
pub fn test_ntfy(topic: String) -> Result<(), String> {
    let url = format!("https://ntfy.sh/{}", topic);
    let body = serde_json::json!({
        "topic": topic,
        "title": "Trellis Test",
        "message": "Push notifications are working!",
        "priority": 3
    });
    ureq::post(&url)
        .set("Content-Type", "application/json")
        .send_string(&body.to_string())
        .map_err(|e| format!("ntfy test failed: {}", e))?;
    Ok(())
}

// ─── Terminal ───────────────────────────────────────────────────────────────

#[tauri::command]
pub async fn run_terminal_command(command: String) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let output = std::process::Command::new("bash")
            .arg("-c")
            .arg(&command)
            .output()
            .map_err(|e| format!("Failed to run command: {}", e))?;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        if !stderr.is_empty() && stdout.is_empty() {
            Ok(stderr)
        } else if !stderr.is_empty() {
            Ok(format!("{}\n{}", stdout, stderr))
        } else {
            Ok(stdout)
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ─── Quick Flash (arduino-cli integration) ─────────────────────────────────

#[tauri::command]
pub fn check_arduino_cli() -> Result<String, String> {
    let output = std::process::Command::new("arduino-cli")
        .arg("version")
        .output()
        .map_err(|_| "arduino-cli not found. Install it from https://arduino.github.io/arduino-cli/".to_string())?;
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[tauri::command]
pub async fn compile_sketch(
    app_handle: AppHandle,
    sketch: String,
    board: String,
) -> Result<String, String> {
    let app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        // Map board selection to FQBN
        let fqbn = match board.as_str() {
            "esp32" => "esp32:esp32:esp32",
            "picow" => "rp2040:rp2040:rpipicow",
            _ => return Err(format!("Unknown board: {}", board)),
        };

        // Create temp sketch directory (Arduino requires sketch_name/sketch_name.ino)
        let sketch_dir = app.path().app_data_dir()
            .map_err(|e| format!("No app dir: {}", e))?
            .join("quick_flash");
        let _ = std::fs::remove_dir_all(&sketch_dir); // Clean previous
        std::fs::create_dir_all(&sketch_dir)
            .map_err(|e| format!("Failed to create sketch dir: {}", e))?;

        let sketch_file = sketch_dir.join("quick_flash.ino");
        std::fs::write(&sketch_file, &sketch)
            .map_err(|e| format!("Failed to write sketch: {}", e))?;

        // Run arduino-cli compile
        let output = std::process::Command::new("arduino-cli")
            .args(["compile", "--fqbn", fqbn])
            .arg(&sketch_dir)
            .output()
            .map_err(|e| format!("Failed to run arduino-cli: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            let mut result = String::new();
            if !stdout.is_empty() { result.push_str(&stdout); }
            if !stderr.is_empty() {
                if !result.is_empty() { result.push('\n'); }
                result.push_str(&stderr);
            }
            Ok(result)
        } else {
            Err(format!("{}\n{}", stdout, stderr).trim().to_string())
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn check_arduino_deps(board: String) -> Result<serde_json::Value, String> {
    // Check if the required board core is installed
    let fqbn = match board.as_str() {
        "esp32" => "esp32:esp32:esp32",
        "picow" => "rp2040:rp2040:rpipicow",
        _ => return Err(format!("Unknown board: {}", board)),
    };

    let core_name = match board.as_str() {
        "esp32" => "esp32:esp32",
        "picow" => "rp2040:rp2040",
        _ => "",
    };

    // Check board core
    let core_output = std::process::Command::new("arduino-cli")
        .args(["core", "list"])
        .output()
        .map_err(|e| format!("Failed to check cores: {}", e))?;
    let core_list = String::from_utf8_lossy(&core_output.stdout).to_string();
    let core_installed = core_list.contains(core_name);

    // Check Trellis library
    let lib_output = std::process::Command::new("arduino-cli")
        .args(["lib", "list"])
        .output()
        .map_err(|e| format!("Failed to check libraries: {}", e))?;
    let lib_list = String::from_utf8_lossy(&lib_output.stdout).to_string();
    let trellis_installed = lib_list.contains("Trellis");
    let ardjson_installed = lib_list.contains("ArduinoJson");
    let websockets_installed = lib_list.contains("WebSockets");

    Ok(serde_json::json!({
        "fqbn": fqbn,
        "core_installed": core_installed,
        "core_name": core_name,
        "trellis_installed": trellis_installed,
        "arduinojson_installed": ardjson_installed,
        "websockets_installed": websockets_installed,
    }))
}

#[tauri::command]
pub async fn install_arduino_deps(deps: Vec<String>) -> Result<String, String> {
    tokio::task::spawn_blocking(move || {
        let mut results = Vec::new();
        for dep in &deps {
            let (cmd_type, name) = if dep.contains(':') {
                ("core", dep.as_str())
            } else {
                ("lib", dep.as_str())
            };

            let output = std::process::Command::new("arduino-cli")
                .args([cmd_type, "install", name])
                .output()
                .map_err(|e| format!("Failed to install {}: {}", name, e))?;

            let out = String::from_utf8_lossy(&output.stdout).to_string();
            let err = String::from_utf8_lossy(&output.stderr).to_string();

            if output.status.success() {
                results.push(format!("Installed {}", name));
            } else {
                return Err(format!("Failed to install {}: {}{}", name, out, err));
            }
        }
        Ok(results.join("\n"))
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub async fn flash_sketch(
    app_handle: AppHandle,
    board: String,
    port: String,
) -> Result<String, String> {
    let app = app_handle.clone();
    tokio::task::spawn_blocking(move || {
        let fqbn = match board.as_str() {
            "esp32" => "esp32:esp32:esp32",
            "picow" => "rp2040:rp2040:rpipicow",
            _ => return Err(format!("Unknown board: {}", board)),
        };

        let sketch_dir = app.path().app_data_dir()
            .map_err(|e| format!("No app dir: {}", e))?
            .join("quick_flash");

        if !sketch_dir.join("quick_flash.ino").exists() {
            return Err("No compiled sketch found. Compile first.".to_string());
        }

        // Run arduino-cli upload
        let output = std::process::Command::new("arduino-cli")
            .args(["upload", "--fqbn", fqbn, "--port", &port])
            .arg(&sketch_dir)
            .output()
            .map_err(|e| format!("Failed to run arduino-cli: {}", e))?;

        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();

        if output.status.success() {
            let mut result = String::from("Upload complete!");
            if !stdout.is_empty() { result.push('\n'); result.push_str(&stdout); }
            if !stderr.is_empty() { result.push('\n'); result.push_str(&stderr); }
            Ok(result)
        } else {
            Err(format!("{}\n{}", stdout, stderr).trim().to_string())
        }
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))?
}

// ─── Scenes ─────────────────────────────────────────────────────────────────

#[tauri::command]
pub fn create_scene(
    db: State<'_, Database>, name: String, actions: Vec<SceneActionInput>,
) -> Result<i64, String> {
    if name.trim().is_empty() {
        return Err("Scene name cannot be empty".to_string());
    }
    if actions.is_empty() {
        return Err("Scene must have at least one action".to_string());
    }
    db.create_scene(&name, &actions)
}

#[tauri::command]
pub fn get_scenes(db: State<'_, Database>) -> Result<Vec<Scene>, String> {
    db.get_scenes()
}

#[tauri::command]
pub fn update_scene(
    db: State<'_, Database>, id: i64, name: String, actions: Vec<SceneActionInput>,
) -> Result<(), String> {
    if name.trim().is_empty() {
        return Err("Scene name cannot be empty".to_string());
    }
    if actions.is_empty() {
        return Err("Scene must have at least one action".to_string());
    }
    db.update_scene(id, &name, &actions)
}

#[tauri::command]
pub fn delete_scene(db: State<'_, Database>, id: i64) -> Result<(), String> {
    db.delete_scene(id)
}

#[tauri::command]
pub async fn run_scene(
    state: State<'_, AppState>, db: State<'_, Database>, id: i64,
) -> Result<(), String> {
    let scene = db.get_scene(id)?
        .ok_or_else(|| format!("Scene {} not found", id))?;
    let conn_mgr = state.connection_manager.clone();

    for action in &scene.actions {
        // Look up device IP/port from the saved device record
        let saved = db.get_saved_device(&action.device_id)?;
        let (ip, port) = match saved {
            Some(d) => (d.ip, d.port),
            None => {
                log::warn!("[Scene] Device {} not found, skipping", action.device_id);
                continue;
            }
        };

        let value: serde_json::Value = if action.value == "true" {
            serde_json::Value::Bool(true)
        } else if action.value == "false" {
            serde_json::Value::Bool(false)
        } else if let Ok(n) = action.value.parse::<f64>() {
            serde_json::json!(n)
        } else {
            serde_json::Value::String(action.value.clone())
        };

        let cmd = serde_json::json!({
            "command": "set",
            "id": action.capability_id,
            "value": value
        });
        let msg = serde_json::to_string(&cmd).map_err(|e| e.to_string())?;
        let ws_port = port + 1;

        if let Err(e) = conn_mgr.send_to_device(&action.device_id, &ip, ws_port, &msg) {
            log::warn!("[Scene] Failed to send to {}: {}", action.device_id, e);
        }
    }
    Ok(())
}
