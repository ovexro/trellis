//! Alert evaluator.
//!
//! The `alerts` table + struct + CRUD + REST + Tauri commands have all existed
//! since v0.4-ish, and the desktop frontend has run a `checkAlerts` evaluator
//! in the browser. But nothing on the *backend* ever evaluated alerts: when the
//! desktop app was closed, sensor metrics flowed in (via the WS connection in
//! `connection.rs`) and alerts silently never fired. ntfy push went through
//! the frontend, so closed-desktop = no push either.
//!
//! This module is the missing backend evaluator. Call `evaluate` from any site
//! that produces a numeric capability transition (sensor reading); matching
//! enabled alerts dispatch a webhook (`alert.triggered`) and an ntfy push
//! (when an `ntfy_topic` setting is configured) on a detached worker.
//!
//! 60 s in-memory debounce per (alert_id) mirrors the frontend behavior — the
//! two paths can coexist while the frontend evaluator stays in place for
//! desktop-popup notifications. Backend never duplicates the desktop popup.
//!
//! See `Database::get_alerts` for the row shape and `webhooks::dispatch_event`
//! for the outbound webhook fan-out.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::db::Database;

const DEBOUNCE: Duration = Duration::from_secs(60);

static FIRED: Mutex<Option<HashMap<i64, Instant>>> = Mutex::new(None);

fn should_fire(alert_id: i64) -> bool {
    let mut guard = FIRED.lock().unwrap();
    let map = guard.get_or_insert_with(HashMap::new);
    let now = Instant::now();
    if let Some(last) = map.get(&alert_id) {
        if now.duration_since(*last) < DEBOUNCE {
            return false;
        }
    }
    map.insert(alert_id, now);
    true
}

/// True when a numeric reading crosses the alert's threshold in the configured
/// direction. Pure helper so unit tests don't need a full `Database`.
pub fn condition_matches(condition: &str, value: f64, threshold: f64) -> bool {
    match condition {
        "above" => value > threshold,
        "below" => value < threshold,
        _ => false,
    }
}

/// Look up alerts for `(device_id, metric_id)`, evaluate against `value`, and
/// for each match: (a) dispatch an `alert.triggered` webhook with the alert
/// metadata, (b) push via ntfy if `ntfy_topic` is set in settings. The desktop
/// frontend continues to handle desktop notifications, so this fires in
/// addition: only when the desktop is closed do its ntfy + webhook calls go
/// dark, and now the backend covers both.
///
/// Synchronous DB lookup (returns instantly when nothing matches) then a
/// detached worker performs the I/O.
pub fn evaluate(app_handle: &AppHandle, device_id: &str, metric_id: &str, value: f64) {
    let Some(db) = app_handle.try_state::<Database>() else {
        return;
    };
    let alerts = match db.get_alerts(device_id) {
        Ok(a) => a,
        Err(e) => {
            log::warn!("[Alerts] lookup failed for {}: {}", device_id, e);
            return;
        }
    };

    let triggered: Vec<_> = alerts
        .into_iter()
        .filter(|a| a.enabled && a.metric_id == metric_id)
        .filter(|a| condition_matches(&a.condition, value, a.threshold))
        .filter(|a| should_fire(a.id))
        .collect();

    if triggered.is_empty() {
        return;
    }

    let ntfy_topic = db.get_setting("ntfy_topic").ok().flatten();
    let device_name = db
        .get_saved_device(device_id)
        .ok()
        .flatten()
        .map(|d| d.nickname.unwrap_or(d.name))
        .unwrap_or_else(|| device_id.to_string());
    let device_id_owned = device_id.to_string();
    let metric_id_owned = metric_id.to_string();
    let app_handle_clone = app_handle.clone();

    std::thread::spawn(move || {
        for alert in triggered {
            let payload = json!({
                "alert_id": alert.id,
                "label": &alert.label,
                "metric": &metric_id_owned,
                "value": value,
                "condition": &alert.condition,
                "threshold": alert.threshold,
                "device_name": &device_name,
            });
            crate::webhooks::dispatch_event(
                &app_handle_clone,
                "alert.triggered",
                Some(&device_id_owned),
                payload,
            );

            if let Some(ref topic) = ntfy_topic {
                let title = format!("Trellis: {}", &device_name);
                let message = format!(
                    "{}: {} is {:.1} ({} {})",
                    alert.label, metric_id_owned, value, alert.condition, alert.threshold
                );
                let url = format!("https://ntfy.sh/{}", topic);
                let body = json!({
                    "topic": topic,
                    "title": title,
                    "message": message,
                    "priority": 4
                });
                let _ = ureq::post(&url)
                    .timeout(Duration::from_secs(10))
                    .set("Content-Type", "application/json")
                    .send_string(&body.to_string());
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn condition_matches_above() {
        assert!(condition_matches("above", 30.0, 25.0));
        assert!(!condition_matches("above", 25.0, 25.0));
        assert!(!condition_matches("above", 20.0, 25.0));
    }

    #[test]
    fn condition_matches_below() {
        assert!(condition_matches("below", 20.0, 25.0));
        assert!(!condition_matches("below", 25.0, 25.0));
        assert!(!condition_matches("below", 30.0, 25.0));
    }

    #[test]
    fn condition_matches_unknown_operator_never_fires() {
        assert!(!condition_matches("equals", 25.0, 25.0));
        assert!(!condition_matches("", 30.0, 25.0));
    }

    #[test]
    fn debounce_blocks_immediate_refire() {
        // Use distinct alert_ids so other tests don't interfere.
        let id = 9_001_001;
        assert!(should_fire(id), "first call should fire");
        assert!(!should_fire(id), "second call within window should be blocked");
        assert!(!should_fire(id), "third call within window should be blocked");
    }

    #[test]
    fn debounce_independent_per_alert() {
        let a = 9_001_002;
        let b = 9_001_003;
        assert!(should_fire(a));
        assert!(should_fire(b), "different alert id should not be debounced by another");
    }
}
