use std::str::FromStr;
use std::thread;
use std::time::Duration;

use chrono::Utc;
use cron::Schedule;
use tauri::{AppHandle, Manager};

use crate::connection::ConnectionManager;
use crate::db::Database;

const POLL_INTERVAL: Duration = Duration::from_secs(60);

/// Compute the next firing time for a cron expression as an ISO8601 UTC string.
/// Returns None if the expression is invalid or has no future occurrence.
/// The "0 " seconds prefix matches what `evaluate_schedules` uses.
pub fn compute_next_run(cron: &str) -> Option<String> {
    let cron_expr = format!("0 {}", cron);
    let schedule = Schedule::from_str(&cron_expr).ok()?;
    let next = schedule.after(&Utc::now()).next()?;
    Some(next.to_rfc3339())
}

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

        if let Err(e) = fire_schedule(app_handle, conn_mgr, &schedule) {
            log::warn!("[Scheduler] Firing '{}' failed: {}", schedule.label, e);
        }
    }
}

/// Execute a schedule's action(s) once and stamp last_run on success.
/// Shared by the cron loop and the manual `run_schedule` path.
/// Returns Err on: scene not found / DB error / action send failure.
pub fn fire_schedule(
    app_handle: &AppHandle,
    conn_mgr: &ConnectionManager,
    schedule: &crate::db::Schedule,
) -> Result<(), String> {
    let db = app_handle
        .try_state::<Database>()
        .ok_or_else(|| "Database state unavailable".to_string())?;

    if let Some(scene_id) = schedule.scene_id {
        log::info!(
            "[Scheduler] Firing scene schedule '{}': scene_id={}",
            schedule.label, scene_id
        );
        let scene = db
            .get_scene(scene_id)
            .map_err(|e| format!("Failed to load scene {}: {}", scene_id, e))?
            .ok_or_else(|| format!("Scene {} not found", scene_id))?;

        let fire_result = fire_scene(app_handle, conn_mgr, &scene);

        db.update_schedule_last_run(schedule.id)
            .map_err(|e| format!("Failed to stamp last_run: {}", e))?;

        fire_result
    } else {
        log::info!(
            "[Scheduler] Firing schedule '{}': {}.{} = {}",
            schedule.label, schedule.device_id, schedule.capability_id, schedule.value
        );
        send_action(
            app_handle, conn_mgr,
            &schedule.device_id, &schedule.capability_id, &schedule.value,
        )?;
        db.update_schedule_last_run(schedule.id)
            .map_err(|e| format!("Failed to stamp last_run: {}", e))?;
        log::info!("[Scheduler] Action sent successfully");
        Ok(())
    }
}

/// Execute a rule's action once and stamp last_triggered on success.
/// Shared by the frontend evaluator (after conditions match) and the
/// manual "Run now" path (which deliberately bypasses the conditions).
/// Returns Err on: target device offline / action send failure / DB error.
pub fn fire_rule(
    app_handle: &AppHandle,
    conn_mgr: &ConnectionManager,
    rule: &crate::db::Rule,
) -> Result<(), String> {
    let db = app_handle
        .try_state::<Database>()
        .ok_or_else(|| "Database state unavailable".to_string())?;

    log::info!(
        "[Scheduler] Firing rule '{}': {}.{} = {}",
        rule.label, rule.target_device_id, rule.target_capability_id, rule.target_value
    );

    send_action(
        app_handle, conn_mgr,
        &rule.target_device_id, &rule.target_capability_id, &rule.target_value,
    )?;

    db.update_rule_last_triggered(rule.id)
        .map_err(|e| format!("Failed to stamp last_triggered: {}", e))?;

    log::info!("[Scheduler] Rule action sent successfully");
    Ok(())
}

/// Execute every action on a scene once and stamp last_run on success.
/// Shared by the manual Tauri + REST `run_scene` paths, the Sinric voice
/// scene execution, and `fire_schedule` when the schedule targets a scene.
/// Returns Err listing per-action failures if any action send failed; last_run
/// is stamped when at least one action succeeded (mirrors the prior semantics).
pub fn fire_scene(
    app_handle: &AppHandle,
    conn_mgr: &ConnectionManager,
    scene: &crate::db::Scene,
) -> Result<(), String> {
    let db = app_handle
        .try_state::<Database>()
        .ok_or_else(|| "Database state unavailable".to_string())?;

    log::info!(
        "[Scheduler] Firing scene '{}' ({} actions)",
        scene.name, scene.actions.len()
    );

    let mut failures = Vec::new();
    for action in &scene.actions {
        if let Err(e) = send_action(
            app_handle, conn_mgr,
            &action.device_id, &action.capability_id, &action.value,
        ) {
            log::warn!("[Scheduler] Scene action failed for {}: {}", action.device_id, e);
            failures.push(format!("{}: {}", action.device_id, e));
        }
    }

    if failures.len() < scene.actions.len() {
        db.update_scene_last_run(scene.id)
            .map_err(|e| format!("Failed to stamp last_run: {}", e))?;
    }

    if failures.is_empty() {
        log::info!("[Scheduler] Scene '{}' executed", scene.name);
        Ok(())
    } else {
        Err(format!(
            "{}/{} scene actions failed: {}",
            failures.len(), scene.actions.len(), failures.join("; ")
        ))
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

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{DateTime, Duration as ChronoDuration};

    fn parse(iso: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(iso).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn next_run_hourly_is_within_the_hour() {
        // Every hour at :00
        let next = compute_next_run("0 * * * *").expect("hourly should parse");
        let dt = parse(&next);
        let now = Utc::now();
        assert!(dt > now, "next_run must be in the future");
        assert!(dt <= now + ChronoDuration::hours(1) + ChronoDuration::minutes(1),
                "hourly next must be within ~1h, got {} vs now {}", dt, now);
    }

    #[test]
    fn next_run_daily_is_within_a_day() {
        // Every day at 06:00
        let next = compute_next_run("0 6 * * *").expect("daily should parse");
        let dt = parse(&next);
        let now = Utc::now();
        assert!(dt > now);
        assert!(dt <= now + ChronoDuration::days(1) + ChronoDuration::minutes(1));
    }

    #[test]
    fn next_run_weekly_is_within_a_week() {
        // Every Monday at 09:00 (1 = Monday in cron crate's day-of-week)
        let next = compute_next_run("0 9 * * 1").expect("weekly should parse");
        let dt = parse(&next);
        let now = Utc::now();
        assert!(dt > now);
        assert!(dt <= now + ChronoDuration::days(7) + ChronoDuration::minutes(1));
    }

    #[test]
    fn next_run_invalid_returns_none() {
        assert!(compute_next_run("not a cron expression").is_none());
        assert!(compute_next_run("").is_none());
        assert!(compute_next_run("99 99 99 99 99").is_none());
    }

    #[test]
    fn next_run_is_rfc3339_with_utc_offset() {
        let next = compute_next_run("0 * * * *").expect("hourly should parse");
        // RFC3339 UTC ends with +00:00 or Z
        assert!(next.ends_with("+00:00") || next.ends_with('Z'),
                "expected RFC3339 UTC, got {}", next);
    }
}
