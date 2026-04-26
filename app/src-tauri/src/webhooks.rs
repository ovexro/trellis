//! Webhook event dispatcher.
//!
//! The `webhooks` table has carried an `event_type` column since v0.9.0 and the
//! desktop UI has offered a dropdown of event types ever since — but until now
//! nothing in the backend ever matched those rows against real events. The
//! "Test" button on a webhook card POSTs from the *browser* (it has the URL,
//! it can fetch). Real device transitions, OTA acks, etc. went unobserved by
//! webhooks, so the entire surface acted like a manual-fire endpoint.
//!
//! This module is the missing dispatcher. Call `dispatch_event` from any site
//! that produces an event a user might care about; matching webhooks fan out
//! on a detached worker thread, each POST is capped at 10 s, and the existing
//! `webhook_deliveries` log lights up automatically (so the v0.24.0 "last
//! delivery / N✓ M✗" observability pills work end-to-end).
//!
//! Event names normalize to dot form (`device.online`). Pre-existing rows from
//! older UI revisions used underscore form (`device_online`); the lookup query
//! accepts both so users don't have to recreate webhooks on upgrade.
//!
//! See `Database::get_webhooks_for_event` for the matching SQL.

use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::db::Database;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Fire-and-forget. Looks up matching webhooks synchronously (so the caller
/// returns instantly when there are none), then spawns a worker that does the
/// outbound POSTs and writes to `webhook_deliveries` from the same connection.
///
/// `device_id` is the device the event is *about* (None for system-level
/// events like `ota_applied` outside of a device context). Webhooks with
/// `device_id` NULL match any device for the event; webhooks scoped to a
/// specific device only match when the event carries the matching id.
pub fn dispatch_event(
    app_handle: &AppHandle,
    event_type: &str,
    device_id: Option<&str>,
    payload: Value,
) {
    let event = normalize_event_type(event_type);
    let device = device_id.map(|s| s.to_string());

    let Some(db_state) = app_handle.try_state::<Database>() else {
        log::warn!("[Webhooks] dispatch: Database state unavailable");
        return;
    };
    let matches = match db_state.get_webhooks_for_event(&event, device.as_deref()) {
        Ok(m) => m,
        Err(e) => {
            log::warn!("[Webhooks] dispatch lookup failed: {}", e);
            return;
        }
    };
    if matches.is_empty() {
        return;
    }

    let body = serde_json::json!({
        "event": &event,
        "device_id": &device,
        "payload": payload,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    })
    .to_string();

    let app_handle = app_handle.clone();
    std::thread::spawn(move || {
        for wh in matches {
            let (status_code, success, error) = post_one(&wh.url, &body);
            if let Some(db) = app_handle.try_state::<Database>() {
                let _ = db.log_webhook_delivery(
                    wh.id,
                    &event,
                    status_code,
                    success,
                    error.as_deref(),
                    1,
                );
            }
            log::info!(
                "[Webhooks] event={} → {} ({})",
                event,
                wh.url,
                status_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "transport-err".to_string())
            );
        }
    });
}

fn post_one(url: &str, body: &str) -> (Option<i32>, bool, Option<String>) {
    let resp = ureq::post(url)
        .timeout(HTTP_TIMEOUT)
        .set("Content-Type", "application/json")
        .send_string(body);
    match resp {
        Ok(r) => (Some(r.status() as i32), true, None),
        Err(ureq::Error::Status(code, _)) => {
            (Some(code as i32), false, Some(format!("HTTP {}", code)))
        }
        Err(ureq::Error::Transport(t)) => (None, false, Some(t.to_string())),
    }
}

/// Canonicalize separator. Pre-v0.26.0 UI saved underscore form
/// (`device_online`); the v0.7.0 `_` separator was a UI choice that never
/// matched the dot-form examples in the db tests. We pick dot going forward.
pub fn normalize_event_type(s: &str) -> String {
    s.replace('_', ".")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_underscore_to_dot() {
        assert_eq!(normalize_event_type("device_online"), "device.online");
        assert_eq!(normalize_event_type("device.online"), "device.online");
        assert_eq!(normalize_event_type("alert_triggered"), "alert.triggered");
    }
}
