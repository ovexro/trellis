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

use std::io::Read;
use std::time::Duration;

use serde_json::Value;
use tauri::{AppHandle, Manager};

use crate::db::Database;

const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

/// Body preview bound (request and response, each). Stored on
/// `webhook_deliveries.request_body_preview` / `response_body_preview` so the
/// log surfaces what was actually sent and received without unbounded DB
/// growth. Anything over the bound gets a "…(truncated, N bytes total)"
/// trailer so the UI can signal the cap rather than silently lying.
pub const PREVIEW_BOUND: usize = 4096;

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
    let request_preview = truncate_preview(&body);
    std::thread::spawn(move || {
        for wh in matches {
            let outcome = post_one(&wh.url, &body);
            if let Some(db) = app_handle.try_state::<Database>() {
                let _ = db.log_webhook_delivery(
                    wh.id,
                    &event,
                    outcome.status_code,
                    outcome.success,
                    outcome.error.as_deref(),
                    1,
                    Some(request_preview.as_str()),
                    outcome.response_body.as_deref(),
                );
            }
            log::info!(
                "[Webhooks] event={} → {} ({})",
                event,
                wh.url,
                outcome.status_code
                    .map(|c| c.to_string())
                    .unwrap_or_else(|| "transport-err".to_string())
            );
        }
    });
}

struct PostOutcome {
    status_code: Option<i32>,
    success: bool,
    error: Option<String>,
    response_body: Option<String>,
}

fn post_one(url: &str, body: &str) -> PostOutcome {
    let resp = ureq::post(url)
        .timeout(HTTP_TIMEOUT)
        .set("Content-Type", "application/json")
        .send_string(body);
    match resp {
        Ok(r) => {
            let code = r.status() as i32;
            PostOutcome {
                status_code: Some(code),
                success: true,
                error: None,
                response_body: read_bounded_body(r),
            }
        }
        Err(ureq::Error::Status(code, r)) => PostOutcome {
            status_code: Some(code as i32),
            success: false,
            error: Some(format!("HTTP {}", code)),
            response_body: read_bounded_body(r),
        },
        Err(ureq::Error::Transport(t)) => PostOutcome {
            status_code: None,
            success: false,
            error: Some(t.to_string()),
            response_body: None,
        },
    }
}

/// Drains up to `PREVIEW_BOUND + 1` bytes from the response so the `+1` byte
/// signals overflow without forcing the full body through memory. Lossy UTF-8
/// decoding so binary responses still produce a readable preview rather than
/// erroring the whole capture path. Returns `None` for an empty body.
fn read_bounded_body(resp: ureq::Response) -> Option<String> {
    let mut buf: Vec<u8> = Vec::with_capacity(PREVIEW_BOUND + 1);
    let _ = resp
        .into_reader()
        .take((PREVIEW_BOUND + 1) as u64)
        .read_to_end(&mut buf);
    if buf.is_empty() {
        return None;
    }
    let overflow = buf.len() > PREVIEW_BOUND;
    let visible = if overflow { &buf[..PREVIEW_BOUND] } else { &buf[..] };
    let s = String::from_utf8_lossy(visible).into_owned();
    if overflow {
        Some(format!("{}\n…(truncated)", s))
    } else {
        Some(s)
    }
}

/// Bounded UTF-8-safe truncation for outbound preview strings (request bodies
/// the desktop test button sent, dispatcher payloads). Mirrors
/// `read_bounded_body`'s overflow trailer so the UI shows the same shape for
/// inbound and outbound previews.
pub fn truncate_preview(s: &str) -> String {
    if s.len() <= PREVIEW_BOUND {
        return s.to_string();
    }
    let total = s.len();
    // Walk back from PREVIEW_BOUND to find a UTF-8 char boundary so the
    // truncated slice is always a valid str (lossy String::from is overkill
    // here — we have valid UTF-8 input).
    let mut cut = PREVIEW_BOUND;
    while cut > 0 && !s.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}\n…(truncated, {} bytes total)", &s[..cut], total)
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

    #[test]
    fn truncate_preview_passes_short_strings_through() {
        let s = r#"{"event":"test","payload":{"x":1}}"#;
        assert_eq!(truncate_preview(s), s);
    }

    #[test]
    fn truncate_preview_caps_at_bound_and_trails() {
        let s = "x".repeat(PREVIEW_BOUND + 100);
        let out = truncate_preview(&s);
        assert!(out.starts_with(&"x".repeat(PREVIEW_BOUND)));
        assert!(out.contains("(truncated"));
        assert!(out.contains(&format!("{} bytes total", s.len())));
    }

    #[test]
    fn truncate_preview_walks_back_to_char_boundary() {
        // Build a string where byte PREVIEW_BOUND falls mid-multibyte-char.
        // '🦀' is 4 bytes — repeat enough to cross the bound.
        let crab_count = (PREVIEW_BOUND / 4) + 5;
        let s = "🦀".repeat(crab_count);
        let out = truncate_preview(&s);
        // Trailer present
        assert!(out.contains("(truncated"));
        // Visible prefix is valid UTF-8 (no replacement chars from a mid-char split)
        let head = out.split("\n…").next().unwrap();
        assert!(!head.contains('\u{FFFD}'));
    }

    #[test]
    fn preview_bound_is_4kb() {
        // Defensive: surface schema/UI assumes ≤4 KB so a code-side bump must
        // be a deliberate decision, not a typo.
        assert_eq!(PREVIEW_BOUND, 4096);
    }
}
