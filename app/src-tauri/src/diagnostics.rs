use crate::db::{Database, LogEntry, MetricPoint};
use crate::device::Device;
use chrono::{DateTime, Utc};
use serde::Serialize;

/// Per-device entry in a fleet health report.
#[derive(Debug, Clone, Serialize)]
pub struct FleetDeviceEntry {
    pub device_id: String,
    pub name: String,
    pub online: bool,
    pub overall: String,
    pub critical: u32,
    pub warnings: u32,
}

/// Fleet-wide aggregate of per-device diagnostic rollups.
#[derive(Debug, Clone, Serialize)]
pub struct FleetReport {
    pub generated_at: String,
    pub total: u32,
    pub good: u32,
    pub attention: u32,
    pub unhealthy: u32,
    pub devices: Vec<FleetDeviceEntry>,
}

/// A single check result in the diagnostic report.
#[derive(Debug, Clone, Serialize)]
pub struct Finding {
    pub id: String,
    pub level: String,
    pub title: String,
    pub detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticReport {
    pub device_id: String,
    pub overall: String,
    pub generated_at: String,
    pub findings: Vec<Finding>,
}

const LEVEL_OK: &str = "ok";
const LEVEL_WARN: &str = "warn";
const LEVEL_FAIL: &str = "fail";
const LEVEL_INFO: &str = "info";

const WINDOW_HOURS: u32 = 24;

pub fn diagnose(
    db: &Database,
    device_id: &str,
    live_device: Option<&Device>,
) -> Result<DiagnosticReport, String> {
    let mut findings: Vec<Finding> = Vec::new();

    findings.push(check_online_status(live_device, db, device_id)?);

    // Metric IDs match the ones deviceStore.ts writes via `store_metric`:
    // `_rssi` and `_heap` (underscore-prefixed to distinguish them from
    // user sensor capabilities, which can be arbitrary strings).
    let rssi = db.get_metrics(device_id, "_rssi", WINDOW_HOURS)?;
    findings.push(check_rssi_health(&rssi, live_device));

    let heap = db.get_metrics(device_id, "_heap", WINDOW_HOURS)?;
    findings.push(check_heap_low(&heap, live_device));
    findings.push(check_heap_trend(&heap));

    let state_logs = db.get_logs_filtered(
        device_id,
        1000,
        Some(&["state".to_string()]),
    )?;
    let state_in_window: Vec<&LogEntry> = state_logs
        .iter()
        .filter(|l| is_within_hours(&l.timestamp, WINDOW_HOURS))
        .collect();
    findings.push(check_uptime_percent(&state_in_window));
    findings.push(check_reconnect_count(&state_in_window));

    let error_logs = db.get_logs_filtered(
        device_id,
        500,
        Some(&["error".to_string(), "warn".to_string()]),
    )?;
    let errors_in_window: Vec<&LogEntry> = error_logs
        .iter()
        .filter(|l| is_within_hours(&l.timestamp, WINDOW_HOURS))
        .collect();
    findings.push(check_error_rate(&errors_in_window));

    let history = db.get_firmware_history(device_id)?;
    findings.push(check_firmware_age(&history));

    let overall = roll_up(&findings);

    Ok(DiagnosticReport {
        device_id: device_id.to_string(),
        overall,
        generated_at: Utc::now().to_rfc3339(),
        findings,
    })
}

fn check_online_status(
    live: Option<&Device>,
    db: &Database,
    device_id: &str,
) -> Result<Finding, String> {
    if let Some(d) = live {
        if d.online {
            return Ok(Finding {
                id: "online_status".to_string(),
                level: LEVEL_OK.to_string(),
                title: "Device is online".to_string(),
                detail: format!("Reachable at {}:{}.", d.ip, d.port),
                suggestion: None,
            });
        }
    }

    // Offline — figure out how long by reading the most recent state log.
    let recent = db
        .get_logs_filtered(device_id, 1, Some(&["state".to_string()]))
        .unwrap_or_default();
    let detail = match recent.first() {
        Some(l) => {
            let mins = minutes_since(&l.timestamp).unwrap_or(0);
            if l.message == "offline" {
                if mins > 60 {
                    format!("Offline for ~{}h. Last seen {}.", mins / 60, l.timestamp)
                } else {
                    format!("Offline for ~{}m. Last seen {}.", mins, l.timestamp)
                }
            } else {
                format!("Not reachable right now (last state transition: {} at {}).", l.message, l.timestamp)
            }
        }
        None => "Device not reachable and no state history on record.".to_string(),
    };

    let level = {
        let mins = recent
            .first()
            .and_then(|l| minutes_since(&l.timestamp))
            .unwrap_or(u64::MAX);
        if mins > 60 { LEVEL_FAIL } else { LEVEL_WARN }
    };

    Ok(Finding {
        id: "online_status".to_string(),
        level: level.to_string(),
        title: "Device is offline".to_string(),
        detail,
        suggestion: Some(
            "Check power to the device, verify it is on the expected WiFi network, and confirm the router is reachable."
                .to_string(),
        ),
    })
}

fn check_rssi_health(samples: &[MetricPoint], live: Option<&Device>) -> Finding {
    // Fall back to the live reading if no historical data yet.
    if samples.is_empty() {
        if let Some(d) = live {
            let v = d.system.rssi;
            let (level, detail, suggestion) = rssi_verdict_instant(v);
            return Finding {
                id: "rssi_health".to_string(),
                level,
                title: "WiFi signal strength".to_string(),
                detail,
                suggestion,
            };
        }
        return Finding {
            id: "rssi_health".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "WiFi signal strength".to_string(),
            detail: "No RSSI samples recorded in the last 24h.".to_string(),
            suggestion: None,
        };
    }

    let n = samples.len();
    let avg: f64 = samples.iter().map(|s| s.value).sum::<f64>() / n as f64;
    let min: f64 = samples.iter().map(|s| s.value).fold(f64::INFINITY, f64::min);
    let below_75 = samples.iter().filter(|s| s.value < -75.0).count();
    let pct_below_75 = (below_75 as f64) * 100.0 / (n as f64);

    let level = if avg < -75.0 || pct_below_75 > 50.0 {
        LEVEL_FAIL
    } else if avg < -65.0 || pct_below_75 > 20.0 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };

