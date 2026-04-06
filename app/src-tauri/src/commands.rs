use crate::connection::ConnectionManager;
use crate::db::{AlertRule, Database, DeviceGroup, DeviceTemplate, FirmwareRecord, LogEntry, MetricPoint, Rule, SavedDevice, Schedule, Webhook};
use crate::device::Device;
use crate::discovery::Discovery;
use crate::ota;
use crate::serial::{SerialManager, SerialPortInfo};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub discovery: Arc<Discovery>,
    pub connection_manager: Arc<ConnectionManager>,
    pub serial_manager: Arc<SerialManager>,
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

    tokio::task::spawn_blocking(move || {
        let (url, _stop_flag) = ota::serve_firmware(&firmware_path)?;
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
    device_id: String,
    ip: String,
    port: u16,
    firmware_record_path: String,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;
    tokio::task::spawn_blocking(move || {
        let (url, _stop_flag) = ota::serve_firmware(&firmware_record_path)?;
        let ota_cmd = serde_json::json!({"command": "ota", "url": url});
        let msg = serde_json::to_string(&ota_cmd).map_err(|e| e.to_string())?;
        conn_mgr.send_to_device(&device_id, &ip, ws_port, &msg)?;
        log::info!("[OTA] Rollback triggered for device {}", device_id);
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

// ─── Device logs ─────────────────────────────────────────────────────────────

#[tauri::command]
pub fn get_device_logs(
    db: State<'_, Database>,
    device_id: String,
    limit: u32,
) -> Result<Vec<LogEntry>, String> {
    db.get_logs(&device_id, limit)
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
    value: String, cron: String, label: String,
) -> Result<i64, String> {
    db.create_schedule(&device_id, &capability_id, &value, &cron, &label)
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
) -> Result<i64, String> {
    db.create_rule(&source_device_id, &source_metric_id, &condition, threshold,
        &target_device_id, &target_capability_id, &target_value, &label)
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
