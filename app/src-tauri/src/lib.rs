mod commands;
mod connection;
mod db;
mod device;
mod discovery;
mod ota;
mod scheduler;
mod serial;

use commands::*;
use connection::ConnectionManager;
use discovery::Discovery;
use serial::SerialManager;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
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
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(app_state)
        .invoke_handler(tauri::generate_handler![
            get_devices,
            add_device_by_ip,
            send_command,
            set_device_nickname,
            set_device_tags,
            get_saved_devices,
            get_saved_device,
            list_serial_ports,
            open_serial,
            close_serial,
            send_serial,
            start_ota,
            store_metric,
            get_metrics,
            create_alert,
            get_alerts,
            delete_alert,
            toggle_alert,
            get_device_logs,
            store_log_entry,
            remove_device,
            create_schedule,
            get_schedules,
            delete_schedule,
            toggle_schedule,
            create_rule,
            get_rules,
            delete_rule,
            toggle_rule,
            create_webhook,
            get_webhooks,
            delete_webhook,
            toggle_webhook,
            create_template,
            get_templates,
            delete_template,
            export_metrics_csv,
            run_terminal_command,
        ])
        .setup(move |app| {
            db::init_db(app.handle())?;

            // Set app handle for connection manager
            connection_manager.set_app_handle(app.handle().clone());

            // Start continuous background discovery
            discovery.start_background(app.handle().clone());

            // Schedule periodic metrics cleanup (delete data older than 30 days)
            let cleanup_handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                    if let Some(db) = cleanup_handle.try_state::<db::Database>() {
                        let _ = db.cleanup_old_metrics(30);
                    }
                }
            });

            // System tray with right-click menu
            let show_item = MenuItem::with_id(app, "show", "Show Trellis", true, None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let tray_menu = Menu::with_items(app, &[&show_item, &quit_item])?;

            let _tray = TrayIconBuilder::new()
                .icon(app.default_window_icon().cloned().unwrap_or_else(|| {
                    tauri::image::Image::new(&[0u8; 4], 1, 1)
                }))
                .tooltip("Trellis — Device Control Center")
                .menu(&tray_menu)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "quit" => {
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click { .. } = event {
                        if let Some(window) = tray.app_handle().get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;

            // Intercept window close → minimize to tray
            if let Some(window) = app.get_webview_window("main") {
                let w = window.clone();
                window.on_window_event(move |event| {
                    if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                        api.prevent_close();
                        let _ = w.hide();
                    }
                });
            }

            // Start schedule execution engine
            scheduler::start_scheduler(app.handle().clone(), connection_manager.clone());

            log::info!("[Trellis] Background discovery + scheduler started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Trellis");
}
