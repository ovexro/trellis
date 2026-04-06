use crate::connection::ConnectionManager;
use crate::db::{AlertRule, Database, LogEntry, MetricPoint, SavedDevice};
use crate::device::Device;
use crate::discovery::Discovery;
use crate::ota;
use crate::serial::{SerialManager, SerialPortInfo};
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, State};

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
    device_id: String,
    ip: String,
    port: u16,
    firmware_path: String,
) -> Result<(), String> {
    let conn_mgr = state.connection_manager.clone();
    let ws_port = port + 1;
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
