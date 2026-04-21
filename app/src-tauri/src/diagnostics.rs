use crate::db::{Database, LogEntry, MdnsCadenceSample, MetricPoint, ResetEvent};
use crate::device::Device;
use chrono::{DateTime, Utc};
use serde::Serialize;

/// The most urgent finding on a device, surfaced inline on the Fleet Health
/// widget so users don't need to click-through to see *why* a device is in the
/// attention/unhealthy bucket.
#[derive(Debug, Clone, Serialize)]
pub struct TopFinding {
    pub level: String,
    pub title: String,
    pub detail: String,
}

/// Per-device entry in a fleet health report.
#[derive(Debug, Clone, Serialize)]
pub struct FleetDeviceEntry {
    pub device_id: String,
    pub name: String,
    pub online: bool,
    pub overall: String,
    pub critical: u32,
    pub warnings: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_finding: Option<TopFinding>,
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

/// Structured one-click remediation exposed on a finding. `action_type` tells
/// the UI which button to render; `data` carries the payload the handler needs
/// (for `firmware_update`: download url, release tag, asset name, device info).
#[derive(Debug, Clone, Serialize)]
pub struct FindingAction {
    pub label: String,
    pub action_type: String,
    pub data: serde_json::Value,
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub action: Option<FindingAction>,
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

/// A GitHub release considered eligible for firmware auto-remediation.
/// Pre-computed by the caller of `diagnose` so the pure rule engine stays
/// synchronous. When set, the firmware_age rule escalates to WARN and
/// exposes a one-click OTA action.
#[derive(Debug, Clone)]
pub struct EligibleRelease {
    pub release_tag: String,
    pub asset_name: String,
    pub download_url: String,
}

/// Parse a firmware / release version string into (major, minor, patch).
/// Tolerates a leading `v` and a `-prerelease` suffix. Returns `None` if the
/// string isn't a parseable numeric semver — callers should skip the "is
/// newer" check rather than guess.
pub fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let trimmed = s.trim();
    let core = trimmed.trim_start_matches('v').trim_start_matches('V');
    let core = core.split(['-', '+']).next().unwrap_or(core);
    let parts: Vec<&str> = core.split('.').collect();
    if parts.is_empty() {
        return None;
    }
    let major = parts.first().and_then(|p| p.parse().ok())?;
    let minor = parts.get(1).and_then(|p| p.parse().ok()).unwrap_or(0);
    let patch = parts.get(2).and_then(|p| p.parse().ok()).unwrap_or(0);
    Some((major, minor, patch))
}

/// True iff `candidate` parses to a strictly newer version than `current`.
/// Anything unparseable returns false (don't nag users about unknown-shape versions).
pub fn is_newer_version(candidate: &str, current: &str) -> bool {
    match (parse_version(candidate), parse_version(current)) {
        (Some(c), Some(cur)) => c > cur,
        _ => false,
    }
}

pub fn diagnose(
    db: &Database,
    device_id: &str,
    live_device: Option<&Device>,
    eligible_update: Option<&EligibleRelease>,
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
    findings.push(check_error_rate_trend(&errors_in_window));

    let history = db.get_firmware_history(device_id)?;
    findings.push(check_firmware_age(&history, live_device, eligible_update));
    findings.push(check_ota_success_rate(&history));

    let resets = db.get_resets(device_id, WINDOW_HOURS)?;
    findings.push(check_power_supply_stability(&resets));

    let mdns_samples = db.get_mdns_cadence_samples(device_id, WINDOW_HOURS)?;
    findings.push(check_mdns_latency(&mdns_samples));

    let nvs_writes = db.get_metrics(device_id, "_nvs_writes", WINDOW_HOURS)?;
    findings.push(check_flash_wear(&nvs_writes));

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
                action: None,
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
        action: None,
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
                action: None,
            };
        }
        return Finding {
            id: "rssi_health".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "WiFi signal strength".to_string(),
            detail: "No RSSI samples recorded in the last 24h.".to_string(),
            suggestion: None,
            action: None,
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
        action: None,
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
                action: None,
            };
        }
        return Finding {
            id: "heap_low".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "Free memory".to_string(),
            detail: "No free-heap samples recorded in the last 24h.".to_string(),
            suggestion: None,
            action: None,
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
        action: None,
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
            action: None,
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
        action: None,
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
            action: None,
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
            action: None,
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
        action: None,
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
        action: None,
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
        action: None,
    }
}

/// Compare error/warn events in the last hour to the preceding 23h baseline.
/// Differentiates "something just started breaking" from the existing
/// `check_error_rate` rule, which only looks at the 24h total and can't tell
/// whether the noise is fresh or old-news.
fn check_error_rate_trend(error_logs: &[&LogEntry]) -> Finding {
    let mut last_hour = 0u32;
    let mut prior = 0u32;
    for l in error_logs {
        match minutes_since(&l.timestamp) {
            Some(m) if m <= 60 => last_hour += 1,
            Some(m) if m <= (WINDOW_HOURS as u64) * 60 => prior += 1,
            _ => {}
        }
    }
    let baseline_per_hour = (prior as f64) / 23.0;

    let zero_baseline = prior == 0;
    let ratio = if zero_baseline {
        f64::INFINITY
    } else {
        (last_hour as f64) / baseline_per_hour
    };

    let level = if last_hour >= 10 && (zero_baseline || ratio >= 3.0) {
        LEVEL_FAIL
    } else if last_hour >= 5 && (zero_baseline || ratio >= 2.0) {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };

    let detail = if last_hour == 0 {
        format!(
            "No error or warn events in the last hour ({:.1}/h average over preceding 23h).",
            baseline_per_hour
        )
    } else if zero_baseline {
        format!(
            "{} error/warn events in the last hour with no prior events in the preceding 23h.",
            last_hour
        )
    } else {
        format!(
            "{} error/warn events in the last hour vs {:.1}/h average over preceding 23h ({:.1}x).",
            last_hour, baseline_per_hour, ratio
        )
    };

    let suggestion = if level != LEVEL_OK {
        Some(
            "Error activity just accelerated. Open the device detail panel and scroll the Error/Warn log chips to see what started firing recently."
                .to_string(),
        )
    } else {
        None
    };

    Finding {
        id: "error_rate_trend".to_string(),
        level: level.to_string(),
        title: "Error rate trend".to_string(),
        detail,
        suggestion,
        action: None,
    }
}

