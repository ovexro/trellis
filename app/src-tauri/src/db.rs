use rusqlite::{Connection, OptionalExtension};
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use serde::{Deserialize, Serialize};

pub struct Database {
    pub conn: Mutex<Connection>,
}

fn copy_label(src: &str) -> String {
    let trimmed = src.trim_end();
    if trimmed.is_empty() {
        "(copy)".to_string()
    } else {
        format!("{} (copy)", trimmed)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct FloorPlan {
    pub id: i64,
    pub name: String,
    pub sort_order: i64,
    pub background: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DevicePosition {
    pub device_id: String,
    pub floor_id: i64,
    pub x: f64,
    pub y: f64,
}

#[derive(Debug, Clone, Serialize)]
pub struct FloorPlanRoom {
    pub id: i64,
    pub floor_id: i64,
    pub name: String,
    pub color: String,
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct MetricPoint {
    pub value: f64,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityMeta {
    pub capability_id: String,
    pub nameplate_watts: Option<f64>,
    #[serde(default)]
    pub linear_power: bool,
    #[serde(default)]
    pub slider_max: Option<f64>,
    #[serde(default)]
    pub binary_sensor: bool,
    #[serde(default)]
    pub binary_sensor_device_class: Option<String>,
    #[serde(default)]
    pub cover_position: bool,
    /// HA `light` brightness linkage (v0.27.0). Set on the slider row to
    /// reference a color cap on the same device — the linked color cap then
    /// publishes a single HA `light` entity carrying both rgb and brightness
    /// channels, and this slider's separate `number`/`cover` entity is
    /// retracted. NULL = standalone slider.
    #[serde(default)]
    pub brightness_for_cap_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CapabilityEnergy {
    pub capability_id: String,
    pub nameplate_watts: f64,
    pub on_time_seconds: i64,
    pub wh: f64,
    pub tracked_since: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEnergyReport {
    pub window_hours: i64,
    pub total_wh: f64,
    pub capabilities: Vec<CapabilityEnergy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SavedDevice {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub firmware: String,
    pub platform: String,
    pub nickname: Option<String>,
    pub tags: String,
    pub first_seen: String,
    pub last_seen: String,
    pub group_id: Option<i64>,
    pub sort_order: i64,
    pub favorite: bool,
    /// GitHub owner/repo for firmware auto-remediation. When both are set the
    /// diagnostics engine checks for newer releases and exposes a one-click
    /// OTA button on the firmware_age finding.
    #[serde(default)]
    pub github_owner: Option<String>,
    #[serde(default)]
    pub github_repo: Option<String>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub install_date: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceGroup {
    pub id: i64,
    pub name: String,
    pub color: String,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlertRule {
    pub id: i64,
    pub device_id: String,
    pub metric_id: String,
    pub condition: String, // "above" or "below"
    pub threshold: f64,
    pub label: String,
    pub enabled: bool,
}

/// Build half-open [start, end) offline intervals clipped to the window from a
/// chronological sequence of state transitions. Bootstrap of `None` (no state
/// log ever seen for the device) is treated as online since a device must
/// first be online to go offline — this means devices with no recorded
/// transitions keep the pre-intersection behavior (no subtraction).
fn compute_offline_intervals(
    bootstrap_online: Option<bool>,
    logs: &[(String, i64)],
    window_start_epoch: i64,
    now_epoch: i64,
) -> Vec<(i64, i64)> {
    let mut out: Vec<(i64, i64)> = Vec::new();
    let mut online = bootstrap_online.unwrap_or(true);
    let mut offline_start: Option<i64> = if !online {
        Some(window_start_epoch)
    } else {
        None
    };
    for (msg, ts) in logs {
        let new_online = msg == "online";
        if online && !new_online {
            offline_start = Some(*ts);
        } else if !online && new_online {
            if let Some(s) = offline_start.take() {
                out.push((s, *ts));
            }
        }
        online = new_online;
    }
    if !online {
        if let Some(s) = offline_start.take() {
            out.push((s, now_epoch));
        }
    }
    out
}

/// Sum a capability's ON-time over the window, subtracting any overlap with
/// offline intervals. Transitions beyond window bounds are expected to already
/// be filtered by the caller.
fn compute_on_seconds_online(
    bootstrap: i64,
    rows: &[(i64, i64)],
    window_start_epoch: i64,
    now_epoch: i64,
    offline_intervals: &[(i64, i64)],
) -> i64 {
    let mut on_intervals: Vec<(i64, i64)> = Vec::new();
    let mut current_state = bootstrap;
    let mut last_epoch = window_start_epoch;
    for (state, ts_epoch) in rows {
        if current_state == 1 {
            on_intervals.push((last_epoch, *ts_epoch));
        }
        current_state = *state;
        last_epoch = *ts_epoch;
    }
    if current_state == 1 {
        on_intervals.push((last_epoch, now_epoch));
    }

    let mut total: i64 = 0;
    for (start, end) in on_intervals {
        let dur = (end - start).max(0);
        let mut overlap: i64 = 0;
        for (os, oe) in offline_intervals {
            let ov_start = (*os).max(start);
            let ov_end = (*oe).min(end);
            overlap += (ov_end - ov_start).max(0);
        }
        total += (dur - overlap).max(0);
    }
    total
}

/// Linear-power slider integration. For each window interval `[last_ts, ts)`,
/// accrue `watts × (value/max_safe) × online_dt / 3600` Wh, where
/// `online_dt = dur − offline_overlap` is the online-portion seconds of the
/// interval. Returns `(on_time_seconds, wh)` where `on_time_seconds` is the
/// online-portion time spent with `value > 0` (matching the semantic of
/// `compute_on_seconds_online` for switches — useful for the "tracked since"
/// breakdown). `bootstrap` is the latest value seen before `window_start`
/// (0 when unknown, per the conservative get_device_energy bootstrap rule).
fn compute_numeric_wh_online(
    bootstrap: i64,
    rows: &[(i64, i64)],
    window_start_epoch: i64,
    now_epoch: i64,
    offline_intervals: &[(i64, i64)],
    watts: f64,
    max: f64,
) -> (i64, f64) {
    let max_safe = if max.is_finite() && max > 0.0 { max } else { 255.0 };
    let mut segments: Vec<(i64, i64, i64)> = Vec::new();
    let mut current = bootstrap;
    let mut last = window_start_epoch;
    for (v, ts) in rows {
        segments.push((last, *ts, current));
        current = *v;
        last = *ts;
    }
    segments.push((last, now_epoch, current));

    let mut on_time: i64 = 0;
    let mut wh: f64 = 0.0;
    for (start, end, value) in segments {
        let dur = (end - start).max(0);
        if dur == 0 || value <= 0 {
            continue;
        }
        let mut overlap: i64 = 0;
        for (os, oe) in offline_intervals {
            let ov_start = (*os).max(start);
            let ov_end = (*oe).min(end);
            overlap += (ov_end - ov_start).max(0);
        }
        let online = (dur - overlap).max(0);
        on_time += online;
        let fraction = (value as f64) / max_safe;
        wh += (online as f64) * watts * fraction / 3600.0;
    }
    (on_time, wh)
}

/// Shared compute for lifetime energy on a single metered capability. Returns
/// `None` when the capability has no `nameplate_watts` row or no logged
/// transitions yet. Caller holds the connection lock; we take a `&Connection`
/// reference so this can be invoked inside a method that already locked.
///
/// Window is `[MIN(capability_state_log.timestamp), now)` for this (device,
/// capability). Offline-interval overlap is subtracted from ON-time using the
/// same model as `get_device_energy` (device_logs severity='state' timeline).
/// The severity='state' log IS under retention cleanup so offline fidelity
/// for transitions older than the retention window is best-effort.
fn compute_capability_lifetime(
    conn: &Connection,
    device_id: &str,
    capability_id: &str,
) -> Result<Option<CapabilityEnergy>, String> {
    let meta: Option<(f64, bool, Option<f64>)> = conn
        .query_row(
            "SELECT nameplate_watts, linear_power, slider_max
             FROM capability_meta
             WHERE device_id = ?1 AND capability_id = ?2
               AND nameplate_watts IS NOT NULL",
            rusqlite::params![device_id, capability_id],
            |row| {
                Ok((
                    row.get::<_, f64>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, Option<f64>>(2)?,
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (watts, linear_power, slider_max) = match meta {
        Some(t) => t,
        None => return Ok(None),
    };

    let earliest: Option<(i64, String)> = conn
        .query_row(
            "SELECT CAST(strftime('%s', MIN(timestamp)) AS INTEGER),
                    MIN(timestamp)
             FROM capability_state_log
             WHERE device_id = ?1 AND capability_id = ?2",
            rusqlite::params![device_id, capability_id],
            |row| {
                Ok((
                    row.get::<_, Option<i64>>(0)?.unwrap_or(0),
                    row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                ))
            },
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let (window_start_epoch, tracked_since_ts) = match earliest {
        Some((ts, _)) if ts == 0 => return Ok(None),
        Some((ts, s)) => (ts, if s.is_empty() { None } else { Some(s) }),
        None => return Ok(None),
    };

    let now_epoch: i64 = conn
        .query_row("SELECT CAST(strftime('%s', 'now') AS INTEGER)", [], |r| {
            r.get(0)
        })
        .map_err(|e| e.to_string())?;

    let bootstrap_state_msg: Option<String> = conn
        .query_row(
            "SELECT message FROM device_logs
             WHERE device_id = ?1 AND severity = 'state'
               AND CAST(strftime('%s', timestamp) AS INTEGER) < ?2
             ORDER BY id DESC LIMIT 1",
            rusqlite::params![device_id, window_start_epoch],
            |row| row.get(0),
        )
        .optional()
        .map_err(|e| e.to_string())?;
    let bootstrap_online: Option<bool> =
        bootstrap_state_msg.map(|m| m == "online");

    let mut sls = conn
        .prepare(
            "SELECT message, CAST(strftime('%s', timestamp) AS INTEGER)
             FROM device_logs
             WHERE device_id = ?1 AND severity = 'state'
               AND CAST(strftime('%s', timestamp) AS INTEGER) >= ?2
             ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;
    let state_logs: Vec<(String, i64)> = sls
        .query_map(
            rusqlite::params![device_id, window_start_epoch],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    let offline_intervals = compute_offline_intervals(
        bootstrap_online,
        &state_logs,
        window_start_epoch,
        now_epoch,
    );

    let mut s = conn
        .prepare(
            "SELECT state, CAST(strftime('%s', timestamp) AS INTEGER)
             FROM capability_state_log
             WHERE device_id = ?1 AND capability_id = ?2
             ORDER BY id ASC",
        )
        .map_err(|e| e.to_string())?;
    let rows: Vec<(i64, i64)> = s
        .query_map(rusqlite::params![device_id, capability_id], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    // bootstrap=0 (OFF) is correct by construction: window_start is the
    // timestamp of the very first logged transition, and there is no row
    // before it. The first segment [window_start, first_ts) thus has
    // zero duration anyway.
    let (on_seconds, wh) = if linear_power {
        compute_numeric_wh_online(
            0,
            &rows,
            window_start_epoch,
            now_epoch,
            &offline_intervals,
            watts,
            slider_max.unwrap_or(255.0),
        )
    } else {
        let sec = compute_on_seconds_online(
            0,
            &rows,
            window_start_epoch,
            now_epoch,
            &offline_intervals,
        );
        (sec, (sec as f64) * watts / 3600.0)
    };

    Ok(Some(CapabilityEnergy {
        capability_id: capability_id.to_string(),
        nameplate_watts: watts,
        on_time_seconds: on_seconds,
        wh,
        tracked_since: tracked_since_ts,
    }))
}

impl Database {
    // ─── Device persistence ──────────────────────────────────────────────

    pub fn upsert_device(
        &self,
        id: &str,
        name: &str,
        ip: &str,
        port: u16,
        firmware: &str,
        platform: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO devices (id, name, ip, port, firmware, platform, first_seen, last_seen)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'), datetime('now'))
             ON CONFLICT(id) DO UPDATE SET
                name = ?2, ip = ?3, port = ?4, firmware = ?5, platform = ?6,
                last_seen = datetime('now')",
            rusqlite::params![id, name, ip, port, firmware, platform],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_nickname(&self, device_id: &str, nickname: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let nick = if nickname.is_empty() {
            None
        } else {
            Some(nickname)
        };
        conn.execute(
            "UPDATE devices SET nickname = ?1 WHERE id = ?2",
            rusqlite::params![nick, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_capability_watts(
        &self,
        device_id: &str,
        capability_id: &str,
        nameplate_watts: Option<f64>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        match nameplate_watts {
            Some(w) => conn.execute(
                "INSERT INTO capability_meta (device_id, capability_id, nameplate_watts)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(device_id, capability_id)
                 DO UPDATE SET nameplate_watts = excluded.nameplate_watts",
                rusqlite::params![device_id, capability_id, w],
            ),
            None => conn.execute(
                "INSERT INTO capability_meta (device_id, capability_id, nameplate_watts)
                 VALUES (?1, ?2, NULL)
                 ON CONFLICT(device_id, capability_id)
                 DO UPDATE SET nameplate_watts = NULL",
                rusqlite::params![device_id, capability_id],
            ),
        }
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn log_switch_state(
        &self,
        device_id: &str,
        capability_id: &str,
        on: bool,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let last: Option<i64> = conn
            .query_row(
                "SELECT state FROM capability_state_log
                 WHERE device_id = ?1 AND capability_id = ?2
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![device_id, capability_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let new_state: i64 = if on { 1 } else { 0 };
        if last == Some(new_state) {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO capability_state_log (device_id, capability_id, state)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![device_id, capability_id, new_state],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_device_energy(
        &self,
        device_id: &str,
        hours: i64,
    ) -> Result<DeviceEnergyReport, String> {
        let conn = self.conn.lock().unwrap();
        let hours_clamped = hours.clamp(1, 24 * 365);

        let mut stmt = conn
            .prepare(
                "SELECT capability_id, nameplate_watts, linear_power, slider_max
                 FROM capability_meta
                 WHERE device_id = ?1 AND nameplate_watts IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let metered: Vec<(String, f64, bool, Option<f64>)> = stmt
            .query_map(rusqlite::params![device_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, f64>(1)?,
                    row.get::<_, i64>(2)? != 0,
                    row.get::<_, Option<f64>>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        // Window bounds as unix epoch seconds, computed by SQLite for correctness.
        let now_epoch: i64 = conn
            .query_row("SELECT CAST(strftime('%s', 'now') AS INTEGER)", [], |r| {
                r.get(0)
            })
            .map_err(|e| e.to_string())?;
        let window_start_epoch: i64 = now_epoch - hours_clamped * 3600;

        // Online/offline intervals derived from device_logs severity='state'.
        // Subtracting offline overlap keeps Wh from inflating when a device goes
        // dark with a switch left ON — we can't prove it's still drawing power.
        let bootstrap_state_msg: Option<String> = conn
            .query_row(
                "SELECT message FROM device_logs
                 WHERE device_id = ?1 AND severity = 'state'
                   AND CAST(strftime('%s', timestamp) AS INTEGER) < ?2
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![device_id, window_start_epoch],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        let bootstrap_online: Option<bool> =
            bootstrap_state_msg.map(|m| m == "online");

        let mut sls = conn
            .prepare(
                "SELECT message, CAST(strftime('%s', timestamp) AS INTEGER)
                 FROM device_logs
                 WHERE device_id = ?1 AND severity = 'state'
                   AND CAST(strftime('%s', timestamp) AS INTEGER) >= ?2
                 ORDER BY id ASC",
            )
            .map_err(|e| e.to_string())?;
        let state_logs: Vec<(String, i64)> = sls
            .query_map(
                rusqlite::params![device_id, window_start_epoch],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;

        let offline_intervals = compute_offline_intervals(
            bootstrap_online,
            &state_logs,
            window_start_epoch,
            now_epoch,
        );

        let mut out: Vec<CapabilityEnergy> = Vec::new();
        let mut total_wh: f64 = 0.0;

        for (cap_id, watts, linear_power, slider_max) in metered {
            // Bootstrap: latest state before window_start. If none, assume OFF
            // (conservative — we can't prove what the state was before tracking
            // began).
            let bootstrap: Option<i64> = conn
                .query_row(
                    "SELECT state FROM capability_state_log
                     WHERE device_id = ?1 AND capability_id = ?2
                       AND CAST(strftime('%s', timestamp) AS INTEGER) < ?3
                     ORDER BY id DESC LIMIT 1",
                    rusqlite::params![device_id, cap_id, window_start_epoch],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;

            // Earliest sample for this capability overall — surfaces to the UI as
            // "tracking since" so users can reason about the coverage window.
            let tracked_since: Option<String> = conn
                .query_row(
                    "SELECT timestamp FROM capability_state_log
                     WHERE device_id = ?1 AND capability_id = ?2
                     ORDER BY id ASC LIMIT 1",
                    rusqlite::params![device_id, cap_id],
                    |row| row.get(0),
                )
                .optional()
                .map_err(|e| e.to_string())?;

            // Rows inside the window, chronological.
            let mut s = conn
                .prepare(
                    "SELECT state, CAST(strftime('%s', timestamp) AS INTEGER)
                     FROM capability_state_log
                     WHERE device_id = ?1 AND capability_id = ?2
                       AND CAST(strftime('%s', timestamp) AS INTEGER) >= ?3
                     ORDER BY id ASC",
                )
                .map_err(|e| e.to_string())?;
            let rows: Vec<(i64, i64)> = s
                .query_map(
                    rusqlite::params![device_id, cap_id, window_start_epoch],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?;

            let (on_seconds, wh) = if linear_power {
                compute_numeric_wh_online(
                    bootstrap.unwrap_or(0),
                    &rows,
                    window_start_epoch,
                    now_epoch,
                    &offline_intervals,
                    watts,
                    slider_max.unwrap_or(255.0),
                )
            } else {
                let sec = compute_on_seconds_online(
                    bootstrap.unwrap_or(0),
                    &rows,
                    window_start_epoch,
                    now_epoch,
                    &offline_intervals,
                );
                (sec, (sec as f64) * watts / 3600.0)
            };
            total_wh += wh;
            out.push(CapabilityEnergy {
                capability_id: cap_id,
                nameplate_watts: watts,
                on_time_seconds: on_seconds,
                wh,
                tracked_since,
            });
        }

        Ok(DeviceEnergyReport {
            window_hours: hours_clamped,
            total_wh,
            capabilities: out,
        })
    }

    /// Every (device_id, capability_id, device_class) tuple where
    /// `binary_sensor` is set. Used at startup to hydrate the MQTT bridge's
    /// binary_sensor cache so HA discovery emits the binary_sensor
    /// component on first publish without a DB round-trip per device.
    pub fn get_all_binary_sensors(
        &self,
    ) -> Result<Vec<(String, String, Option<String>)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, capability_id, binary_sensor_device_class
                 FROM capability_meta
                 WHERE binary_sensor = 1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Every (device_id, capability_id, nameplate_watts, linear_power,
    /// slider_max) tuple where `nameplate_watts` is set. Used at startup to
    /// hydrate the MQTT bridge's meta cache so HA discovery emits the
    /// per-capability `_power` + `_energy` entities on first publish without
    /// a DB round-trip per device.
    pub fn get_all_capability_meters(
        &self,
    ) -> Result<Vec<(String, String, f64, bool, Option<f64>)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, capability_id, nameplate_watts, linear_power, slider_max
                 FROM capability_meta
                 WHERE nameplate_watts IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, f64>(2)?,
                    row.get::<_, i64>(3)? != 0,
                    row.get::<_, Option<f64>>(4)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Lifetime cumulative Wh for one metered capability, suitable for an HA
    /// `total_increasing` energy sensor. Bounds the integration at
    /// `MIN(timestamp)` of this capability's own `capability_state_log` rows —
    /// `capability_state_log` is NOT subject to data-retention cleanup, so
    /// that minimum is stable across the device's lifetime and the cumulative
    /// stays monotonic (modulo user edits to `nameplate_watts`/`slider_max`,
    /// which HA treats as counter resets per the `total_increasing` spec).
    ///
    /// Returns `0.0` (not an error) when the capability has no meta row, no
    /// `nameplate_watts`, or no logged transitions yet — lets callers publish
    /// unconditionally without branching.
    ///
    /// Offline-interval overlap is subtracted from ON-time (same model as
    /// `get_device_energy`). The severity='state' boundary log IS under
    /// retention cleanup, so for timestamps older than the retention window
    /// we lose offline-overlap fidelity and may over-count slightly. This is
    /// BACKLOG-acknowledged and acceptable at the HA Energy-dashboard
    /// granularity.
    pub fn get_capability_lifetime_wh(
        &self,
        device_id: &str,
        capability_id: &str,
    ) -> Result<f64, String> {
        let conn = self.conn.lock().unwrap();
        Ok(compute_capability_lifetime(&conn, device_id, capability_id)?
            .map(|c| c.wh)
            .unwrap_or(0.0))
    }

    /// Lifetime energy report for every metered capability on a device. Window
    /// is the whole recorded history (from `MIN(capability_state_log.timestamp)`
    /// forward, per-capability). `window_hours` is reported as `0` to signal
    /// lifetime mode to the UI; `capabilities[]` entries carry on_time_seconds,
    /// Wh, and a `tracked_since` timestamp. Silent capability is any that has
    /// no nameplate_watts set or has no logged transitions yet.
    pub fn get_device_lifetime_energy(
        &self,
        device_id: &str,
    ) -> Result<DeviceEnergyReport, String> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn
            .prepare(
                "SELECT capability_id
                 FROM capability_meta
                 WHERE device_id = ?1 AND nameplate_watts IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let metered_ids: Vec<String> = stmt
            .query_map(rusqlite::params![device_id], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        drop(stmt);

        let mut out: Vec<CapabilityEnergy> = Vec::new();
        let mut total_wh: f64 = 0.0;
        for cap_id in metered_ids {
            if let Some(ce) = compute_capability_lifetime(&conn, device_id, &cap_id)? {
                total_wh += ce.wh;
                out.push(ce);
            }
        }

        Ok(DeviceEnergyReport {
            window_hours: 0,
            total_wh,
            capabilities: out,
        })
    }

    pub fn get_device_capability_meta(
        &self,
        device_id: &str,
    ) -> Result<Vec<CapabilityMeta>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT capability_id, nameplate_watts, linear_power, slider_max,
                        binary_sensor, binary_sensor_device_class, cover_position,
                        brightness_for_cap_id
                 FROM capability_meta
                 WHERE device_id = ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id], |row| {
                Ok(CapabilityMeta {
                    capability_id: row.get(0)?,
                    nameplate_watts: row.get(1)?,
                    linear_power: row.get::<_, i64>(2)? != 0,
                    slider_max: row.get(3)?,
                    binary_sensor: row.get::<_, i64>(4)? != 0,
                    binary_sensor_device_class: row.get(5)?,
                    cover_position: row.get::<_, i64>(6)? != 0,
                    brightness_for_cap_id: row.get(7)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Upsert the linear_power flag for a capability along with the slider
    /// max value that the integration will use as its denominator. The flag
    /// is only meaningful when nameplate_watts is also set, but the fields
    /// are independently editable (see set_capability_watts). `slider_max`
    /// is captured from the live Capability.max at opt-in time so the
    /// energy computation doesn't require the device online.
    pub fn set_capability_linear_power(
        &self,
        device_id: &str,
        capability_id: &str,
        linear_power: bool,
        slider_max: Option<f64>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let flag: i64 = if linear_power { 1 } else { 0 };
        conn.execute(
            "INSERT INTO capability_meta (device_id, capability_id, linear_power, slider_max)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id, capability_id)
             DO UPDATE SET linear_power = excluded.linear_power,
                           slider_max   = excluded.slider_max",
            rusqlite::params![device_id, capability_id, flag, slider_max],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Upsert the HA cover routing flag for a slider capability. When
    /// `cover_position` is true the MQTT bridge publishes the cap's
    /// discovery config under HA's `cover` component instead of `number`,
    /// using the slider's live max as `position_open` and 0 as
    /// `position_closed`. Independent of energy fields.
    pub fn set_capability_cover(
        &self,
        device_id: &str,
        capability_id: &str,
        cover_position: bool,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let flag: i64 = if cover_position { 1 } else { 0 };
        conn.execute(
            "INSERT INTO capability_meta (device_id, capability_id, cover_position)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(device_id, capability_id)
             DO UPDATE SET cover_position = excluded.cover_position",
            rusqlite::params![device_id, capability_id, flag],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Every (device_id, capability_id) pair where `cover_position` is set.
    /// Used at startup to hydrate the MQTT bridge's cover cache so HA
    /// discovery emits the cover component on first publish without a DB
    /// round-trip per device.
    pub fn get_all_covers(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, capability_id
                 FROM capability_meta
                 WHERE cover_position = 1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Upsert the HA binary_sensor flag + optional device_class for a
    /// sensor capability. When `binary_sensor` is true the MQTT bridge
    /// publishes the cap's discovery config under HA's `binary_sensor`
    /// component instead of `sensor`. `device_class` (motion, door,
    /// occupancy, etc.) is forwarded verbatim — None for a generic
    /// binary_sensor with no specific class. Independent of energy fields.
    pub fn set_capability_binary_sensor(
        &self,
        device_id: &str,
        capability_id: &str,
        binary_sensor: bool,
        device_class: Option<String>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let flag: i64 = if binary_sensor { 1 } else { 0 };
        let dc = if binary_sensor { device_class } else { None };
        conn.execute(
            "INSERT INTO capability_meta (device_id, capability_id, binary_sensor, binary_sensor_device_class)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id, capability_id)
             DO UPDATE SET binary_sensor              = excluded.binary_sensor,
                           binary_sensor_device_class = excluded.binary_sensor_device_class",
            rusqlite::params![device_id, capability_id, flag, dc],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Upsert the HA `light` brightness linkage for a slider capability.
    /// `link_target` carries the color cap's `capability_id` (must be on the
    /// same device); `None` clears the link. The bridge promotes the linked
    /// color cap's discovery to an HA `light` with brightness fields and
    /// retracts this slider's separate `number`/`cover` entity. Independent
    /// of energy / cover / binary_sensor fields.
    pub fn set_capability_brightness_link(
        &self,
        device_id: &str,
        slider_capability_id: &str,
        link_target: Option<String>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO capability_meta (device_id, capability_id, brightness_for_cap_id)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(device_id, capability_id)
             DO UPDATE SET brightness_for_cap_id = excluded.brightness_for_cap_id",
            rusqlite::params![device_id, slider_capability_id, link_target],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Every (device_id, color_capability_id, slider_capability_id) triple
    /// where a slider's `brightness_for_cap_id` points at a color cap. Used
    /// at startup to hydrate the MQTT bridge's brightness-link cache so HA
    /// sees the unified `light` entity on first publish without a DB
    /// round-trip per device.
    pub fn get_all_brightness_links(
        &self,
    ) -> Result<Vec<(String, String, String)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, brightness_for_cap_id, capability_id
                 FROM capability_meta
                 WHERE brightness_for_cap_id IS NOT NULL",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    /// Dedup-write a slider value transition, but only when the capability
    /// has opted in to linear-power tracking AND has nameplate_watts set.
    /// Mirrors log_switch_state — reads the latest row for (device, cap),
    /// skips the INSERT if the new value matches. Value is rounded to i64
    /// (slider NVS persistence is integer, so we never lose precision in
    /// practice). No-op when the capability hasn't opted in, so the caller
    /// can invoke this on every numeric update without extra branching.
    pub fn log_slider_value_if_linear(
        &self,
        device_id: &str,
        capability_id: &str,
        value: i64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let opt_in: bool = conn
            .query_row(
                "SELECT 1 FROM capability_meta
                 WHERE device_id = ?1 AND capability_id = ?2
                   AND linear_power = 1
                   AND nameplate_watts IS NOT NULL",
                rusqlite::params![device_id, capability_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|e| e.to_string())?
            .unwrap_or(false);
        if !opt_in {
            return Ok(());
        }
        let last: Option<i64> = conn
            .query_row(
                "SELECT state FROM capability_state_log
                 WHERE device_id = ?1 AND capability_id = ?2
                 ORDER BY id DESC LIMIT 1",
                rusqlite::params![device_id, capability_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| e.to_string())?;
        if last == Some(value) {
            return Ok(());
        }
        conn.execute(
            "INSERT INTO capability_state_log (device_id, capability_id, state)
             VALUES (?1, ?2, ?3)",
            rusqlite::params![device_id, capability_id, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_tags(&self, device_id: &str, tags: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET tags = ?1 WHERE id = ?2",
            rusqlite::params![tags, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_device_notes(&self, device_id: &str, notes: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET notes = ?1 WHERE id = ?2",
            rusqlite::params![notes, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_device_install_date(&self, device_id: &str, install_date: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET install_date = ?1 WHERE id = ?2",
            rusqlite::params![install_date, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_saved_device(&self, device_id: &str) -> Result<Option<SavedDevice>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order, favorite, github_owner, github_repo, notes, install_date FROM devices WHERE id = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(rusqlite::params![device_id], |row| {
                Ok(SavedDevice {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    ip: row.get(2)?,
                    port: row.get::<_, i32>(3)? as u16,
                    firmware: row.get(4)?,
                    platform: row.get(5)?,
                    nickname: row.get(6)?,
                    tags: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    first_seen: row.get(8)?,
                    last_seen: row.get(9)?,
                    group_id: row.get(10)?,
                    sort_order: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
                    favorite: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
                    github_owner: row.get(13)?,
                    github_repo: row.get(14)?,
                    notes: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                    install_date: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?;
        match rows.next() {
            Some(Ok(d)) => Ok(Some(d)),
            _ => Ok(None),
        }
    }

    pub fn get_all_saved_devices(&self) -> Result<Vec<SavedDevice>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order, favorite, github_owner, github_repo, notes, install_date FROM devices ORDER BY sort_order ASC, last_seen DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(SavedDevice {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    ip: row.get(2)?,
                    port: row.get::<_, i32>(3)? as u16,
                    firmware: row.get(4)?,
                    platform: row.get(5)?,
                    nickname: row.get(6)?,
                    tags: row.get::<_, Option<String>>(7)?.unwrap_or_default(),
                    first_seen: row.get(8)?,
                    last_seen: row.get(9)?,
                    group_id: row.get(10)?,
                    sort_order: row.get::<_, Option<i64>>(11)?.unwrap_or(0),
                    favorite: row.get::<_, Option<i64>>(12)?.unwrap_or(0) != 0,
                    github_owner: row.get(13)?,
                    github_repo: row.get(14)?,
                    notes: row.get::<_, Option<String>>(15)?.unwrap_or_default(),
                    install_date: row.get::<_, Option<String>>(16)?.unwrap_or_default(),
                })
            })
            .map_err(|e| e.to_string())?;
        let mut devices = Vec::new();
        for row in rows {
            devices.push(row.map_err(|e| e.to_string())?);
        }
        Ok(devices)
    }

    /// Update per-device GitHub repo binding. Empty strings clear the binding
    /// (stored as NULL) so the firmware_age rule reverts to INFO-only.
    pub fn set_device_github_repo(
        &self,
        device_id: &str,
        owner: Option<&str>,
        repo: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        let o = owner.filter(|s| !s.is_empty());
        let r = repo.filter(|s| !s.is_empty());
        conn.execute(
            "UPDATE devices SET github_owner = ?1, github_repo = ?2 WHERE id = ?3",
            rusqlite::params![o, r, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn reorder_devices(&self, order: &[(String, i64)]) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        for (id, sort_order) in order {
            conn.execute(
                "UPDATE devices SET sort_order = ?1 WHERE id = ?2",
                rusqlite::params![sort_order, id],
            )
            .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_device(&self, device_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM devices WHERE id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM metrics WHERE device_id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM alerts WHERE device_id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM device_logs WHERE device_id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM favorite_capabilities WHERE device_id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM device_positions WHERE device_id = ?1", rusqlite::params![device_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Metrics ─────────────────────────────────────────────────────────

    pub fn store_metric(
        &self,
        device_id: &str,
        metric_id: &str,
        value: f64,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO metrics (device_id, metric_id, value, timestamp) VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![device_id, metric_id, value],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_metrics(
        &self,
        device_id: &str,
        metric_id: &str,
        hours: u32,
    ) -> Result<Vec<MetricPoint>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT value, timestamp FROM metrics
                 WHERE device_id = ?1 AND metric_id = ?2
                 AND timestamp >= datetime('now', ?3)
                 ORDER BY timestamp ASC",
            )
            .map_err(|e| e.to_string())?;
        let time_offset = format!("-{} hours", hours);
        let rows = stmt
            .query_map(rusqlite::params![device_id, metric_id, time_offset], |row| {
                Ok(MetricPoint {
                    value: row.get(0)?,
                    timestamp: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut points = Vec::new();
        for row in rows {
            points.push(row.map_err(|e| e.to_string())?);
        }
        Ok(points)
    }

    pub fn cleanup_old_metrics(&self, days: u32) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        let offset = format!("-{} days", days);
        let deleted = conn
            .execute(
                "DELETE FROM metrics WHERE timestamp < datetime('now', ?1)",
                rusqlite::params![offset],
            )
            .map_err(|e| e.to_string())?;
        Ok(deleted)
    }

    pub fn cleanup_old_logs(&self, days: u32) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        let offset = format!("-{} days", days);
        let deleted = conn
            .execute(
                "DELETE FROM device_logs WHERE timestamp < datetime('now', ?1)",
                rusqlite::params![offset],
            )
            .map_err(|e| e.to_string())?;
        Ok(deleted)
    }

    // `capability_state_log` is intentionally NOT swept — `get_capability_lifetime_wh`
    // integrates from `MIN(timestamp)` so HA's `total_increasing` Wh sensor stays
    // monotonic. Deleting old rows would shift the floor and HA would treat it as
    // a counter reset.
    pub fn cleanup_old_webhook_deliveries(&self, days: u32) -> Result<usize, String> {
        let conn = self.conn.lock().unwrap();
        let offset = format!("-{} days", days);
        let deleted = conn
            .execute(
                "DELETE FROM webhook_deliveries WHERE timestamp < datetime('now', ?1)",
                rusqlite::params![offset],
            )
            .map_err(|e| e.to_string())?;
        Ok(deleted)
    }

    // ─── Alert rules ─────────────────────────────────────────────────────

    pub fn create_alert(
        &self,
        device_id: &str,
        metric_id: &str,
        condition: &str,
        threshold: f64,
        label: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO alerts (device_id, metric_id, condition, threshold, label, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            rusqlite::params![device_id, metric_id, condition, threshold, label],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_alerts(&self, device_id: &str) -> Result<Vec<AlertRule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, device_id, metric_id, condition, threshold, label, enabled FROM alerts WHERE device_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id], |row| {
                Ok(AlertRule {
                    id: row.get(0)?,
                    device_id: row.get(1)?,
                    metric_id: row.get(2)?,
                    condition: row.get(3)?,
                    threshold: row.get(4)?,
                    label: row.get(5)?,
                    enabled: row.get(6)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut alerts = Vec::new();
        for row in rows {
            alerts.push(row.map_err(|e| e.to_string())?);
        }
        Ok(alerts)
    }

    pub fn delete_alert(&self, alert_id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM alerts WHERE id = ?1", rusqlite::params![alert_id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn toggle_alert(&self, alert_id: i64, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE alerts SET enabled = ?1 WHERE id = ?2",
            rusqlite::params![enabled, alert_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Device logs ─────────────────────────────────────────────────────

    pub fn store_log(
        &self,
        device_id: &str,
        severity: &str,
        message: &str,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_logs (device_id, severity, message, timestamp)
             VALUES (?1, ?2, ?3, datetime('now'))",
            rusqlite::params![device_id, severity, message],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_logs(
        &self,
        device_id: &str,
        limit: u32,
    ) -> Result<Vec<LogEntry>, String> {
        self.get_logs_filtered(device_id, limit, None)
    }

    /// Same as `get_logs` but optionally restricts to a set of severities.
    /// Used by the annotation click-through path to fetch only the rows
    /// that can ever appear as annotations (state/error/warn), so noisy
    /// `info` logs cannot push older annotation rows out of the window.
    pub fn get_logs_filtered(
        &self,
        device_id: &str,
        limit: u32,
        severities: Option<&[String]>,
    ) -> Result<Vec<LogEntry>, String> {
        let conn = self.conn.lock().unwrap();
        // Build the SQL with all-anonymous `?` placeholders so positional
        // binding stays unambiguous regardless of how many severities the
        // caller passes (rusqlite gets confused if `?N` and `?` are mixed).
        let sev_list: &[String] = severities.unwrap_or(&[]);
        let sev_clause = if sev_list.is_empty() {
            String::new()
        } else {
            let placeholders: Vec<&str> = (0..sev_list.len()).map(|_| "?").collect();
            format!(" AND severity IN ({})", placeholders.join(","))
        };
        let sql = format!(
            "SELECT severity, message, timestamp FROM device_logs
             WHERE device_id = ?{}
             ORDER BY timestamp DESC LIMIT ?",
            sev_clause
        );
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        // Bind in left-to-right order matching the SQL above:
        //   device_id, severities..., limit
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::with_capacity(2 + sev_list.len());
        params.push(Box::new(device_id.to_string()));
        for sev in sev_list {
            params.push(Box::new(sev.clone()));
        }
        params.push(Box::new(limit));
        let param_refs: Vec<&dyn rusqlite::ToSql> =
            params.iter().map(|b| b.as_ref()).collect();
        let rows = stmt
            .query_map(param_refs.as_slice(), |row| {
                Ok(LogEntry {
                    severity: row.get(0)?,
                    message: row.get(1)?,
                    timestamp: row.get(2)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut logs = Vec::new();
        for row in rows {
            logs.push(row.map_err(|e| e.to_string())?);
        }
        logs.reverse(); // Oldest first
        Ok(logs)
    }
    /// Cross-device activity feed for the Home overview. Returns the most
    /// recent log entries across ALL devices, newest first. Filters to
    /// state/error/warn severities by default (the event types that matter
    /// for an at-a-glance feed — not noisy info/debug chatter).
    pub fn get_recent_activity(
        &self,
        limit: u32,
    ) -> Result<Vec<ActivityEntry>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT device_id, severity, message, timestamp FROM device_logs
                 WHERE severity IN ('state', 'error', 'warn')
                 ORDER BY timestamp DESC LIMIT ?1",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![limit], |row| {
                Ok(ActivityEntry {
                    device_id: row.get(0)?,
                    severity: row.get(1)?,
                    message: row.get(2)?,
                    timestamp: row.get(3)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut entries = Vec::new();
        for row in rows {
            entries.push(row.map_err(|e| e.to_string())?);
        }
        Ok(entries)
    }

    // ─── Schedules ─────────────────────────────────────────────────────

    pub fn create_schedule(
        &self, device_id: &str, capability_id: &str, value: &str,
        cron: &str, label: &str, scene_id: Option<i64>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO schedules (device_id, capability_id, value, cron, label, enabled, scene_id)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6)",
            rusqlite::params![device_id, capability_id, value, cron, label, scene_id],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_schedules(&self) -> Result<Vec<Schedule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, capability_id, value, cron, label, enabled, last_run, scene_id FROM schedules"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            let cron: String = row.get(4)?;
            let next_run = crate::scheduler::compute_next_run(&cron);
            Ok(Schedule {
                id: row.get(0)?, device_id: row.get(1)?, capability_id: row.get(2)?,
                value: row.get(3)?, cron, label: row.get(5)?,
                enabled: row.get(6)?, last_run: row.get(7)?, scene_id: row.get(8)?,
                next_run,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_schedule(&self, id: i64) -> Result<Option<Schedule>, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, device_id, capability_id, value, cron, label, enabled, last_run, scene_id
             FROM schedules WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                let cron: String = row.get(4)?;
                let next_run = crate::scheduler::compute_next_run(&cron);
                Ok(Schedule {
                    id: row.get(0)?, device_id: row.get(1)?, capability_id: row.get(2)?,
                    value: row.get(3)?, cron, label: row.get(5)?,
                    enabled: row.get(6)?, last_run: row.get(7)?, scene_id: row.get(8)?,
                    next_run,
                })
            },
        ).optional().map_err(|e| e.to_string())
    }

    pub fn delete_schedule(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM schedules WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_schedule_last_run(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE schedules SET last_run = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn toggle_schedule(&self, id: i64, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE schedules SET enabled = ?1 WHERE id = ?2", rusqlite::params![enabled, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn duplicate_schedule(&self, id: i64) -> Result<i64, String> {
        let src = self.get_schedule(id)?
            .ok_or_else(|| format!("Schedule {} not found", id))?;
        self.create_schedule(
            &src.device_id, &src.capability_id, &src.value,
            &src.cron, &copy_label(&src.label), src.scene_id,
        )
    }

    // ─── Conditional rules ───────────────────────────────────────────────

    pub fn create_rule(
        &self, source_device_id: &str, source_metric_id: &str,
        condition: &str, threshold: f64,
        target_device_id: &str, target_capability_id: &str, target_value: &str,
        label: &str, logic: &str, conditions: Option<&str>,
        scene_id: Option<i64>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled, logic, conditions, scene_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10, ?11)",
            rusqlite::params![source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, logic, conditions, scene_id],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_rules(&self) -> Result<Vec<Rule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled,
             logic, conditions, last_triggered, scene_id FROM rules"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Rule {
                id: row.get(0)?, source_device_id: row.get(1)?, source_metric_id: row.get(2)?,
                condition: row.get(3)?, threshold: row.get(4)?, target_device_id: row.get(5)?,
                target_capability_id: row.get(6)?, target_value: row.get(7)?,
                label: row.get(8)?, enabled: row.get(9)?,
                logic: row.get::<_, Option<String>>(10)?.unwrap_or_else(|| "and".to_string()),
                conditions: row.get(11)?,
                last_triggered: row.get(12)?,
                scene_id: row.get(13)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_rule(&self, id: i64) -> Result<Option<Rule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled,
             logic, conditions, last_triggered, scene_id FROM rules WHERE id = ?1"
        ).map_err(|e| e.to_string())?;
        let mut rows = stmt.query_map(rusqlite::params![id], |row| {
            Ok(Rule {
                id: row.get(0)?, source_device_id: row.get(1)?, source_metric_id: row.get(2)?,
                condition: row.get(3)?, threshold: row.get(4)?, target_device_id: row.get(5)?,
                target_capability_id: row.get(6)?, target_value: row.get(7)?,
                label: row.get(8)?, enabled: row.get(9)?,
                logic: row.get::<_, Option<String>>(10)?.unwrap_or_else(|| "and".to_string()),
                conditions: row.get(11)?,
                last_triggered: row.get(12)?,
                scene_id: row.get(13)?,
            })
        }).map_err(|e| e.to_string())?;
        match rows.next() {
            Some(r) => r.map(Some).map_err(|e| e.to_string()),
            None => Ok(None),
        }
    }

    pub fn update_rule_last_triggered(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE rules SET last_triggered = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_rule(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM rules WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn toggle_rule(&self, id: i64, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE rules SET enabled = ?1 WHERE id = ?2", rusqlite::params![enabled, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn duplicate_rule(&self, id: i64) -> Result<i64, String> {
        let src = self.get_rule(id)?
            .ok_or_else(|| format!("Rule {} not found", id))?;
        self.create_rule(
            &src.source_device_id, &src.source_metric_id,
            &src.condition, src.threshold,
            &src.target_device_id, &src.target_capability_id, &src.target_value,
            &copy_label(&src.label), &src.logic, src.conditions.as_deref(),
            src.scene_id,
        )
    }

    // ─── Webhooks ────────────────────────────────────────────────────────

    pub fn create_webhook(
        &self, event_type: &str, device_id: Option<&str>, url: &str, label: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO webhooks (event_type, device_id, url, label, enabled) VALUES (?1, ?2, ?3, ?4, 1)",
            rusqlite::params![event_type, device_id, url, label],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_webhooks(&self) -> Result<Vec<Webhook>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT w.id, w.event_type, w.device_id, w.url, w.label, w.enabled,
                    (SELECT timestamp FROM webhook_deliveries
                       WHERE webhook_id = w.id ORDER BY id DESC LIMIT 1) AS last_delivery,
                    (SELECT success FROM webhook_deliveries
                       WHERE webhook_id = w.id ORDER BY id DESC LIMIT 1) AS last_success,
                    (SELECT COUNT(*) FROM webhook_deliveries
                       WHERE webhook_id = w.id AND success = 1) AS success_count,
                    (SELECT COUNT(*) FROM webhook_deliveries
                       WHERE webhook_id = w.id AND success = 0) AS failure_count
             FROM webhooks w"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Webhook {
                id: row.get(0)?, event_type: row.get(1)?, device_id: row.get(2)?,
                url: row.get(3)?, label: row.get(4)?, enabled: row.get(5)?,
                last_delivery: row.get(6)?,
                last_success: row.get(7)?,
                success_count: row.get(8)?,
                failure_count: row.get(9)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn get_webhook(&self, id: i64) -> Result<Option<Webhook>, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, event_type, device_id, url, label, enabled
             FROM webhooks WHERE id = ?1",
            rusqlite::params![id],
            |row| {
                Ok(Webhook {
                    id: row.get(0)?, event_type: row.get(1)?, device_id: row.get(2)?,
                    url: row.get(3)?, label: row.get(4)?, enabled: row.get(5)?,
                    last_delivery: None, last_success: None,
                    success_count: 0, failure_count: 0,
                })
            },
        ).optional().map_err(|e| e.to_string())
    }

    pub fn delete_webhook(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM webhooks WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn toggle_webhook(&self, id: i64, enabled: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("UPDATE webhooks SET enabled = ?1 WHERE id = ?2", rusqlite::params![enabled, id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn duplicate_webhook(&self, id: i64) -> Result<i64, String> {
        let src = self.get_webhook(id)?
            .ok_or_else(|| format!("Webhook {} not found", id))?;
        self.create_webhook(
            &src.event_type, src.device_id.as_deref(), &src.url,
            &copy_label(&src.label),
        )
    }

    /// Fetch webhooks the event dispatcher should fire for `(event_type,
    /// device_id)`. Returns rows where `enabled = 1` AND the event type
    /// matches AND the device filter is satisfied.
    ///
    /// Event-type matching accepts both dot form (`device.online`) and
    /// underscore form (`device_online`) so webhooks created by older UI
    /// revisions still fire after the v0.26.0 dispatcher lands. Callers
    /// pass either form; both equivalents are queried.
    ///
    /// Device-filter semantics:
    ///   * Webhook `device_id` IS NULL → fires for any device on this event.
    ///   * Webhook `device_id` set → fires only when the event's `device_id`
    ///     matches exactly.
    ///   * When the event has no device (`device_id` parameter is `None`),
    ///     `device_id = NULL` evaluates to NULL → only NULL-device webhooks
    ///     match. This is the correct semantics for system events like
    ///     `ota_applied` issued outside a per-device context.
    pub fn get_webhooks_for_event(
        &self, event_type: &str, device_id: Option<&str>,
    ) -> Result<Vec<Webhook>, String> {
        let conn = self.conn.lock().unwrap();
        let dot = event_type.replace('_', ".");
        let underscore = event_type.replace('.', "_");
        let mut stmt = conn.prepare(
            "SELECT id, event_type, device_id, url, label, enabled
             FROM webhooks
             WHERE enabled = 1
               AND (event_type = ?1 OR event_type = ?2)
               AND (device_id IS NULL OR device_id = ?3)"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![dot, underscore, device_id], |row| {
            Ok(Webhook {
                id: row.get(0)?, event_type: row.get(1)?, device_id: row.get(2)?,
                url: row.get(3)?, label: row.get(4)?, enabled: row.get(5)?,
                last_delivery: None, last_success: None,
                success_count: 0, failure_count: 0,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    // ─── Webhook delivery history ───────────────────────────────────────

    pub fn log_webhook_delivery(
        &self, webhook_id: i64, event_type: &str, status_code: Option<i32>,
        success: bool, error: Option<&str>, attempt: i32,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO webhook_deliveries (webhook_id, event_type, status_code, success, error, attempt, timestamp)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))",
            rusqlite::params![webhook_id, event_type, status_code, success, error, attempt],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_webhook_deliveries(&self, webhook_id: i64, limit: i64) -> Result<Vec<WebhookDelivery>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, webhook_id, event_type, status_code, success, error, attempt, timestamp
             FROM webhook_deliveries WHERE webhook_id = ?1 ORDER BY id DESC LIMIT ?2"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![webhook_id, limit], |row| {
            Ok(WebhookDelivery {
                id: row.get(0)?, webhook_id: row.get(1)?, event_type: row.get(2)?,
                status_code: row.get(3)?, success: row.get(4)?, error: row.get(5)?,
                attempt: row.get(6)?, timestamp: row.get(7)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    // ─── Device templates ────────────────────────────────────────────────

    pub fn create_template(
        &self, name: &str, description: &str, capabilities: &str,
        icon: &str, author: &str, board: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_templates (name, description, capabilities, icon, author, board)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            rusqlite::params![name, description, capabilities, icon, author, board],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_templates(&self) -> Result<Vec<DeviceTemplate>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, capabilities, icon, author, board, created_at
             FROM device_templates ORDER BY created_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(DeviceTemplate {
                id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                capabilities: row.get(3)?, icon: row.get(4)?, author: row.get(5)?,
                board: row.get(6)?, created_at: row.get(7)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn delete_template(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM device_templates WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Device groups ──────────────────────────────────────────────────

    pub fn create_group(&self, name: &str, color: &str) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let max_order: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), -1) FROM device_groups", [], |row| row.get(0))
            .unwrap_or(-1);
        conn.execute(
            "INSERT INTO device_groups (name, color, sort_order) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, color, max_order + 1],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_groups(&self) -> Result<Vec<DeviceGroup>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, color, sort_order FROM device_groups ORDER BY sort_order ASC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(DeviceGroup {
                id: row.get(0)?, name: row.get(1)?,
                color: row.get(2)?, sort_order: row.get(3)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    pub fn update_group(&self, id: i64, name: &str, color: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE device_groups SET name = ?1, color = ?2 WHERE id = ?3",
            rusqlite::params![name, color, id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_group(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        // Unassign all devices from this group first
        conn.execute("UPDATE devices SET group_id = NULL WHERE group_id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM device_groups WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_device_group(&self, device_id: &str, group_id: Option<i64>) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET group_id = ?1 WHERE id = ?2",
            rusqlite::params![group_id, device_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn set_device_favorite(&self, device_id: &str, favorite: bool) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET favorite = ?1 WHERE id = ?2",
            rusqlite::params![favorite as i64, device_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Capability-level favorites ─────────────────────────────────────

    pub fn toggle_favorite_capability(&self, device_id: &str, capability_id: &str) -> Result<bool, String> {
        let conn = self.conn.lock().unwrap();
        let exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM favorite_capabilities WHERE device_id = ?1 AND capability_id = ?2",
            rusqlite::params![device_id, capability_id],
            |row| row.get::<_, i64>(0),
        ).map_err(|e| e.to_string())? > 0;

        if exists {
            conn.execute(
                "DELETE FROM favorite_capabilities WHERE device_id = ?1 AND capability_id = ?2",
                rusqlite::params![device_id, capability_id],
            ).map_err(|e| e.to_string())?;
            Ok(false)
        } else {
            conn.execute(
                "INSERT INTO favorite_capabilities (device_id, capability_id) VALUES (?1, ?2)",
                rusqlite::params![device_id, capability_id],
            ).map_err(|e| e.to_string())?;
            Ok(true)
        }
    }

    pub fn get_favorite_capabilities(&self) -> Result<Vec<(String, String)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT device_id, capability_id FROM favorite_capabilities")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| e.to_string())?);
        }
        Ok(result)
    }

    // ─── Floor plans ─────────────────────────────────────────────────────

    pub fn get_floor_plans(&self) -> Result<Vec<FloorPlan>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT id, name, sort_order, background FROM floor_plans ORDER BY sort_order, id")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(FloorPlan {
                id: row.get(0)?,
                name: row.get(1)?,
                sort_order: row.get(2)?,
                background: row.get(3)?,
            })
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| e.to_string())?);
        }
        Ok(result)
    }

    pub fn create_floor_plan(&self, name: &str) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let next_order: i64 = conn
            .query_row("SELECT COALESCE(MAX(sort_order), -1) + 1 FROM floor_plans", [], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO floor_plans (name, sort_order) VALUES (?1, ?2)",
            rusqlite::params![name, next_order],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_floor_plan(&self, id: i64, name: Option<&str>, background: Option<Option<&str>>) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        if let Some(n) = name {
            conn.execute("UPDATE floor_plans SET name = ?1 WHERE id = ?2", rusqlite::params![n, id])
                .map_err(|e| e.to_string())?;
        }
        if let Some(bg) = background {
            // Normalize empty string to NULL
            let bg_val: Option<&str> = bg.filter(|s| !s.is_empty());
            conn.execute("UPDATE floor_plans SET background = ?1 WHERE id = ?2", rusqlite::params![bg_val, id])
                .map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn delete_floor_plan(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM device_positions WHERE floor_id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM floor_plans WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Floor plan positions ────────────────────────────────────────────

    pub fn get_device_positions(&self, floor_id: i64) -> Result<Vec<DevicePosition>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT device_id, floor_id, x, y FROM device_positions WHERE floor_id = ?1")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![floor_id], |row| {
            Ok(DevicePosition {
                device_id: row.get(0)?,
                floor_id: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
            })
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| e.to_string())?);
        }
        Ok(result)
    }

    pub fn get_all_device_positions(&self) -> Result<Vec<DevicePosition>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT device_id, floor_id, x, y FROM device_positions")
            .map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(DevicePosition {
                device_id: row.get(0)?,
                floor_id: row.get(1)?,
                x: row.get(2)?,
                y: row.get(3)?,
            })
        }).map_err(|e| e.to_string())?;
        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| e.to_string())?);
        }
        Ok(result)
    }

    pub fn set_device_position(&self, device_id: &str, floor_id: i64, x: f64, y: f64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_positions (device_id, floor_id, x, y) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(device_id) DO UPDATE SET floor_id = ?2, x = ?3, y = ?4",
            rusqlite::params![device_id, floor_id, x, y],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn remove_device_position(&self, device_id: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM device_positions WHERE device_id = ?1",
            rusqlite::params![device_id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Floor plan rooms ───────────────────────────────────────────────

    pub fn get_rooms(&self, floor_id: i64) -> Result<Vec<FloorPlanRoom>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, floor_id, name, color, x, y, w, h, sort_order
                 FROM floor_plan_rooms WHERE floor_id = ?1
                 ORDER BY sort_order, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![floor_id], |r| {
                Ok(FloorPlanRoom {
                    id: r.get(0)?,
                    floor_id: r.get(1)?,
                    name: r.get(2)?,
                    color: r.get(3)?,
                    x: r.get(4)?,
                    y: r.get(5)?,
                    w: r.get(6)?,
                    h: r.get(7)?,
                    sort_order: r.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn get_all_rooms(&self) -> Result<Vec<FloorPlanRoom>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, floor_id, name, color, x, y, w, h, sort_order
                 FROM floor_plan_rooms
                 ORDER BY floor_id, sort_order, id",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok(FloorPlanRoom {
                    id: r.get(0)?,
                    floor_id: r.get(1)?,
                    name: r.get(2)?,
                    color: r.get(3)?,
                    x: r.get(4)?,
                    y: r.get(5)?,
                    w: r.get(6)?,
                    h: r.get(7)?,
                    sort_order: r.get(8)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    pub fn create_room(
        &self,
        floor_id: i64,
        name: &str,
        color: &str,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let sort_order: i64 = conn
            .query_row(
                "SELECT COALESCE(MAX(sort_order), -1) + 1 FROM floor_plan_rooms WHERE floor_id = ?1",
                rusqlite::params![floor_id],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        conn.execute(
            "INSERT INTO floor_plan_rooms (floor_id, name, color, x, y, w, h, sort_order)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![floor_id, name, color, x, y, w, h, sort_order],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_room(
        &self,
        id: i64,
        name: Option<&str>,
        color: Option<&str>,
        x: Option<f64>,
        y: Option<f64>,
        w: Option<f64>,
        h: Option<f64>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        if let Some(n) = name {
            conn.execute(
                "UPDATE floor_plan_rooms SET name = ?1 WHERE id = ?2",
                rusqlite::params![n, id],
            )
            .map_err(|e| e.to_string())?;
        }
        if let Some(c) = color {
            conn.execute(
                "UPDATE floor_plan_rooms SET color = ?1 WHERE id = ?2",
                rusqlite::params![c, id],
            )
            .map_err(|e| e.to_string())?;
        }
        // x/y/w/h typically move together on resize/move; batch when all 4 supplied.
        if let (Some(xv), Some(yv), Some(wv), Some(hv)) = (x, y, w, h) {
            conn.execute(
                "UPDATE floor_plan_rooms SET x = ?1, y = ?2, w = ?3, h = ?4 WHERE id = ?5",
                rusqlite::params![xv, yv, wv, hv, id],
            )
            .map_err(|e| e.to_string())?;
        } else {
            if let Some(v) = x {
                conn.execute(
                    "UPDATE floor_plan_rooms SET x = ?1 WHERE id = ?2",
                    rusqlite::params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
            if let Some(v) = y {
                conn.execute(
                    "UPDATE floor_plan_rooms SET y = ?1 WHERE id = ?2",
                    rusqlite::params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
            if let Some(v) = w {
                conn.execute(
                    "UPDATE floor_plan_rooms SET w = ?1 WHERE id = ?2",
                    rusqlite::params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
            if let Some(v) = h {
                conn.execute(
                    "UPDATE floor_plan_rooms SET h = ?1 WHERE id = ?2",
                    rusqlite::params![v, id],
                )
                .map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }

    pub fn delete_room(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM floor_plan_rooms WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── Settings ────────────────────────────────────────────────────────

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT value FROM settings WHERE key = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt.query_map(rusqlite::params![key], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        match rows.next() {
            Some(Ok(v)) => Ok(Some(v)),
            _ => Ok(None),
        }
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO settings (key, value) VALUES (?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            rusqlite::params![key, value],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn delete_setting(&self, key: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM settings WHERE key = ?1", rusqlite::params![key])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    // ─── CSV export ──────────────────────────────────────────────────────

    pub fn export_metrics_csv(
        &self, device_id: &str, metric_id: &str, hours: u32,
    ) -> Result<String, String> {
        let points = self.get_metrics(device_id, metric_id, hours)?;
        let mut csv = String::from("timestamp,value\n");
        for p in points {
            csv.push_str(&format!("{},{}\n", p.timestamp, p.value));
        }
        Ok(csv)
    }

    // ─── Firmware history ────────────────────────────────────────────────

    /// Inserts a firmware_history row. `ack_nonce` is a single-use token the
    /// desktop embeds in the ack URL sent to the device; the two-phase OTA
    /// handler validates it on `/api/ota/ack/<nonce>`. Callers that re-serve
    /// an existing row (rollback) skip this method entirely — they already
    /// have the row id and pass `history_row_id: None` to `serve_firmware`.
    pub fn store_firmware_record(
        &self, device_id: &str, version: &str, file_path: &str, file_size: i64,
        ack_nonce: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO firmware_history (device_id, version, file_path, file_size,
                delivery_ack_nonce)
             VALUES (?1, ?2, ?3, ?4, ?5)",
            rusqlite::params![device_id, version, file_path, file_size, ack_nonce],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_firmware_history(&self, device_id: &str) -> Result<Vec<FirmwareRecord>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, version, file_path, file_size, uploaded_at,
                    delivery_status, delivered_at, delivery_error, delivery_applied_at
             FROM firmware_history WHERE device_id = ?1 ORDER BY uploaded_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![device_id], |row| {
            Ok(FirmwareRecord {
                id: row.get(0)?, device_id: row.get(1)?, version: row.get(2)?,
                file_path: row.get(3)?, file_size: row.get(4)?, uploaded_at: row.get(5)?,
                delivery_status: row.get(6)?, delivered_at: row.get(7)?,
                delivery_error: row.get(8)?,
                delivery_applied_at: row.get(9)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    /// Record the outcome of an OTA upload after the device confirms (or
    /// fails to confirm) the apply. `status` is "delivered" or "failed".
    /// `error` carries the serve_firmware-side failure category (e.g.
    /// "body: Broken pipe") for "failed" rows; callers pass None for the
    /// "delivered" case so the column stays null on success.
    /// Updates the exact firmware_history row identified by `row_id` — the
    /// caller captures this from `store_firmware_record` so concurrent OTAs
    /// to the same device can't land each other's outcomes (v0.15.0).
    pub fn mark_firmware_delivery(
        &self, row_id: i64, status: &str, error: Option<&str>,
    ) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE firmware_history
             SET delivery_status = ?2, delivered_at = datetime('now'),
                 delivery_error = ?3
             WHERE id = ?1",
            rusqlite::params![row_id, status, error],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Two-phase OTA apply confirmation (v0.16.0). Looks up a firmware_history
    /// row by its single-use `delivery_ack_nonce`, stamps `delivery_applied_at`,
    /// and returns a classification the ack handler uses to pick an HTTP code.
    ///
    /// Outcomes:
    /// - `Applied(row_id)` — nonce matched an unapplied `delivered` row; newly
    ///   stamped.
    /// - `AlreadyApplied(row_id)` — nonce matched an already-stamped row.
    ///   Idempotent re-POSTs (e.g. device boot-loop) land here.
    /// - `UnknownNonce` — nonce doesn't match any row. Device should drop its
    ///   pending ack (the desktop has lost the row, or the nonce was spoofed).
    /// - `DeliveryNotOk(status)` — nonce matched, but the row's delivery_status
    ///   isn't "delivered" (it's "cancelled" or "failed"). A device POSTing
    ///   here is impossible under normal flows, but we return the mismatched
    ///   status so the handler can log it and the device can drop the ack.
    pub fn mark_firmware_applied_by_nonce(
        &self,
        nonce: &str,
    ) -> Result<AckLookupOutcome, String> {
        let conn = self.conn.lock().unwrap();
        let row: Option<(i64, Option<String>, Option<String>)> = conn
            .query_row(
                "SELECT id, delivery_status, delivery_applied_at
                 FROM firmware_history
                 WHERE delivery_ack_nonce = ?1",
                rusqlite::params![nonce],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .ok();
        let Some((row_id, status_opt, applied_opt)) = row else {
            return Ok(AckLookupOutcome::UnknownNonce);
        };
        if applied_opt.is_some() {
            return Ok(AckLookupOutcome::AlreadyApplied(row_id));
        }
        match status_opt.as_deref() {
            Some("delivered") => {
                conn.execute(
                    "UPDATE firmware_history
                     SET delivery_applied_at = datetime('now')
                     WHERE id = ?1",
                    rusqlite::params![row_id],
                )
                .map_err(|e| e.to_string())?;
                Ok(AckLookupOutcome::Applied(row_id))
            }
            other => Ok(AckLookupOutcome::DeliveryNotOk(
                other.unwrap_or("pending").to_string(),
            )),
        }
    }

    /// Returns the device_id the given firmware_history row belongs to, if
    /// the row exists. Used by the ack handler to address the downstream
    /// `ota_applied` Tauri event + web-dashboard WS broadcast.
    pub fn get_firmware_device_id(&self, row_id: i64) -> Result<Option<String>, String> {
        let conn = self.conn.lock().unwrap();
        let found: Option<String> = conn
            .query_row(
                "SELECT device_id FROM firmware_history WHERE id = ?1",
                rusqlite::params![row_id],
                |r| r.get(0),
            )
            .ok();
        Ok(found)
    }

    pub fn delete_firmware_record(&self, id: i64) -> Result<String, String> {
        let conn = self.conn.lock().unwrap();
        let path: String = conn.query_row(
            "SELECT file_path FROM firmware_history WHERE id = ?1",
            rusqlite::params![id],
            |row| row.get(0),
        ).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM firmware_history WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(path)
    }

    // ─── Reset history (power-supply stability signal) ──────────────────

    /// Record a reboot. Called from `discovery` when a device's reported
    /// `uptime_s` drops below the previously-seen value — that monotonic
    /// break is how we attribute a reboot to the exact moment discovery
    /// saw it. The `reset_reason` comes from the device's self-reported
    /// esp_reset_reason() string ("brownout", "panic", "poweron", etc.).
    pub fn record_reset(&self, device_id: &str, reset_reason: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_reset_history (device_id, reset_reason, recorded_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![device_id, reset_reason],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Return reset events for a device within the given rolling window,
    /// newest-first. Caller decides how many to classify as brownout vs
    /// clean-boot. Cap at 50 to bound memory on a fleet that's thrashing.
    pub fn get_resets(&self, device_id: &str, hours: u32) -> Result<Vec<ResetEvent>, String> {
        let conn = self.conn.lock().unwrap();
        let window = format!("-{} hours", hours);
        let mut stmt = conn
            .prepare(
                "SELECT reset_reason, recorded_at FROM device_reset_history
                 WHERE device_id = ?1 AND recorded_at >= datetime('now', ?2)
                 ORDER BY recorded_at DESC LIMIT 50",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                Ok(ResetEvent {
                    reset_reason: row.get(0)?,
                    recorded_at: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    // ─── mDNS latency samples (network health signal) ─────────────────────

    /// Record a single mDNS cadence sample for a device — the interval in
    /// ms between the previous and current `ServiceResolved` event for the
    /// same service instance, already de-duped across listening interfaces
    /// and the sub-debounce redundant re-fires. Called from the discovery
    /// browse loop on every accepted Resolved after the first one.
    pub fn record_mdns_cadence(&self, device_id: &str, interval_ms: u32) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_mdns_latency (device_id, latency_ms, sample_kind, recorded_at)
             VALUES (?1, ?2, 'cadence', datetime('now'))",
            rusqlite::params![device_id, interval_ms],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Return mDNS cadence samples for a device within the given rolling
    /// window, newest-first. Filters out legacy `'resolution'` rows so the
    /// rule's percentile math doesn't mix two incompatible units. Cap at
    /// 500 to bound memory — a healthy ESP32 with 120s TTL emits ~720
    /// intervals per 24h; the cap trims the oldest of those.
    pub fn get_mdns_cadence_samples(&self, device_id: &str, hours: u32) -> Result<Vec<MdnsCadenceSample>, String> {
        let conn = self.conn.lock().unwrap();
        let window = format!("-{} hours", hours);
        let mut stmt = conn
            .prepare(
                "SELECT latency_ms, recorded_at FROM device_mdns_latency
                 WHERE device_id = ?1 AND sample_kind = 'cadence'
                   AND recorded_at >= datetime('now', ?2)
                 ORDER BY recorded_at DESC LIMIT 500",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                Ok(MdnsCadenceSample {
                    interval_ms: row.get::<_, i64>(0)? as u32,
                    recorded_at: row.get(1)?,
                })
            })
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }
        Ok(out)
    }

    // ─── Annotations (chart event markers) ───────────────────────────────

    /// Collect point-in-time events for a device within the given rolling
    /// window (in hours). Unions OTA uploads from firmware_history with
    /// state/error/warn rows from device_logs. Sorted oldest-first.
    /// Capped at 200 points — if exceeded, the newest 200 are kept so the
    /// most recent activity is always visible on the chart.
    pub fn get_annotations(
        &self,
        device_id: &str,
        hours: u32,
    ) -> Result<Vec<Annotation>, String> {
        let conn = self.conn.lock().unwrap();
        let window = format!("-{} hours", hours);
        let mut out: Vec<Annotation> = Vec::new();

        // OTA events — one row per firmware upload
        let mut stmt = conn
            .prepare(
                "SELECT version, uploaded_at FROM firmware_history
                 WHERE device_id = ?1 AND uploaded_at >= datetime('now', ?2)
                 ORDER BY uploaded_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                let version: String = row.get(0)?;
                let ts: String = row.get(1)?;
                Ok(Annotation {
                    timestamp: ts,
                    kind: "ota".to_string(),
                    label: format!("OTA v{}", version),
                    severity: "info".to_string(),
                })
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }

        // Reset events — one row per observed reboot, bucketed by
        // reset_reason so the frontend can render the marker with a color
        // that tracks the `power_supply_stability` rule's taxonomy:
        //   brownout → reset_brownout (error, red)
        //   panic / watchdog family → reset_fault (warn, purple)
        //   poweron / software / external / deepsleep / unknown → reset (info, slate)
        let mut stmt = conn
            .prepare(
                "SELECT reset_reason, recorded_at FROM device_reset_history
                 WHERE device_id = ?1 AND recorded_at >= datetime('now', ?2)
                 ORDER BY recorded_at ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                let reason: String = row.get(0)?;
                let ts: String = row.get(1)?;
                let (kind, severity) = match reason.as_str() {
                    "brownout" => ("reset_brownout", "error"),
                    "panic" | "watchdog" | "task_watchdog" | "interrupt_watchdog" => {
                        ("reset_fault", "warn")
                    }
                    _ => ("reset", "info"),
                };
                let label = match reason.as_str() {
                    "brownout" => "Brownout reset".to_string(),
                    "panic" => "Panic reset".to_string(),
                    "watchdog" => "Watchdog reset".to_string(),
                    "task_watchdog" => "Task-watchdog reset".to_string(),
                    "interrupt_watchdog" => "Interrupt-watchdog reset".to_string(),
                    "poweron" => "Power-on reset".to_string(),
                    "software" => "Software reset".to_string(),
                    "external" => "External reset".to_string(),
                    "deepsleep" => "Deep-sleep wake".to_string(),
                    other => format!("Reset ({})", other),
                };
                Ok(Annotation {
                    timestamp: ts,
                    kind: kind.to_string(),
                    label,
                    severity: severity.to_string(),
                })
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }

        // State transitions + device-reported errors/warnings
        let mut stmt = conn
            .prepare(
                "SELECT severity, message, timestamp FROM device_logs
                 WHERE device_id = ?1
                   AND timestamp >= datetime('now', ?2)
                   AND (severity = 'state' OR severity = 'error' OR severity = 'warn')
                 ORDER BY timestamp ASC",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                let severity: String = row.get(0)?;
                let message: String = row.get(1)?;
                let ts: String = row.get(2)?;
                let kind = match severity.as_str() {
                    "state" => match message.as_str() {
                        "online" => "online",
                        "offline" => "offline",
                        _ => "state",
                    },
                    "error" => "error",
                    "warn" => "warn",
                    _ => "other",
                }
                .to_string();
                // Humanize the default state labels; everything else passes
                // through the device-reported message unchanged.
                let label = match (severity.as_str(), message.as_str()) {
                    ("state", "online") => "Came online".to_string(),
                    ("state", "offline") => "Went offline".to_string(),
                    _ => message,
                };
                Ok(Annotation {
                    timestamp: ts,
                    kind,
                    label,
                    severity,
                })
            })
            .map_err(|e| e.to_string())?;
        for r in rows {
            out.push(r.map_err(|e| e.to_string())?);
        }

        // Merge the two streams by timestamp.
        out.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));

        // Cap at 200 total — keep the most recent so the newest activity
        // is always visible when errors are noisy on a 7-day window.
        const MAX: usize = 200;
        if out.len() > MAX {
            let skip = out.len() - MAX;
            out = out.split_off(skip);
        }

        Ok(out)
    }

    // ─── API tokens ──────────────────────────────────────────────────────

    /// Insert a new token row. The caller is responsible for generating the
    /// plaintext token and computing its SHA-256 hex digest — this method
    /// only stores the digest. Returns the new row id so the caller can
    /// echo it back to the UI alongside the plaintext.
    pub fn create_api_token(&self, name: &str, token_hash: &str, expires_at: Option<&str>, role: &str) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO api_tokens (name, token_hash, expires_at, role) VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![name, token_hash, expires_at, role],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    /// Return all tokens for the Settings UI listing — name + timestamps,
    /// no hash field. The hash is never exposed via the UI; it's only used
    /// internally by `find_api_token_by_hash` for auth checks.
    pub fn list_api_tokens(&self) -> Result<Vec<ApiToken>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, created_at, last_used_at, expires_at, role FROM api_tokens ORDER BY created_at DESC")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok(ApiToken {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    created_at: row.get(2)?,
                    last_used_at: row.get(3)?,
                    expires_at: row.get(4)?,
                    role: row.get::<_, Option<String>>(5)?.unwrap_or_else(|| "admin".to_string()),
                })
            })
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())
    }

    /// Look up a token by its SHA-256 hex digest. Returns `(row_id, expires_at, role)`
    /// if found. The auth gate uses `expires_at` to reject expired tokens with
    /// a distinct error message and `role` for permission enforcement.
    /// `None` means "no such token" (auth-failure path).
    pub fn find_api_token_by_hash(&self, token_hash: &str) -> Result<Option<(i64, Option<String>, String)>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, expires_at, role FROM api_tokens WHERE token_hash = ?1")
            .map_err(|e| e.to_string())?;
        let mut rows = stmt
            .query_map(rusqlite::params![token_hash], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "admin".to_string()),
                ))
            })
            .map_err(|e| e.to_string())?;
        match rows.next() {
            Some(Ok(tuple)) => Ok(Some(tuple)),
            _ => Ok(None),
        }
    }

    /// Bump the `last_used_at` column. Called from the auth path on every
    /// successful authenticated request — best-effort, errors are logged
    /// but not surfaced to the user (a stale last-used timestamp is not a
    /// reason to fail their request).
    pub fn touch_api_token(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE api_tokens SET last_used_at = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Delete a token by row id. After this returns Ok, any subsequent
    /// request bearing the matching token gets 401 — there's no soft-delete
    /// or grace period, revocation is immediate.
    pub fn delete_api_token(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM api_tokens WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Return the count of tokens. The auth middleware uses this as a fast
    /// pre-check: when zero tokens exist, every non-loopback request gets a
    /// distinct error message ("mint a token first") instead of the generic
    /// "invalid token" 401, which is more useful for first-time users.
    pub fn count_api_tokens(&self) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row("SELECT COUNT(*) FROM api_tokens", [], |row| row.get(0))
            .map_err(|e| e.to_string())
    }

    // ─── Scenes ─────────────────────────────────────────────────────────

    pub fn create_scene(&self, name: &str, actions: &[SceneActionInput]) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO scenes (name) VALUES (?1)",
            rusqlite::params![name],
        ).map_err(|e| e.to_string())?;
        let scene_id = conn.last_insert_rowid();
        for (i, action) in actions.iter().enumerate() {
            conn.execute(
                "INSERT INTO scene_actions (scene_id, device_id, capability_id, value, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![scene_id, action.device_id, action.capability_id, action.value, i as i64],
            ).map_err(|e| e.to_string())?;
        }
        Ok(scene_id)
    }

    pub fn get_scenes(&self) -> Result<Vec<Scene>, String> {
        let conn = self.conn.lock().unwrap();
        let mut scene_stmt = conn.prepare(
            "SELECT id, name, created_at, last_run FROM scenes ORDER BY id"
        ).map_err(|e| e.to_string())?;
        let scene_rows = scene_stmt.query_map([], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            ))
        }).map_err(|e| e.to_string())?;
        let mut scenes = Vec::new();
        for row in scene_rows {
            let (id, name, created_at, last_run) = row.map_err(|e| e.to_string())?;
            scenes.push(Scene { id, name, created_at, last_run, actions: Vec::new() });
        }
        drop(scene_stmt);

        for scene in &mut scenes {
            let mut action_stmt = conn.prepare(
                "SELECT device_id, capability_id, value FROM scene_actions
                 WHERE scene_id = ?1 ORDER BY sort_order"
            ).map_err(|e| e.to_string())?;
            let action_rows = action_stmt.query_map(rusqlite::params![scene.id], |row| {
                Ok(SceneAction {
                    device_id: row.get(0)?,
                    capability_id: row.get(1)?,
                    value: row.get(2)?,
                })
            }).map_err(|e| e.to_string())?;
            scene.actions = action_rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        }
        Ok(scenes)
    }

    pub fn get_scene(&self, id: i64) -> Result<Option<Scene>, String> {
        let conn = self.conn.lock().unwrap();
        let scene = conn.query_row(
            "SELECT id, name, created_at, last_run FROM scenes WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
            )),
        );
        let (scene_id, name, created_at, last_run) = match scene {
            Ok(s) => s,
            Err(rusqlite::Error::QueryReturnedNoRows) => return Ok(None),
            Err(e) => return Err(e.to_string()),
        };
        let mut action_stmt = conn.prepare(
            "SELECT device_id, capability_id, value FROM scene_actions
             WHERE scene_id = ?1 ORDER BY sort_order"
        ).map_err(|e| e.to_string())?;
        let actions = action_stmt.query_map(rusqlite::params![scene_id], |row| {
            Ok(SceneAction {
                device_id: row.get(0)?,
                capability_id: row.get(1)?,
                value: row.get(2)?,
            })
        }).map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())?;
        Ok(Some(Scene { id: scene_id, name, created_at, last_run, actions }))
    }

    pub fn update_scene_last_run(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE scenes SET last_run = datetime('now') WHERE id = ?1",
            rusqlite::params![id],
        ).map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn update_scene(&self, id: i64, name: &str, actions: &[SceneActionInput]) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE scenes SET name = ?1 WHERE id = ?2",
            rusqlite::params![name, id],
        ).map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM scene_actions WHERE scene_id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        for (i, action) in actions.iter().enumerate() {
            conn.execute(
                "INSERT INTO scene_actions (scene_id, device_id, capability_id, value, sort_order)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                rusqlite::params![id, action.device_id, action.capability_id, action.value, i as i64],
            ).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    pub fn duplicate_scene(&self, id: i64) -> Result<i64, String> {
        let src = self.get_scene(id)?
            .ok_or_else(|| format!("Scene {} not found", id))?;
        let inputs: Vec<SceneActionInput> = src.actions.iter().map(|a| SceneActionInput {
            device_id: a.device_id.clone(),
            capability_id: a.capability_id.clone(),
            value: a.value.clone(),
        }).collect();
        self.create_scene(&copy_label(&src.name), &inputs)
    }

    pub fn delete_scene(&self, id: i64) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM scene_actions WHERE scene_id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM scenes WHERE id = ?1", rusqlite::params![id])
            .map_err(|e| e.to_string())?;
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Scene {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    #[serde(default)]
    pub last_run: Option<String>,
    pub actions: Vec<SceneAction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SceneAction {
    pub device_id: String,
    pub capability_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SceneActionInput {
    pub device_id: String,
    pub capability_id: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Schedule {
    pub id: i64,
    pub device_id: String,
    pub capability_id: String,
    pub value: String,
    pub cron: String,
    pub label: String,
    pub enabled: bool,
    pub last_run: Option<String>,
    pub scene_id: Option<i64>,
    #[serde(default)]
    pub next_run: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Rule {
    pub id: i64,
    pub source_device_id: String,
    pub source_metric_id: String,
    pub condition: String,
    pub threshold: f64,
    pub target_device_id: String,
    pub target_capability_id: String,
    pub target_value: String,
    pub label: String,
    pub enabled: bool,
    #[serde(default = "default_logic")]
    pub logic: String,
    #[serde(default)]
    pub conditions: Option<String>,
    #[serde(default)]
    pub last_triggered: Option<String>,
    #[serde(default)]
    pub scene_id: Option<i64>,
}

fn default_logic() -> String {
    "and".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Webhook {
    pub id: i64,
    pub event_type: String,
    pub device_id: Option<String>,
    pub url: String,
    pub label: String,
    pub enabled: bool,
    #[serde(default)]
    pub last_delivery: Option<String>,
    #[serde(default)]
    pub last_success: Option<bool>,
    #[serde(default)]
    pub success_count: i64,
    #[serde(default)]
    pub failure_count: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDelivery {
    pub id: i64,
    pub webhook_id: i64,
    pub event_type: String,
    pub status_code: Option<i32>,
    pub success: bool,
    pub error: Option<String>,
    pub attempt: i32,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTemplate {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub capabilities: String,
    pub icon: String,
    pub author: String,
    pub board: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirmwareRecord {
    pub id: i64,
    pub device_id: String,
    pub version: String,
    pub file_path: String,
    pub file_size: i64,
    pub uploaded_at: String,
    pub delivery_status: Option<String>,
    pub delivered_at: Option<String>,
    pub delivery_error: Option<String>,
    /// Set by the device-initiated ack handler once the new firmware has
    /// booted and POSTed to /api/ota/ack/<nonce> (v0.16.0).
    pub delivery_applied_at: Option<String>,
}

/// Outcome of looking up an ack nonce. See `mark_firmware_applied_by_nonce`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AckLookupOutcome {
    Applied(i64),
    AlreadyApplied(i64),
    UnknownNonce,
    DeliveryNotOk(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub severity: String,
    pub message: String,
    pub timestamp: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ActivityEntry {
    pub device_id: String,
    pub severity: String,
    pub message: String,
    pub timestamp: String,
}

/// Chart annotation event — a point-in-time marker drawn as a vertical line
/// on the metric charts. Produced by unioning firmware_history (OTA events)
/// and device_logs rows whose severity is 'state', 'error', or 'warn'.
/// `kind` is a stable frontend identifier that drives the color/label in the
/// chart legend: "ota", "online", "offline", "error", "warn".
#[derive(Debug, Clone, Serialize)]
pub struct Annotation {
    pub timestamp: String,
    pub kind: String,
    pub label: String,
    pub severity: String,
}

/// One row of `device_reset_history`. Inserted each time discovery detects
/// a reboot (observed uptime_s dropping below the previously-seen value),
/// carrying the ESP32's self-reported reset reason captured at boot via
/// `esp_reset_reason()`. The power-supply-stability diagnostics rule reads
/// these to surface brownout clusters.
#[derive(Debug, Clone, Serialize)]
pub struct ResetEvent {
    pub reset_reason: String,
    pub recorded_at: String,
}

/// One row of `device_mdns_latency` with `sample_kind = 'cadence'`. Each
/// sample is the interval in ms between two successive `ServiceResolved`
/// events for the same service instance (deduped across interfaces),
/// captured in `mdns_browse_loop`. The mdns_latency diagnostics rule reads
/// trailing samples and compares p50/p95 against the expected TTL-driven
/// refresh cadence — stretching cadence is a health proxy for a device or
/// LAN path that's dropping announcements.
#[derive(Debug, Clone, Serialize)]
pub struct MdnsCadenceSample {
    pub interval_ms: u32,
    pub recorded_at: String,
}

/// Public-facing view of an API token row. The plaintext token is **never**
/// stored or returned after creation — only its SHA-256 digest lives in
/// SQLite. List/get endpoints return this struct so the UI can show name,
/// creation, and last-used timestamps without exposing anything sensitive.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ApiToken {
    pub id: i64,
    pub name: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
    pub expires_at: Option<String>,
    pub role: String,
}

/// Rebuild `rules` and `schedules` to drop the FKs on the target/device columns
/// that block scene-targeted rows. Idempotent — gated on `pragma_foreign_key_list`
/// reporting the FK still in place. Wraps each table rebuild in a transaction
/// and toggles `PRAGMA foreign_keys=OFF` for the duration to satisfy SQLite's
/// rebuild contract (per https://sqlite.org/lang_altertable.html).
fn rebuild_drop_target_device_fks(conn: &Connection) -> Result<(), String> {
    fn fk_from_columns(conn: &Connection, table: &str) -> Result<Vec<String>, String> {
        let sql = format!("SELECT \"from\" FROM pragma_foreign_key_list('{}')", table);
        let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
    }

    let rules_needs = fk_from_columns(conn, "rules")?
        .iter()
        .any(|c| c == "target_device_id");
    let schedules_needs = fk_from_columns(conn, "schedules")?
        .iter()
        .any(|c| c == "device_id");

    if !rules_needs && !schedules_needs {
        return Ok(());
    }

    conn.execute_batch("PRAGMA foreign_keys = OFF;").map_err(|e| e.to_string())?;

    if rules_needs {
        conn.execute_batch("
            BEGIN;
            CREATE TABLE rules_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_device_id TEXT NOT NULL,
                source_metric_id TEXT NOT NULL,
                condition TEXT NOT NULL,
                threshold REAL NOT NULL,
                target_device_id TEXT NOT NULL,
                target_capability_id TEXT NOT NULL,
                target_value TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                logic TEXT NOT NULL DEFAULT 'and',
                conditions TEXT,
                last_triggered TEXT,
                scene_id INTEGER REFERENCES scenes(id),
                FOREIGN KEY (source_device_id) REFERENCES devices(id)
            );
            INSERT INTO rules_new (id, source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, enabled,
                logic, conditions, last_triggered, scene_id)
                SELECT id, source_device_id, source_metric_id, condition, threshold,
                    target_device_id, target_capability_id, target_value, label, enabled,
                    logic, conditions, last_triggered, scene_id FROM rules;
            DROP TABLE rules;
            ALTER TABLE rules_new RENAME TO rules;
            COMMIT;
        ").map_err(|e| e.to_string())?;
    }

    if schedules_needs {
        conn.execute_batch("
            BEGIN;
            CREATE TABLE schedules_new (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                value TEXT NOT NULL,
                cron TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                scene_id INTEGER REFERENCES scenes(id)
            );
            INSERT INTO schedules_new (id, device_id, capability_id, value, cron, label,
                enabled, last_run, scene_id)
                SELECT id, device_id, capability_id, value, cron, label,
                    enabled, last_run, scene_id FROM schedules;
            DROP TABLE schedules;
            ALTER TABLE schedules_new RENAME TO schedules;
            COMMIT;
        ").map_err(|e| e.to_string())?;
    }

    conn.execute_batch("PRAGMA foreign_keys = ON;").map_err(|e| e.to_string())?;
    Ok(())
}

pub fn init_db(app: &AppHandle) -> Result<(), Box<dyn std::error::Error>> {
    let app_dir = app
        .path()
        .app_data_dir()
        .expect("failed to get app data dir");
    std::fs::create_dir_all(&app_dir)?;

    let db_path = app_dir.join("trellis.db");
    let conn = Connection::open(db_path)?;

    conn.execute_batch(
        "
        CREATE TABLE IF NOT EXISTS devices (
            id TEXT PRIMARY KEY,
            name TEXT NOT NULL,
            ip TEXT NOT NULL,
            port INTEGER NOT NULL,
            firmware TEXT,
            platform TEXT,
            nickname TEXT,
            tags TEXT DEFAULT '',
            first_seen TEXT NOT NULL DEFAULT (datetime('now')),
            last_seen TEXT NOT NULL DEFAULT (datetime('now')),
            notes TEXT NOT NULL DEFAULT '',
            install_date TEXT NOT NULL DEFAULT ''
        );

        CREATE TABLE IF NOT EXISTS metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            metric_id TEXT NOT NULL,
            value REAL NOT NULL,
            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE TABLE IF NOT EXISTS alerts (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            metric_id TEXT NOT NULL,
            condition TEXT NOT NULL,
            threshold REAL NOT NULL,
            label TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE TABLE IF NOT EXISTS device_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            severity TEXT NOT NULL,
            message TEXT NOT NULL,
            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE TABLE IF NOT EXISTS schedules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            value TEXT NOT NULL,
            cron TEXT NOT NULL,
            label TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            last_run TEXT,
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE TABLE IF NOT EXISTS rules (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            source_device_id TEXT NOT NULL,
            source_metric_id TEXT NOT NULL,
            condition TEXT NOT NULL,
            threshold REAL NOT NULL,
            target_device_id TEXT NOT NULL,
            target_capability_id TEXT NOT NULL,
            target_value TEXT NOT NULL,
            label TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (source_device_id) REFERENCES devices(id),
            FOREIGN KEY (target_device_id) REFERENCES devices(id)
        );

        CREATE TABLE IF NOT EXISTS webhooks (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            event_type TEXT NOT NULL,
            device_id TEXT,
            url TEXT NOT NULL,
            label TEXT NOT NULL,
            enabled INTEGER NOT NULL DEFAULT 1
        );

        CREATE TABLE IF NOT EXISTS device_templates (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            description TEXT DEFAULT '',
            capabilities TEXT NOT NULL,
            icon TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            board TEXT NOT NULL DEFAULT 'esp32',
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );

        CREATE TABLE IF NOT EXISTS device_groups (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            color TEXT NOT NULL DEFAULT '#6366f1',
            sort_order INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS settings (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS firmware_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            version TEXT NOT NULL,
            file_path TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            uploaded_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        -- API tokens for the REST API on :9090. The plaintext token is
        -- shown to the user exactly once at creation and never persisted —
        -- only the SHA-256 hex digest lives here. The hash column is the
        -- lookup key for auth checks (UNIQUE INDEX makes it O(log n)).
        CREATE TABLE IF NOT EXISTS api_tokens (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            token_hash TEXT NOT NULL UNIQUE,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            last_used_at TEXT
        );

        -- Reboot attribution (post-v0.16.0). One row per reboot observed by
        -- the desktop via uptime decrease on /api/info, carrying the ESP32's
        -- self-reported esp_reset_reason() string. The power-supply-stability
        -- diagnostics rule reads the last N rows in its window and escalates
        -- on repeated brownouts.
        CREATE TABLE IF NOT EXISTS device_reset_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            reset_reason TEXT NOT NULL,
            recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        -- Per-sample mDNS timing, in ms. `sample_kind` distinguishes the
        -- two capture models the rule has used:
        --   'resolution' — legacy v0.17.0: elapsed time between a
        --   ServiceFound and its matching ServiceResolved. Captured once
        --   per new announcement; rare in steady state (TTL refreshes
        --   don't emit Found). No longer written; pre-existing rows age
        --   out of the 24h window naturally.
        --   'cadence' — v0.18.0+: interval between successive
        --   ServiceResolved events for the same service instance,
        --   de-duped across interfaces. Fires on every mDNS TTL refresh,
        --   so there is a steady signal — stretching cadence is a health
        --   proxy for a flaky device or LAN path.
        CREATE TABLE IF NOT EXISTS device_mdns_latency (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            latency_ms INTEGER NOT NULL,
            sample_kind TEXT NOT NULL DEFAULT 'resolution',
            recorded_at TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE INDEX IF NOT EXISTS idx_metrics_device_time
            ON metrics(device_id, metric_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_device_time
            ON device_logs(device_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_firmware_device
            ON firmware_history(device_id, uploaded_at);
        CREATE INDEX IF NOT EXISTS idx_api_tokens_hash
            ON api_tokens(token_hash);
        CREATE INDEX IF NOT EXISTS idx_logs_timestamp
            ON device_logs(timestamp);
        CREATE INDEX IF NOT EXISTS idx_reset_device_time
            ON device_reset_history(device_id, recorded_at);
        CREATE INDEX IF NOT EXISTS idx_mdns_device_time
            ON device_mdns_latency(device_id, recorded_at);
        ",
    )?;

    // Add group_id column to devices if it doesn't exist
    let _ = conn.execute("ALTER TABLE devices ADD COLUMN group_id INTEGER REFERENCES device_groups(id)", []);

    // Add expires_at column to api_tokens if it doesn't exist (v0.4.4 — token TTL)
    let _ = conn.execute("ALTER TABLE api_tokens ADD COLUMN expires_at TEXT", []);

    // Add sort_order column to devices if it doesn't exist (dashboard card ordering)
    let _ = conn.execute("ALTER TABLE devices ADD COLUMN sort_order INTEGER NOT NULL DEFAULT 0", []);

    // Add role column to api_tokens if it doesn't exist (v0.5.0 — RBAC).
    // Default 'admin' preserves existing behavior: pre-migration tokens get
    // full access without requiring manual re-configuration.
    let _ = conn.execute("ALTER TABLE api_tokens ADD COLUMN role TEXT NOT NULL DEFAULT 'admin'", []);

    // Add favorite column to devices if it doesn't exist (post-v0.6.0 — pinned devices, device-level, kept for compat)
    let _ = conn.execute("ALTER TABLE devices ADD COLUMN favorite INTEGER NOT NULL DEFAULT 0", []);

    // Per-device GitHub repo binding for Diagnostics v3 firmware auto-remediation (post-v0.13.0)
    let _ = conn.execute("ALTER TABLE devices ADD COLUMN github_owner TEXT", []);
    let _ = conn.execute("ALTER TABLE devices ADD COLUMN github_repo TEXT", []);

    let _ = conn.execute("ALTER TABLE devices ADD COLUMN notes TEXT NOT NULL DEFAULT ''", []);

    let _ = conn.execute("ALTER TABLE devices ADD COLUMN install_date TEXT NOT NULL DEFAULT ''", []);

    // OTA delivery outcome persistence (v0.15.0). Null on existing rows and on
    // any new upload until the device confirms (or fails to confirm) the apply.
    let _ = conn.execute("ALTER TABLE firmware_history ADD COLUMN delivery_status TEXT", []);
    let _ = conn.execute("ALTER TABLE firmware_history ADD COLUMN delivered_at TEXT", []);
    // Captured error string for "failed" rows (v0.15.0). The payload that
    // serve_firmware already emits on `ota_delivery_failed` — now durable so
    // the diagnostics rule can show the category (e.g. "body: Connection
    // reset by peer") instead of a bare "N/M delivered".
    let _ = conn.execute("ALTER TABLE firmware_history ADD COLUMN delivery_error TEXT", []);

    // Two-phase OTA tracking (v0.16.0). `delivery_ack_nonce` is a single-use
    // capability token the desktop mints at upload and embeds in the ack URL
    // sent to the device; the device POSTs to /api/ota/ack/<nonce> after it
    // boots into the new firmware. `delivery_applied_at` records when that
    // ack landed. Null until the device confirms apply; stays null for rows
    // without a nonce (rollbacks) or for pre-v0.16.0 rows.
    let _ = conn.execute("ALTER TABLE firmware_history ADD COLUMN delivery_ack_nonce TEXT", []);
    let _ = conn.execute("ALTER TABLE firmware_history ADD COLUMN delivery_applied_at TEXT", []);

    // mDNS capture-model switch (v0.18.0): legacy rows are Found→Resolved
    // latency; new rows are inter-Resolved cadence. Default 'resolution'
    // tags existing data correctly so the new rule's cadence-only read
    // doesn't treat stale latency numbers as intervals.
    let _ = conn.execute("ALTER TABLE device_mdns_latency ADD COLUMN sample_kind TEXT NOT NULL DEFAULT 'resolution'", []);

    // Linear-power opt-in flag for slider energy tracking (phase 2).
    // When set, the connection state-log writer persists slider value
    // transitions and get_device_energy integrates value/max over time.
    // slider_max is captured at opt-in from the live Capability.max so the
    // integration doesn't need the device online to compute Wh.
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN linear_power INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN slider_max REAL", []);

    // HA binary_sensor opt-in (v0.27.0): when set on a sensor capability,
    // its HA discovery config is published under the `binary_sensor`
    // component instead of `sensor`. Optional `binary_sensor_device_class`
    // (motion/door/occupancy/etc.) carries through to HA so the entity
    // gets the right icon and translation.
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN binary_sensor INTEGER NOT NULL DEFAULT 0", []);
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN binary_sensor_device_class TEXT", []);

    // HA cover routing opt-in (v0.27.0): when set on a slider capability,
    // its HA discovery config is published under the `cover` component
    // (with position_topic/set_position_topic) instead of `number`. The
    // cover's `position_open` is taken from the slider's max at publish
    // time; `position_closed` is 0. Independent of energy fields.
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN cover_position INTEGER NOT NULL DEFAULT 0", []);

    // HA `light` brightness linkage (v0.27.0): when set on a slider
    // capability, points to a color cap on the same device. The MQTT bridge
    // promotes the color cap's HA discovery to a `light` entity with
    // `brightness: true` + `brightness_scale = slider.max`, retracts this
    // slider's separate `number`/`cover` entity, and merges color + slider
    // state into a synthetic `_light/state` topic that HA subscribes to.
    let _ = conn.execute("ALTER TABLE capability_meta ADD COLUMN brightness_for_cap_id TEXT", []);

    // Capability-level favorites (post-v0.6.0 — replaces device-level favorite for granular pinning)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS favorite_capabilities (
            device_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            PRIMARY KEY (device_id, capability_id),
            FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
        );
    ").map_err(|e| e.to_string())?;

    // Floor plans table (v0.8.0 — multi-floor support)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS floor_plans (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            background TEXT
        );
    ").map_err(|e| e.to_string())?;

    // Floor plan: device positions on the spatial canvas.
    // Fresh installs get floor_id in the CREATE; upgrades from v0.7.0 get it via ALTER below.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS device_positions (
            device_id TEXT NOT NULL PRIMARY KEY,
            x REAL NOT NULL DEFAULT 0.0,
            y REAL NOT NULL DEFAULT 0.0,
            floor_id INTEGER NOT NULL DEFAULT 1,
            FOREIGN KEY (device_id) REFERENCES devices(id) ON DELETE CASCADE
        );
    ").map_err(|e| e.to_string())?;

    // Migrate device_positions: add floor_id column if missing (v0.8.0).
    // Creates a default "Floor 1" and moves existing positions + background onto it.
    {
        let has_floor_id: bool = conn
            .prepare("SELECT floor_id FROM device_positions LIMIT 0")
            .is_ok();
        if !has_floor_id {
            // Ensure at least one floor exists for the migration
            conn.execute(
                "INSERT INTO floor_plans (name, sort_order) SELECT 'Floor 1', 0 WHERE NOT EXISTS (SELECT 1 FROM floor_plans)",
                [],
            ).map_err(|e| e.to_string())?;

            // Move background from settings into the default floor
            let bg: Option<String> = conn
                .prepare("SELECT value FROM settings WHERE key = 'floor_plan_background'")
                .ok()
                .and_then(|mut s| s.query_row([], |r| r.get(0)).ok());
            if let Some(ref bg_val) = bg {
                let default_id: i64 = conn
                    .query_row("SELECT id FROM floor_plans ORDER BY sort_order, id LIMIT 1", [], |r| r.get(0))
                    .map_err(|e| e.to_string())?;
                conn.execute(
                    "UPDATE floor_plans SET background = ?1 WHERE id = ?2",
                    rusqlite::params![bg_val, default_id],
                ).map_err(|e| e.to_string())?;
                let _ = conn.execute("DELETE FROM settings WHERE key = 'floor_plan_background'", []);
            }

            // Add floor_id column
            conn.execute(
                "ALTER TABLE device_positions ADD COLUMN floor_id INTEGER NOT NULL DEFAULT 1",
                [],
            ).map_err(|e| e.to_string())?;

            // Point existing positions to the default floor
            let default_id: i64 = conn
                .query_row("SELECT id FROM floor_plans ORDER BY sort_order, id LIMIT 1", [], |r| r.get(0))
                .map_err(|e| e.to_string())?;
            conn.execute(
                "UPDATE device_positions SET floor_id = ?1",
                rusqlite::params![default_id],
            ).map_err(|e| e.to_string())?;
        }
    }

    // Floor plan rooms (post-v0.10.1 — named rectangular regions on a floor).
    // Devices placed inside a room inherit a derived `room` property in the UI
    // (computed, not stored on the device). Rectangles only in v1; polygons and
    // rotation deferred to a future Floor Plan v3 if demand shows up.
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS floor_plan_rooms (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            floor_id INTEGER NOT NULL,
            name TEXT NOT NULL,
            color TEXT NOT NULL DEFAULT '#6366f1',
            x REAL NOT NULL DEFAULT 10.0,
            y REAL NOT NULL DEFAULT 10.0,
            w REAL NOT NULL DEFAULT 30.0,
            h REAL NOT NULL DEFAULT 30.0,
            sort_order INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (floor_id) REFERENCES floor_plans(id) ON DELETE CASCADE
        );
    ").map_err(|e| e.to_string())?;

    // Scenes + scene actions (backend-backed scenes, replaces localStorage)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS scenes (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE TABLE IF NOT EXISTS scene_actions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            scene_id INTEGER NOT NULL,
            device_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            value TEXT NOT NULL,
            sort_order INTEGER NOT NULL DEFAULT 0,
            FOREIGN KEY (scene_id) REFERENCES scenes(id) ON DELETE CASCADE,
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );
    ").map_err(|e| e.to_string())?;

    // Add scene_id column to schedules if it doesn't exist (scene scheduling)
    let _ = conn.execute("ALTER TABLE schedules ADD COLUMN scene_id INTEGER REFERENCES scenes(id)", []);

    // Add compound conditions support to rules (post-v0.9.0 — AND/OR logic)
    let _ = conn.execute("ALTER TABLE rules ADD COLUMN logic TEXT NOT NULL DEFAULT 'and'", []);
    let _ = conn.execute("ALTER TABLE rules ADD COLUMN conditions TEXT", []);
    let _ = conn.execute("ALTER TABLE rules ADD COLUMN last_triggered TEXT", []);
    let _ = conn.execute("ALTER TABLE rules ADD COLUMN scene_id INTEGER REFERENCES scenes(id)", []);
    let _ = conn.execute("ALTER TABLE scenes ADD COLUMN last_run TEXT", []);

    // Drop FKs on the target/device columns that block scene-targeted rows.
    // Pre-v0.26.0, scene-targeted schedules (v0.7.0) and scene-targeted rules
    // sent empty target/device strings, which violate the FK to devices(id) and
    // get rejected — the path has never actually worked end-to-end despite
    // shipping. SQLite has no ALTER COLUMN, so this is a CREATE/COPY/RENAME
    // rebuild gated by introspecting the current FK list (idempotent).
    rebuild_drop_target_device_fks(&conn).map_err(|e| e.to_string())?;

    // Webhook delivery history (post-v0.9.0 — retry + delivery log)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS webhook_deliveries (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            webhook_id INTEGER NOT NULL,
            event_type TEXT NOT NULL,
            status_code INTEGER,
            success INTEGER NOT NULL DEFAULT 0,
            error TEXT,
            attempt INTEGER NOT NULL DEFAULT 1,
            timestamp TEXT NOT NULL DEFAULT (datetime('now')),
            FOREIGN KEY (webhook_id) REFERENCES webhooks(id) ON DELETE CASCADE
        );
        CREATE INDEX IF NOT EXISTS idx_webhook_deliveries_webhook
            ON webhook_deliveries(webhook_id, id DESC);
    ").map_err(|e| e.to_string())?;

    // device_templates: promote saved templates into the marketplace grid (v0.30.0 slot 3/3).
    // Adds the same icon/author/board metadata bundled templates carry, so saved
    // templates render as cards alongside the curated set instead of a separate list.
    let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN icon TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN author TEXT NOT NULL DEFAULT ''", []);
    let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN board TEXT NOT NULL DEFAULT 'esp32'", []);

    // Per-capability metadata (energy tracking phase 1 — nameplate watts on switches)
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS capability_meta (
            device_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            nameplate_watts REAL,
            linear_power INTEGER NOT NULL DEFAULT 0,
            slider_max REAL,
            binary_sensor INTEGER NOT NULL DEFAULT 0,
            binary_sensor_device_class TEXT,
            cover_position INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (device_id, capability_id)
        );
        CREATE TABLE IF NOT EXISTS capability_state_log (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            capability_id TEXT NOT NULL,
            state INTEGER NOT NULL,
            timestamp TEXT NOT NULL DEFAULT (datetime('now'))
        );
        CREATE INDEX IF NOT EXISTS idx_cap_state_device_cap
            ON capability_state_log(device_id, capability_id, id DESC);
    ").map_err(|e| e.to_string())?;

    app.manage(Database {
        conn: Mutex::new(conn),
    });

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn log(msg: &str, ts: i64) -> (String, i64) {
        (msg.to_string(), ts)
    }

    #[test]
    fn offline_intervals_empty_when_no_transitions_and_no_bootstrap() {
        let intervals = compute_offline_intervals(None, &[], 0, 3600);
        assert!(intervals.is_empty(), "no evidence → no subtraction");
    }

    #[test]
    fn offline_intervals_empty_when_online_throughout() {
        let logs = vec![log("online", 100), log("online", 200)];
        let intervals = compute_offline_intervals(Some(true), &logs, 0, 3600);
        assert!(intervals.is_empty());
    }

    #[test]
    fn offline_intervals_from_bootstrap_offline() {
        let logs = vec![log("online", 500)];
        let intervals = compute_offline_intervals(Some(false), &logs, 0, 3600);
        assert_eq!(intervals, vec![(0, 500)]);
    }

    #[test]
    fn offline_intervals_stays_offline_until_now() {
        let logs = vec![log("offline", 200)];
        let intervals = compute_offline_intervals(Some(true), &logs, 0, 3600);
        assert_eq!(intervals, vec![(200, 3600)]);
    }

    #[test]
    fn offline_intervals_multiple_outages() {
        let logs = vec![
            log("offline", 100),
            log("online", 200),
            log("offline", 400),
            log("online", 500),
        ];
        let intervals = compute_offline_intervals(Some(true), &logs, 0, 3600);
        assert_eq!(intervals, vec![(100, 200), (400, 500)]);
    }

    #[test]
    fn on_seconds_no_offline_matches_pre_fix_math() {
        // 19s on, no offline intervals → 19s (v0.19.0 live-verified math).
        let rows = vec![(1i64, 100i64), (0i64, 119i64)];
        let s = compute_on_seconds_online(0, &rows, 0, 3600, &[]);
        assert_eq!(s, 19);
    }

    #[test]
    fn on_seconds_open_interval_closes_at_now() {
        // Switch turned on at 100, still on at now (3600). No offline.
        let rows = vec![(1i64, 100i64)];
        let s = compute_on_seconds_online(0, &rows, 0, 3600, &[]);
        assert_eq!(s, 3500);
    }

    #[test]
    fn on_seconds_subtracts_offline_overlap_from_open_interval() {
        // Switch ON at t=100, device goes offline at t=200 and stays offline
        // until now. Pre-fix: accrues 3500 seconds. Post-fix: only 100 seconds
        // (100 → 200 online).
        let rows = vec![(1i64, 100i64)];
        let offline = vec![(200i64, 3600i64)];
        let s = compute_on_seconds_online(0, &rows, 0, 3600, &offline);
        assert_eq!(s, 100);
    }

    #[test]
    fn on_seconds_fully_offline_window_is_zero() {
        // Bootstrap: switch ON before window. Device offline throughout.
        let offline = vec![(0i64, 3600i64)];
        let s = compute_on_seconds_online(1, &[], 0, 3600, &offline);
        assert_eq!(s, 0);
    }

    #[test]
    fn on_seconds_partial_overlap_with_multiple_offline_windows() {
        // Switch ON from t=0 (bootstrap) through t=1000 (turned off).
        // Device offline 200-300 and 700-900 within that interval.
        let rows = vec![(0i64, 1000i64)];
        let offline = vec![(200i64, 300i64), (700i64, 900i64)];
        let s = compute_on_seconds_online(1, &rows, 0, 3600, &offline);
        // 1000 total − 100 (200-300) − 200 (700-900) = 700.
        assert_eq!(s, 700);
    }

    #[test]
    fn numeric_wh_constant_half_value_half_power() {
        // Slider pinned at 128 (bootstrap) for full hour, watts=100, max=255.
        // Fraction ≈ 0.502, Wh ≈ 50.2. No offline overlap.
        let (on, wh) = compute_numeric_wh_online(128, &[], 0, 3600, &[], 100.0, 255.0);
        assert_eq!(on, 3600);
        let expected = 3600.0 * 100.0 * (128.0 / 255.0) / 3600.0;
        assert!(
            (wh - expected).abs() < 1e-9,
            "wh {} vs expected {}",
            wh,
            expected
        );
    }

    #[test]
    fn numeric_wh_stepped_transitions_sum_correctly() {
        // Bootstrap 0 (off). At t=600 → 255 (full power). At t=1800 → 64
        // (quarter power). Window closes at t=3600. No offline.
        // Segment 1: [0, 600)   value 0     → 0 Wh
        // Segment 2: [600, 1800) value 255  → 1200 × 100 × 1.0 / 3600
        // Segment 3: [1800, 3600) value 64  → 1800 × 100 × (64/255) / 3600
        let rows = vec![(255i64, 600i64), (64i64, 1800i64)];
        let (on, wh) = compute_numeric_wh_online(0, &rows, 0, 3600, &[], 100.0, 255.0);
        let expected_wh =
            (1200.0 * 100.0 * 1.0 + 1800.0 * 100.0 * (64.0 / 255.0)) / 3600.0;
        assert_eq!(on, 3000);
        assert!((wh - expected_wh).abs() < 1e-6);
    }

    #[test]
    fn numeric_wh_subtracts_offline_overlap() {
        // Slider at 255 throughout, watts=60, max=255, window 3600s.
        // Device offline 600-1800 (1200s overlap). Online Wh = 2400×60/3600.
        let offline = vec![(600i64, 1800i64)];
        let (on, wh) = compute_numeric_wh_online(255, &[], 0, 3600, &offline, 60.0, 255.0);
        assert_eq!(on, 2400);
        let expected_wh = 2400.0 * 60.0 * 1.0 / 3600.0;
        assert!((wh - expected_wh).abs() < 1e-9);
    }

    #[test]
    fn numeric_wh_zero_bootstrap_and_no_rows_is_zero() {
        // No history, no bootstrap → nothing to integrate.
        let (on, wh) = compute_numeric_wh_online(0, &[], 0, 3600, &[], 50.0, 255.0);
        assert_eq!(on, 0);
        assert_eq!(wh, 0.0);
    }

    #[test]
    fn numeric_wh_uses_default_max_when_zero_passed() {
        // Caller forgot to set slider_max (value 0 or negative). Fallback 255
        // prevents div-by-zero; result is the same as explicit 255.
        let (_, wh_default) =
            compute_numeric_wh_online(128, &[], 0, 3600, &[], 100.0, 0.0);
        let (_, wh_explicit) =
            compute_numeric_wh_online(128, &[], 0, 3600, &[], 100.0, 255.0);
        assert!((wh_default - wh_explicit).abs() < 1e-12);
    }

    fn new_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE capability_meta (
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                nameplate_watts REAL,
                linear_power INTEGER NOT NULL DEFAULT 0,
                slider_max REAL,
                binary_sensor INTEGER NOT NULL DEFAULT 0,
                binary_sensor_device_class TEXT,
                cover_position INTEGER NOT NULL DEFAULT 0,
                brightness_for_cap_id TEXT,
                PRIMARY KEY (device_id, capability_id)
             );
             CREATE TABLE capability_state_log (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                state INTEGER NOT NULL,
                timestamp TEXT NOT NULL DEFAULT (datetime('now'))
             );
             CREATE TABLE device_logs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                severity TEXT NOT NULL,
                message TEXT NOT NULL,
                timestamp TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        Database {
            conn: Mutex::new(conn),
        }
    }

    /// ISO-8601 UTC timestamp at (now - seconds_ago). Matches what SQLite
    /// returns from `datetime('now')` so the stored row compares correctly
    /// against strftime('%s', timestamp).
    fn ts_ago(db: &Database, seconds_ago: i64) -> String {
        let c = db.conn.lock().unwrap();
        c.query_row(
            "SELECT datetime('now', ?1)",
            rusqlite::params![format!("-{} seconds", seconds_ago)],
            |r| r.get::<_, String>(0),
        )
        .unwrap()
    }

    #[test]
    fn lifetime_wh_returns_zero_without_meta() {
        let db = new_test_db();
        // No capability_meta row at all.
        let wh = db.get_capability_lifetime_wh("devX", "capX").unwrap();
        assert_eq!(wh, 0.0);
    }

    #[test]
    fn lifetime_wh_returns_zero_when_no_nameplate_watts() {
        let db = new_test_db();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO capability_meta (device_id, capability_id, nameplate_watts)
                 VALUES (?1, ?2, NULL)",
                rusqlite::params!["dev1", "led"],
            )
            .unwrap();
        let wh = db.get_capability_lifetime_wh("dev1", "led").unwrap();
        assert_eq!(wh, 0.0);
    }

    #[test]
    fn lifetime_wh_returns_zero_when_no_state_log_rows() {
        let db = new_test_db();
        db.conn
            .lock()
            .unwrap()
            .execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power)
                 VALUES ('dev1', 'led', 60.0, 0)",
                [],
            )
            .unwrap();
        let wh = db.get_capability_lifetime_wh("dev1", "led").unwrap();
        assert_eq!(wh, 0.0, "no logged transitions → 0 Wh");
    }

    #[test]
    fn lifetime_wh_switch_path_integrates_on_intervals() {
        // Switch flipped ON 1000s ago (state=1), OFF 400s ago (state=0),
        // watts=60 → 600s of ON-time → 600 * 60 / 3600 = 10.0 Wh.
        let db = new_test_db();
        let ts_1000 = ts_ago(&db, 1000);
        let ts_400 = ts_ago(&db, 400);
        {
            let c = db.conn.lock().unwrap();
            c.execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power)
                 VALUES ('dev1', 'led', 60.0, 0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_state_log
                 (device_id, capability_id, state, timestamp)
                 VALUES ('dev1', 'led', 1, ?1), ('dev1', 'led', 0, ?2)",
                rusqlite::params![ts_1000, ts_400],
            )
            .unwrap();
        }
        let wh = db.get_capability_lifetime_wh("dev1", "led").unwrap();
        // Expected = 600 * 60 / 3600 = 10.0. Allow small jitter from the
        // difference between the INSERT time and the strftime('%s', 'now')
        // inside the method (we sample ts_ago first, then the method samples
        // later).
        assert!(
            (wh - 10.0).abs() < 0.05,
            "switch path wh {} ≈ 10.0 Wh",
            wh
        );
    }

    #[test]
    fn lifetime_wh_slider_linear_path_integrates_value_fraction() {
        // Slider opted into linear_power with slider_max=100, watts=20.
        // Transitions: 100 (full) at 900s ago → 50 (half) at 600s ago →
        // 0 at 300s ago.
        // Segment 1: [window_start=900, 600) value=0 (bootstrap) → 0 Wh.
        //   Wait: bootstrap=0 by construction inside get_capability_lifetime_wh,
        //   so the first segment from window_start..first_ts is zero-duration
        //   (window_start IS the first row's ts). So actual segments:
        // Segment 1: [900, 600) value=100 → 300 * 20 * (100/100) / 3600 = 1.667 Wh
        // Segment 2: [600, 300) value=50  → 300 * 20 * (50/100)  / 3600 = 0.833 Wh
        // Segment 3: [300, now) value=0   → 0 Wh
        // Total ≈ 2.5 Wh.
        let db = new_test_db();
        let ts_900 = ts_ago(&db, 900);
        let ts_600 = ts_ago(&db, 600);
        let ts_300 = ts_ago(&db, 300);
        {
            let c = db.conn.lock().unwrap();
            c.execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power, slider_max)
                 VALUES ('dev1', 'bright', 20.0, 1, 100.0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_state_log
                 (device_id, capability_id, state, timestamp)
                 VALUES ('dev1', 'bright', 100, ?1),
                        ('dev1', 'bright', 50, ?2),
                        ('dev1', 'bright', 0, ?3)",
                rusqlite::params![ts_900, ts_600, ts_300],
            )
            .unwrap();
        }
        let wh = db.get_capability_lifetime_wh("dev1", "bright").unwrap();
        let expected = (300.0 * 20.0 * 1.0 + 300.0 * 20.0 * 0.5) / 3600.0;
        assert!(
            (wh - expected).abs() < 0.05,
            "slider path wh {} ≈ {} Wh",
            wh,
            expected
        );
    }

    #[test]
    fn device_lifetime_energy_empty_when_no_metered_caps() {
        // No capability_meta rows → report has 0 caps, 0 Wh, window_hours=0.
        let db = new_test_db();
        let r = db.get_device_lifetime_energy("devX").unwrap();
        assert_eq!(r.window_hours, 0);
        assert_eq!(r.capabilities.len(), 0);
        assert_eq!(r.total_wh, 0.0);
    }

    #[test]
    fn device_lifetime_energy_aggregates_per_metered_cap() {
        // Switch "led" @ 60W: ON 1000s ago, OFF 400s ago → 600s on-time → 10 Wh.
        // Linear slider "bright" @ 20W max=100: 100 @ 900s ago → 50 @ 600s →
        // 0 @ 300s → ≈2.5 Wh. Total expected ≈ 12.5 Wh.
        let db = new_test_db();
        let ts_1000 = ts_ago(&db, 1000);
        let ts_900 = ts_ago(&db, 900);
        let ts_600a = ts_ago(&db, 600);
        let ts_600b = ts_ago(&db, 600);
        let ts_400 = ts_ago(&db, 400);
        let ts_300 = ts_ago(&db, 300);
        {
            let c = db.conn.lock().unwrap();
            c.execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power)
                 VALUES ('dev1', 'led', 60.0, 0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power, slider_max)
                 VALUES ('dev1', 'bright', 20.0, 1, 100.0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_state_log
                 (device_id, capability_id, state, timestamp)
                 VALUES ('dev1', 'led', 1, ?1), ('dev1', 'led', 0, ?2)",
                rusqlite::params![ts_1000, ts_400],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_state_log
                 (device_id, capability_id, state, timestamp)
                 VALUES ('dev1', 'bright', 100, ?1),
                        ('dev1', 'bright', 50, ?2),
                        ('dev1', 'bright', 0, ?3)",
                rusqlite::params![ts_900, ts_600a, ts_300],
            )
            .unwrap();
            let _ = ts_600b;
        }
        let r = db.get_device_lifetime_energy("dev1").unwrap();
        assert_eq!(r.window_hours, 0);
        assert_eq!(r.capabilities.len(), 2);
        // Per-cap sums the same as get_capability_lifetime_wh (tested above).
        let led = r.capabilities.iter().find(|c| c.capability_id == "led").unwrap();
        let br = r.capabilities.iter().find(|c| c.capability_id == "bright").unwrap();
        assert!((led.wh - 10.0).abs() < 0.05, "led wh {} ≈ 10.0", led.wh);
        let expected_br = (300.0 * 20.0 + 300.0 * 20.0 * 0.5) / 3600.0;
        assert!(
            (br.wh - expected_br).abs() < 0.05,
            "bright wh {} ≈ {}", br.wh, expected_br
        );
        assert!(
            (r.total_wh - (led.wh + br.wh)).abs() < 1e-9,
            "total_wh should equal sum of per-cap wh"
        );
        assert!(led.tracked_since.is_some());
        assert!(br.tracked_since.is_some());
    }

    #[test]
    fn device_lifetime_energy_skips_unmetered_capability() {
        // Capability with NULL nameplate_watts should be skipped entirely.
        let db = new_test_db();
        let ts_600 = ts_ago(&db, 600);
        {
            let c = db.conn.lock().unwrap();
            c.execute(
                "INSERT INTO capability_meta
                 (device_id, capability_id, nameplate_watts, linear_power)
                 VALUES ('dev1', 'no_watts', NULL, 0)",
                [],
            )
            .unwrap();
            c.execute(
                "INSERT INTO capability_state_log
                 (device_id, capability_id, state, timestamp)
                 VALUES ('dev1', 'no_watts', 1, ?1)",
                rusqlite::params![ts_600],
            )
            .unwrap();
        }
        let r = db.get_device_lifetime_energy("dev1").unwrap();
        assert_eq!(r.capabilities.len(), 0, "unmetered cap must be skipped");
        assert_eq!(r.total_wh, 0.0);
    }

    fn new_rules_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_device_id TEXT NOT NULL,
                source_metric_id TEXT NOT NULL,
                condition TEXT NOT NULL,
                threshold REAL NOT NULL,
                target_device_id TEXT NOT NULL,
                target_capability_id TEXT NOT NULL,
                target_value TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                logic TEXT NOT NULL DEFAULT 'and',
                conditions TEXT,
                last_triggered TEXT,
                scene_id INTEGER
            );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn get_rule_returns_none_for_missing_id() {
        let db = new_rules_test_db();
        assert!(db.get_rule(999).unwrap().is_none());
    }

    #[test]
    fn new_rule_has_null_last_triggered() {
        let db = new_rules_test_db();
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "dev2", "fan", "true", "Cool it", "and", None, None)
            .unwrap();
        let rule = db.get_rule(id).unwrap().unwrap();
        assert_eq!(rule.last_triggered, None);
    }

    #[test]
    fn update_rule_last_triggered_stamps_timestamp() {
        let db = new_rules_test_db();
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "dev2", "fan", "true", "Cool it", "and", None, None)
            .unwrap();
        db.update_rule_last_triggered(id).unwrap();
        let rule = db.get_rule(id).unwrap().unwrap();
        let ts = rule.last_triggered.expect("last_triggered should be set");
        // SQLite datetime('now') format: "YYYY-MM-DD HH:MM:SS", 19 chars
        assert_eq!(ts.len(), 19, "unexpected timestamp shape: {}", ts);
        assert!(ts.starts_with('2'), "expected century prefix, got {}", ts);
    }

    #[test]
    fn toggle_rule_flips_enabled_state() {
        let db = new_rules_test_db();
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "dev2", "fan", "true", "Cool it", "and", None, None)
            .unwrap();
        assert!(db.get_rule(id).unwrap().unwrap().enabled, "new rule defaults to enabled");
        db.toggle_rule(id, false).unwrap();
        assert!(!db.get_rule(id).unwrap().unwrap().enabled, "toggle to false disables rule");
        db.toggle_rule(id, true).unwrap();
        assert!(db.get_rule(id).unwrap().unwrap().enabled, "toggle back to true re-enables rule");
    }

    #[test]
    fn delete_rule_removes_row() {
        let db = new_rules_test_db();
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "dev2", "fan", "true", "Cool it", "and", None, None)
            .unwrap();
        assert!(db.get_rule(id).unwrap().is_some());
        db.delete_rule(id).unwrap();
        assert!(db.get_rule(id).unwrap().is_none(), "rule must not exist after delete");
    }

    fn new_schedules_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                value TEXT NOT NULL,
                cron TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                scene_id INTEGER
            );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn toggle_schedule_flips_enabled_state() {
        let db = new_schedules_test_db();
        let id = db
            .create_schedule("dev1", "led", "true", "0 6 * * *", "Morning", None)
            .unwrap();
        assert!(db.get_schedule(id).unwrap().unwrap().enabled, "new schedule defaults to enabled");
        db.toggle_schedule(id, false).unwrap();
        assert!(!db.get_schedule(id).unwrap().unwrap().enabled, "toggle to false disables schedule");
        db.toggle_schedule(id, true).unwrap();
        assert!(db.get_schedule(id).unwrap().unwrap().enabled, "toggle back to true re-enables schedule");
    }

    #[test]
    fn delete_schedule_removes_row() {
        let db = new_schedules_test_db();
        let id = db
            .create_schedule("dev1", "led", "true", "0 6 * * *", "Morning", None)
            .unwrap();
        assert!(db.get_schedule(id).unwrap().is_some());
        db.delete_schedule(id).unwrap();
        assert!(db.get_schedule(id).unwrap().is_none(), "schedule must not exist after delete");
    }

    fn new_webhooks_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE webhooks (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                event_type TEXT NOT NULL,
                device_id TEXT,
                url TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1
            );
            CREATE TABLE webhook_deliveries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                webhook_id INTEGER NOT NULL,
                event_type TEXT NOT NULL,
                status_code INTEGER,
                success INTEGER NOT NULL,
                error TEXT,
                attempt INTEGER NOT NULL DEFAULT 1,
                timestamp TEXT NOT NULL
            );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn toggle_webhook_flips_enabled_state() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        let fetched = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert!(fetched.enabled, "new webhook defaults to enabled");
        db.toggle_webhook(id, false).unwrap();
        let disabled = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert!(!disabled.enabled, "toggle to false disables webhook");
        db.toggle_webhook(id, true).unwrap();
        let re_enabled = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert!(re_enabled.enabled, "toggle back to true re-enables webhook");
    }

    #[test]
    fn delete_webhook_removes_row() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        assert!(db.get_webhooks().unwrap().iter().any(|w| w.id == id));
        db.delete_webhook(id).unwrap();
        assert!(!db.get_webhooks().unwrap().iter().any(|w| w.id == id), "webhook must not exist after delete");
    }

    #[test]
    fn new_webhook_has_null_summary_fields() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        let w = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert!(w.last_delivery.is_none(), "new webhook has no deliveries");
        assert!(w.last_success.is_none(), "new webhook has no last_success");
        assert_eq!(w.success_count, 0);
        assert_eq!(w.failure_count, 0);
    }

    #[test]
    fn get_webhooks_aggregates_delivery_stats() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(200), true, None, 1).unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(500), false, Some("oops"), 1).unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(200), true, None, 1).unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(200), true, None, 1).unwrap();
        db.log_webhook_delivery(id, "device.offline", None, false, Some("timeout"), 2).unwrap();

        let w = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert_eq!(w.success_count, 3, "should have 3 successful deliveries");
        assert_eq!(w.failure_count, 2, "should have 2 failed deliveries");
        assert!(w.last_delivery.is_some(), "last_delivery populated after logging");
        assert_eq!(w.last_success, Some(false), "last delivery was a failure");
    }

    #[test]
    fn get_webhooks_last_success_reflects_most_recent() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(500), false, Some("oops"), 1).unwrap();
        db.log_webhook_delivery(id, "device.offline", Some(200), true, None, 1).unwrap();
        let w = db.get_webhooks().unwrap().into_iter().find(|w| w.id == id).unwrap();
        assert_eq!(w.last_success, Some(true), "last delivery was successful");
    }

    fn insert_webhook_delivery_at(db: &Database, webhook_id: i64, days_ago: i64) {
        let c = db.conn.lock().unwrap();
        c.execute(
            "INSERT INTO webhook_deliveries (webhook_id, event_type, status_code, success, error, attempt, timestamp)
             VALUES (?1, 'device.offline', 200, 1, NULL, 1, datetime('now', ?2))",
            rusqlite::params![webhook_id, format!("-{} days", days_ago)],
        ).unwrap();
    }

    #[test]
    fn cleanup_old_webhook_deliveries_removes_rows_older_than_window() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        insert_webhook_delivery_at(&db, id, 100);
        insert_webhook_delivery_at(&db, id, 60);
        insert_webhook_delivery_at(&db, id, 5);
        insert_webhook_delivery_at(&db, id, 0);

        let deleted = db.cleanup_old_webhook_deliveries(30).unwrap();
        assert_eq!(deleted, 2, "rows at 100d + 60d should be deleted, 5d + 0d kept");

        let remaining = db.get_webhook_deliveries(id, 100).unwrap();
        assert_eq!(remaining.len(), 2, "two recent rows should remain");
    }

    #[test]
    fn cleanup_old_webhook_deliveries_keeps_all_when_window_exceeds_oldest() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        insert_webhook_delivery_at(&db, id, 5);
        insert_webhook_delivery_at(&db, id, 10);

        let deleted = db.cleanup_old_webhook_deliveries(365).unwrap();
        assert_eq!(deleted, 0, "no rows older than 365d");

        let remaining = db.get_webhook_deliveries(id, 100).unwrap();
        assert_eq!(remaining.len(), 2);
    }

    #[test]
    fn cleanup_old_webhook_deliveries_isolates_per_table() {
        // Sanity: cleanup of webhook_deliveries must not touch the parent webhook row.
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Offline ping")
            .unwrap();
        insert_webhook_delivery_at(&db, id, 100);
        db.cleanup_old_webhook_deliveries(30).unwrap();
        let still_there = db.get_webhooks().unwrap().into_iter().any(|w| w.id == id);
        assert!(still_there, "parent webhook row must survive delivery cleanup");
    }

    #[test]
    fn webhooks_for_event_matches_dot_form() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "All devices")
            .unwrap();
        let m = db.get_webhooks_for_event("device.offline", Some("dev1")).unwrap();
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].id, id);
    }

    #[test]
    fn webhooks_for_event_back_compat_underscore_form() {
        // Pre-v0.26.0 UI saved underscore-form rows. The dispatcher receives
        // dot form. Both must match each other so users don't have to recreate
        // webhooks across the upgrade.
        let db = new_webhooks_test_db();
        let id_underscore = db
            .create_webhook("device_offline", None, "https://example.com/hook", "Legacy row")
            .unwrap();
        let m1 = db.get_webhooks_for_event("device.offline", Some("dev1")).unwrap();
        assert!(m1.iter().any(|w| w.id == id_underscore), "dot form lookup must hit underscore row");

        let id_dot = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Modern row")
            .unwrap();
        let m2 = db.get_webhooks_for_event("device_offline", Some("dev1")).unwrap();
        assert!(m2.iter().any(|w| w.id == id_dot), "underscore form lookup must hit dot row");
    }

    #[test]
    fn webhooks_for_event_filters_by_device() {
        let db = new_webhooks_test_db();
        let any = db
            .create_webhook("device.offline", None, "https://example.com/any", "Any device")
            .unwrap();
        let dev1 = db
            .create_webhook("device.offline", Some("dev1"), "https://example.com/dev1", "Only dev1")
            .unwrap();
        let dev2 = db
            .create_webhook("device.offline", Some("dev2"), "https://example.com/dev2", "Only dev2")
            .unwrap();

        let m = db.get_webhooks_for_event("device.offline", Some("dev1")).unwrap();
        let ids: Vec<i64> = m.iter().map(|w| w.id).collect();
        assert!(ids.contains(&any), "all-devices webhook fires for dev1");
        assert!(ids.contains(&dev1), "dev1-scoped webhook fires for dev1");
        assert!(!ids.contains(&dev2), "dev2-scoped webhook must NOT fire for dev1");
    }

    #[test]
    fn webhooks_for_event_skips_disabled() {
        let db = new_webhooks_test_db();
        let id = db
            .create_webhook("device.offline", None, "https://example.com/hook", "Off")
            .unwrap();
        db.toggle_webhook(id, false).unwrap();
        let m = db.get_webhooks_for_event("device.offline", Some("dev1")).unwrap();
        assert!(m.is_empty(), "disabled webhook must not match");
    }

    #[test]
    fn webhooks_for_event_no_event_device_only_matches_null_device_webhooks() {
        // System-level events (caller passes None) must NOT fire webhooks
        // scoped to a specific device — the device id literally does not exist
        // in this event.
        let db = new_webhooks_test_db();
        let any = db
            .create_webhook("ota_applied", None, "https://example.com/any", "Any")
            .unwrap();
        let scoped = db
            .create_webhook("ota_applied", Some("dev1"), "https://example.com/dev1", "Scoped")
            .unwrap();
        let m = db.get_webhooks_for_event("ota_applied", None).unwrap();
        let ids: Vec<i64> = m.iter().map(|w| w.id).collect();
        assert!(ids.contains(&any), "NULL-device webhook fires for system event");
        assert!(!ids.contains(&scoped), "device-scoped webhook must NOT fire for system event");
    }

    #[test]
    fn webhooks_for_event_skips_other_event_types() {
        let db = new_webhooks_test_db();
        db.create_webhook("device.online", None, "https://example.com/online", "Online")
            .unwrap();
        let m = db.get_webhooks_for_event("device.offline", Some("dev1")).unwrap();
        assert!(m.is_empty(), "device.offline lookup must not match device.online webhook");
    }

    fn new_scenes_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE scenes (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now')),
                last_run TEXT
            );
            CREATE TABLE scene_actions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scene_id INTEGER NOT NULL,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                value TEXT NOT NULL,
                sort_order INTEGER NOT NULL DEFAULT 0
            );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    fn seed_scene(db: &Database, name: &str) -> i64 {
        let actions = vec![SceneActionInput {
            device_id: "dev1".into(),
            capability_id: "led".into(),
            value: "true".into(),
        }];
        db.create_scene(name, &actions).unwrap()
    }

    #[test]
    fn new_scene_has_null_last_run() {
        let db = new_scenes_test_db();
        let id = seed_scene(&db, "Good night");
        let scene = db.get_scene(id).unwrap().unwrap();
        assert_eq!(scene.last_run, None);
    }

    #[test]
    fn update_scene_last_run_stamps_timestamp() {
        let db = new_scenes_test_db();
        let id = seed_scene(&db, "Movie mode");
        db.update_scene_last_run(id).unwrap();
        let scene = db.get_scene(id).unwrap().unwrap();
        let ts = scene.last_run.expect("last_run should be set");
        assert_eq!(ts.len(), 19, "unexpected timestamp shape: {}", ts);
        assert!(ts.starts_with('2'), "expected century prefix, got {}", ts);
    }

    #[test]
    fn get_scenes_exposes_last_run_on_every_row() {
        let db = new_scenes_test_db();
        let a = seed_scene(&db, "A");
        let b = seed_scene(&db, "B");
        db.update_scene_last_run(a).unwrap();
        let scenes = db.get_scenes().unwrap();
        let sa = scenes.iter().find(|s| s.id == a).unwrap();
        let sb = scenes.iter().find(|s| s.id == b).unwrap();
        assert!(sa.last_run.is_some(), "A has been fired, last_run should be set");
        assert_eq!(sb.last_run, None, "B has not been fired, last_run should be null");
    }

    // ─── Copy / duplicate parity ─────────────────────────────────────────

    #[test]
    fn copy_label_appends_suffix_and_handles_blank() {
        assert_eq!(copy_label("Morning"), "Morning (copy)");
        assert_eq!(copy_label("Morning   "), "Morning (copy)", "trailing space trimmed");
        assert_eq!(copy_label(""), "(copy)");
        assert_eq!(copy_label("   "), "(copy)", "all-whitespace label");
    }

    #[test]
    fn duplicate_schedule_creates_independent_row() {
        let db = new_schedules_test_db();
        let src_id = db
            .create_schedule("dev1", "led", "true", "0 6 * * *", "Morning", None)
            .unwrap();
        db.toggle_schedule(src_id, false).unwrap();

        let new_id = db.duplicate_schedule(src_id).unwrap();
        assert_ne!(src_id, new_id);

        let dup = db.get_schedule(new_id).unwrap().unwrap();
        assert_eq!(dup.label, "Morning (copy)");
        assert_eq!(dup.cron, "0 6 * * *");
        assert_eq!(dup.device_id, "dev1");
        assert_eq!(dup.capability_id, "led");
        assert_eq!(dup.value, "true");
        assert!(dup.enabled, "duplicate starts enabled even if source is disabled");
        assert_eq!(dup.last_run, None, "duplicate has fresh last_run");

        // Source untouched
        let src = db.get_schedule(src_id).unwrap().unwrap();
        assert_eq!(src.label, "Morning");
        assert!(!src.enabled);
    }

    #[test]
    fn duplicate_schedule_preserves_scene_link() {
        let db = new_schedules_test_db();
        let src_id = db
            .create_schedule("dev1", "led", "true", "0 7 * * *", "Scene fire", Some(42))
            .unwrap();
        let new_id = db.duplicate_schedule(src_id).unwrap();
        let dup = db.get_schedule(new_id).unwrap().unwrap();
        assert_eq!(dup.scene_id, Some(42));
    }

    #[test]
    fn duplicate_schedule_404s_on_missing() {
        let db = new_schedules_test_db();
        let err = db.duplicate_schedule(999).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn duplicate_rule_creates_independent_row_with_conditions() {
        let db = new_rules_test_db();
        let conditions_json = r#"[{"device_id":"dev1","metric_id":"temp","operator":"above","threshold":30.0}]"#;
        let src_id = db
            .create_rule(
                "dev1", "temp", "above", 30.0,
                "dev2", "fan", "true", "Cool it",
                "or", Some(conditions_json), None,
            )
            .unwrap();
        db.toggle_rule(src_id, false).unwrap();

        let new_id = db.duplicate_rule(src_id).unwrap();
        assert_ne!(src_id, new_id);

        let dup = db.get_rule(new_id).unwrap().unwrap();
        assert_eq!(dup.label, "Cool it (copy)");
        assert_eq!(dup.threshold, 30.0);
        assert_eq!(dup.condition, "above");
        assert_eq!(dup.logic, "or");
        assert_eq!(dup.conditions.as_deref(), Some(conditions_json));
        assert!(dup.enabled, "duplicate starts enabled even if source is disabled");
        assert_eq!(dup.last_triggered, None, "duplicate has fresh last_triggered");
    }

    #[test]
    fn duplicate_rule_404s_on_missing() {
        let db = new_rules_test_db();
        let err = db.duplicate_rule(999).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn new_rule_has_null_scene_id() {
        let db = new_rules_test_db();
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "dev2", "fan", "true", "Cool it", "and", None, None)
            .unwrap();
        let rule = db.get_rule(id).unwrap().unwrap();
        assert_eq!(rule.scene_id, None);
    }

    #[test]
    fn create_rule_with_scene_id_persists() {
        let db = new_rules_test_db();
        // Scene-targeted rules carry empty target_* fields — fire_rule branches on scene_id
        // and ignores them, mirroring the schedule scene-fire path.
        let id = db
            .create_rule("dev1", "temp", "above", 30.0, "", "", "", "Cooldown scene", "and", None, Some(7))
            .unwrap();
        let rule = db.get_rule(id).unwrap().unwrap();
        assert_eq!(rule.scene_id, Some(7));
        assert_eq!(rule.target_device_id, "");
        assert_eq!(rule.target_capability_id, "");
    }

    #[test]
    fn migration_drops_target_device_fk_on_rules_and_schedules() {
        // Seed a connection with the pre-v0.26.0 schema (FK on target_device_id /
        // device_id) plus a tiny devices+scenes universe, insert a row, and verify
        // the migration rebuilds the table so empty target_device_id no longer
        // violates FK while existing rows are preserved.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        conn.execute_batch("
            CREATE TABLE devices (id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE scenes (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, last_run TEXT);
            CREATE TABLE rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_device_id TEXT NOT NULL,
                source_metric_id TEXT NOT NULL,
                condition TEXT NOT NULL,
                threshold REAL NOT NULL,
                target_device_id TEXT NOT NULL,
                target_capability_id TEXT NOT NULL,
                target_value TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                logic TEXT NOT NULL DEFAULT 'and',
                conditions TEXT,
                last_triggered TEXT,
                scene_id INTEGER REFERENCES scenes(id),
                FOREIGN KEY (source_device_id) REFERENCES devices(id),
                FOREIGN KEY (target_device_id) REFERENCES devices(id)
            );
            CREATE TABLE schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                value TEXT NOT NULL,
                cron TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                scene_id INTEGER REFERENCES scenes(id),
                FOREIGN KEY (device_id) REFERENCES devices(id)
            );
            INSERT INTO devices (id, name) VALUES ('dev1', 'D1');
            INSERT INTO scenes (id, name) VALUES (7, 'S');
            INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, enabled,
                logic, conditions, last_triggered, scene_id)
                VALUES ('dev1', 'temp', 'above', 30.0, 'dev1', 'fan', 'true', 'pre', 1, 'and', NULL, NULL, NULL);
            INSERT INTO schedules (device_id, capability_id, value, cron, label, enabled, last_run, scene_id)
                VALUES ('dev1', 'led', 'true', '0 6 * * *', 'pre', 1, NULL, NULL);
        ").unwrap();

        // Pre-condition: empty target_device_id rejected by FK.
        let pre_err = conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, enabled, logic, conditions, last_triggered, scene_id)
                VALUES ('dev1', 'temp', 'above', 30.0, '', '', '', 'should fail', 1, 'and', NULL, NULL, 7)",
            [],
        );
        assert!(pre_err.is_err(), "pre-migration: empty target_device_id should hit FK");

        rebuild_drop_target_device_fks(&conn).unwrap();

        // Post-condition: empty target_device_id accepted, scene-targeted rule lands.
        conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, enabled, logic, conditions, last_triggered, scene_id)
                VALUES ('dev1', 'temp', 'above', 30.0, '', '', '', 'scene-rule', 1, 'and', NULL, NULL, 7)",
            [],
        ).expect("post-migration: empty target_device_id should be accepted");
        conn.execute(
            "INSERT INTO schedules (device_id, capability_id, value, cron, label, enabled, last_run, scene_id)
                VALUES ('', '', '', '0 6 * * *', 'scene-sched', 1, NULL, 7)",
            [],
        ).expect("post-migration: empty schedule device_id should be accepted");

        // Existing rows preserved.
        let rules_count: i64 = conn.query_row("SELECT COUNT(*) FROM rules WHERE label='pre'", [], |r| r.get(0)).unwrap();
        let scheds_count: i64 = conn.query_row("SELECT COUNT(*) FROM schedules WHERE label='pre'", [], |r| r.get(0)).unwrap();
        assert_eq!(rules_count, 1, "pre-existing rule must survive migration");
        assert_eq!(scheds_count, 1, "pre-existing schedule must survive migration");

        // Source FK on rules.source_device_id is preserved (still rejects bogus source).
        let bogus = conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, enabled, logic, conditions, last_triggered, scene_id)
                VALUES ('ghost', 'temp', 'above', 30.0, '', '', '', 'bogus', 1, 'and', NULL, NULL, 7)",
            [],
        );
        assert!(bogus.is_err(), "post-migration: source_device_id FK still in force");
    }

    #[test]
    fn migration_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch("PRAGMA foreign_keys = ON;").unwrap();
        // Already-migrated shape: no FK on target_device_id / device_id.
        conn.execute_batch("
            CREATE TABLE devices (id TEXT PRIMARY KEY, name TEXT);
            CREATE TABLE scenes (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT, last_run TEXT);
            CREATE TABLE rules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                source_device_id TEXT NOT NULL,
                source_metric_id TEXT NOT NULL,
                condition TEXT NOT NULL,
                threshold REAL NOT NULL,
                target_device_id TEXT NOT NULL,
                target_capability_id TEXT NOT NULL,
                target_value TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                logic TEXT NOT NULL DEFAULT 'and',
                conditions TEXT,
                last_triggered TEXT,
                scene_id INTEGER REFERENCES scenes(id),
                FOREIGN KEY (source_device_id) REFERENCES devices(id)
            );
            CREATE TABLE schedules (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                device_id TEXT NOT NULL,
                capability_id TEXT NOT NULL,
                value TEXT NOT NULL,
                cron TEXT NOT NULL,
                label TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                last_run TEXT,
                scene_id INTEGER REFERENCES scenes(id)
            );
        ").unwrap();
        // Second run is a no-op.
        rebuild_drop_target_device_fks(&conn).unwrap();
        rebuild_drop_target_device_fks(&conn).unwrap();
    }

    #[test]
    fn duplicate_rule_preserves_scene_link() {
        let db = new_rules_test_db();
        let src_id = db
            .create_rule("dev1", "temp", "above", 30.0, "", "", "", "Cooldown scene", "and", None, Some(42))
            .unwrap();
        let new_id = db.duplicate_rule(src_id).unwrap();
        let dup = db.get_rule(new_id).unwrap().unwrap();
        assert_eq!(dup.scene_id, Some(42));
        assert_eq!(dup.label, "Cooldown scene (copy)");
    }

    #[test]
    fn duplicate_webhook_creates_independent_row() {
        let db = new_webhooks_test_db();
        let src_id = db
            .create_webhook("device.offline", Some("dev1"), "https://example.com/hook", "Offline ping")
            .unwrap();
        db.log_webhook_delivery(src_id, "device.offline", Some(500), false, Some("oops"), 1).unwrap();
        db.toggle_webhook(src_id, false).unwrap();

        let new_id = db.duplicate_webhook(src_id).unwrap();
        assert_ne!(src_id, new_id);

        let dup = db.get_webhook(new_id).unwrap().unwrap();
        assert_eq!(dup.label, "Offline ping (copy)");
        assert_eq!(dup.event_type, "device.offline");
        assert_eq!(dup.device_id.as_deref(), Some("dev1"));
        assert_eq!(dup.url, "https://example.com/hook");
        assert!(dup.enabled, "duplicate starts enabled even if source is disabled");

        // Delivery history is per-webhook and must NOT carry over
        let dup_full = db.get_webhooks().unwrap().into_iter().find(|w| w.id == new_id).unwrap();
        assert_eq!(dup_full.success_count, 0);
        assert_eq!(dup_full.failure_count, 0);
        assert!(dup_full.last_delivery.is_none());
    }

    #[test]
    fn duplicate_webhook_handles_null_device_id() {
        let db = new_webhooks_test_db();
        let src_id = db
            .create_webhook("alert.triggered", None, "https://example.com/all", "All alerts")
            .unwrap();
        let new_id = db.duplicate_webhook(src_id).unwrap();
        let dup = db.get_webhook(new_id).unwrap().unwrap();
        assert!(dup.device_id.is_none(), "device_id NULL must round-trip");
    }

    #[test]
    fn duplicate_webhook_404s_on_missing() {
        let db = new_webhooks_test_db();
        let err = db.duplicate_webhook(999).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn duplicate_scene_copies_actions_and_resets_last_run() {
        let db = new_scenes_test_db();
        let actions = vec![
            SceneActionInput { device_id: "dev1".into(), capability_id: "led".into(), value: "true".into() },
            SceneActionInput { device_id: "dev2".into(), capability_id: "fan".into(), value: "false".into() },
            SceneActionInput { device_id: "dev3".into(), capability_id: "bright".into(), value: "0.4".into() },
        ];
        let src_id = db.create_scene("Movie mode", &actions).unwrap();
        db.update_scene_last_run(src_id).unwrap();

        let new_id = db.duplicate_scene(src_id).unwrap();
        assert_ne!(src_id, new_id);

        let dup = db.get_scene(new_id).unwrap().unwrap();
        assert_eq!(dup.name, "Movie mode (copy)");
        assert_eq!(dup.actions.len(), 3);
        assert_eq!(dup.actions[0].device_id, "dev1");
        assert_eq!(dup.actions[1].capability_id, "fan");
        assert_eq!(dup.actions[2].value, "0.4");
        assert_eq!(dup.last_run, None, "duplicate has fresh last_run");

        // Source untouched
        let src = db.get_scene(src_id).unwrap().unwrap();
        assert_eq!(src.name, "Movie mode");
        assert!(src.last_run.is_some());
        assert_eq!(src.actions.len(), 3, "source still has its 3 actions");
    }

    #[test]
    fn duplicate_scene_preserves_action_order() {
        let db = new_scenes_test_db();
        let actions = vec![
            SceneActionInput { device_id: "z".into(), capability_id: "c1".into(), value: "1".into() },
            SceneActionInput { device_id: "a".into(), capability_id: "c2".into(), value: "2".into() },
            SceneActionInput { device_id: "m".into(), capability_id: "c3".into(), value: "3".into() },
        ];
        let src_id = db.create_scene("Ordering", &actions).unwrap();
        let new_id = db.duplicate_scene(src_id).unwrap();
        let dup = db.get_scene(new_id).unwrap().unwrap();
        let ordered: Vec<&str> = dup.actions.iter().map(|a| a.device_id.as_str()).collect();
        assert_eq!(ordered, vec!["z", "a", "m"], "duplicate preserves sort_order from source");
    }

    #[test]
    fn duplicate_scene_404s_on_missing() {
        let db = new_scenes_test_db();
        let err = db.duplicate_scene(999).unwrap_err();
        assert!(err.contains("not found"), "got: {err}");
    }

    #[test]
    fn binary_sensor_default_off_for_new_meta_row() {
        let db = new_test_db();
        // Setting only nameplate_watts must leave binary_sensor at default 0.
        db.set_capability_watts("dev1", "motion", Some(0.0)).unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "motion").unwrap();
        assert!(!row.binary_sensor);
        assert!(row.binary_sensor_device_class.is_none());
    }

    #[test]
    fn set_capability_binary_sensor_persists_flag_and_class() {
        let db = new_test_db();
        db.set_capability_binary_sensor(
            "dev1",
            "motion",
            true,
            Some("motion".to_string()),
        )
        .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "motion").unwrap();
        assert!(row.binary_sensor);
        assert_eq!(row.binary_sensor_device_class.as_deref(), Some("motion"));
    }

    #[test]
    fn set_capability_binary_sensor_clears_class_when_disabled() {
        let db = new_test_db();
        // Opt in with a device_class first.
        db.set_capability_binary_sensor(
            "dev1",
            "motion",
            true,
            Some("motion".to_string()),
        )
        .unwrap();
        // Then opt out — device_class must be cleared so it doesn't linger
        // and resurrect when the flag is re-enabled later without an arg.
        db.set_capability_binary_sensor("dev1", "motion", false, None)
            .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "motion").unwrap();
        assert!(!row.binary_sensor);
        assert!(row.binary_sensor_device_class.is_none());
    }

    #[test]
    fn binary_sensor_independent_of_nameplate_watts() {
        // Both fields must be independently settable on the same row —
        // setting one must not clobber the other (slot 1 must not regress
        // slot v0.22.0's energy work).
        let db = new_test_db();
        db.set_capability_watts("dev1", "led", Some(60.0)).unwrap();
        db.set_capability_binary_sensor(
            "dev1",
            "led",
            true,
            Some("light".to_string()),
        )
        .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "led").unwrap();
        assert_eq!(row.nameplate_watts, Some(60.0));
        assert!(row.binary_sensor);
        assert_eq!(row.binary_sensor_device_class.as_deref(), Some("light"));
    }

    #[test]
    fn get_all_binary_sensors_returns_only_opted_in() {
        let db = new_test_db();
        db.set_capability_binary_sensor(
            "dev1",
            "motion",
            true,
            Some("motion".to_string()),
        )
        .unwrap();
        db.set_capability_binary_sensor("dev1", "ambient", false, None)
            .unwrap();
        // A row that never had binary_sensor toggled — get_all shouldn't see it.
        db.set_capability_watts("dev2", "led", Some(15.0)).unwrap();
        let rows = db.get_all_binary_sensors().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "dev1");
        assert_eq!(rows[0].1, "motion");
        assert_eq!(rows[0].2.as_deref(), Some("motion"));
    }

    #[test]
    fn cover_position_default_off_for_new_meta_row() {
        // A meta row created by any other setter must leave cover_position
        // at default 0 — opt-in is explicit only.
        let db = new_test_db();
        db.set_capability_watts("dev1", "blind", Some(0.0)).unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "blind").unwrap();
        assert!(!row.cover_position);
    }

    #[test]
    fn set_capability_cover_persists_and_round_trips() {
        let db = new_test_db();
        db.set_capability_cover("dev1", "blind", true).unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "blind").unwrap();
        assert!(row.cover_position);
    }

    #[test]
    fn set_capability_cover_opt_out_clears_flag() {
        // Opt-in then opt-out — flag flips back to false. Mirrors the
        // binary_sensor opt-out test so future setters can't drift apart.
        let db = new_test_db();
        db.set_capability_cover("dev1", "blind", true).unwrap();
        db.set_capability_cover("dev1", "blind", false).unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "blind").unwrap();
        assert!(!row.cover_position);
    }

    #[test]
    fn cover_position_independent_of_energy_fields() {
        // cover_position must be independently settable from
        // nameplate_watts / linear_power / slider_max — toggling one must
        // not clobber the other on the same (device, capability) row.
        let db = new_test_db();
        db.set_capability_watts("dev1", "blind", Some(40.0)).unwrap();
        db.set_capability_linear_power("dev1", "blind", true, Some(100.0))
            .unwrap();
        db.set_capability_cover("dev1", "blind", true).unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows.iter().find(|r| r.capability_id == "blind").unwrap();
        assert_eq!(row.nameplate_watts, Some(40.0));
        assert!(row.linear_power);
        assert_eq!(row.slider_max, Some(100.0));
        assert!(row.cover_position);
    }

    #[test]
    fn get_all_covers_returns_only_opted_in() {
        let db = new_test_db();
        db.set_capability_cover("dev1", "blind", true).unwrap();
        db.set_capability_cover("dev1", "shade", false).unwrap();
        // A row that never had cover_position toggled — get_all shouldn't see it.
        db.set_capability_watts("dev2", "led", Some(15.0)).unwrap();
        let rows = db.get_all_covers().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0, "dev1");
        assert_eq!(rows[0].1, "blind");
    }

    #[test]
    fn brightness_link_default_null_for_new_meta_row() {
        // Any meta row created via another setter must leave
        // brightness_for_cap_id at NULL — the link is opt-in.
        let db = new_test_db();
        db.set_capability_watts("dev1", "brightness", Some(15.0))
            .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows
            .iter()
            .find(|r| r.capability_id == "brightness")
            .unwrap();
        assert!(row.brightness_for_cap_id.is_none());
    }

    #[test]
    fn set_capability_brightness_link_persists_and_round_trips() {
        let db = new_test_db();
        db.set_capability_brightness_link(
            "dev1",
            "brightness",
            Some("rgb".to_string()),
        )
        .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows
            .iter()
            .find(|r| r.capability_id == "brightness")
            .unwrap();
        assert_eq!(row.brightness_for_cap_id.as_deref(), Some("rgb"));
    }

    #[test]
    fn set_capability_brightness_link_unlink_clears_field() {
        // Opt-in then opt-out — column flips back to NULL. Mirrors the
        // cover/binary_sensor opt-out tests so future setters can't drift.
        let db = new_test_db();
        db.set_capability_brightness_link(
            "dev1",
            "brightness",
            Some("rgb".to_string()),
        )
        .unwrap();
        db.set_capability_brightness_link("dev1", "brightness", None)
            .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows
            .iter()
            .find(|r| r.capability_id == "brightness")
            .unwrap();
        assert!(row.brightness_for_cap_id.is_none());
    }

    #[test]
    fn brightness_link_independent_of_other_meta_fields() {
        // brightness_for_cap_id must be independently settable from
        // nameplate_watts / linear_power / slider_max / cover_position so
        // toggling the link doesn't clobber the energy-tracking setup that
        // a slider may already participate in.
        let db = new_test_db();
        db.set_capability_watts("dev1", "brightness", Some(15.0))
            .unwrap();
        db.set_capability_linear_power("dev1", "brightness", true, Some(255.0))
            .unwrap();
        db.set_capability_brightness_link(
            "dev1",
            "brightness",
            Some("rgb".to_string()),
        )
        .unwrap();
        let rows = db.get_device_capability_meta("dev1").unwrap();
        let row = rows
            .iter()
            .find(|r| r.capability_id == "brightness")
            .unwrap();
        assert_eq!(row.nameplate_watts, Some(15.0));
        assert!(row.linear_power);
        assert_eq!(row.slider_max, Some(255.0));
        assert_eq!(row.brightness_for_cap_id.as_deref(), Some("rgb"));
    }

    #[test]
    fn get_all_brightness_links_returns_only_linked_rows() {
        let db = new_test_db();
        db.set_capability_brightness_link(
            "dev1",
            "brightness",
            Some("rgb".to_string()),
        )
        .unwrap();
        // A slider that was linked then unlinked — get_all must skip it.
        db.set_capability_brightness_link(
            "dev1",
            "scratch_slider",
            Some("rgb".to_string()),
        )
        .unwrap();
        db.set_capability_brightness_link("dev1", "scratch_slider", None)
            .unwrap();
        // A row that only has cover_position — must not appear.
        db.set_capability_cover("dev2", "blind", true).unwrap();
        let rows = db.get_all_brightness_links().unwrap();
        assert_eq!(rows.len(), 1);
        // Triple shape is (device_id, color_cap_id, slider_cap_id).
        assert_eq!(rows[0].0, "dev1");
        assert_eq!(rows[0].1, "rgb");
        assert_eq!(rows[0].2, "brightness");
    }

    // ─── device_templates: marketplace-shape metadata (v0.30.0 slot 3/3) ────

    fn new_templates_test_db() -> Database {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE device_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                capabilities TEXT NOT NULL,
                icon TEXT NOT NULL DEFAULT '',
                author TEXT NOT NULL DEFAULT '',
                board TEXT NOT NULL DEFAULT 'esp32',
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );",
        )
        .unwrap();
        Database { conn: Mutex::new(conn) }
    }

    #[test]
    fn create_template_round_trips_metadata() {
        let db = new_templates_test_db();
        let id = db
            .create_template(
                "Greenhouse",
                "Soil + temp + relay",
                "[{\"type\":\"sensor\",\"id\":\"soil\"}]",
                "sprout",
                "@gardener",
                "picow",
            )
            .unwrap();
        let rows = db.get_templates().unwrap();
        assert_eq!(rows.len(), 1);
        let t = &rows[0];
        assert_eq!(t.id, id);
        assert_eq!(t.name, "Greenhouse");
        assert_eq!(t.description, "Soil + temp + relay");
        assert_eq!(t.icon, "sprout");
        assert_eq!(t.author, "@gardener");
        assert_eq!(t.board, "picow");
    }

    #[test]
    fn create_template_empty_metadata_uses_blank_strings() {
        let db = new_templates_test_db();
        db.create_template(
            "Bare",
            "",
            "[]",
            "",
            "",
            "esp32",
        )
        .unwrap();
        let rows = db.get_templates().unwrap();
        assert_eq!(rows.len(), 1);
        let t = &rows[0];
        // Empty strings are valid + meaningful: frontend treats "" as "no icon".
        assert_eq!(t.icon, "");
        assert_eq!(t.author, "");
        assert_eq!(t.board, "esp32");
    }

    #[test]
    fn alter_table_for_legacy_device_templates_is_idempotent() {
        // Simulates an upgrade from a pre-v0.30.0 install where the table only had
        // (id, name, description, capabilities, created_at). The slot 3/3 migration
        // ADDs three columns; reapplying the migration on a freshly-migrated table
        // must not crash the boot path.
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            "CREATE TABLE device_templates (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                name TEXT NOT NULL,
                description TEXT DEFAULT '',
                capabilities TEXT NOT NULL,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
             );
             INSERT INTO device_templates (name, description, capabilities) VALUES ('Legacy', '', '[]');",
        )
        .unwrap();
        // First migration pass — adds the three columns.
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN icon TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN author TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN board TEXT NOT NULL DEFAULT 'esp32'", []);
        // Second pass — idempotent (each ALTER errors silently because the column already exists).
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN icon TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN author TEXT NOT NULL DEFAULT ''", []);
        let _ = conn.execute("ALTER TABLE device_templates ADD COLUMN board TEXT NOT NULL DEFAULT 'esp32'", []);
        // Legacy row gets defaults via the ALTER's DEFAULT clause.
        let (icon, author, board): (String, String, String) = conn
            .query_row(
                "SELECT icon, author, board FROM device_templates WHERE name = 'Legacy'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .unwrap();
        assert_eq!(icon, "");
        assert_eq!(author, "");
        assert_eq!(board, "esp32", "legacy rows must get the ESP32 default board");
    }

    #[test]
    fn delete_template_removes_row() {
        let db = new_templates_test_db();
        let id = db
            .create_template("To delete", "", "[]", "", "", "esp32")
            .unwrap();
        assert_eq!(db.get_templates().unwrap().len(), 1);
        db.delete_template(id).unwrap();
        assert_eq!(db.get_templates().unwrap().len(), 0);
    }
}
