use crate::connection::ConnectionManager;
use crate::device::Device;
use crate::discovery::Discovery;
use crate::serial;
use serde_json::Value;
use std::sync::Arc;
use tauri::{AppHandle, Manager, State};

pub struct AppState {
    pub discovery: Arc<Discovery>,
    pub connection_manager: Arc<ConnectionManager>,
}

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

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<serial::SerialPortInfo>, String> {
    Ok(serial::list_ports())
}

#[tauri::command]
pub fn open_serial(
    _app_handle: AppHandle,
    _port: String,
    _baud: u32,
) -> Result<(), String> {
    // TODO: Batch 2
    Ok(())
}

#[tauri::command]
pub fn close_serial(_port: String) -> Result<(), String> {
    // TODO: Batch 2
    Ok(())
}

#[tauri::command]
pub fn send_serial(_port: String, _data: String) -> Result<(), String> {
    // TODO: Batch 2
    Ok(())
}

#[tauri::command]
pub async fn start_ota(
    _ip: String,
    _port: u16,
    _firmware_path: String,
) -> Result<(), String> {
    // TODO: Batch 3
    Ok(())
}