    let detail = format!(
        "Avg RSSI {:.0} dBm over last 24h ({} samples); min {:.0} dBm; {:.0}% of samples below -75 dBm.",
        avg, n, min, pct_below_75
    );
    let suggestion = if level != LEVEL_OK {
        Some(
            "Weak signal causes dropped connections and slower OTA. Try moving the device closer to the router, repositioning the antenna, or adding a repeater."
                .to_string(),
        )
    } else {
        None
    };

    Finding {
        id: "rssi_health".to_string(),
        level: level.to_string(),
        title: "WiFi signal strength".to_string(),
        detail,
        suggestion,
    }
}

fn rssi_verdict_instant(rssi: i32) -> (String, String, Option<String>) {
    let v = rssi as f64;
    let level = if v < -80.0 {
        LEVEL_FAIL
    } else if v < -65.0 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };
    let detail = format!("Current RSSI {} dBm (no historical samples yet).", rssi);
    let suggestion = if level != LEVEL_OK {
        Some("Signal is weak. Move the device closer to the router or add a repeater.".to_string())
    } else {
        None
    };
    (level.to_string(), detail, suggestion)
}

fn check_heap_low(samples: &[MetricPoint], live: Option<&Device>) -> Finding {
    if samples.is_empty() {
        if let Some(d) = live {
            let v = d.system.heap_free as f64;
            let level = if v < 10_000.0 {
                LEVEL_FAIL
            } else if v < 40_000.0 {
                LEVEL_WARN
            } else {
                LEVEL_OK
            };
            return Finding {
                id: "heap_low".to_string(),
                level: level.to_string(),
                title: "Free memory".to_string(),
                detail: format!("Current free heap {} bytes (no historical samples yet).", d.system.heap_free),
                suggestion: if level != LEVEL_OK {
                    Some("Low free heap increases crash risk. Check for large String concatenations in loop(), unclosed WiFi/HTTP clients, and leaking dynamic allocations.".to_string())
                } else {
                    None
                },
            };
        }
        return Finding {
            id: "heap_low".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Free memory".to_string(),
            detail: "No free-heap samples recorded in the last 24h.".to_string(),
            suggestion: None,
        };
    }

    let min: f64 = samples.iter().map(|s| s.value).fold(f64::INFINITY, f64::min);
    let avg: f64 = samples.iter().map(|s| s.value).sum::<f64>() / samples.len() as f64;

    let level = if min < 10_000.0 {
        LEVEL_FAIL
    } else if min < 40_000.0 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };

    let detail = format!(
        "Min free heap {:.0} bytes, avg {:.0} bytes over last 24h ({} samples).",
        min, avg, samples.len()
    );
    let suggestion = if level != LEVEL_OK {
        Some(
            "Low free heap increases crash risk. Check for large String concatenations in loop(), unclosed WiFi/HTTP clients, and leaking dynamic allocations."
                .to_string(),
        )
    } else {
        None
    };
    Finding {
        id: "heap_low".to_string(),
        level: level.to_string(),
        title: "Free memory".to_string(),
        detail,
        suggestion,
    }
}

