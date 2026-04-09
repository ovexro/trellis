use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::Value;

use crate::auth::{self, AuthResult, REQUIRE_AUTH_LOCALHOST_KEY};
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
    /// Raw value of the `Authorization:` header, if present. The auth
    /// middleware extracts the Bearer token from this — see auth.rs.
    authorization: Option<String>,
    /// WebSocket upgrade fields — captured for the device proxy.
    is_websocket_upgrade: bool,
    sec_websocket_key: Option<String>,
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

    // Read headers. We capture Content-Length, Authorization, and
    // WebSocket upgrade fields. Other headers are dropped.
    let mut content_length: usize = 0;
    let mut authorization: Option<String> = None;
    let mut is_websocket_upgrade = false;
    let mut sec_websocket_key: Option<String> = None;
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
        } else if lower.starts_with("authorization:") {
            // Preserve the original-case header value (the token body is
            // case-sensitive base64url) — only the header *name* match is
            // case-insensitive per RFC 7230.
            if let Some(idx) = trimmed.find(':') {
                let val = trimmed[idx + 1..].trim().to_string();
                if !val.is_empty() {
                    authorization = Some(val);
                }
            }
        } else if lower.starts_with("upgrade:") && lower.contains("websocket") {
            is_websocket_upgrade = true;
        } else if lower.starts_with("sec-websocket-key:") {
            if let Some(idx) = trimmed.find(':') {
                let val = trimmed[idx + 1..].trim().to_string();
                if !val.is_empty() {
                    sec_websocket_key = Some(val);
                }
            }
        }
    }

    // Read body
    let mut body = String::new();
    if content_length > 0 {
        let mut buf = vec![0u8; content_length];
        reader.read_exact(&mut buf).map_err(|e| e.to_string())?;
        body = String::from_utf8_lossy(&buf).to_string();
    }

    Ok(HttpRequest {
        method,
        path,
        query,
        body,
        authorization,
        is_websocket_upgrade,
        sec_websocket_key,
    })
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
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n{}",
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
    let response = "HTTP/1.1 204 No Content\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, PUT, DELETE, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nAccess-Control-Max-Age: 86400\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

