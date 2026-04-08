use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::Value;

use crate::connection::ConnectionManager;
use crate::db::Database;
use crate::discovery::Discovery;
use crate::mqtt::{MqttBridge, MqttConfig};
use crate::secret_store::{self, SecretStore};

struct ApiContext {
    db: Database,
    discovery: Arc<Discovery>,
    connection_manager: Arc<ConnectionManager>,
    mqtt_bridge: Arc<MqttBridge>,
    secret_store: Arc<SecretStore>,
}

/// Setting keys whose raw values must NEVER be returned by — or written
/// through — the generic `/api/settings/<key>` GET/PUT endpoints. The REST
/// API binds to 0.0.0.0:9090, so anything served via the generic key getter
/// is visible to anyone on the LAN. These keys must be accessed via their
/// dedicated typed endpoints (e.g. `GET /api/settings/mqtt`) which return a
/// password-redacted view.
const SENSITIVE_SETTING_KEYS: &[&str] = &["mqtt_config"];

fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_SETTING_KEYS.iter().any(|k| *k == key)
}

pub fn start_api_server(
    db_path: PathBuf,
    discovery: Arc<Discovery>,
    connection_manager: Arc<ConnectionManager>,
    mqtt_bridge: Arc<MqttBridge>,
    secret_store: Arc<SecretStore>,
) {
    std::thread::spawn(move || {
        let conn = match Connection::open(&db_path) {
            Ok(c) => c,
            Err(e) => {
                log::error!("[API] Failed to open database: {}", e);
                return;
            }
        };
        let ctx = Arc::new(ApiContext {
            db: Database { conn: Mutex::new(conn) },
            discovery,
            connection_manager,
            mqtt_bridge,
            secret_store,
        });

        let listener = match TcpListener::bind("0.0.0.0:9090") {
            Ok(l) => l,
            Err(e) => {
                log::error!("[API] Failed to bind port 9090: {}", e);
                return;
            }
        };

        log::info!("[API] REST API server listening on http://0.0.0.0:9090");

        for stream in listener.incoming() {
            if let Ok(stream) = stream {
                let ctx = ctx.clone();
                std::thread::spawn(move || {
                    if let Err(e) = handle_connection(stream, &ctx) {
                        log::warn!("[API] Request error: {}", e);
                    }
                });
            }
        }
    });
}

// ─── HTTP parsing ───────────────────────────────────────────────────────────

struct HttpRequest {
    method: String,
    path: String,
    query: HashMap<String, String>,
    body: String,
}

fn parse_request(stream: &TcpStream) -> Result<HttpRequest, String> {
    let mut reader = BufReader::new(stream);

    // Read request line
    let mut request_line = String::new();
    reader.read_line(&mut request_line).map_err(|e| e.to_string())?;
    let parts: Vec<&str> = request_line.trim().split_whitespace().collect();
    if parts.len() < 2 {
        return Err("Invalid request line".to_string());
    }

    let method = parts[0].to_string();
    let full_path = parts[1].to_string();

    // Parse path and query string
    let (path, query) = if let Some(idx) = full_path.find('?') {
        let path = full_path[..idx].to_string();
        let qs = &full_path[idx + 1..];
        let query = parse_query_string(qs);
        (path, query)
    } else {
        (full_path, HashMap::new())
    };

    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut line = String::new();
        reader.read_line(&mut line).map_err(|e| e.to_string())?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            break;
        }
        let lower = trimmed.to_ascii_lowercase();
        if let Some(val) = lower.strip_prefix("content-length:") {
            content_length = val.trim().parse().unwrap_or(0);
        }
    }

    // Read body
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
        body = String::from_utf8_lossy(&buf).to_string();
    }

    Ok(HttpRequest { method, path, query, body })
}

fn parse_query_string(qs: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in qs.split('&') {
        if let Some(idx) = pair.find('=') {
            let key = &pair[..idx];
            let val = &pair[idx + 1..];
            map.insert(key.to_string(), val.to_string());
        }
    }
    map
}

