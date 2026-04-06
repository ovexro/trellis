mod commands;
mod connection;
mod db;
mod device;
mod discovery;
mod ota;
mod serial;

use commands::*;
use connection::ConnectionManager;
use discovery::Discovery;
use serial::SerialManager;
use std::sync::Arc;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let connection_manager = Arc::new(ConnectionManager::new());
    let discovery = Arc::new(Discovery::new(connection_manager.clone()));
    let serial_manager = Arc::new(SerialManager::new());

    let app_state = AppState {
        discovery: discovery.clone(),
        connection_manager: connection_manager.clone(),
        serial_manager,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_devices,
            add_device_by_ip,
            send_command,
            list_serial_ports,
            open_serial,
            close_serial,
            send_serial,
            start_ota,
            store_metric,
            get_metrics,
        ])
        .setup(move |app| {
            db::init_db(app.handle())?;

            // Set app handle for connection manager (needed for emitting events)
            connection_manager.set_app_handle(app.handle().clone());

            // Start continuous background discovery
            discovery.start_background(app.handle().clone());

            log::info!("[Trellis] Background discovery started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Trellis");
}