fn send_proxy_response(stream: &mut TcpStream, status: u16, content_type: &str, body: &[u8]) {
    let status_text = match status {
        200 => "OK",
        304 => "Not Modified",
        404 => "Not Found",
        _ => "OK",
    };
    let header = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Headers: Content-Type, Authorization\r\nConnection: close\r\n\r\n",
        status, status_text, content_type, body.len()
    );
    let _ = stream.write_all(header.as_bytes());
    let _ = stream.write_all(body);
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

    // Capture peer addr BEFORE consuming the stream for parsing — this is
    // the loopback-vs-remote signal the auth gate needs.
    let peer_addr: SocketAddr = stream
        .peer_addr()
        .map_err(|e| format!("peer_addr unavailable: {}", e))?;

    let req = parse_request(&stream)?;

    // CORS preflight: always allow. Browsers send these without credentials
    // before any cross-origin call; the actual request that follows still
    // gets gated by the auth check below.
    if req.method == "OPTIONS" {
        send_cors_preflight(&mut stream);
        return Ok(());
    }

    // Embedded web UI: always allow `GET /`. The HTML itself is harmless
    // static content (no secrets, no device data) and contains its own
    // token-login flow that activates the moment its first `/api/*` fetch
    // returns 401. This makes the dashboard reachable through a remote-
    // access tunnel (Cloudflare Tunnel, Tailscale Funnel) where the request
    // arrives from a non-loopback peer — without this special case the
    // page would never load and the user would see a bare JSON 401.
    //
    // The dynamic surface stays gated: every `/api/*` call below still
    // runs through `auth::check_auth`, so the page can't actually display
    // any device data without a valid token. v0.4.0 ships this together
    // with a token-aware `api()` helper inside web_ui.html that pops a
    // login modal on the first 401 and persists the pasted token in
    // localStorage.
    if req.method == "GET" && req.path == "/" {
        send_html(&mut stream, &get_web_ui());
        return Ok(());
    }

    // Auth gate. Runs on every non-OPTIONS request. Reads the
    // `require_auth_localhost` setting once per request — cheap (single
    // SQLite SELECT against the keyed `settings` row) and avoids the
    // alternative of caching it in process memory and having to invalidate
    // when the user changes it via the Settings UI.
    let require_strict = ctx
        .db
        .get_setting(REQUIRE_AUTH_LOCALHOST_KEY)
        .ok()
        .flatten()
        .map(|v| v == "true" || v == "1")
        .unwrap_or(false);

    match auth::check_auth(
        &ctx.db,
        &peer_addr,
        req.authorization.as_deref(),
        require_strict,
    ) {
        AuthResult::Allow(token_id) => {
            // Bump last_used_at on the matched token. Best-effort — a
            // failure here just leaves the timestamp stale, the request
            // still proceeds.
            if let Some(id) = token_id {
                if let Err(e) = ctx.db.touch_api_token(id) {
                    log::warn!("[Auth] Failed to touch token {}: {}", id, e);
                }
            }
        }
        AuthResult::Deny(status, msg) => {
            // Log every auth failure at WARN. Useful for spotting LAN
            // probing attempts and for debugging legitimate clients that
            // forgot to include the header. Includes peer addr + status +
            // method + path so a single grep tells the whole story.
            log::warn!(
                "[Auth] Denied {} {} from {} -> {} ({})",
                req.method,
                req.path,
                peer_addr,
                status,
                msg
            );
            // The pre-auth special case for `GET /` upstream means a browser
            // hitting the dashboard URL will never reach this branch — it
            // gets the (token-aware) embedded web UI, which handles the
            // 401-on-first-fetch flow itself. Everything else (bare API
            // calls without a token) gets a standard JSON 401.
            let body = serde_json::json!({"error": msg}).to_string();
            send_json(&mut stream, status, &body);
            return Ok(());
        }
    }

    // Device proxy: `/proxy/{device-id}/{path...}` forwards to the
    // device's embedded HTTP server on :8080 (and WebSocket on :8081).
    // This lets remote users (through a tunnel) reach individual device
    // dashboards without direct LAN access.
    if req.path.starts_with("/proxy/") {
        return handle_proxy(ctx, &req, &mut stream);
    }

    let (status, body) = route(&req, ctx);

    // Status 202 is used as an in-band signal that the body is CSV (the
    // metrics export route). Everything else is JSON. The web UI HTML is
    // served upstream of `route()` by the pre-auth `GET /` branch, so this
    // dispatch only ever sees JSON or CSV.
    if status == 202 {
        send_csv(&mut stream, &body);
    } else {
        send_json(&mut stream, status, &body);
    }

    Ok(())
}

// ─── Device proxy ──────────────────────────────────────────────────────────

fn handle_proxy(ctx: &ApiContext, req: &HttpRequest, stream: &mut TcpStream) -> Result<(), String> {
    let after_proxy = &req.path["/proxy/".len()..];

    // Parse: /proxy/{device-id}/{rest...}
    let (raw_id, device_path) = match after_proxy.find('/') {
        Some(idx) => (&after_proxy[..idx], &after_proxy[idx..]),
        None => {
            // Redirect /proxy/{id} → /proxy/{id}/ so relative URLs in the
            // proxied HTML resolve correctly.
            let location = format!("{}/", req.path);
            let resp = format!(
                "HTTP/1.1 301 Moved Permanently\r\nLocation: {}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                location
            );
            let _ = stream.write_all(resp.as_bytes());
            return Ok(());
        }
    };

    // URL-decode the device ID (ids may contain characters like `:`)
    let device_id = urlencoding::decode(raw_id)
        .unwrap_or_else(|_| raw_id.into());

    // Look up device
    let devices = ctx.discovery.get_devices();
    let device = match devices.iter().find(|d| d.id == *device_id) {
        Some(d) => d.clone(),
        None => {
            let body = serde_json::json!({"error": "Device not found"}).to_string();
            send_json(stream, 404, &body);
            return Ok(());
        }
    };

    if !device.online {
        let body = serde_json::json!({"error": "Device offline"}).to_string();
        send_json(stream, 503, &body);
        return Ok(());
    }

    // WebSocket upgrade → bridge to device WS port
    if req.is_websocket_upgrade && device_path == "/ws" {
        return handle_proxy_ws(stream, &device, req);
    }

    // HTTP proxy → forward to device HTTP port
    handle_proxy_http(stream, &device, req, device_path)
}

