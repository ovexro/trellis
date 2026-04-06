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
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen FROM devices WHERE id = ?1")
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
            .prepare("SELECT id, name, ip, port, firmware, platform, nickname, tags, first_seen, last_seen FROM devices ORDER BY last_seen DESC")
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
                })
            })
            .map_err(|e| e.to_string())?;
        let mut devices = Vec::new();
        for row in rows {
            devices.push(row.map_err(|e| e.to_string())?);
        }
        Ok(devices)
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT severity, message, timestamp FROM device_logs
                 WHERE device_id = ?1
                 ORDER BY timestamp DESC LIMIT ?2",
            )
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map(rusqlite::params![device_id, limit], |row| {
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

#[derive(Debug, Clone, Serialize)]
pub struct LogEntry {
    pub severity: String,
    pub message: String,
    pub timestamp: String,
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

        CREATE INDEX IF NOT EXISTS idx_metrics_device_time
            ON metrics(device_id, metric_id, timestamp);
        CREATE INDEX IF NOT EXISTS idx_logs_device_time
            ON device_logs(device_id, timestamp);
        ",
    )?;

    app.manage(Database {
        conn: Mutex::new(conn),
    });

    Ok(())
}
