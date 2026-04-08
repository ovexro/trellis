mod api;
mod commands;
mod connection;
mod db;
mod device;
mod discovery;
mod mqtt;
mod ota;
mod scheduler;
mod serial;

use commands::*;
use connection::ConnectionManager;
use discovery::Discovery;
use mqtt::MqttBridge;
use serial::SerialManager;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    env_logger::init();

    let connection_manager = Arc::new(ConnectionManager::new());
    let mqtt_bridge = Arc::new(MqttBridge::new(connection_manager.clone()));
    let discovery = Arc::new(Discovery::new(connection_manager.clone()));
    let serial_manager = Arc::new(SerialManager::new());

    // Wire the bridge into ConnectionManager so device-event updates are
    // mirrored to MQTT in real time, and into Discovery so HA discovery
    // configs are republished when devices appear or change. Also give the
    // bridge a back-reference to Discovery so polish #1 (instant discovery
    // on enable) and polish #2 (republish on broker reconnect) can read the
    // current device list.
    connection_manager.set_mqtt_bridge(mqtt_bridge.clone());
    discovery.set_mqtt_bridge(mqtt_bridge.clone());
    mqtt_bridge.set_discovery(discovery.clone());

    let app_state = AppState {
        discovery: discovery.clone(),
        connection_manager: connection_manager.clone(),
        serial_manager,
        mqtt_bridge: mqtt_bridge.clone(),
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
            create_group,
            get_groups,
            update_group,
            delete_group,
            set_device_group,
            export_metrics_csv,
            get_setting,
            set_setting,
            delete_setting,
            get_firmware_history,
            delete_firmware_record,
            rollback_firmware,
            send_ntfy,
            test_ntfy,
            run_terminal_command,
            check_arduino_cli,
            check_arduino_deps,
            install_arduino_deps,
            compile_sketch,
            flash_sketch,
            get_mqtt_config,
            set_mqtt_config,
            get_mqtt_status,
            test_mqtt_connection,
        ])
        .setup(move |app| {
            db::init_db(app.handle())?;

            // Set app handle for connection manager
            connection_manager.set_app_handle(app.handle().clone());

            // Hydrate saved devices from SQLite into the in-memory map BEFORE
            // starting background discovery. This makes saved devices visible
            // to every consumer (desktop UI, REST API, web dashboard, MQTT
            // bridge) immediately on launch as offline placeholders. The
            // health check loop's first probe will flip them online if
            // reachable.
            discovery.hydrate_from_db(app.handle());

            // Start continuous background discovery
            discovery.start_background(app.handle().clone());

            // Schedule periodic metrics cleanup (delete data older than 30 days)
            let cleanup_handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                    if let Some(db) = cleanup_handle.try_state::<db::Database>() {
                        let _ = db.cleanup_old_metrics(30);
                        let _ = db.cleanup_old_logs(30);
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

            // Restore saved window state (size + position)
            if let Some(window) = app.get_webview_window("main") {
                if let Some(db_state) = app.try_state::<db::Database>() {
                    if let Ok(Some(state_json)) = db_state.get_setting("window_state") {
                        if let Ok(state) = serde_json::from_str::<serde_json::Value>(&state_json) {
                            if let (Some(w), Some(h)) = (
                                state.get("width").and_then(|v| v.as_f64()),
                                state.get("height").and_then(|v| v.as_f64()),
                            ) {
                                let _ = window.set_size(tauri::LogicalSize::new(w, h));
                            }
                            if let (Some(x), Some(y)) = (
                                state.get("x").and_then(|v| v.as_f64()),
                                state.get("y").and_then(|v| v.as_f64()),
                            ) {
                                let _ = window.set_position(tauri::LogicalPosition::new(x, y));
                            }
                            if state.get("maximized").and_then(|v| v.as_bool()).unwrap_or(false) {
                                let _ = window.maximize();
                            }
                        }
                    }
                }

                // Save window state on resize/move/close, and intercept close → minimize to tray
                let w = window.clone();
                let save_handle = app.handle().clone();
                window.on_window_event(move |event| {
                    match event {
                        tauri::WindowEvent::CloseRequested { api, .. } => {
                            api.prevent_close();
                            let _ = w.hide();
                        }
                        tauri::WindowEvent::Resized(_) | tauri::WindowEvent::Moved(_) => {
                            // Save current window state
                            if let (Ok(size), Ok(pos), Ok(maximized)) = (
                                w.outer_size(),
                                w.outer_position(),
                                w.is_maximized(),
                            ) {
                                if let Ok(scale) = w.scale_factor() {
                                    let state = serde_json::json!({
                                        "width": size.width as f64 / scale,
                                        "height": size.height as f64 / scale,
                                        "x": pos.x as f64 / scale,
                                        "y": pos.y as f64 / scale,
                                        "maximized": maximized,
                                    });
                                    if let Some(db) = save_handle.try_state::<db::Database>() {
                                        let _ = db.set_setting("window_state", &state.to_string());
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                });
            }

            // Start schedule execution engine
            scheduler::start_scheduler(app.handle().clone(), connection_manager.clone());

            // Start REST API server on port 9090
            let db_path = app.path().app_data_dir()
                .expect("failed to get app data dir")
                .join("trellis.db");
            api::start_api_server(db_path, discovery.clone(), connection_manager.clone(), mqtt_bridge.clone());

            // Restore saved MQTT bridge config and start it if it was enabled
            if let Some(db_state) = app.try_state::<db::Database>() {
                if let Ok(Some(json)) = db_state.get_setting("mqtt_config") {
                    if let Ok(cfg) = serde_json::from_str::<mqtt::MqttConfig>(&json) {
                        if let Err(e) = mqtt_bridge.apply_config(cfg) {
                            log::warn!("[MQTT] Failed to start bridge from saved config: {}", e);
                        }
                    }
                }
            }

            log::info!("[Trellis] Background discovery + scheduler + API server started");
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Trellis");
}