fn handle_proxy_http(
    stream: &mut TcpStream,
    device: &crate::device::Device,
    req: &HttpRequest,
    device_path: &str,
) -> Result<(), String> {
    let url = format!("http://{}:{}{}", device.ip, device.port, device_path);

    let upstream = match req.method.as_str() {
        "GET" => ureq::get(&url).call(),
        "POST" => ureq::post(&url)
            .set("Content-Type", "application/json")
            .send_string(&req.body),
        "PUT" => ureq::put(&url)
            .set("Content-Type", "application/json")
            .send_string(&req.body),
        "DELETE" => ureq::delete(&url).call(),
        _ => {
            let body = serde_json::json!({"error": "Method not allowed"}).to_string();
            send_json(stream, 405, &body);
            return Ok(());
        }
    };

    match upstream {
        Ok(resp) => {
            let status = resp.status();
            let content_type = resp.content_type().to_string();
            let body = resp.into_string().unwrap_or_default();

            // Rewrite the root HTML so fetch + WebSocket URLs route
            // back through the proxy instead of hitting the device
            // directly (which is unreachable through a remote tunnel).
            if device_path == "/" && content_type.contains("text/html") {
                let rewritten = rewrite_device_html(&body, &device.id);
                send_proxy_response(stream, status, "text/html; charset=utf-8", rewritten.as_bytes());
            } else {
                send_proxy_response(stream, status, &content_type, body.as_bytes());
            }
        }
        Err(ureq::Error::Status(code, resp)) => {
            let body = resp.into_string().unwrap_or_default();
            send_proxy_response(stream, code, "application/json", body.as_bytes());
        }
        Err(e) => {
            let body = serde_json::json!({"error": format!("Device unreachable: {}", e)}).to_string();
            send_json(stream, 502, &body);
        }
    }
    Ok(())
}

/// Rewrite the device's embedded web dashboard HTML so that:
/// 1. `fetch("/api/info")` becomes `fetch("api/info")` — a relative URL that
///    resolves to `/proxy/{id}/api/info` when the page is served at `/proxy/{id}/`.
/// 2. The WebSocket constructor uses the proxy path instead of `host:port+1`.
fn rewrite_device_html(html: &str, device_id: &str) -> String {
    let encoded_id = urlencoding::encode(device_id);

    // fetch("/api/info") → fetch("api/info")  (relative — resolves via base URL)
    let html = html.replace(
        r#"fetch("/api/info")"#,
        r#"fetch("api/info")"#,
    );

    // WebSocket: replace direct device connection with proxy path.
    // Original: ws=new WebSocket("ws://"+host+":"+wsPort+"/")
    // Rewritten: protocol-aware, routes through /proxy/{id}/ws
    html.replace(
        r#"ws=new WebSocket("ws://"+host+":"+wsPort+"/")"#,
        &format!(
            r#"ws=new WebSocket((location.protocol==="https:"?"wss:":"ws:")+"//"+location.host+"/proxy/{}/ws")"#,
            encoded_id
        ),
    )
}

