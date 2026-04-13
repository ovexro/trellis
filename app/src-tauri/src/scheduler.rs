use std::str::FromStr;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use tauri::{AppHandle, Manager};

use crate::connection::ConnectionManager;
use crate::db::Database;

const POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Start the schedule evaluation loop in a background thread.
/// Checks all enabled schedules every 60 seconds, fires matching ones.
pub fn start_scheduler(
    app_handle: AppHandle,
    connection_manager: std::sync::Arc<ConnectionManager>,
) {
    thread::spawn(move || {
        log::info!("[Scheduler] Started, polling every 60s");

        loop {
            thread::sleep(POLL_INTERVAL);
            evaluate_schedules(&app_handle, &connection_manager);
        }
    });
}

fn evaluate_schedules(app_handle: &AppHandle, conn_mgr: &ConnectionManager) {
    let db = match app_handle.try_state::<Database>() {
        Some(db) => db,
        None => return,
    };

    let schedules = match db.get_schedules() {
        Ok(s) => s,
        Err(e) => {
            log::warn!("[Scheduler] Failed to load schedules: {}", e);
            return;
        }
    };

    let now = Utc::now();

    for schedule in schedules {
        if !schedule.enabled {
            continue;
        }

        // Parse cron expression (prepend seconds field since cron crate expects 6-7 fields)
        let cron_expr = format!("0 {}", schedule.cron);
        let cron_schedule = match Schedule::from_str(&cron_expr) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "[Scheduler] Invalid cron '{}' for schedule '{}': {}",
                    schedule.cron,
                    schedule.label,
                    e
                );
                continue;
            }
        };

        // Check if this schedule should have fired in the last poll interval
        let should_fire = if let Some(last_run) = &schedule.last_run {
            // Find next occurrence after last_run
            if let Ok(last) = chrono::NaiveDateTime::parse_from_str(last_run, "%Y-%m-%d %H:%M:%S") {
                let last_utc = last.and_utc();
                if let Some(next) = cron_schedule.after(&last_utc).next() {
                    next <= now
                } else {
                    false
                }
            } else {
                // Can't parse last_run, just check if due now
                cron_schedule
                    .after(&(now - chrono::Duration::seconds(61)))
                    .next()
                    .map(|n| n <= now)
                    .unwrap_or(false)
            }
        } else {
            // Never run before — check if due in last interval
            cron_schedule
                .after(&(now - chrono::Duration::seconds(61)))
                .next()
                .map(|n| n <= now)
                .unwrap_or(false)
        };

        if !should_fire {
            continue;
        }

        if let Some(scene_id) = schedule.scene_id {
            // Scene schedule — execute all actions in the scene
            log::info!(
                "[Scheduler] Firing scene schedule '{}': scene_id={}",
                schedule.label, scene_id
            );
            match db.get_scene(scene_id) {
                Ok(Some(scene)) => {
                    let mut ok = true;
                    for action in &scene.actions {
                        if let Err(e) = send_action(app_handle, conn_mgr, &action.device_id, &action.capability_id, &action.value) {
                            log::warn!("[Scheduler] Scene action failed for {}: {}", action.device_id, e);
                            ok = false;
                        }
                    }
                    if ok {
                        log::info!("[Scheduler] Scene '{}' executed ({} actions)", scene.name, scene.actions.len());
                    }
                    let _ = db.update_schedule_last_run(schedule.id);
                }
                Ok(None) => {
                    log::warn!("[Scheduler] Scene {} not found for schedule '{}'", scene_id, schedule.label);
                }
                Err(e) => {
                    log::warn!("[Scheduler] Failed to load scene {}: {}", scene_id, e);
                }
            }
        } else {
            // Single-action schedule
            log::info!(
                "[Scheduler] Firing schedule '{}': {}.{} = {}",
                schedule.label,
                schedule.device_id,
                schedule.capability_id,
                schedule.value
            );
            match send_action(app_handle, conn_mgr, &schedule.device_id, &schedule.capability_id, &schedule.value) {
                Ok(_) => {
                    log::info!("[Scheduler] Action sent successfully");
                    let _ = db.update_schedule_last_run(schedule.id);
                }
                Err(e) => {
                    log::warn!("[Scheduler] Failed to send action: {}", e);
                }
            }
        }
    }
}

fn send_action(
    app_handle: &AppHandle, conn_mgr: &ConnectionManager,
    device_id: &str, capability_id: &str, value_str: &str,
) -> Result<(), String> {
    let (ip, port) = get_device_info(app_handle, device_id)
        .ok_or_else(|| format!("Device {} not found or offline", device_id))?;

    let value: serde_json::Value = if value_str == "true" {
        serde_json::Value::Bool(true)
    } else if value_str == "false" {
        serde_json::Value::Bool(false)
    } else if let Ok(n) = value_str.parse::<f64>() {
        serde_json::json!(n)
    } else {
        serde_json::Value::String(value_str.to_string())
    };

    let cmd = serde_json::json!({
        "command": "set",
        "id": capability_id,
        "value": value
    });
    let msg = serde_json::to_string(&cmd).unwrap_or_default();
    let ws_port = port + 1;
    conn_mgr.send_to_device(device_id, &ip, ws_port, &msg)
}

fn get_device_info(app_handle: &AppHandle, device_id: &str) -> Option<(String, u16)> {
    // Try to get from the discovery's in-memory device list via the get_devices command
    // Since we can't easily access Discovery from here, we'll look up the saved device in DB
    if let Some(db) = app_handle.try_state::<Database>() {
        if let Ok(Some(saved)) = db.get_saved_device(device_id) {
            return Some((saved.ip, saved.port));
        }
    }
    None
}
