use rusqlite::Connection;
use std::sync::Mutex;
use tauri::{AppHandle, Manager};

pub struct Database {
    pub conn: Mutex<Connection>,
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
            first_seen TEXT NOT NULL,
            last_seen TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS metrics (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            device_id TEXT NOT NULL,
            metric_id TEXT NOT NULL,
            value REAL NOT NULL,
            timestamp TEXT NOT NULL,
            FOREIGN KEY (device_id) REFERENCES devices(id)
        );

        CREATE INDEX IF NOT EXISTS idx_metrics_device_time
            ON metrics(device_id, timestamp);
        ",
    )?;

    app.manage(Database {
        conn: Mutex::new(conn),
    });

    Ok(())
}
