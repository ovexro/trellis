mod alerts;
mod api;
mod auth;
mod commands;
mod connection;
mod db;
mod device;
mod diagnostics;
mod discovery;
mod lib_manifest;
mod marketplace;
mod marketplace_remote;
mod mqtt;
mod ota;
mod scheduler;
mod secret_store;
mod serial;
mod sinric;
mod sketch_gen;
mod webhooks;

use commands::*;
use connection::ConnectionManager;
use discovery::Discovery;
use mqtt::MqttBridge;
use secret_store::SecretStore;
use serial::SerialManager;
use sinric::SinricBridge;
use std::sync::Arc;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::Manager;

/// Initialize `env_logger`. When `TRELLIS_LOG` is set, logs go to a file
/// (append mode) instead of stderr — useful when Trellis is launched from
/// the tray and stderr is not reachable.
///
/// - `TRELLIS_LOG` unset / empty → stderr (default `env_logger` behavior)
/// - `TRELLIS_LOG=1`             → `$HOME/.config/trellis/trellis.log`
/// - `TRELLIS_LOG=<path>`        → that literal path
///
/// If `RUST_LOG` is unset and `TRELLIS_LOG` is active, level defaults to
/// `info` so the file is useful without extra configuration.
fn init_logging() {
    let mut builder = env_logger::Builder::from_default_env();

    let trellis_log = std::env::var("TRELLIS_LOG").ok().filter(|v| !v.is_empty());
    let Some(val) = trellis_log else {
        builder.init();
        return;
    };

    if std::env::var_os("RUST_LOG").is_none() {
        builder.filter_level(log::LevelFilter::Info);
    }

    let path: std::path::PathBuf = if val == "1" {
        match std::env::var("HOME") {
            Ok(home) => std::path::PathBuf::from(home).join(".config/trellis/trellis.log"),
            Err(_) => {
                eprintln!("TRELLIS_LOG=1 requested but $HOME is unset; logging to stderr");
                builder.init();
                return;
            }
        }
    } else {
        std::path::PathBuf::from(val)
    };

    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!(
                    "TRELLIS_LOG: failed to create {}: {}; logging to stderr",
                    parent.display(),
                    e
                );
                builder.init();
                return;
            }
        }
    }

    match std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        Ok(file) => {
            builder.target(env_logger::Target::Pipe(Box::new(file)));
            builder.init();
            eprintln!("Trellis logging to {}", path.display());
        }
        Err(e) => {
            eprintln!(
                "TRELLIS_LOG: failed to open {}: {}; logging to stderr",
                path.display(),
                e
            );
            builder.init();
        }
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_logging();

    let connection_manager = Arc::new(ConnectionManager::new());
    let mqtt_bridge = Arc::new(MqttBridge::new(connection_manager.clone()));
    let sinric_bridge = Arc::new(SinricBridge::new(connection_manager.clone()));
    let discovery = Arc::new(Discovery::new(connection_manager.clone()));
    let serial_manager = Arc::new(SerialManager::new());
    let ota_registry = Arc::new(ota::OtaRegistry::new());

    // Wire bridges into ConnectionManager so device-event updates are
    // mirrored to MQTT and Sinric in real time, and into Discovery so HA
    // discovery configs are republished when devices appear or change.
    let ws_broadcaster = Arc::new(api::WsBroadcaster::new());

    connection_manager.set_mqtt_bridge(mqtt_bridge.clone());
    connection_manager.set_sinric_bridge(sinric_bridge.clone());
    connection_manager.set_ws_broadcaster(ws_broadcaster.clone());
    discovery.set_mqtt_bridge(mqtt_bridge.clone());
    discovery.set_sinric_bridge(sinric_bridge.clone());
    discovery.set_ws_broadcaster(ws_broadcaster.clone());
    mqtt_bridge.set_discovery(discovery.clone());
    sinric_bridge.set_discovery(discovery.clone());

    let app_state = AppState {
        discovery: discovery.clone(),
        connection_manager: connection_manager.clone(),
        serial_manager,
        mqtt_bridge: mqtt_bridge.clone(),
        sinric_bridge: sinric_bridge.clone(),
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .manage(app_state)
        .manage(ota_registry.clone())
        .invoke_handler(tauri::generate_handler![
            get_devices,
            add_device_by_ip,
            send_command,
            set_device_nickname,
            set_device_tags,
            set_device_notes,
            set_device_install_date,
            set_capability_watts,
            set_capability_linear_power,
            set_capability_binary_sensor,
            set_capability_cover,
            set_capability_brightness_link,
            get_device_capability_meta,
            get_device_energy,
            get_device_lifetime_energy,
            get_saved_devices,
            get_saved_device,
            list_serial_ports,
            open_serial,
            close_serial,
            send_serial,
            start_ota,
            cancel_ota,
            store_metric,
            get_metrics,
            get_device_annotations,
            get_recent_activity,
            create_alert,
            get_alerts,
            delete_alert,
            toggle_alert,
            get_device_logs,
            diagnose_device,
            diagnose_fleet,
            set_device_github_repo,
            store_log_entry,
            remove_device,
            create_schedule,
            get_schedules,
            delete_schedule,
            toggle_schedule,
            run_schedule,
            duplicate_schedule,
            create_rule,
            get_rules,
            delete_rule,
            toggle_rule,
            run_rule,
            duplicate_rule,
            create_webhook,
            get_webhooks,
            delete_webhook,
            toggle_webhook,
            duplicate_webhook,
            log_webhook_delivery,
            get_webhook_deliveries,
            create_template,
            get_templates,
            delete_template,
            create_group,
            get_groups,
            update_group,
            delete_group,
            set_device_group,
            set_device_favorite,
            toggle_favorite_capability,
            get_favorite_capabilities,
            get_floor_plans,
            create_floor_plan,
            update_floor_plan,
            delete_floor_plan,
            get_device_positions,
            get_all_device_positions,
            set_device_position,
            remove_device_position,
            get_rooms,
            get_all_rooms,
            create_room,
            update_room,
            delete_room,
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
            generate_sketch_command,
            get_sketch_lib_info_command,
            get_marketplace_templates_command,
            get_marketplace_remote_command,
            refresh_marketplace_remote_command,
            get_mqtt_config,
            set_mqtt_config,
            clear_mqtt_password,
            get_mqtt_status,
            test_mqtt_connection,
            list_api_tokens,
            create_api_token,
            revoke_api_token,
            probe_remote_url,
            reorder_devices,
            check_github_releases,
            start_github_ota,
            get_sinric_config,
            set_sinric_config,
            clear_sinric_secret,
            get_sinric_status,
            test_sinric_connection,
            create_scene,
            get_scenes,
            update_scene,
            delete_scene,
            run_scene,
            duplicate_scene,
        ])
        .setup(move |app| {
            db::init_db(app.handle())?;

            // Initialize the at-rest secret store. Bootstraps an x25519
            // identity in the OS keyring (or a 0600 file fallback) and
            // registers it as a Tauri-managed state so commands can encrypt
            // and decrypt stored secrets without re-loading the key on every
            // call. Fails fast if BOTH backends are unwritable — this is
            // extremely rare (would require keyring + filesystem both
            // broken) and continuing without encryption would leave the
            // user with a half-broken app that can't decrypt previously-
            // saved secrets. Better to surface the error.
            let app_data_dir = app
                .path()
                .app_data_dir()
                .expect("failed to get app data dir");
            let secret_store = SecretStore::load_or_create(&app_data_dir)
                .map(Arc::new)
                .expect("Failed to initialize SecretStore — both OS keyring and file fallback are unavailable");
            app.manage(secret_store.clone());

            // Set app handle for connection manager and Sinric bridge
            connection_manager.set_app_handle(app.handle().clone());
            sinric_bridge.set_app_handle(app.handle().clone());
            mqtt_bridge.set_app_handle(app.handle().clone());

            // Hydrate saved devices from SQLite into the in-memory map BEFORE
            // starting background discovery. This makes saved devices visible
            // to every consumer (desktop UI, REST API, web dashboard, MQTT
            // bridge) immediately on launch as offline placeholders. The
            // health check loop's first probe will flip them online if
            // reachable.
            discovery.hydrate_from_db(app.handle());

            // Start continuous background discovery
            discovery.start_background(app.handle().clone());

            // Schedule periodic metrics cleanup (configurable retention period)
            let cleanup_handle = app.handle().clone();
            std::thread::spawn(move || {
                loop {
                    std::thread::sleep(std::time::Duration::from_secs(3600));
                    if let Some(db) = cleanup_handle.try_state::<db::Database>() {
                        let days = db
                            .get_setting("data_retention_days")
                            .ok()
                            .flatten()
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(30);
                        if days > 0 {
                            let _ = db.cleanup_old_metrics(days);
                            let _ = db.cleanup_old_logs(days);
                        }
                        let webhook_days = db
                            .get_setting("webhook_delivery_retention_days")
                            .ok()
                            .flatten()
                            .and_then(|v| v.parse::<u32>().ok())
                            .unwrap_or(30);
                        if webhook_days > 0 {
                            let _ = db.cleanup_old_webhook_deliveries(webhook_days);
                        }
                    }
                }
            });

            // Periodic `_energy/state` refresh for every metered capability.
            // On every transition, `connection.rs` publishes fresh Wh. But
            // long steady-state intervals (e.g. a lamp left ON for hours)
            // produce no transitions, so HA's Energy dashboard would see a
            // flat counter across hourly buckets. Re-publishing every 60s
            // keeps `total_increasing` integrating correctly between
            // transitions. The tick is a no-op when the bridge is disabled
            // (publish_energy early-returns on cfg.enabled=false).
            let energy_handle = app.handle().clone();
            let energy_bridge = mqtt_bridge.clone();
            std::thread::spawn(move || loop {
                std::thread::sleep(std::time::Duration::from_secs(60));
                let Some(db) = energy_handle.try_state::<db::Database>() else {
                    continue;
                };
                for (device_id, cap_id) in energy_bridge.metered_capabilities() {
                    if let Ok(wh) =
                        db.get_capability_lifetime_wh(&device_id, &cap_id)
                    {
                        energy_bridge.publish_energy(&device_id, &cap_id, wh);
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
            api::start_api_server(
                db_path,
                discovery.clone(),
                connection_manager.clone(),
                mqtt_bridge.clone(),
                sinric_bridge.clone(),
                secret_store.clone(),
                ws_broadcaster.clone(),
                app.handle().clone(),
            );

            // Restore saved MQTT bridge config and start it if it was
            // enabled. The on-disk password may be:
            //   1. enc:v1: encrypted (current format) — decrypt then apply
            //   2. plaintext (legacy from pre-encryption builds) — apply
            //      as-is, then re-save so the next read is encrypted
            //   3. empty (bridge runs without auth) — apply as-is
            // Migration is lazy here: any plaintext password gets upgraded
            // to enc:v1: on the very first launch of this build, no user
            // action required.

            // Hydrate the MQTT bridge's nameplate-watts cache from the
            // `capability_meta` table BEFORE restoring the saved config —
            // `apply_config` publishes HA discovery as soon as the bridge
            // connects, and that publish needs the cache populated to emit
            // the per-switch `sensor.<cap>_power` entities on first boot.
            if let Some(db_state) = app.try_state::<db::Database>() {
                if let Ok(entries) = db_state.get_all_capability_meters() {
                    mqtt_bridge.hydrate_meters(entries);
                }
                if let Ok(entries) = db_state.get_all_binary_sensors() {
                    mqtt_bridge.hydrate_binary_sensors(entries);
                }
                if let Ok(entries) = db_state.get_all_covers() {
                    mqtt_bridge.hydrate_covers(entries);
                }
                if let Ok(entries) = db_state.get_all_brightness_links() {
                    mqtt_bridge.hydrate_brightness_links(entries);
                }
            }

            if let Some(db_state) = app.try_state::<db::Database>() {
                if let Ok(Some(json)) = db_state.get_setting("mqtt_config") {
                    match serde_json::from_str::<mqtt::MqttConfig>(&json) {
                        Ok(mut cfg) => {
                            let was_legacy_plaintext = !cfg.password.is_empty()
                                && !secret_store::is_encrypted(&cfg.password);
                            if let Err(e) = secret_store::decrypt_mqtt_password(
                                secret_store.as_ref(),
                                &mut cfg,
                            ) {
                                log::warn!("[MQTT] Failed to decrypt stored password: {}", e);
                            }
                            if let Err(e) = mqtt_bridge.apply_config(cfg.clone()) {
                                log::warn!("[MQTT] Failed to start bridge from saved config: {}", e);
                            }
                            // Lazy migration: re-save so the on-disk blob is
                            // encrypted from now on. Only fires once per
                            // legacy install.
                            if was_legacy_plaintext {
                                let mut to_save = cfg;
                                if let Err(e) = secret_store::encrypt_mqtt_password(
                                    secret_store.as_ref(),
                                    &mut to_save,
                                ) {
                                    log::warn!("[MQTT] Migration encrypt failed: {}", e);
                                } else if let Ok(json) = serde_json::to_string(&to_save) {
                                    if let Err(e) = db_state.set_setting("mqtt_config", &json) {
                                        log::warn!("[MQTT] Migration save failed: {}", e);
                                    } else {
                                        log::info!("[MQTT] Migrated legacy plaintext password to enc:v1");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("[MQTT] Failed to parse saved config JSON: {}", e);
                        }
                    }
                }
            }

            // Restore saved Sinric Pro bridge config (same pattern as MQTT above)
            if let Some(db_state) = app.try_state::<db::Database>() {
                if let Ok(Some(json)) = db_state.get_setting("sinric_config") {
                    match serde_json::from_str::<sinric::SinricConfig>(&json) {
                        Ok(mut cfg) => {
                            let was_legacy_plaintext = !cfg.api_secret.is_empty()
                                && !secret_store::is_encrypted(&cfg.api_secret);
                            if let Err(e) = secret_store::decrypt_sinric_secret(
                                secret_store.as_ref(),
                                &mut cfg,
                            ) {
                                log::warn!("[Sinric] Failed to decrypt stored secret: {}", e);
                            }
                            if let Err(e) = sinric_bridge.apply_config(cfg.clone()) {
                                log::warn!("[Sinric] Failed to start bridge from saved config: {}", e);
                            }
                            if was_legacy_plaintext {
                                let mut to_save = cfg;
                                if let Err(e) = secret_store::encrypt_sinric_secret(
                                    secret_store.as_ref(),
                                    &mut to_save,
                                ) {
                                    log::warn!("[Sinric] Migration encrypt failed: {}", e);
                                } else if let Ok(json) = serde_json::to_string(&to_save) {
                                    if let Err(e) = db_state.set_setting("sinric_config", &json) {
                                        log::warn!("[Sinric] Migration save failed: {}", e);
                                    } else {
                                        log::info!("[Sinric] Migrated legacy plaintext secret to enc:v1");
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("[Sinric] Failed to parse saved config JSON: {}", e);
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