fn handle_proxy_ws(
    client_stream: &mut TcpStream,
    device: &crate::device::Device,
    req: &HttpRequest,
) -> Result<(), String> {
    let ws_port = device.port + 1;
    let ws_addr = format!("{}:{}", device.ip, ws_port);

    let addr: std::net::SocketAddr = ws_addr
        .parse()
        .map_err(|e| format!("Bad device WS addr: {}", e))?;

    // Connect TCP to device WebSocket port
    let device_stream = TcpStream::connect_timeout(
        &addr,
        std::time::Duration::from_secs(5),
    )
    .map_err(|e| format!("Device WS connect failed: {}", e))?;

    // We need a write half for the device *before* wrapping in BufReader.
    let mut device_wr = device_stream
        .try_clone()
        .map_err(|e| format!("clone: {}", e))?;

    // Forward a WebSocket upgrade request to the device.
    // Use the client's Sec-WebSocket-Key so the Accept hash matches
    // what the client expects.
    let ws_key = req
        .sec_websocket_key
        .as_deref()
        .unwrap_or("dGhlIHNhbXBsZSBub25jZQ==");

    let upgrade_req = format!(
        "GET / HTTP/1.1\r\n\
         Host: {}\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Key: {}\r\n\
         Sec-WebSocket-Version: 13\r\n\
         \r\n",
        ws_addr, ws_key,
    );
    device_wr
        .write_all(upgrade_req.as_bytes())
        .map_err(|e| format!("WS upgrade write failed: {}", e))?;

    // Wrap the device read side in a BufReader for header parsing.
    // IMPORTANT: keep this BufReader alive through the bridge phase —
    // dropping it would lose any bytes it buffered beyond the headers.
    let mut device_reader = BufReader::new(device_stream);

    // Read the device's 101 response and forward it verbatim to the client.
    let mut response_header = String::new();
    loop {
        let mut line = String::new();
        device_reader
            .read_line(&mut line)
            .map_err(|e| format!("WS upgrade read: {}", e))?;
        if line.trim().is_empty() {
            response_header.push_str("\r\n");
            break;
        }
        response_header.push_str(&line);
    }

    client_stream
        .write_all(response_header.as_bytes())
        .map_err(|e| format!("WS upgrade reply: {}", e))?;
    client_stream.flush().map_err(|e| e.to_string())?;

    // Bridge raw bytes between client and device. Remove the read
    // timeout set in handle_connection — WS connections are long-lived.
    client_stream.set_read_timeout(None).ok();
    device_wr.set_read_timeout(None).ok();

    // client → device: read from client TCP, write to device TCP
    let mut client_rd = client_stream
        .try_clone()
        .map_err(|e| format!("clone: {}", e))?;
    let mut client_to_device = device_wr;

    // device → client: read from device BufReader (preserves buffered
    // bytes), write to client TCP
    let mut device_to_client = client_stream
        .try_clone()
        .map_err(|e| format!("clone: {}", e))?;

    let t1 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut client_rd, &mut client_to_device);
        let _ = client_to_device.shutdown(std::net::Shutdown::Both);
    });
    let t2 = std::thread::spawn(move || {
        let _ = std::io::copy(&mut device_reader, &mut device_to_client);
        let _ = device_to_client.shutdown(std::net::Shutdown::Both);
    });

    t1.join().ok();
    t2.join().ok();
    Ok(())
}

fn route(req: &HttpRequest, ctx: &ApiContext) -> (u16, String) {
    let path = req.path.as_str();
    let method = req.method.as_str();

    match (method, path) {
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

        // ─── API tokens ──────────────────────────────────────────────
        // The auth gate has already cleared the request before reaching
        // this point, so anyone who can hit these endpoints is already
        // authorized to mint/revoke tokens.
        ("GET", "/api/tokens") => match ctx.db.list_api_tokens() {
            Ok(tokens) => json_ok(&tokens),
            Err(e) => json_error(500, &e),
        },

        ("POST", "/api/tokens") => handle_create_token(ctx, &req.body),

        ("DELETE", p) if p.starts_with("/api/tokens/") => {
            let id: i64 = match p["/api/tokens/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid token ID"),
            };
            match ctx.db.delete_api_token(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
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

fn handle_create_token(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = v["name"].as_str().unwrap_or("").trim();
    if name.is_empty() {
        return json_error(400, "Token name is required");
    }
    let (plaintext, hash) = auth::generate_token();
    match ctx.db.create_api_token(name, &hash) {
        Ok(id) => {
            // The plaintext is returned ONCE here and never persisted.
            // Once this response is on the wire, the only proof of the
            // token is the SHA-256 digest in `api_tokens.token_hash`.
            let resp = serde_json::json!({
                "id": id,
                "name": name,
                "token": plaintext,
                "warning": "Store this token now — it will not be shown again."
            });
            (201, resp.to_string())
        }
        Err(e) => json_error(500, &e),
    }
}

// ─── Web UI (placeholder — will be replaced in Batch 4) ─────────────────────

fn get_web_ui() -> String {
    include_str!("web_ui.html").to_string()
}
