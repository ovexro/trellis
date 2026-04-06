mod commands;
mod db;
mod device;
mod discovery;
mod serial;

use commands::*;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            scan_devices,
            get_devices,
            send_command,
            list_serial_ports,
            open_serial,
            close_serial,
            send_serial,
            start_ota,
        ])
        .setup(|app| {
            db::init_db(app.handle())?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Trellis");
}
