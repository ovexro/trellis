use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

use serde::{Deserialize, Serialize};

pub struct Database {
    pub conn: Mutex<Connection>,
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
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order FROM devices WHERE id = ?1")
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
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen, group_id, sort_order FROM devices ORDER BY sort_order ASC, last_seen DESC")
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
                })
            })
            .map_err(|e| e.to_string())?;
        let mut devices = Vec::new();
        for row in rows {
            devices.push(row.map_err(|e| e.to_string())?);
        }
        Ok(devices)
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
    // ─── Schedules ─────────────────────────────────────────────────────

    pub fn create_schedule(
        &self, device_id: &str, capability_id: &str, value: &str,
        cron: &str, label: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO schedules (device_id, capability_id, value, cron, label, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, 1)",
            rusqlite::params![device_id, capability_id, value, cron, label],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_schedules(&self) -> Result<Vec<Schedule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, capability_id, value, cron, label, enabled, last_run FROM schedules"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Schedule {
                id: row.get(0)?, device_id: row.get(1)?, capability_id: row.get(2)?,
                value: row.get(3)?, cron: row.get(4)?, label: row.get(5)?,
                enabled: row.get(6)?, last_run: row.get(7)?,
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
        label: &str,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO rules (source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1)",
            rusqlite::params![source_device_id, source_metric_id, condition, threshold,
                target_device_id, target_capability_id, target_value, label],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_rules(&self) -> Result<Vec<Rule>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, source_device_id, source_metric_id, condition, threshold,
             target_device_id, target_capability_id, target_value, label, enabled FROM rules"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |row| {
            Ok(Rule {
                id: row.get(0)?, source_device_id: row.get(1)?, source_metric_id: row.get(2)?,
                condition: row.get(3)?, threshold: row.get(4)?, target_device_id: row.get(5)?,
                target_capability_id: row.get(6)?, target_value: row.get(7)?,
                label: row.get(8)?, enabled: row.get(9)?,
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

    pub fn store_firmware_record(
        &self, device_id: &str, version: &str, file_path: &str, file_size: i64,
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO firmware_history (device_id, version, file_path, file_size)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![device_id, version, file_path, file_size],
        ).map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn get_firmware_history(&self, device_id: &str) -> Result<Vec<FirmwareRecord>, String> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT id, device_id, version, file_path, file_size, uploaded_at
             FROM firmware_history WHERE device_id = ?1 ORDER BY uploaded_at DESC"
        ).map_err(|e| e.to_string())?;
        let rows = stmt.query_map(rusqlite::params![device_id], |row| {
            Ok(FirmwareRecord {
                id: row.get(0)?, device_id: row.get(1)?, version: row.get(2)?,
                file_path: row.get(3)?, file_size: row.get(4)?, uploaded_at: row.get(5)?,
            })
        }).map_err(|e| e.to_string())?;
        rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
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
}

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
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

        CREATE INDEX IF NOT EXISTS idx_metrics_device_time
            ON metrics(device_id, metric_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_device_time
            ON device_logs(device_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_firmware_device
            ON firmware_history(device_id, uploaded_at);
        CREATE INDEX IF NOT EXISTS idx_api_tokens_hash
            ON api_tokens(token_hash);
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

    app.manage(Database {
        conn: Mutex::new(conn),
    });

    Ok(())
}