fn check_heap_trend(samples: &[MetricPoint]) -> Finding {
    if samples.len() < 20 {
        return Finding {
            id: "heap_trend".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Memory leak pattern".to_string(),
            detail: format!(
                "Need at least 20 free-heap samples for trend analysis; have {}.",
                samples.len()
            ),
            suggestion: None,
        };
    }

    // Least-squares slope of free_heap vs sample index.
    // Negative slope → heap shrinking → possible leak.
    let n = samples.len() as f64;
    let xs: Vec<f64> = (0..samples.len()).map(|i| i as f64).collect();
    let ys: Vec<f64> = samples.iter().map(|s| s.value).collect();
    let mean_x = xs.iter().sum::<f64>() / n;
    let mean_y = ys.iter().sum::<f64>() / n;
    let mut num = 0.0;
    let mut den = 0.0;
    for (x, y) in xs.iter().zip(ys.iter()) {
        num += (x - mean_x) * (y - mean_y);
        den += (x - mean_x) * (x - mean_x);
    }
    let slope_per_sample = if den == 0.0 { 0.0 } else { num / den };

    // Approximate samples-per-hour: samples come in with each heartbeat (~10s)
    // so ~360/hour; but if retention / cleanup has trimmed rows, fall back to
    // normalizing by elapsed time between first/last sample.
    let hours_span = hours_between(
        &samples.first().unwrap().timestamp,
        &samples.last().unwrap().timestamp,
    )
    .unwrap_or(WINDOW_HOURS as f64);
    let samples_per_hour = if hours_span > 0.1 {
        n / hours_span
    } else {
        360.0
    };
    let slope_per_hour = slope_per_sample * samples_per_hour;

    let level = if slope_per_hour < -500.0 {
        LEVEL_FAIL
    } else if slope_per_hour < -100.0 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };

    let detail = if slope_per_hour.abs() < 50.0 {
        format!(
            "Free heap is stable (slope {:+.0} bytes/hour over last 24h, {} samples).",
            slope_per_hour,
            samples.len()
        )
    } else if slope_per_hour < 0.0 {
        format!(
            "Free heap is decreasing at {:.0} bytes/hour over last 24h ({} samples).",
            slope_per_hour.abs(),
            samples.len()
        )
    } else {
        format!(
            "Free heap is increasing at {:+.0} bytes/hour (likely recovering after restart, {} samples).",
            slope_per_hour,
            samples.len()
        )
    };
    let suggestion = if level != LEVEL_OK {
        Some(
            "A steady downward trend suggests a memory leak. Check for String concatenation inside loop(), dynamic allocations that are never freed, or HTTP clients not cleaned up between requests."
                .to_string(),
        )
    } else {
        None
    };
    Finding {
        id: "heap_trend".to_string(),
        level: level.to_string(),
        title: "Memory leak pattern".to_string(),
        detail,
        suggestion,
    }
}

