use crate::device::Device;
use crate::discovery::Discovery;
use crate::serial;
use serde_json::Value;
use std::sync::Mutex;
use tauri::State;

pub struct AppState {
    pub discovery: Mutex<Discovery>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            discovery: Mutex::new(Discovery::new()),
        }
    }
}

#[tauri::command]
pub async fn scan_devices(state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    let discovery = state.discovery.lock().map_err(|e| e.to_string())?;
    Ok(discovery.scan().await)
}

#[tauri::command]
pub fn get_devices(state: State<'_, AppState>) -> Result<Vec<Device>, String> {
    let discovery = state.discovery.lock().map_err(|e| e.to_string())?;
    Ok(discovery.get_devices())
}

#[tauri::command]
pub async fn send_command(ip: String, port: u16, command: Value) -> Result<(), String> {
    let url = format!("ws://{}:{}/ws", ip, port);
    let (mut socket, _) =
        tungstenite::connect(&url).map_err(|e| format!("WebSocket connect error: {}", e))?;
    let msg = serde_json::to_string(&command).map_err(|e| e.to_string())?;
    socket
        .send(tungstenite::Message::Text(msg))
        .map_err(|e| format!("WebSocket send error: {}", e))?;
    socket
        .close(None)
        .map_err(|e| format!("WebSocket close error: {}", e))?;
    Ok(())
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
