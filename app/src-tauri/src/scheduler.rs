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

        log::info!(
            "[Scheduler] Firing schedule '{}': {}.{} = {}",
            schedule.label,
            schedule.device_id,
            schedule.capability_id,
            schedule.value
        );

        // Find the device IP and port from the discovery state
        // We need to get it from the in-memory device list
        let device_info = get_device_info(app_handle, &schedule.device_id);

        if let Some((ip, port)) = device_info {
            // Parse value
            let value: serde_json::Value = if schedule.value == "true" {
                serde_json::Value::Bool(true)
            } else if schedule.value == "false" {
                serde_json::Value::Bool(false)
            } else if let Ok(n) = schedule.value.parse::<f64>() {
                serde_json::json!(n)
            } else {
                serde_json::Value::String(schedule.value.clone())
            };

            let cmd = serde_json::json!({
                "command": "set",
                "id": schedule.capability_id,
                "value": value
            });

            let msg = serde_json::to_string(&cmd).unwrap_or_default();
            let ws_port = port + 1;

            match conn_mgr.send_to_device(&schedule.device_id, &ip, ws_port, &msg) {
                Ok(_) => {
                    log::info!("[Scheduler] Action sent successfully");
                    let _ = db.update_schedule_last_run(schedule.id);
                }
                Err(e) => {
                    log::warn!("[Scheduler] Failed to send action: {}", e);
                }
            }
        } else {
            log::warn!(
                "[Scheduler] Device {} not found or offline, skipping",
                schedule.device_id
            );
        }
    }
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