// ─── HTTP response helpers ──────────────────────────────────────────────────

fn send_json(stream: &mut TcpStream, status: u16, body: &str) {
    let status_text = match status {
        200 => "OK",
        201 => "Created",
        204 => "No Content",
        400 => "Bad Request",
        404 => "Not Found",
        500 => "Internal Server Error",
        _ => "OK",
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n{}",
        status, status_text, body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn send_html(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn send_csv(stream: &mut TcpStream, body: &str) {
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: text/csv\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nConnection: close\r\n\r\n{}",
        body.len(), body
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn send_cors_preflight(stream: &mut TcpStream) {
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nAccess-Control-Max-Age: 86400\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn json_ok(data: &impl serde::Serialize) -> (u16, String) {
    (200, serde_json::to_string(data).unwrap_or_else(|_| "null".to_string()))
}

fn json_created(data: &impl serde::Serialize) -> (u16, String) {
    (201, serde_json::to_string(data).unwrap_or_else(|_| "null".to_string()))
}

fn json_error(status: u16, msg: &str) -> (u16, String) {
    (status, serde_json::json!({"error": msg}).to_string())
}

// ─── Route handling ─────────────────────────────────────────────────────────

fn handle_connection(mut stream: TcpStream, ctx: &ApiContext) -> Result<(), String> {
    stream.set_read_timeout(Some(std::time::Duration::from_secs(10))).ok();

    let req = parse_request(&stream)?;

    // CORS preflight
    if req.method == "OPTIONS" {
        send_cors_preflight(&mut stream);
        return Ok(());
    }

    let (status, body) = route(&req, ctx);

    if status == 0 {
        // Special case: HTML response (web UI)
        send_html(&mut stream, &body);
    } else if status == 202 {
        // Special case: CSV response
        send_csv(&mut stream, &body);
    } else {
        send_json(&mut stream, status, &body);
    }

    Ok(())
}

fn route(req: &HttpRequest, ctx: &ApiContext) -> (u16, String) {
    let path = req.path.as_str();
    let method = req.method.as_str();

    match (method, path) {
        // ─── Web UI ──────────────────────────────────────────────────
        ("GET", "/") => (0, get_web_ui()),

        // ─── Devices ─────────────────────────────────────────────────
        ("GET", "/api/devices") => {
            let devices = ctx.discovery.get_devices();
            json_ok(&devices)
        }

        ("GET", p) if p.starts_with("/api/devices/") && !p["/api/devices/".len()..].contains('/') => {
            let id = &p["/api/devices/".len()..];
            let devices = ctx.discovery.get_devices();
            match devices.iter().find(|d| d.id == id) {
                Some(d) => json_ok(d),
                None => json_error(404, "Device not found"),
            }
        }

        ("POST", p) if p.ends_with("/command") && p.starts_with("/api/devices/") => {
            let id = &p["/api/devices/".len()..p.len() - "/command".len()];
            handle_send_command(ctx, id, &req.body)
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/metrics") => {
            let id = &p["/api/devices/".len()..p.len() - "/metrics".len()];
            let metric = req.query.get("metric").cloned().unwrap_or_default();
            let hours: u32 = req.query.get("hours").and_then(|h| h.parse().ok()).unwrap_or(24);
            match ctx.db.get_metrics(id, &metric, hours) {
                Ok(m) => json_ok(&m),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/logs") => {
            let id = &p["/api/devices/".len()..p.len() - "/logs".len()];
            let limit: u32 = req.query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(100);
            match ctx.db.get_logs(id, limit) {
                Ok(l) => json_ok(&l),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/alerts") => {
            let id = &p["/api/devices/".len()..p.len() - "/alerts".len()];
            match ctx.db.get_alerts(id) {
                Ok(a) => json_ok(&a),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", p) if p.starts_with("/api/devices/") && p.ends_with("/alerts") => {
            let id = &p["/api/devices/".len()..p.len() - "/alerts".len()];
            handle_create_alert(ctx, id, &req.body)
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/firmware") => {
            let id = &p["/api/devices/".len()..p.len() - "/firmware".len()];
            match ctx.db.get_firmware_history(id) {
                Ok(h) => json_ok(&h),
                Err(e) => json_error(500, &e),
            }
        }

        ("PUT", p) if p.starts_with("/api/devices/") && p.ends_with("/group") => {
            let id = &p["/api/devices/".len()..p.len() - "/group".len()];
            handle_set_device_group(ctx, id, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/devices/") && p.ends_with("/nickname") => {
            let id = &p["/api/devices/".len()..p.len() - "/nickname".len()];
            handle_set_nickname(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/devices/") && !p["/api/devices/".len()..].contains('/') => {
            let id = &p["/api/devices/".len()..];
            match ctx.db.delete_device(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Groups ─────────────────────────────────────────────────
        ("GET", "/api/groups") => {
            match ctx.db.get_groups() {
                Ok(g) => json_ok(&g),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/groups") => handle_create_group(ctx, &req.body),

        ("PUT", p) if p.starts_with("/api/groups/") => {
            let id: i64 = match p["/api/groups/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid group ID"),
            };
            handle_update_group(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/groups/") => {
            let id: i64 = match p["/api/groups/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid group ID"),
            };
            match ctx.db.delete_group(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Schedules ──────────────────────────────────────────────
        ("GET", "/api/schedules") => {
            match ctx.db.get_schedules() {
                Ok(s) => json_ok(&s),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/schedules") => handle_create_schedule(ctx, &req.body),

        ("DELETE", p) if p.starts_with("/api/schedules/") => {
            let id: i64 = match p["/api/schedules/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid schedule ID"),
            };
            match ctx.db.delete_schedule(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Rules ──────────────────────────────────────────────────
        ("GET", "/api/rules") => {
            match ctx.db.get_rules() {
                Ok(r) => json_ok(&r),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/rules") => handle_create_rule(ctx, &req.body),

        ("DELETE", p) if p.starts_with("/api/rules/") => {
            let id: i64 = match p["/api/rules/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid rule ID"),
            };
            match ctx.db.delete_rule(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Webhooks ───────────────────────────────────────────────
        ("GET", "/api/webhooks") => {
            match ctx.db.get_webhooks() {
                Ok(w) => json_ok(&w),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/webhooks") => handle_create_webhook(ctx, &req.body),

        ("DELETE", p) if p.starts_with("/api/webhooks/") => {
            let id: i64 = match p["/api/webhooks/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid webhook ID"),
            };
            match ctx.db.delete_webhook(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Templates ──────────────────────────────────────────────
        ("GET", "/api/templates") => {
            match ctx.db.get_templates() {
                Ok(t) => json_ok(&t),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Alerts (global) ────────────────────────────────────────
        ("DELETE", p) if p.starts_with("/api/alerts/") => {
            let id: i64 = match p["/api/alerts/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid alert ID"),
            };
            match ctx.db.delete_alert(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── MQTT bridge ────────────────────────────────────────────
        // Defined BEFORE the generic /api/settings/ routes so /api/settings/mqtt
        // hits this typed handler instead of the raw key-value getter.
        ("GET", "/api/settings/mqtt") => {
            // Returns the password-redacted public view. Safe to serve over
            // the LAN-exposed REST API.
            json_ok(&ctx.mqtt_bridge.get_config_public())
        }

        ("PUT", "/api/settings/mqtt") => {
            let cfg: MqttConfig = match serde_json::from_str(&req.body) {
                Ok(c) => c,
                Err(e) => return json_error(400, &format!("Invalid MQTT config JSON: {}", e)),
            };
            // Apply via the user-facing path so an empty `password` in the
            // request preserves the existing stored password rather than
            // wiping it. To explicitly clear, callers must POST to
            // /api/mqtt/clear-password.
            if let Err(e) = ctx.mqtt_bridge.apply_config_from_user(cfg) {
                return json_error(500, &e);
            }
            // Persist the *merged* config (post-preserve), encrypted, so a
            // restart picks up the same auth state and the on-disk blob
            // never holds plaintext.
            let mut merged = ctx.mqtt_bridge.get_config();
            if let Err(e) = secret_store::encrypt_mqtt_password(
                ctx.secret_store.as_ref(),
                &mut merged,
            ) {
                return json_error(500, &e);
            }
            match serde_json::to_string(&merged) {
                Ok(json) => {
                    if let Err(e) = ctx.db.set_setting("mqtt_config", &json) {
                        return json_error(500, &e);
                    }
                }
                Err(e) => return json_error(500, &e.to_string()),
            }
            json_ok(&ctx.mqtt_bridge.get_status())
        }

        ("POST", "/api/mqtt/clear-password") => {
            // Explicit password clear path. Distinct from PUT with empty
            // password (which preserves the existing one).
            if let Err(e) = ctx.mqtt_bridge.clear_password() {
                return json_error(500, &e);
            }
            let mut cleared = ctx.mqtt_bridge.get_config();
            if let Err(e) = secret_store::encrypt_mqtt_password(
                ctx.secret_store.as_ref(),
                &mut cleared,
            ) {
                return json_error(500, &e);
            }
            match serde_json::to_string(&cleared) {
                Ok(json) => {
                    if let Err(e) = ctx.db.set_setting("mqtt_config", &json) {
                        return json_error(500, &e);
                    }
                }
                Err(e) => return json_error(500, &e.to_string()),
            }
            json_ok(&ctx.mqtt_bridge.get_status())
        }

        ("GET", "/api/mqtt/status") => {
            json_ok(&ctx.mqtt_bridge.get_status())
        }

        // ─── Settings ───────────────────────────────────────────────
        ("GET", p) if p.starts_with("/api/settings/") => {
            let key = &p["/api/settings/".len()..];
            // Block any sensitive key from the generic key-value getter.
            // Sensitive keys (e.g. mqtt_config) must be accessed via their
            // dedicated typed endpoints which apply password redaction.
            if is_sensitive_key(key) {
                return json_error(
                    403,
                    "This setting key is restricted. Use its dedicated endpoint (e.g. /api/settings/mqtt for MQTT config).",
                );
            }
            match ctx.db.get_setting(key) {
                Ok(Some(v)) => json_ok(&serde_json::json!({"key": key, "value": v})),
                Ok(None) => json_error(404, "Setting not found"),
                Err(e) => json_error(500, &e),
            }
        }

        ("PUT", p) if p.starts_with("/api/settings/") => {
            let key = &p["/api/settings/".len()..];
            // Block writing to sensitive keys via the generic setter so the
            // typed endpoint's validation (and the merge_preserving_password
            // logic for MQTT) can't be bypassed.
            if is_sensitive_key(key) {
                return json_error(
                    403,
                    "This setting key is restricted. Use its dedicated endpoint to update it.",
                );
            }
            handle_set_setting(ctx, key, &req.body)
        }

        // ─── Metrics export ─────────────────────────────────────────
        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/metrics/csv") => {
            let id = &p["/api/devices/".len()..p.len() - "/metrics/csv".len()];
            let metric = req.query.get("metric").cloned().unwrap_or_default();
            let hours: u32 = req.query.get("hours").and_then(|h| h.parse().ok()).unwrap_or(24);
            match ctx.db.export_metrics_csv(id, &metric, hours) {
                Ok(csv) => {
                    // Return CSV with proper content type (handled specially below)
                    (202, csv) // 202 signals CSV response to handle_connection
                }
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Saved devices ──────────────────────────────────────────
        ("GET", "/api/saved-devices") => {
            match ctx.db.get_all_saved_devices() {
                Ok(d) => json_ok(&d),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Fallback ───────────────────────────────────────────────
        _ => json_error(404, &format!("Not found: {} {}", req.method, req.path)),
    }
}

// ─── Handler functions ──────────────────────────────────────────────────────

fn handle_send_command(ctx: &ApiContext, device_id: &str, body: &str) -> (u16, String) {
    let command: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };

    let devices = ctx.discovery.get_devices();
    let device = match devices.iter().find(|d| d.id == device_id) {
        Some(d) => d.clone(),
        None => return json_error(404, "Device not found or offline"),
    };

    let ws_port = device.port + 1;
    let msg = match serde_json::to_string(&command) {
        Ok(m) => m,
        Err(e) => return json_error(500, &format!("Serialize failed: {}", e)),
    };

    match ctx.connection_manager.send_to_device(&device.id, &device.ip, ws_port, &msg) {
        Ok(()) => json_ok(&serde_json::json!({"sent": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_alert(ctx: &ApiContext, device_id: &str, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let metric_id = v["metric_id"].as_str().unwrap_or("");
    let condition = v["condition"].as_str().unwrap_or("above");
    let threshold = v["threshold"].as_f64().unwrap_or(0.0);
    let label = v["label"].as_str().unwrap_or("Alert");

    match ctx.db.create_alert(device_id, metric_id, condition, threshold, label) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_set_device_group(ctx: &ApiContext, device_id: &str, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let group_id = v["group_id"].as_i64();

    match ctx.db.set_device_group(device_id, group_id) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_set_nickname(ctx: &ApiContext, device_id: &str, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let nickname = v["nickname"].as_str().unwrap_or("");

    match ctx.db.set_nickname(device_id, nickname) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_group(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = v["name"].as_str().unwrap_or("New Group");
    let color = v["color"].as_str().unwrap_or("#6366f1");

    match ctx.db.create_group(name, color) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_update_group(ctx: &ApiContext, id: i64, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = v["name"].as_str().unwrap_or("Group");
    let color = v["color"].as_str().unwrap_or("#6366f1");

    match ctx.db.update_group(id, name, color) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_schedule(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let device_id = v["device_id"].as_str().unwrap_or("");
    let capability_id = v["capability_id"].as_str().unwrap_or("");
    let value = v["value"].as_str().unwrap_or("");
    let cron = v["cron"].as_str().unwrap_or("");
    let label = v["label"].as_str().unwrap_or("Schedule");

    match ctx.db.create_schedule(device_id, capability_id, value, cron, label) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_rule(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let source_device_id = v["source_device_id"].as_str().unwrap_or("");
    let source_metric_id = v["source_metric_id"].as_str().unwrap_or("");
    let condition = v["condition"].as_str().unwrap_or("above");
    let threshold = v["threshold"].as_f64().unwrap_or(0.0);
    let target_device_id = v["target_device_id"].as_str().unwrap_or("");
    let target_capability_id = v["target_capability_id"].as_str().unwrap_or("");
    let target_value = v["target_value"].as_str().unwrap_or("");
    let label = v["label"].as_str().unwrap_or("Rule");

    match ctx.db.create_rule(
        source_device_id, source_metric_id, condition, threshold,
        target_device_id, target_capability_id, target_value, label,
    ) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_webhook(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let event_type = v["event_type"].as_str().unwrap_or("");
    let device_id = v["device_id"].as_str();
    let url = v["url"].as_str().unwrap_or("");
    let label = v["label"].as_str().unwrap_or("Webhook");

    match ctx.db.create_webhook(event_type, device_id, url, label) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_set_setting(ctx: &ApiContext, key: &str, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let value = v["value"].as_str().unwrap_or("");

    match ctx.db.set_setting(key, value) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

// ─── Web UI (placeholder — will be replaced in Batch 4) ─────────────────────

fn get_web_ui() -> String {
    include_str!("web_ui.html").to_string()
}
