use crate::device::Device;
use crate::discovery::Discovery;
use crate::serial;
use serde_json::Value;
use std::sync::Arc;
use tauri::State;

pub struct AppState {
    pub discovery: Arc<Discovery>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            discovery: Arc::new(Discovery::new()),
        }
    }
}

#[tauri::command]
pub async fn scan_devices(state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    let discovery = state.discovery.clone();
    tokio::task::spawn_blocking(move || discovery.scan())
        .await
        .map_err(|e| format!("Scan task failed: {}", e))
}

#[tauri::command]
pub fn get_devices(state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    Ok(state.discovery.get_devices())
}

#[tauri::command]
pub async fn send_command(ip: String, port: u16, command: Value) -> Result<(), String> {
    let ws_port = port + 1;
    let url = format!("ws://{}:{}", ip, ws_port);
    let msg = serde_json::to_string(&command).map_err(|e| e.to_string())?;

    tokio::task::spawn_blocking(move || {
        let (mut socket, _) =
            tungstenite::connect(&url).map_err(|e| format!("WebSocket connect: {}", e))?;
        socket
            .send(tungstenite::Message::Text(msg))
            .map_err(|e| format!("WebSocket send: {}", e))?;
        socket
            .close(None)
            .map_err(|e| format!("WebSocket close: {}", e))?;
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| format!("Task failed: {}", e))??;

    Ok(())
}

#[tauri::command]
pub async fn add_device_by_ip(
    state: State<'_, AppState>,
    ip: String,
    port: u16,
) -> Result<Device, String> {
    let discovery = state.discovery.clone();
    tokio::task::spawn_blocking(move || discovery.add_by_ip(&ip, port))
        .await
        .map_err(|e| format!("Task failed: {}", e))?
}

#[tauri::command]
pub fn list_serial_ports() -> Result<Vec<serial::SerialPortInfo>, String> {
    Ok(serial::list_ports())
}

#[tauri::command]
pub fn open_serial(_port: String, _baud: u32) -> Result<(), String> {
    // TODO: open serial port and start reading in background
    Ok(())
}

#[tauri::command]
pub fn close_serial(_port: String) -> Result<(), String> {
    // TODO: close serial port
    Ok(())
}

#[tauri::command]
pub fn send_serial(_port: String, _data: String) -> Result<(), String> {
    // TODO: write data to open serial port
    Ok(())
}

#[tauri::command]
pub async fn start_ota(_ip: String, _port: u16, _firmware_path: String) -> Result<(), String> {
    // TODO: serve firmware file via local HTTP and send OTA command to device
    Ok(())
}