fn check_firmware_age(
    history: &[crate::db::FirmwareRecord],
    live: Option<&Device>,
    eligible_update: Option<&EligibleRelease>,
) -> Finding {
    // Pick the "current" firmware version for comparison against an eligible
    // release. Prefer the live-reported value (authoritative); fall back to
    // the most recent OTA record; fall back to empty.
    let current_version: String = live
        .map(|d| d.firmware.clone())
        .filter(|v| !v.is_empty())
        .or_else(|| history.first().map(|r| r.version.clone()))
        .unwrap_or_default();

    let age_detail = history.first().map(|latest| {
        let days = minutes_since(&latest.uploaded_at)
            .map(|m| m / 60 / 24)
            .unwrap_or(0);
        if days == 0 {
            format!("Firmware v{} pushed today.", latest.version)
        } else if days == 1 {
            format!("Firmware v{} pushed yesterday.", latest.version)
        } else {
            format!("Firmware v{} pushed {} days ago.", latest.version, days)
        }
    });

    // If an eligible release was pre-fetched AND it's strictly newer than
    // the device's current version, escalate to WARN with a one-click action.
    if let Some(release) = eligible_update {
        if !current_version.is_empty()
            && is_newer_version(&release.release_tag, &current_version)
        {
            let current_label = if current_version.starts_with('v') {
                current_version.clone()
            } else {
                format!("v{}", current_version)
            };
            let detail = format!(
                "{} is available (currently {}).",
                release.release_tag, current_label
            );
            let action_data = serde_json::json!({
                "release_tag": release.release_tag,
                "asset_name": release.asset_name,
                "download_url": release.download_url,
            });
            return Finding {
                id: "firmware_age".to_string(),
                level: LEVEL_WARN.to_string(),
                title: "Firmware update available".to_string(),
                detail,
                suggestion: Some(
                    "A newer firmware release is published in the bound GitHub repo. Click Update to flash it over the air.".to_string(),
                ),
                action: Some(FindingAction {
                    label: format!("Update to {}", release.release_tag),
                    action_type: "firmware_update".to_string(),
                    data: action_data,
                }),
            };
        }
    }

    // Fallback: keep the existing INFO-only behavior.
    let detail = age_detail.unwrap_or_else(|| {
        "No firmware OTA updates recorded for this device.".to_string()
    });
    Finding {
        id: "firmware_age".to_string(),
        level: LEVEL_INFO.to_string(),
        title: "Firmware".to_string(),
        detail,
        suggestion: None,
        action: None,
    }
}

/// OTA apply success rate over the last `OTA_WINDOW` attempts that have a
/// recorded outcome. Pre-v0.15.0 rows have `delivery_status = NULL` and are
/// ignored — the rule only earns trust as new uploads accumulate. Stays
/// silent (skipped) until at least `OTA_MIN_SAMPLES` attempts have been
/// recorded so we don't fail-flag a single bad upload on a fresh device.
/// User-initiated cancellations (`delivery_status = "cancelled"`, v0.16.0)
/// are excluded from both the numerator and denominator — they represent
/// a human abort, not a delivery failure.
///
/// Two-phase semantics (v0.16.0):
/// - Numerator counts rows the device has explicitly acknowledged applying
///   (`delivery_applied_at IS NOT NULL`).
/// - Rows delivered but not yet applied are ambiguous — the bytes left the
///   desktop socket but the device hasn't booted + POSTed ack. Until
///   `OTA_APPLY_GRACE_HOURS` has elapsed they don't count against the
///   device (grace period for the reboot). After that they count as a
///   miss and the detail line surfaces the "bytes sent but no apply
///   confirmation" category so admins can distinguish the two failure
///   modes: never-delivered vs delivered-but-not-applied.
///
/// Rollbacks (and any pre-v0.16.0 OTA) serve firmware with no nonce, so
/// their rows have `delivery_ack_nonce = NULL` — those are treated as
/// "always applied on delivery" to avoid false negatives from paths that
/// predate two-phase tracking.
fn check_ota_success_rate(history: &[crate::db::FirmwareRecord]) -> Finding {
    const OTA_WINDOW: usize = 10;
    const OTA_MIN_SAMPLES: usize = 3;

    let recent: Vec<&crate::db::FirmwareRecord> = history
        .iter()
        .filter(|r| {
            r.delivery_status.is_some() && r.delivery_status.as_deref() != Some("cancelled")
        })
        .take(OTA_WINDOW)
        .collect();
    let total = recent.len();

    if total < OTA_MIN_SAMPLES {
        return Finding {
            id: "ota_success_rate".to_string(),
            level: LEVEL_INFO.to_string(),
            title: "OTA delivery success rate".to_string(),
            detail: format!(
                "{} OTA outcome{} recorded so far (need {} for trend).",
                total,
                if total == 1 { "" } else { "s" },
                OTA_MIN_SAMPLES
            ),
            suggestion: None,
            action: None,
        };
    }

    // Count applied (device-confirmed) rows. A row without an ack nonce
    // is a rollback / pre-v0.16.0 row — treat "delivered" there as good
    // since there was never an ack path to walk.
    let applied = recent
        .iter()
        .filter(|r| {
            r.delivery_applied_at.is_some()
                || (r.delivery_status.as_deref() == Some("delivered")
                    && !has_ack_window(r))
        })
        .count();
    let delivered_not_applied = recent
        .iter()
        .filter(|r| {
            r.delivery_status.as_deref() == Some("delivered")
                && r.delivery_applied_at.is_none()
                && has_ack_window(r)
        })
        .count();
    let success_rate = (applied as f64) / (total as f64);
    let pct = success_rate * 100.0;

    let level = if success_rate < 0.5 {
        LEVEL_FAIL
    } else if success_rate < 0.8 {
        LEVEL_WARN
    } else {
        LEVEL_OK
    };

    // When the rule tips WARN/FAIL, surface the most recent failure reason
    // (captured in firmware_history.delivery_error, v0.15.0) so admins see
    // the failure category inline instead of having to cross-reference the
    // firmware history tab. Two-phase (v0.16.0) adds a second, distinct
    // category — "delivered but the device never confirmed apply" — which
    // points at reboot failures (bad firmware, power loss) rather than
    // transport issues.
    let last_error = if level != LEVEL_OK {
        recent.iter()
            .find(|r| r.delivery_status.as_deref() == Some("failed"))
            .and_then(|r| r.delivery_error.as_deref())
            .filter(|s| !s.is_empty())
    } else {
        None
    };
    let detail = match (last_error, delivered_not_applied, level) {
        (Some(err), _, _) => format!(
            "{}/{} of the last OTA uploads were confirmed applied ({:.0}%). Last error: {}.",
            applied, total, pct, err
        ),
        (None, n, lvl) if n > 0 && lvl != LEVEL_OK => format!(
            "{}/{} of the last OTA uploads were confirmed applied ({:.0}%). {} delivered but the device never confirmed the new firmware booted.",
            applied, total, pct, n
        ),
        _ => format!(
            "{}/{} of the last OTA uploads were confirmed applied ({:.0}%).",
            applied, total, pct
        ),
    };
    let suggestion = if level != LEVEL_OK {
        Some(
            "OTA uploads are dropping mid-transfer or failing to boot the new firmware. Check WiFi signal strength on the device, or push the firmware while the device is on the same AP as Trellis."
                .to_string(),
        )
    } else {
        None
    };

    Finding {
        id: "ota_success_rate".to_string(),
        level: level.to_string(),
        title: "OTA delivery success rate".to_string(),
        detail,
        suggestion,
        action: None,
    }
}