fn check_uptime_percent(state_logs: &[&LogEntry]) -> Finding {
    // Walk transitions in order, pair online→offline into downtime segments.
    // Anything before the first transition is counted as "observed" starting
    // at window-start, with the inferred state being the opposite of the
    // first transition we saw (transitions only fire on change).
    if state_logs.is_empty() {
        return Finding {
            id: "uptime_percent".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Uptime over last 24h".to_string(),
            detail: "No state transitions recorded — device has been steady.".to_string(),
            suggestion: None,
        };
    }

    let now_mins = 0u64; // "now" = 0 minutes ago; window-start = WINDOW_HOURS*60 ago
    let window_start_mins = (WINDOW_HOURS as u64) * 60;

    // Convert each log to minutes-ago. Rows come DESC from get_logs_filtered.
    let mut events: Vec<(u64, &str)> = state_logs
        .iter()
        .filter_map(|l| minutes_since(&l.timestamp).map(|m| (m, l.message.as_str())))
        .collect();
    events.sort_by_key(|(m, _)| std::cmp::Reverse(*m)); // oldest-first by minutes-ago

    // Infer initial state: opposite of first transition seen.
    let first_msg = events.first().map(|(_, m)| *m).unwrap_or("online");
    let mut current_state = if first_msg == "online" { "offline" } else { "online" };
    let mut prev_mins = window_start_mins;

    let mut online_mins = 0u64;
    let mut offline_mins = 0u64;

    for (mins, msg) in &events {
        let segment_mins = prev_mins.saturating_sub(*mins);
        match current_state {
            "online" => online_mins += segment_mins,
            "offline" => offline_mins += segment_mins,
            _ => {}
        }
        current_state = msg;
        prev_mins = *mins;
    }
    // Trailing segment from last transition to now.
    let tail = prev_mins.saturating_sub(now_mins);
    match current_state {
        "online" => online_mins += tail,
        "offline" => offline_mins += tail,
        _ => {}
    }

    let total = online_mins + offline_mins;
    if total == 0 {
        return Finding {
            id: "uptime_percent".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Uptime over last 24h".to_string(),
            detail: "Not enough data to compute uptime.".to_string(),
            suggestion: None,
        };
    }
    let pct = (online_mins as f64) * 100.0 / (total as f64);

    let level = if pct >= 95.0 {
        LEVEL_OK
    } else if pct >= 80.0 {
        LEVEL_WARN
    } else {
        LEVEL_FAIL
    };

    let detail = format!(
        "{:.1}% online over last 24h ({}h {}m online, {}h {}m offline).",
        pct,
        online_mins / 60,
        online_mins % 60,
        offline_mins / 60,
        offline_mins % 60
    );
    let suggestion = if level != LEVEL_OK {
        Some(
            "Frequent or long outages often point to weak WiFi, power instability, or firmware crashes. Check RSSI, power supply, and error logs."
                .to_string(),
        )
    } else {
        None
    };
    Finding {
        id: "uptime_percent".to_string(),
        level: level.to_string(),
        title: "Uptime over last 24h".to_string(),
        detail,
        suggestion,
    }
}

