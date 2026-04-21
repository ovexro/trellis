use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use serde::{Deserialize, Serialize};

pub struct Database {
    pub conn: Mutex<Connection>,
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

    pub fn set_tags(&self, device_id: &str, tags: &str) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE devices SET tags = ?1 WHERE id = ?2",
            rusqlite::params![tags, device_id],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    pub fn get_saved_device(&self, device_id: &str) -> Result<Option<SavedDevice>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order, favorite, github_owner, github_repo FROM devices WHERE id = ?1")
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
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order, favorite, github_owner, github_repo FROM devices ORDER BY sort_order ASC, last_seen DESC")
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
            Ok(Schedule {
                id: row.get(0)?, device_id: row.get(1)?, capability_id: row.get(2)?,
                value: row.get(3)?, cron: row.get(4)?, label: row.get(5)?,
                enabled: row.get(6)?, last_run: row.get(7)?, scene_id: row.get(8)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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

    // ─── Conditional rules ───────────────────────────────────────────────

    pub fn create_rule(
        &self, source_device_id: &str, source_metric_id: &str,
        condition: &str, threshold: f64,
        target_device_id: &str, target_capability_id: &str, target_value: &str,
        label: &str, logic: &str, conditions: Option<&str>,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled, logic, conditions)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9, ?10)",
            rusqlite::params![source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label, logic, conditions],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_rules(&self) -> Result<Vec<Rule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled,
             logic, conditions FROM rules"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Rule {
                id: row.get(0)?, source_device_id: row.get(1)?, source_metric_id: row.get(2)?,
                condition: row.get(3)?, threshold: row.get(4)?, target_device_id: row.get(5)?,
                target_capability_id: row.get(6)?, target_value: row.get(7)?,
                label: row.get(8)?, enabled: row.get(9)?,
                logic: row.get::<_, Option<String>>(10)?.unwrap_or_else(|| "and".to_string()),
                conditions: row.get(11)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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
            "SELECT id, event_type, device_id, url, label, enabled FROM webhooks"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Webhook {
                id: row.get(0)?, event_type: row.get(1)?, device_id: row.get(2)?,
                url: row.get(3)?, label: row.get(4)?, enabled: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_templates (name, description, capabilities) VALUES (?1, ?2, ?3)",
            rusqlite::params![name, description, capabilities],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_templates(&self) -> Result<Vec<DeviceTemplate>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, name, description, capabilities, created_at FROM device_templates ORDER BY created_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(DeviceTemplate {
                id: row.get(0)?, name: row.get(1)?, description: row.get(2)?,
                capabilities: row.get(3)?, created_at: row.get(4)?,
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

    /// Record a single mDNS resolution latency sample for a device. Called
    /// from the discovery browse loop after a `ServiceFound → ServiceResolved`
    /// pair completes for the device, using the elapsed time as the sample.
    pub fn record_mdns_latency(&self, device_id: &str, latency_ms: u32) -> Result<(), String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO device_mdns_latency (device_id, latency_ms, recorded_at)
             VALUES (?1, ?2, datetime('now'))",
            rusqlite::params![device_id, latency_ms],
        )
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Return mDNS latency samples for a device within the given rolling
    /// window, newest-first. Cap at 50 to bound memory on a chatty network —
    /// the rule only needs enough points to compute a stable median.
    pub fn get_mdns_samples(&self, device_id: &str, hours: u32) -> Result<Vec<MdnsLatencySample>, String> {
        let conn = self.conn.lock().unwrap();
        let window = format!("-{} hours", hours);
        let mut stmt = conn
            .prepare(
                "SELECT latency_ms, recorded_at FROM device_mdns_latency
                 WHERE device_id = ?1 AND recorded_at >= datetime('now', ?2)
                 ORDER BY recorded_at DESC LIMIT 50",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, window], |row| {
                Ok(MdnsLatencySample {
                    latency_ms: row.get::<_, i64>(0)? as u32,
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
            "SELECT id, name, created_at FROM scenes ORDER BY id"
        ).map_err(|e| e.to_string())?;
        let scene_rows = scene_stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
        }).map_err(|e| e.to_string())?;
        let mut scenes = Vec::new();
        for row in scene_rows {
            let (id, name, created_at) = row.map_err(|e| e.to_string())?;
            scenes.push(Scene { id, name, created_at, actions: Vec::new() });
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
            "SELECT id, name, created_at FROM scenes WHERE id = ?1",
            rusqlite::params![id],
            |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?)),
        );
        let (scene_id, name, created_at) = match scene {
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
        Ok(Some(Scene { id: scene_id, name, created_at, actions }))
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

/// One row of `device_mdns_latency`. Each sample is a single
/// `ServiceFound → ServiceResolved` elapsed time captured in
/// `mdns_browse_loop`. The mdns_latency diagnostics rule reads trailing
/// samples to surface slow-network / mdns-sd-pressure situations.
#[derive(Debug, Clone, Serialize)]
pub struct MdnsLatencySample {
    pub latency_ms: u32,
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
            last_seen TEXT NOT NULL DEFAULT (datetime('now'))
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

        -- Per-sample mDNS resolution latency (ServiceFound → ServiceResolved
        -- elapsed, in ms) captured inside the discovery daemon. Feeds the
        -- mdns_latency diagnostics rule; a slow median or a single large
        -- spike is a usable signal for network path congestion or mdns-sd
        -- CPU pressure on the desktop side.
        CREATE TABLE IF NOT EXISTS device_mdns_latency (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            latency_ms INTEGER NOT NULL,
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

    app.manage(Database {
        conn: Mutex::new(conn),
    });

    Ok(())
}