/// True when enough time has passed since `delivered_at` for the device to
/// have realistically booted and POSTed its ack. Delivered-but-not-applied
/// rows inside this window are treated as pending, not miss.
/// Matches the device's boot+WiFi reconnect budget with margin.
fn has_ack_window(r: &crate::db::FirmwareRecord) -> bool {
    const OTA_APPLY_GRACE_MINUTES: i64 = 5;
    let Some(delivered_at) = r.delivered_at.as_deref() else {
        return false;
    };
    let Ok(ts) = chrono::NaiveDateTime::parse_from_str(delivered_at, "%Y-%m-%d %H:%M:%S")
    else {
        return false;
    };
    let now = chrono::Utc::now().naive_utc();
    (now - ts) >= chrono::Duration::minutes(OTA_APPLY_GRACE_MINUTES)
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
    eligible_updates: &std::collections::HashMap<String, EligibleRelease>,
) -> Result<FleetReport, String> {
    let saved = db.get_all_saved_devices()?;
    let mut good = 0u32;
    let mut attention = 0u32;
    let mut unhealthy = 0u32;
    let mut devices: Vec<FleetDeviceEntry> = Vec::with_capacity(saved.len());

    for sd in &saved {
        let live = live_devices.iter().find(|d| d.id == sd.id);
        let eligible = eligible_updates.get(&sd.id);
        let report = match diagnose(db, &sd.id, live, eligible) {
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
        let top_finding = pick_top_finding(&report.findings);
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
            top_finding,
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

/// Pick the most urgent finding for surfacing inline on the Fleet Health row:
/// first FAIL (in the order rules were evaluated in `diagnose`), else first
/// WARN, else None. OK/INFO never get surfaced — they're not actionable on a
/// rollup widget.
fn pick_top_finding(findings: &[Finding]) -> Option<TopFinding> {
    findings
        .iter()
        .find(|f| f.level == LEVEL_FAIL)
        .or_else(|| findings.iter().find(|f| f.level == LEVEL_WARN))
        .map(|f| TopFinding {
            level: f.level.clone(),
            title: f.title.clone(),
            detail: f.detail.clone(),
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

// ─── Power-supply stability (post-v0.16.0) ───────────────────────────────────

/// Reset reasons that should never happen on a healthy device. Brownouts are
/// the direct power-supply signal; panics and watchdog timeouts are secondary
/// — they frequently ride along during PSU sag (rail dips deep enough to
/// corrupt a transaction but not deep enough to trip the brownout detector).
///
/// Reasons kept OFF this list are expected: `poweron` (cold boot), `software`
/// (our own ESP.restart() from OTA), `external` (reset pin), `deepsleep`
/// (normal wake), `unknown` (old firmware or new IDF enum we don't map yet —
/// would false-positive otherwise).
fn is_unexpected_reset(reason: &str) -> bool {
    matches!(
        reason,
        "brownout" | "panic" | "interrupt_watchdog" | "task_watchdog" | "watchdog"
    )
}

fn check_power_supply_stability(resets: &[ResetEvent]) -> Finding {
    let id = "power_supply_stability".to_string();
    let title = "Power-supply stability".to_string();

    if resets.is_empty() {
        return Finding {
            id,
            level: LEVEL_INFO.to_string(),
            title,
            detail: format!("No reboots observed in the last {}h.", WINDOW_HOURS),
            suggestion: None,
            action: None,
        };
    }

    let brownouts = resets.iter().filter(|r| r.reset_reason == "brownout").count();
    let unexpected = resets.iter().filter(|r| is_unexpected_reset(&r.reset_reason)).count();
    let total = resets.len();

    let (level, suggestion_extra) = if brownouts >= 2 {
        (
            LEVEL_FAIL,
            Some("Two or more brownouts in 24h strongly suggest the power supply can't hold the rail under load. Swap to a higher-current USB adapter (≥1 A for ESP32, ≥2 A for ESP32-S3/variants with large WiFi bursts), use shorter/thicker USB cables, or add a bulk decoupling cap near the ESP32 VCC pin."),
        )
    } else if brownouts >= 1 || unexpected >= 3 {
        (
            LEVEL_WARN,
            Some("One brownout or a cluster of unexpected resets in the last 24h. Monitor — if the pattern continues, treat it as a power-supply issue first before chasing firmware bugs."),
        )
    } else {
        (LEVEL_OK, None)
    };

    // Breakdown ordered by how diagnostic each reason is.
    let mut breakdown: Vec<String> = Vec::new();
    for (label, reason) in [
        ("brownout", "brownout"),
        ("panic", "panic"),
        ("watchdog", "watchdog"),
        ("task_watchdog", "task_watchdog"),
        ("interrupt_watchdog", "interrupt_watchdog"),
        ("poweron", "poweron"),
        ("software", "software"),
        ("external", "external"),
        ("deepsleep", "deepsleep"),
    ] {
        let n = resets.iter().filter(|r| r.reset_reason == reason).count();
        if n > 0 {
            breakdown.push(format!("{} {}", n, label));
        }
    }
    // Collect anything we don't explicitly name (e.g. "unknown" from pre-v0.17.0 firmwares).
    let named: std::collections::HashSet<&str> = [
        "brownout", "panic", "watchdog", "task_watchdog", "interrupt_watchdog",
        "poweron", "software", "external", "deepsleep",
    ].iter().copied().collect();
    let other = resets.iter().filter(|r| !named.contains(r.reset_reason.as_str())).count();
    if other > 0 {
        breakdown.push(format!("{} other", other));
    }

    let detail = format!(
        "{} reboot{} in last {}h ({}).",
        total,
        if total == 1 { "" } else { "s" },
        WINDOW_HOURS,
        breakdown.join(", "),
    );

    Finding {
        id,
        level: level.to_string(),
        title,
        detail,
        suggestion: suggestion_extra.map(|s| s.to_string()),
        action: None,
    }
}

/// Rule: surface mDNS announcement cadence stretching beyond the
/// expected TTL-driven refresh rate. Reads trailing inter-Resolved
/// intervals; escalates on elevated p95 (announcements arriving late or
/// being dropped). Below the minimum sample floor the rule is INFO —
/// freshly-discovered devices won't have accumulated intervals yet, and
/// cross-subnet devices (reached via the health-check loop) never will.
///
/// Threshold reasoning: ESPmDNS on Arduino advertises with a default
/// 120s TTL and clients (mdns-sd-rs) re-query at ~80% of TTL, so a
/// healthy steady state lands intervals in the 60–120s band. Thresholds
/// reference the TTL rather than a hardcoded latency window:
///   - p95 < 2.5× TTL (≤ 300s) → OK
///   - p95 in [2.5×, 5×] TTL (300–600s) → WARN (occasional drops)
///   - p95 ≥ 5× TTL (≥ 600s) → FAIL (likely offline / flaking)
/// Keeping the rule id as `mdns_latency` so dashboards, alerts, and
/// stored configurations referencing it don't churn across versions,
/// even though the underlying signal is now cadence rather than
/// Found→Resolved elapsed time.
fn check_mdns_latency(samples: &[MdnsCadenceSample]) -> Finding {
    let id = "mdns_latency".to_string();
    let title = "mDNS announcement cadence".to_string();

    const MIN_SAMPLES: usize = 3;
    const EXPECTED_TTL_S: u32 = 120;
    const P95_WARN_S: u32 = 300;
    const P95_FAIL_S: u32 = 600;

    if samples.is_empty() {
        // Distinct text from the "1-2 samples" case: 0 is the common
        // cross-subnet shape (no multicast traversal, discovery happens
        // via HTTP health-check), so it's worth calling out explicitly
        // rather than lumping with "still ramping up."
        return Finding {
            id,
            level: LEVEL_INFO.to_string(),
            title,
            detail: format!(
                "No mDNS cadence samples in last {}h. Cross-subnet devices are discovered via HTTP health-check instead (mDNS doesn't traverse subnets), so an empty reading is expected on those. Need \u{2265}{} for a reading.",
                WINDOW_HOURS, MIN_SAMPLES,
            ),
            suggestion: None,
            action: None,
        };
    }

    if samples.len() < MIN_SAMPLES {
        return Finding {
            id,
            level: LEVEL_INFO.to_string(),
            title,
            detail: format!(
                "{} mDNS cadence sample{} in last {}h. Each sample is the interval between TTL-driven re-announcements — need \u{2265}{} for a stable reading.",
                samples.len(),
                if samples.len() == 1 { "" } else { "s" },
                WINDOW_HOURS,
                MIN_SAMPLES,
            ),
            suggestion: None,
            action: None,
        };
    }

    let mut ms: Vec<u32> = samples.iter().map(|s| s.interval_ms).collect();
    ms.sort_unstable();
    let p50_s = ms[ms.len() / 2] / 1000;
    // p95 index: ceil(0.95 * n) - 1, clamped to last. For n=3 this is 2
    // (the max); for n=20 it is 18. Small samples lean on the max which
    // is the intent — single-spike sensitivity at low N.
    let p95_idx = (((samples.len() as f64) * 0.95).ceil() as usize)
        .saturating_sub(1)
        .min(samples.len() - 1);
    let p95_s = ms[p95_idx] / 1000;

    let (level, suggestion) = if p95_s >= P95_FAIL_S {
        (
            LEVEL_FAIL,
            Some("mDNS announcements from this device are arriving far slower than the expected TTL refresh cadence. The device is likely dropping off the network intermittently, or the LAN path to it is lossy. Check WiFi signal (RSSI rule), confirm the device's uptime isn't cycling, and rule out the access-point side before chasing firmware — a spectrum-analyser scan or moving the device closer to the AP usually isolates this within a few minutes."),
        )
    } else if p95_s >= P95_WARN_S {
        (
            LEVEL_WARN,
            Some("mDNS cadence is stretched past healthy baseline — occasional announcements are being missed. Common causes: flaky WiFi, a saturated AP, or an overloaded mdns-sd daemon on this machine. If it persists, restart the desktop app to reset mdns-sd state; if that doesn't clear it, treat the LAN path as suspect."),
        )
    } else {
        (LEVEL_OK, None)
    };

    Finding {
        id,
        level: level.to_string(),
        title,
        detail: format!(
            "{} sample{} in last {}h — p50 {}s, p95 {}s (expected cadence \u{2248} {}s).",
            samples.len(),
            if samples.len() == 1 { "" } else { "s" },
            WINDOW_HOURS,
            p50_s,
            p95_s,
            EXPECTED_TTL_S,
        ),
        suggestion: suggestion.map(|s| s.to_string()),
        action: None,
    }
}

/// Rule: surface unhealthy NVS-write rates that would chew through the ESP32
/// flash partition prematurely. The library samples a RAM-only monotonic
/// counter of capability-persist operations (setSwitch / setSlider) and
/// reports it via `system.nvs_writes` on each heartbeat. Because the counter
/// resets to 0 on reboot, rate is computed as the sum of positive deltas
/// between consecutive samples — negative deltas (counter going backward) are
/// treated as reboot boundaries and skipped.
///
/// Threshold reasoning: a typical ESP32 NVS partition is ~20 KB / 5 sectors
/// at ~100k erase cycles per sector, with ~2× write amplification from
/// wear-leveling. That is on the order of 250k lifetime writes before the
/// first sector starts to wear. Translating to sustained rates:
///   - <500/24h  → decades of life → OK
///   - 500–5000  → years → INFO (active automation, fine)
///   - 5000–20k  → months → WARN (investigate automation loops)
///   - >20k      → weeks → FAIL (pathological, will wear the partition)
fn check_flash_wear(samples: &[MetricPoint]) -> Finding {
    let id = "flash_wear".to_string();
    let title = "Flash-wear rate".to_string();

    if samples.len() < 2 {
        return Finding {
            id,
            level: LEVEL_INFO.to_string(),
            title,
            detail: format!(
                "{} nvs_writes sample{} in last {}h. Need at least 2 samples over a non-trivial span for a rate reading.",
                samples.len(),
                if samples.len() == 1 { "" } else { "s" },
                WINDOW_HOURS,
            ),
            suggestion: None,
            action: None,
        };
    }

    // MetricPoint::value is f64; the counter is a uint32 on the device side so
    // fractional values never occur. Sum positive deltas only; a negative
    // delta means the device rebooted between samples and the counter was
    // reset to 0 — that transition carries no write-rate information.
    let mut total: f64 = 0.0;
    for pair in samples.windows(2) {
        let delta = pair[1].value - pair[0].value;
        if delta > 0.0 {
            total += delta;
        }
    }

    // Time span covered by the samples.
    let first_ts = parse_ts(&samples.first().unwrap().timestamp);
    let last_ts = parse_ts(&samples.last().unwrap().timestamp);
    let span_hours = match (first_ts, last_ts) {
        (Some(a), Some(b)) => (b - a).num_seconds() as f64 / 3600.0,
        _ => 0.0,
    };

    if span_hours < 1.0 {
        return Finding {
            id,
            level: LEVEL_INFO.to_string(),
            title,
            detail: format!(
                "{} NVS write{} recorded across {} sample{} in less than 1h — not enough span for a rate reading yet.",
                total as u64,
                if total as u64 == 1 { "" } else { "s" },
                samples.len(),
                if samples.len() == 1 { "" } else { "s" },
            ),
            suggestion: None,
            action: None,
        };
    }

    let rate_per_24h = total / span_hours * 24.0;

    let (level, suggestion): (&str, Option<&str>) = if rate_per_24h >= 20_000.0 {
        (
            LEVEL_FAIL,
            Some("NVS writes are happening at a rate that will wear the flash partition within weeks. This is almost always a tight automation loop (a rule or scene firing setSwitch/setSlider repeatedly) rather than real user intent. Inspect your scenes, schedules, and rules for anything that toggles a capability without a state check — the library persists every set call, not just changes."),
        )
    } else if rate_per_24h >= 5_000.0 {
        (
            LEVEL_WARN,
            Some("NVS write rate is high enough to shorten flash lifetime to months. Check whether any rule or scene is re-setting the same capability at short intervals — the library persists every setSwitch/setSlider, whether the value changed or not."),
        )
    } else if rate_per_24h >= 500.0 {
        (
            LEVEL_INFO,
            Some("NVS write rate is elevated but within a healthy range (years of flash life). Typical for devices driven by active automation; no action needed unless the rate keeps climbing."),
        )
    } else {
        (LEVEL_OK, None)
    };

    Finding {
        id,
        level: level.to_string(),
        title,
        detail: format!(
            "{:.0} NVS writes/24h projected ({} write{} across {} sample{} spanning {:.1}h).",
            rate_per_24h,
            total as u64,
            if total as u64 == 1 { "" } else { "s" },
            samples.len(),
            if samples.len() == 1 { "" } else { "s" },
            span_hours,
        ),
        suggestion: suggestion.map(|s| s.to_string()),
        action: None,
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

    // ─── Error-rate trend (acceleration detector) ────────────────────────────

    fn err_log(mins_ago: i64) -> LogEntry {
        LogEntry {
            severity: "error".to_string(),
            message: "boom".to_string(),
            timestamp: (Utc::now() - Duration::minutes(mins_ago))
                .format("%Y-%m-%d %H:%M:%S")
                .to_string(),
        }
    }

    #[test]
    fn error_rate_trend_ok_when_silent() {
        let logs: Vec<LogEntry> = vec![];
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn error_rate_trend_fail_for_sudden_spike_with_zero_baseline() {
        // 12 events all within last hour, nothing prior.
        let logs: Vec<LogEntry> = (0..12).map(|i| err_log(i * 4)).collect();
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn error_rate_trend_fail_for_3x_spike_with_baseline() {
        // Last hour: 15 events. Preceding 23h: 23 events → baseline 1.0/h. Ratio 15x.
        let mut logs: Vec<LogEntry> = (0..15).map(|i| err_log(i * 3)).collect();
        logs.extend((0..23).map(|i| err_log(90 + i * 50))); // 90m → ~20h ago
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_FAIL);
    }

    #[test]
    fn error_rate_trend_warn_for_moderate_spike() {
        // Last hour: 6 events. Preceding 23h: 23 events → baseline 1.0/h. Ratio 6x.
        // Fails the >=10 bar so WARN, not FAIL.
        let mut logs: Vec<LogEntry> = (0..6).map(|i| err_log(i * 8)).collect();
        logs.extend((0..23).map(|i| err_log(90 + i * 50)));
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_WARN);
    }

    #[test]
    fn error_rate_trend_ok_when_ratio_below_threshold() {
        // Last hour: 4 events. Preceding 23h: 69 events → baseline 3.0/h. Ratio 1.3x.
        // Below the 5-event floor anyway → OK.
        let mut logs: Vec<LogEntry> = (0..4).map(|i| err_log(i * 12)).collect();
        logs.extend((0..69).map(|i| err_log(90 + i * 15)));
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn error_rate_trend_ok_when_noise_is_steady() {
        // Last hour: 5 events. Preceding 23h: 115 events → baseline 5.0/h. Ratio 1.0x.
        // Same rate as baseline — not a spike, even though total is noisy.
        let mut logs: Vec<LogEntry> = (0..5).map(|i| err_log(i * 10)).collect();
        logs.extend((0..115).map(|i| err_log(90 + i * 10)));
        let refs: Vec<&LogEntry> = logs.iter().collect();
        let f = check_error_rate_trend(&refs);
        assert_eq!(f.level, LEVEL_OK);
    }

    #[test]
    fn roll_up_unhealthy_if_any_fail() {
        let findings = vec![
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
            Finding { id: "b".into(), level: LEVEL_FAIL.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
            Finding { id: "c".into(), level: LEVEL_WARN.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
        ];
        assert_eq!(roll_up(&findings), "unhealthy");
    }

    #[test]
    fn roll_up_attention_for_warn_only() {
        let findings = vec![
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
            Finding { id: "b".into(), level: LEVEL_WARN.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
        ];
        assert_eq!(roll_up(&findings), "attention");
    }

    fn finding(id: &str, level: &str) -> Finding {
        Finding {
            id: id.into(),
            level: level.into(),
            title: format!("Title {}", id),
            detail: format!("Detail {}", id),
            suggestion: None,
            action: None,
        }
    }

    #[test]
    fn top_finding_prefers_fail_over_warn() {
        let findings = vec![
            finding("a", LEVEL_OK),
            finding("b", LEVEL_WARN),
            finding("c", LEVEL_FAIL),
            finding("d", LEVEL_FAIL),
        ];
        let top = pick_top_finding(&findings).expect("should have a top finding");
        assert_eq!(top.level, LEVEL_FAIL);
        // First fail in iteration order wins, not the last.
        assert_eq!(top.title, "Title c");
    }

    #[test]
    fn top_finding_falls_back_to_warn_when_no_fail() {
        let findings = vec![
            finding("a", LEVEL_OK),
            finding("b", LEVEL_INFO),
            finding("c", LEVEL_WARN),
            finding("d", LEVEL_WARN),
        ];
        let top = pick_top_finding(&findings).expect("should have a top finding");
        assert_eq!(top.level, LEVEL_WARN);
        assert_eq!(top.title, "Title c");
    }

    #[test]
    fn top_finding_none_when_all_ok_or_info() {
        let findings = vec![
            finding("a", LEVEL_OK),
            finding("b", LEVEL_INFO),
            finding("c", LEVEL_OK),
        ];
        assert!(pick_top_finding(&findings).is_none());
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
            Finding { id: "a".into(), level: LEVEL_OK.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
            Finding { id: "b".into(), level: LEVEL_INFO.into(), title: "".into(), detail: "".into(), suggestion: None, action: None },
        ];
        assert_eq!(roll_up(&findings), "good");
    }

    // ─── Version parsing + "is newer" ──────────────────────────────────────

    #[test]
    fn parse_version_tolerates_v_prefix_and_prerelease() {
        assert_eq!(parse_version("0.13.0"), Some((0, 13, 0)));
        assert_eq!(parse_version("v0.13.0"), Some((0, 13, 0)));
        assert_eq!(parse_version("V1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3-beta"), Some((1, 2, 3)));
        assert_eq!(parse_version("1.2.3+build"), Some((1, 2, 3)));
        // Missing minor / patch → zero-filled.
        assert_eq!(parse_version("5"), Some((5, 0, 0)));
        assert_eq!(parse_version("5.6"), Some((5, 6, 0)));
    }

    #[test]
    fn parse_version_rejects_non_numeric() {
        // Non-parseable major → None so callers skip the is-newer check.
        assert_eq!(parse_version(""), None);
        assert_eq!(parse_version("release-2024-01"), None);
        assert_eq!(parse_version("abc123"), None);
        assert_eq!(parse_version("v"), None);
    }

    #[test]
    fn is_newer_version_compares_semver_components() {
        assert!(is_newer_version("0.14.0", "0.13.0"));
        assert!(is_newer_version("1.0.0", "0.99.99"));
        assert!(is_newer_version("v0.13.1", "v0.13.0"));
        assert!(!is_newer_version("0.13.0", "0.13.0"));
        assert!(!is_newer_version("0.12.9", "0.13.0"));
    }

    #[test]
    fn is_newer_version_false_when_unparseable() {
        // Release tag like "nightly-2024-01-01" can't be compared — skip, no nag.
        assert!(!is_newer_version("nightly-build", "0.13.0"));
        assert!(!is_newer_version("0.14.0", ""));
    }

    // ─── check_firmware_age with eligible update ──────────────────────────

    fn fw_rec(version: &str, mins_ago: u64) -> crate::db::FirmwareRecord {
        let ts = Utc::now() - chrono::Duration::minutes(mins_ago as i64);
        crate::db::FirmwareRecord {
            id: 1,
            device_id: "test-dev".into(),
            version: version.into(),
            file_path: "".into(),
            file_size: 0,
            uploaded_at: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
            delivery_status: None,
            delivered_at: None,
            delivery_error: None,
            delivery_applied_at: None,
        }
    }

    fn eligible(tag: &str) -> EligibleRelease {
        EligibleRelease {
            release_tag: tag.into(),
            asset_name: "firmware.bin".into(),
            download_url: format!("https://example.test/{}/firmware.bin", tag),
        }
    }

    #[test]
    fn firmware_age_info_without_eligible_update() {
        let history = vec![fw_rec("0.13.0", 60)];
        let f = check_firmware_age(&history, None, None);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.action.is_none());
    }

    #[test]
    fn firmware_age_info_when_up_to_date() {
        // Eligible pre-fetch returned a release with the SAME version → not newer.
        let history = vec![fw_rec("0.13.0", 60)];
        let elig = eligible("0.13.0");
        let f = check_firmware_age(&history, None, Some(&elig));
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.action.is_none());
    }

    #[test]
    fn firmware_age_warns_with_action_when_newer_available() {
        let history = vec![fw_rec("0.13.0", 60)];
        let elig = eligible("0.14.0");
        let f = check_firmware_age(&history, None, Some(&elig));
        assert_eq!(f.level, LEVEL_WARN);
        assert_eq!(f.title, "Firmware update available");
        let a = f.action.expect("action present when update available");
        assert_eq!(a.action_type, "firmware_update");
        assert_eq!(a.data["release_tag"], "0.14.0");
        assert_eq!(a.data["asset_name"], "firmware.bin");
    }

    #[test]
    fn firmware_age_no_action_when_current_version_unknown() {
        // No firmware history, no live device → can't compare; stay INFO.
        let elig = eligible("0.14.0");
        let f = check_firmware_age(&[], None, Some(&elig));
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.action.is_none());
    }

    // ─── check_ota_success_rate ──────────────────────────────────────────

    /// Default helper: a "delivered" row auto-sets `delivery_applied_at` so
    /// historical tests that read "delivered" as "successful OTA" still
    /// pass under the new v0.16.0 semantics (numerator counts applied).
    /// Tests that want to exercise the delivered-but-not-applied branch
    /// call `fw_rec_pending` instead.
    fn fw_rec_with_status(
        version: &str, mins_ago: u64, status: Option<&str>,
    ) -> crate::db::FirmwareRecord {
        fw_rec_full(version, mins_ago, status, None, status == Some("delivered"))
    }

    fn fw_rec_full(
        version: &str, mins_ago: u64, status: Option<&str>, error: Option<&str>,
        applied: bool,
    ) -> crate::db::FirmwareRecord {
        let ts = Utc::now() - Duration::minutes(mins_ago as i64);
        crate::db::FirmwareRecord {
            id: 1,
            device_id: "test-dev".into(),
            version: version.into(),
            file_path: "".into(),
            file_size: 0,
            uploaded_at: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
            delivery_status: status.map(|s| s.to_string()),
            delivered_at: status.map(|_| ts.format("%Y-%m-%d %H:%M:%S").to_string()),
            delivery_error: error.map(|s| s.to_string()),
            delivery_applied_at: if applied { Some(ts.format("%Y-%m-%d %H:%M:%S").to_string()) } else { None },
        }
    }

    /// Delivered, but the device hasn't POSTed the apply ack yet. Age set
    /// to whatever the test wants so it can probe either side of the grace
    /// window (see `has_ack_window` in the rule).
    fn fw_rec_pending(version: &str, mins_ago: u64) -> crate::db::FirmwareRecord {
        fw_rec_full(version, mins_ago, Some("delivered"), None, false)
    }

    #[test]
    fn ota_success_rate_info_when_no_recorded_outcomes() {
        // History exists but all rows are pre-v0.15.0 (delivery_status NULL).
        let history = vec![
            fw_rec_with_status("0.13.0", 10, None),
            fw_rec_with_status("0.12.0", 100, None),
        ];
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("0 OTA outcomes recorded"));
    }

    #[test]
    fn ota_success_rate_info_below_min_samples() {
        let history = vec![
            fw_rec_with_status("0.14.0", 5, Some("delivered")),
            fw_rec_with_status("0.13.0", 100, Some("delivered")),
        ];
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("2 OTA outcomes recorded"));
    }

    #[test]
    fn ota_success_rate_ok_for_high_success() {
        // 9/10 delivered = 90% → OK.
        let mut history: Vec<_> = (0..9)
            .map(|i| fw_rec_with_status("0.14.0", (i * 60) as u64, Some("delivered")))
            .collect();
        history.push(fw_rec_with_status("0.13.0", 600, Some("failed")));
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("9/10"));
        assert!(f.detail.contains("90%"));
    }

    #[test]
    fn ota_success_rate_warns_for_moderate_failure() {
        // 7/10 delivered = 70% → WARN (below 80%).
        let mut history: Vec<_> = (0..7)
            .map(|i| fw_rec_with_status("0.14.0", (i * 60) as u64, Some("delivered")))
            .collect();
        for i in 0..3 {
            history.push(fw_rec_with_status("0.13.0", 600 + (i * 60), Some("failed")));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_WARN);
        assert!(f.suggestion.is_some());
    }

    #[test]
    fn ota_success_rate_fails_for_majority_failure() {
        // 4/10 delivered = 40% → FAIL.
        let mut history: Vec<_> = (0..4)
            .map(|i| fw_rec_with_status("0.14.0", (i * 60) as u64, Some("delivered")))
            .collect();
        for i in 0..6 {
            history.push(fw_rec_with_status("0.13.0", 600 + (i * 60), Some("failed")));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_FAIL);
        assert!(f.detail.contains("4/10"));
    }

    #[test]
    fn ota_success_rate_window_caps_at_ten() {
        // 12 delivered + 2 failed; only the most recent 10 should count.
        // Order matches what get_firmware_history returns: newest first.
        let mut history: Vec<_> = Vec::new();
        // newest 10 are all delivered → expect OK
        for i in 0..10 {
            history.push(fw_rec_with_status("0.14.0", i, Some("delivered")));
        }
        // older failures should be ignored once window is full
        for i in 0..2 {
            history.push(fw_rec_with_status("0.13.0", 1000 + i, Some("failed")));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("10/10"));
    }

    #[test]
    fn ota_success_rate_detail_includes_last_error_when_not_ok() {
        // 4/10 delivered → FAIL. The newest failure carries an error string;
        // detail should surface it so the admin sees the failure category.
        let mut history: Vec<_> = Vec::new();
        // newest failure is at mins_ago=0 with a specific error
        history.push(fw_rec_full(
            "0.14.0", 0, Some("failed"), Some("body: Broken pipe"), false,
        ));
        // older failures with different errors — the rule should pick the newest
        for i in 1..6 {
            history.push(fw_rec_full(
                "0.14.0", i * 60, Some("failed"), Some("accept: timed out"), false,
            ));
        }
        for i in 6..10 {
            history.push(fw_rec_with_status("0.13.0", i * 60, Some("delivered")));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_FAIL);
        assert!(f.detail.contains("body: Broken pipe"), "detail: {}", f.detail);
        assert!(f.detail.contains("Last error:"), "detail: {}", f.detail);
    }

    #[test]
    fn ota_success_rate_detail_omits_error_when_ok() {
        // 9/10 delivered → OK. Even though one row has an error string,
        // OK findings stay silent on errors.
        let mut history: Vec<_> = (0..9)
            .map(|i| fw_rec_with_status("0.14.0", (i * 60) as u64, Some("delivered")))
            .collect();
        history.push(fw_rec_full(
            "0.13.0", 600, Some("failed"), Some("body: Broken pipe"), false,
        ));
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(!f.detail.contains("Last error"), "detail: {}", f.detail);
    }

    #[test]
    fn ota_success_rate_skips_null_rows_when_counting() {
        // Mixed: 3 recorded outcomes (2 delivered + 1 failed) interleaved
        // with NULL pre-v0.15.0 rows. Should compute on the 3 recorded ones,
        // not be diluted by the NULLs (66% → WARN).
        let history = vec![
            fw_rec_with_status("0.14.0", 5, Some("delivered")),
            fw_rec_with_status("0.13.0", 100, None),
            fw_rec_with_status("0.13.0", 200, Some("failed")),
            fw_rec_with_status("0.12.0", 300, None),
            fw_rec_with_status("0.12.0", 400, Some("delivered")),
        ];
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_WARN);
        assert!(f.detail.contains("2/3"));
    }

    #[test]
    fn ota_success_rate_excludes_cancelled_from_ratio() {
        // 3 delivered + 2 cancelled. The cancellations are user-initiated
        // aborts (rtl8xxxu flake, mind-changed, etc.) and must NOT be
        // counted against the delivery rate. The denominator should be 3,
        // not 5 — all three delivered → OK.
        let history = vec![
            fw_rec_with_status("0.14.0", 5, Some("delivered")),
            fw_rec_with_status("0.14.0", 100, Some("cancelled")),
            fw_rec_with_status("0.13.0", 200, Some("delivered")),
            fw_rec_with_status("0.13.0", 300, Some("cancelled")),
            fw_rec_with_status("0.12.0", 400, Some("delivered")),
        ];
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("3/3"), "detail: {}", f.detail);
    }

    // ─── v0.16.0 two-phase tracking ──────────────────────────────────────

    #[test]
    fn ota_success_rate_counts_delivered_not_applied_as_miss_after_grace() {
        // 2 applied + 5 delivered-but-not-applied-outside-grace = 2/7 applied
        // (28%) → FAIL. The grace window is 5 minutes; these rows are well
        // past that, so the device clearly never booted + acked.
        let mut history: Vec<_> = Vec::new();
        for i in 0..2 {
            history.push(fw_rec_with_status("0.14.0", 60 + i * 10, Some("delivered")));
        }
        for i in 0..5 {
            history.push(fw_rec_pending("0.14.0", 60 + i * 10));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_FAIL);
        assert!(f.detail.contains("2/7"), "detail: {}", f.detail);
        assert!(
            f.detail.contains("never confirmed"),
            "detail should mention the unconfirmed-apply category: {}",
            f.detail
        );
    }

    #[test]
    fn ota_success_rate_grace_window_hides_recent_delivered_not_applied() {
        // 7 applied + 3 delivered-but-not-applied-inside-grace. The three
        // pending rows are very recent (0–3 mins ago), the device is still
        // rebooting. Don't penalise yet — count as applied so the rule
        // stays OK.
        let mut history: Vec<_> = Vec::new();
        for i in 0..7 {
            history.push(fw_rec_with_status("0.14.0", 60 + i * 10, Some("delivered")));
        }
        for i in 0..3 {
            history.push(fw_rec_pending("0.14.0", i));
        }
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("10/10"), "detail: {}", f.detail);
    }

    #[test]
    fn ota_success_rate_rollback_rows_without_nonce_still_count_as_applied() {
        // Rollback reuses an existing firmware_history row — no new row is
        // inserted, so the re-applied rollback doesn't show up as a pending
        // row. The original row (pre-rollback) stays "delivered" + applied
        // from the prior apply. Simulated here with 3 plain "delivered"
        // rows that got set applied by the helper default.
        let history: Vec<_> = (0..3)
            .map(|i| fw_rec_with_status("0.13.0", 60 + i * 10, Some("delivered")))
            .collect();
        let f = check_ota_success_rate(&history);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("3/3"), "detail: {}", f.detail);
    }

    // ─── Power-supply stability ──────────────────────────────────────────

    fn reset(reason: &str, mins_ago: i64) -> ResetEvent {
        let ts = Utc::now() - Duration::minutes(mins_ago);
        ResetEvent {
            reset_reason: reason.to_string(),
            recorded_at: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    #[test]
    fn power_supply_info_when_no_reboots_seen() {
        let f = check_power_supply_stability(&[]);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("No reboots"));
    }

    #[test]
    fn power_supply_ok_for_clean_reboots() {
        // Normal reboots from OTA + cold boot should never escalate.
        let resets = vec![
            reset("software", 30),
            reset("poweron", 120),
            reset("external", 240),
        ];
        let f = check_power_supply_stability(&resets);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.suggestion.is_none());
    }

    #[test]
    fn power_supply_warn_for_single_brownout() {
        let resets = vec![
            reset("brownout", 90),
            reset("poweron", 200),
        ];
        let f = check_power_supply_stability(&resets);
        assert_eq!(f.level, LEVEL_WARN);
        assert!(f.detail.contains("1 brownout"));
        assert!(f.suggestion.is_some());
    }

    #[test]
    fn power_supply_fail_for_two_brownouts() {
        let resets = vec![
            reset("brownout", 30),
            reset("brownout", 300),
            reset("poweron", 1000),
        ];
        let f = check_power_supply_stability(&resets);
        assert_eq!(f.level, LEVEL_FAIL);
        assert!(f.detail.contains("2 brownout"));
        assert!(
            f.suggestion.as_ref().unwrap().contains("higher-current"),
            "suggestion should point at the power supply, got: {:?}", f.suggestion
        );
    }

    #[test]
    fn power_supply_warn_for_three_panics_without_brownout() {
        // No direct brownout signal but 3+ unexpected resets — still
        // worth flagging as WARN since panic/watchdog clusters often
        // correlate with PSU sag.
        let resets = vec![
            reset("panic", 30),
            reset("task_watchdog", 300),
            reset("panic", 800),
        ];
        let f = check_power_supply_stability(&resets);
        assert_eq!(f.level, LEVEL_WARN);
    }

    #[test]
    fn power_supply_ok_when_unknown_reasons_only() {
        // Pre-v0.17.0 firmwares report "unknown" — not brownouts, not in
        // the unexpected-reset set, so the rule should stay OK and just
        // note the count. This keeps existing fleets from getting a sea
        // of false warnings the moment they upgrade the desktop.
        let resets = vec![
            reset("unknown", 30),
            reset("unknown", 300),
            reset("unknown", 800),
        ];
        let f = check_power_supply_stability(&resets);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("3 other"));
    }

    #[test]
    fn power_supply_is_unexpected_reset_covers_watchdog_family() {
        assert!(is_unexpected_reset("brownout"));
        assert!(is_unexpected_reset("panic"));
        assert!(is_unexpected_reset("interrupt_watchdog"));
        assert!(is_unexpected_reset("task_watchdog"));
        assert!(is_unexpected_reset("watchdog"));

        assert!(!is_unexpected_reset("poweron"));
        assert!(!is_unexpected_reset("software"));
        assert!(!is_unexpected_reset("external"));
        assert!(!is_unexpected_reset("deepsleep"));
        assert!(!is_unexpected_reset("unknown"));
    }

    // ─── mDNS cadence ────────────────────────────────────────────────────

    fn mdns_sample(interval_ms: u32, mins_ago: i64) -> MdnsCadenceSample {
        let ts = Utc::now() - Duration::minutes(mins_ago);
        MdnsCadenceSample {
            interval_ms,
            recorded_at: ts.format("%Y-%m-%d %H:%M:%S").to_string(),
        }
    }

    #[test]
    fn mdns_latency_info_when_no_samples() {
        // Distinct "zero samples" path with cross-subnet explanation.
        let f = check_mdns_latency(&[]);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("No mDNS cadence samples"));
        assert!(
            f.detail.contains("Cross-subnet"),
            "0-sample text should explain cross-subnet discovery path; got: {}",
            f.detail
        );
    }

    #[test]
    fn mdns_latency_info_below_sample_floor() {
        // Two samples — still below the 3-sample minimum, don't claim a
        // reading yet.
        let samples = vec![mdns_sample(90_000, 5), mdns_sample(110_000, 40)];
        let f = check_mdns_latency(&samples);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("2 mDNS cadence"), "detail: {}", f.detail);
    }

    #[test]
    fn mdns_latency_ok_for_healthy_cadence() {
        // 10 samples in the expected 80-130s band — steady TTL refresh.
        let samples: Vec<_> = (0..10)
            .map(|i| mdns_sample(80_000 + (i as u32) * 5_000, (i as i64) * 5))
            .collect();
        let f = check_mdns_latency(&samples);
        assert_eq!(f.level, LEVEL_OK, "detail: {}", f.detail);
        assert!(f.suggestion.is_none());
        assert!(f.detail.contains("p50"), "detail: {}", f.detail);
        assert!(f.detail.contains("p95"), "detail: {}", f.detail);
    }

    #[test]
    fn mdns_latency_warn_on_stretched_p95() {
        // p50 healthy (120s) but p95 lands in the WARN band (400s).
        // With 20 samples, the 95th-percentile index is 18 (last=19),
        // so values at indices 18+ drive the p95 read.
        let mut samples: Vec<_> = (0..18)
            .map(|i| mdns_sample(100_000 + (i as u32) * 2_000, (i as i64) * 5))
            .collect();
        samples.push(mdns_sample(400_000, 3));
        samples.push(mdns_sample(420_000, 2));
        let f = check_mdns_latency(&samples);
        assert_eq!(f.level, LEVEL_WARN, "detail: {}", f.detail);
        assert!(f.suggestion.is_some());
    }

    #[test]
    fn mdns_latency_fail_when_p95_past_fail_line() {
        // p95 stretches past 600s — device very likely offline-ish.
        let mut samples: Vec<_> = (0..18)
            .map(|i| mdns_sample(100_000 + (i as u32) * 2_000, (i as i64) * 5))
            .collect();
        samples.push(mdns_sample(700_000, 3));
        samples.push(mdns_sample(750_000, 2));
        let f = check_mdns_latency(&samples);
        assert_eq!(f.level, LEVEL_FAIL, "detail: {}", f.detail);
        assert!(
            f.suggestion.as_ref().unwrap().contains("dropping off"),
            "FAIL suggestion should name the failure mode; got: {:?}",
            f.suggestion
        );
    }

    #[test]
    fn mdns_latency_small_sample_single_spike_trips_fail() {
        // n=3: p95_idx = ceil(0.95*3)-1 = 2 = last. A single extreme
        // spike on a 3-sample set trips FAIL, which is the intended
        // sensitivity at low N — we'd rather over-escalate than sit on
        // a visible disruption.
        let samples = vec![
            mdns_sample(100_000, 30),
            mdns_sample(120_000, 20),
            mdns_sample(800_000, 5),
        ];
        let f = check_mdns_latency(&samples);
        assert_eq!(f.level, LEVEL_FAIL, "detail: {}", f.detail);
    }

    #[test]
    fn mdns_latency_detail_reports_p50_p95_in_seconds() {
        // Unit-conversion regression: the rule stores ms but the detail
        // text reports seconds (human-readable for 100s cadences). Make
        // the conversion explicit in a test so a future change can't
        // silently mix units.
        let samples: Vec<_> = (0..5)
            .map(|_| mdns_sample(120_000, 10))
            .collect();
        let f = check_mdns_latency(&samples);
        assert!(f.detail.contains("p50 120s"), "detail: {}", f.detail);
        assert!(f.detail.contains("p95 120s"), "detail: {}", f.detail);
        assert!(
            f.detail.contains("expected cadence"),
            "detail should mention expected cadence baseline; got: {}",
            f.detail
        );
    }

    #[test]
    fn mdns_latency_rule_id_is_stable() {
        // Dashboards and alert configs reference this id — it must not
        // churn even though the underlying signal is now cadence.
        let f = check_mdns_latency(&[]);
        assert_eq!(f.id, "mdns_latency");
    }

    // ─── Flash-wear rate ─────────────────────────────────────────────────

    #[test]
    fn flash_wear_info_when_fewer_than_two_samples() {
        let f = check_flash_wear(&[]);
        assert_eq!(f.level, LEVEL_INFO);
        let f = check_flash_wear(&[point(100.0, 30)]);
        assert_eq!(f.level, LEVEL_INFO);
    }

    #[test]
    fn flash_wear_info_when_span_under_one_hour() {
        // Two samples 30 minutes apart — not enough span to project a rate.
        let samples = vec![point(0.0, 50), point(10.0, 20)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_INFO);
        assert!(f.detail.contains("not enough span"), "detail: {}", f.detail);
    }

    #[test]
    fn flash_wear_ok_for_light_use() {
        // 100 writes over 10h → 240/24h, well under 500.
        let samples = vec![point(0.0, 600), point(100.0, 0)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_OK, "detail: {}", f.detail);
        assert!(f.suggestion.is_none());
    }

    #[test]
    fn flash_wear_info_for_active_automation() {
        // 1000 writes over 10h → 2400/24h — elevated but years-of-life range.
        let samples = vec![point(0.0, 600), point(1000.0, 0)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_INFO, "detail: {}", f.detail);
        assert!(f.suggestion.is_some());
    }

    #[test]
    fn flash_wear_warn_for_months_of_life_rate() {
        // 3000 writes over 10h → 7200/24h (inside 5000–20000 band).
        let samples = vec![point(0.0, 600), point(3000.0, 0)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_WARN, "detail: {}", f.detail);
        assert!(f.suggestion.as_ref().unwrap().contains("short intervals"));
    }

    #[test]
    fn flash_wear_fail_for_pathological_rate() {
        // 10000 writes over 10h → 24000/24h (above 20k FAIL line).
        let samples = vec![point(0.0, 600), point(10_000.0, 0)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_FAIL, "detail: {}", f.detail);
        assert!(f.suggestion.as_ref().unwrap().contains("automation loop"));
    }

    #[test]
    fn flash_wear_skips_negative_deltas_across_reboot() {
        // Counter: 0 → 2000 (over 4h), reboot, 0 → 500 (over 4h).
        // Total writes = 2000 + 500 = 2500 across 8h → 7500/24h (WARN band).
        // If reboot wasn't handled, the naive (last - first) would be 500 over
        // 8h → 1500/24h and land in INFO, so this test catches the regression.
        let samples = vec![
            point(0.0, 480),
            point(2000.0, 240),
            point(0.0, 239), // reboot — counter reset
            point(500.0, 0),
        ];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_WARN, "detail: {}", f.detail);
        assert!(f.detail.contains("2500"), "detail should count both sides of the reboot: {}", f.detail);
    }

    #[test]
    fn flash_wear_flat_counter_is_ok() {
        // Counter stays at 1234 across 12h (device idle, no writes).
        let samples = vec![point(1234.0, 720), point(1234.0, 0)];
        let f = check_flash_wear(&samples);
        assert_eq!(f.level, LEVEL_OK);
        assert!(f.detail.contains("0 NVS writes/24h"));
    }
}