fn check_reconnect_count(state_logs: &[&LogEntry]) -> Finding {
    let reconnects = state_logs
        .iter()
        .filter(|l| l.message == "online")
        .count();
    let level = if reconnects > 10 {
        LEVEL_FAIL
    } else if reconnects > 2 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };
    let detail = format!("{} reconnect events in last 24h.", reconnects);
    let suggestion = if level != LEVEL_OK {
        Some(
            "Repeated reconnects (flapping) usually mean WiFi signal issues or the router dropping the device. Check RSSI and router DHCP leases."
                .to_string(),
        )
    } else {
        None
    };
    Finding {
        id: "reconnect_count".to_string(),
        level: level.to_string(),
        title: "Connection stability".to_string(),
        detail,
        suggestion,
    }
}

fn check_error_rate(error_logs: &[&LogEntry]) -> Finding {
    let errors = error_logs.iter().filter(|l| l.severity == "error").count();
    let warns = error_logs.iter().filter(|l| l.severity == "warn").count();
    let level = if errors > 10 {
        LEVEL_FAIL
    } else if errors > 0 || warns > 20 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };
    let detail = format!(
        "{} error and {} warning events logged in the last 24h.",
        errors, warns
    );
    let suggestion = if level != LEVEL_OK {
        Some("Open the device detail panel and filter logs by Error/Warn to inspect what the firmware is reporting.".to_string())
    } else {
        None
    };
    Finding {
        id: "error_rate".to_string(),
        level: level.to_string(),
        title: "Error log rate".to_string(),
        detail,
        suggestion,
    }
}

fn check_firmware_age(history: &[crate::db::FirmwareRecord]) -> Finding {
    if let Some(latest) = history.first() {
        // firmware_history is ORDER BY uploaded_at DESC in the default get_firmware_history.
        let days = minutes_since(&latest.uploaded_at)
            .map(|m| m / 60 / 24)
            .unwrap_or(0);
        let detail = if days == 0 {
            format!("Firmware v{} pushed today.", latest.version)
        } else if days == 1 {
            format!("Firmware v{} pushed yesterday.", latest.version)
        } else {
            format!("Firmware v{} pushed {} days ago.", latest.version, days)
        };
        Finding {
            id: "firmware_age".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Firmware".to_string(),
            detail,
            suggestion: None,
        }
    } else {
        Finding {
            id: "firmware_age".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Firmware".to_string(),
            detail: "No firmware OTA updates recorded for this device.".to_string(),
            suggestion: None,
        }
    }
}

fn roll_up(findings: &[Finding]) -> String {
    let has_fail = findings.iter().any(|f| f.level == LEVEL_FAIL);
    let has_warn = findings.iter().any(|f| f.level == LEVEL_WARN);
    if has_fail {
        "unhealthy".to_string()
    } else if has_warn {
        "attention".to_string()
    } else {
        "good".to_string()
    }
}

/// Aggregate per-device diagnostics across the known fleet.
/// Pure read-only: re-uses `diagnose` per device and rolls the `overall`
/// verdicts into totals. Devices whose individual check errors out are
/// skipped (they simply don't contribute to any bucket) so one bad row
/// can't hide the rest.
pub fn diagnose_fleet(
    db: &Database,
    live_devices: &[Device],
) -> Result<FleetReport, String> {
    let saved = db.get_all_saved_devices()?;
    let mut good = 0u32;
    let mut attention = 0u32;
    let mut unhealthy = 0u32;
    let mut devices: Vec<FleetDeviceEntry> = Vec::with_capacity(saved.len());

    for sd in &saved {
        let live = live_devices.iter().find(|d| d.id == sd.id);
        let report = match diagnose(db, &sd.id, live) {
            Ok(r) => r,
            Err(_) => continue,
        };
        match report.overall.as_str() {
            "good" => good += 1,
            "attention" => attention += 1,
            "unhealthy" => unhealthy += 1,
            _ => {}
        }
        let critical = report.findings.iter().filter(|f| f.level == LEVEL_FAIL).count() as u32;
        let warnings = report.findings.iter().filter(|f| f.level == LEVEL_WARN).count() as u32;
        let name = sd
            .nickname
            .clone()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| sd.name.clone());
        devices.push(FleetDeviceEntry {
            device_id: sd.id.clone(),
            name,
            online: live.map(|d| d.online).unwrap_or(false),
            overall: report.overall,
            critical,
            warnings,
        });
    }

    // Sort most-urgent first so the UI can take the top slice.
    devices.sort_by(|a, b| severity_rank(&a.overall).cmp(&severity_rank(&b.overall)));

    let total = devices.len() as u32;
    Ok(FleetReport {
        generated_at: Utc::now().to_rfc3339(),
        total,
        good,
        attention,
        unhealthy,
        devices,
    })
}

fn severity_rank(overall: &str) -> u8 {
    match overall {
        "unhealthy" => 0,
        "attention" => 1,
        "good" => 2,
        _ => 3,
    }
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse_ts(ts: &str) -> Option<DateTime<Utc>> {
    // SQLite default datetime('now') format: "2026-04-18 09:12:34" (no TZ).
    // Treat as UTC.
    if let Ok(naive) = chrono::NaiveDateTime::parse_from_str(ts, "%Y-%m-%d %H:%M:%S") {
        return Some(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    if let Ok(dt) = DateTime::parse_from_rfc3339(ts) {
        return Some(dt.with_timezone(&Utc));
    }
    None
}

fn minutes_since(ts: &str) -> Option<u64> {
    let then = parse_ts(ts)?;
    let now = Utc::now();
    let secs = (now - then).num_seconds();
    if secs < 0 {
        Some(0)
    } else {
        Some((secs / 60) as u64)
    }
}

fn is_within_hours(ts: &str, hours: u32) -> bool {
    minutes_since(ts).map(|m| m <= (hours as u64) * 60).unwrap_or(false)
}

fn hours_between(a: &str, b: &str) -> Option<f64> {
    let ta = parse_ts(a)?;
    let tb = parse_ts(b)?;
    let secs = (tb - ta).num_seconds().abs();
    Some(secs as f64 / 3600.0)
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn point(value: f64, mins_ago: i64) -> MetricPoint {
        let ts = Utc::now() - Duration::minutes(mins_ago);
        MetricPoint {
            value,
            timestamp: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    fn state(msg: &str, mins_ago: i64) -> LogEntry {
        let ts = Utc::now() - Duration::minutes(mins_ago);
        LogEntry {
            severity: "state".to_string(),
            message: msg.to_string(),
            timestamp: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    #[test]
    fn rssi_ok_for_strong_signal() {
        let samples: Vec<MetricPoint> = (0..60).map(|i| point(-55.0, (i * 20) as i64)).collect();
        let f = check_rssi_health(&samples, None);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn rssi_warn_for_moderate_signal() {
        let samples: Vec<MetricPoint> = (0..60).map(|i| point(-70.0, (i * 20) as i64)).collect();
        let f = check_rssi_health(&samples, None);
        assert_eq!(f.level, LEVEL_WARN);
    }

    #[test]
    fn rssi_fail_for_weak_signal() {
        let samples: Vec<MetricPoint> = (0..60).map(|i| point(-82.0, (i * 20) as i64)).collect();
        let f = check_rssi_health(&samples, None);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn rssi_info_when_empty_and_no_live() {
        let f = check_rssi_health(&[], None);
        assert_eq!(f.level, LEVEL_INFO);
    }

    #[test]
    fn heap_low_fail_below_10k() {
        let samples: Vec<MetricPoint> =
            (0..30).map(|i| point(8_000.0 + (i as f64) * 10.0, (i * 40) as i64)).collect();
        let f = check_heap_low(&samples, None);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn heap_low_ok_for_healthy_device() {
        let samples: Vec<MetricPoint> =
            (0..30).map(|i| point(80_000.0, (i * 40) as i64)).collect();
        let f = check_heap_low(&samples, None);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn heap_trend_fail_for_leak() {
        // Heap dropping ~1000 bytes per sample → big negative slope.
        let samples: Vec<MetricPoint> = (0..40)
            .map(|i| point(100_000.0 - (i as f64) * 1000.0, (40 - i as i64) * 20))
            .collect();
        let f = check_heap_trend(&samples);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn heap_trend_ok_for_stable() {
        let samples: Vec<MetricPoint> =
            (0..40).map(|i| point(80_000.0, (40 - i as i64) * 20)).collect();
        let f = check_heap_trend(&samples);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn heap_trend_info_too_few_samples() {
        let samples: Vec<MetricPoint> =
            (0..5).map(|i| point(80_000.0, (i * 20) as i64)).collect();
        let f = check_heap_trend(&samples);
        assert_eq!(f.level, LEVEL_INFO);
    }

    #[test]
    fn uptime_ok_when_mostly_online() {
        // Steady online for 24h: no transitions.
        let logs: Vec<LogEntry> = vec![];
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_uptime_percent(&refs);
        assert_eq!(f.level, LEVEL_INFO);
    }

    #[test]
    fn uptime_fail_for_long_outage() {
        // Went offline 20h ago, online 1h ago → 1h online out of 21h since first event.
        let logs = vec![state("offline", 20 * 60), state("online", 60)];
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_uptime_percent(&refs);
        // With the "infer opposite of first transition" rule: first=offline →
        // inferred initial=online, so the 4h before the first transition counts
        // as online. That gives 4 + 1 = 5h online vs 19h offline → 21% uptime.
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn reconnect_fail_for_flapping() {
        let logs: Vec<LogEntry> = (0..12)
            .map(|i| state("online", 30 + i * 50))
            .collect();
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_reconnect_count(&refs);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn reconnect_ok_for_stable() {
        let logs = vec![state("online", 30)];
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_reconnect_count(&refs);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn error_rate_fail_for_many_errors() {
        let logs: Vec<LogEntry> = (0..15)
            .map(|i| LogEntry {
                severity: "error".to_string(),
                message: format!("err {}", i),
                timestamp: (Utc::now() - Duration::minutes(i * 30))
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            })
            .collect();
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate(&refs);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn error_rate_ok_when_clean() {
        let logs: Vec<LogEntry> = vec![];
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate(&refs);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn roll_up_unhealthy_if_any_fail() {
        let findings = vec![
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None },
            Finding { id: "b".into(), level: LEVEL_FAIL.into(), title: "".into(), detail: "".into(), suggestion: None },
            Finding { id: "c".into(), level: LEVEL_WARN.into(), title: "".into(), detail: "".into(), suggestion: None },
        ];
        assert_eq!(roll_up(&findings), "unhealthy");
    }

    #[test]
    fn roll_up_attention_for_warn_only() {
        let findings = vec![
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None },
            Finding { id: "b".into(), level: LEVEL_WARN.into(), title: "".into(), detail: "".into(), suggestion: None },
        ];
        assert_eq!(roll_up(&findings), "attention");
    }

    #[test]
    fn severity_rank_orders_most_urgent_first() {
        let mut labels = vec!["good", "unhealthy", "attention", "good", "unhealthy"];
        labels.sort_by_key(|l| severity_rank(l));
        assert_eq!(
            labels,
            vec!["unhealthy", "unhealthy", "attention", "good", "good"]
        );
    }

    #[test]
    fn roll_up_good_when_all_ok_or_info() {
        let findings = vec![
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None },
            Finding { id: "b".into(), level: LEVEL_INFO.into(), title: "".into(), detail: "".into(), suggestion: None },
        ];
        assert_eq!(roll_up(&findings), "good");
    }
}
