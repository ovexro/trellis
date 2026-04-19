use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};

use rusqlite::Connection;
use serde_json::Value;

use tauri::Manager as _;

use crate::auth::{self, AuthResult, RateLimiter, Role, REQUIRE_AUTH_LOCALHOST_KEY};
use crate::connection::ConnectionManager;
use crate::db::Database;
use crate::discovery::Discovery;
use crate::mqtt::{MqttBridge, MqttConfig};
use crate::secret_store::{self, SecretStore};
use crate::sinric::{SinricBridge, SinricConfig};

/// Fan-out broadcaster for :9090 WebSocket dashboard clients.
/// Each connected browser gets a `mpsc::Sender`; dead senders are
/// auto-pruned on the next `broadcast()` call.
pub struct WsBroadcaster {
    clients: Mutex<Vec<mpsc::Sender<String>>>,
}

impl WsBroadcaster {
    pub fn new() -> Self {
        Self {
            clients: Mutex::new(Vec::new()),
        }
    }

    /// Register a new WS client. Returns the receiver the client thread
    /// reads from.
    pub fn subscribe(&self) -> mpsc::Receiver<String> {
        let (tx, rx) = mpsc::channel();
        self.clients.lock().unwrap().push(tx);
        rx
    }

    /// Send a message to all connected clients. Prunes disconnected senders.
    pub fn broadcast(&self, msg: String) {
        let mut clients = self.clients.lock().unwrap();
        clients.retain(|tx| tx.send(msg.clone()).is_ok());
    }

    /// Current number of connected WS clients.
    pub fn client_count(&self) -> usize {
        self.clients.lock().unwrap().len()
    }
}

struct ApiContext {
    db: Database,
    discovery: Arc<Discovery>,
    connection_manager: Arc<ConnectionManager>,
    mqtt_bridge: Arc<MqttBridge>,
    sinric_bridge: Arc<SinricBridge>,
    secret_store: Arc<SecretStore>,
    rate_limiter: RateLimiter,
    ws_broadcaster: Arc<WsBroadcaster>,
    app_handle: tauri::AppHandle,
}

/// Setting keys whose raw values must NEVER be returned by — or written
/// through — the generic `/api/settings/<key>` GET/PUT endpoints. The REST
/// API binds to 0.0.0.0:9090, so anything served via the generic key getter
/// is visible to anyone on the LAN. These keys must be accessed via their
/// dedicated typed endpoints (e.g. `GET /api/settings/mqtt`) which return a
/// password-redacted view.
const SENSITIVE_SETTING_KEYS: &[&str] = &["mqtt_config", "sinric_config"];

/// Maximum allowed Content-Length for incoming requests (1 MB). The REST API
/// only processes JSON payloads — the largest legitimate body is an MQTT config
/// save or a bulk import, well under this limit. OTA firmware uploads go
/// directly to the device's :8080 endpoint, not through :9090. Without this
/// cap a malicious caller could force an unbounded heap allocation before the
/// auth gate even runs.
const MAX_BODY_SIZE: usize = 1_048_576;

fn is_sensitive_key(key: &str) -> bool {
    SENSITIVE_SETTING_KEYS.iter().any(|k| *k == key)
}

pub fn start_api_server(
    db_path: PathBuf,
    discovery: Arc<Discovery>,
    connection_manager: Arc<ConnectionManager>,
    mqtt_bridge: Arc<MqttBridge>,
    sinric_bridge: Arc<SinricBridge>,
    secret_store: Arc<SecretStore>,
    ws_broadcaster: Arc<WsBroadcaster>,
    app_handle: tauri::AppHandle,
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
            sinric_bridge,
            secret_store,
            rate_limiter: RateLimiter::new(),
            app_handle,
            ws_broadcaster,
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

    // Read body — reject before allocating if Content-Length exceeds the cap.
    let mut body = String::new();
    if content_length > MAX_BODY_SIZE {
        return Err(format!(
            "Content-Length {} exceeds maximum allowed size of {} bytes",
            content_length, MAX_BODY_SIZE
        ));
    }
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
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        413 => "Content Too Large",
        429 => "Too Many Requests",
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

    let req = match parse_request(&stream) {
        Ok(r) => r,
        Err(e) if e.contains("exceeds maximum allowed size") => {
            log::warn!("[API] Rejected oversized body from {}: {}", peer_addr, e);
            let body = serde_json::json!({"error": e}).to_string();
            send_json(&mut stream, 413, &body);
            return Ok(());
        }
        Err(e) => return Err(e),
    };

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

    // Service worker: must be served from root scope for SW to control `/`.
    // Pre-auth like GET / — it's inert static JS.
    if req.method == "GET" && req.path == "/sw.js" {
        let body = include_str!("sw.js");
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/javascript; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-cache\r\n\
             Service-Worker-Allowed: /\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        return Ok(());
    }

    // Web app manifest for PWA install prompt
    if req.method == "GET" && req.path == "/manifest.json" {
        let body = include_str!("manifest.json");
        let response = format!(
            "HTTP/1.1 200 OK\r\n\
             Content-Type: application/manifest+json; charset=utf-8\r\n\
             Content-Length: {}\r\n\
             Cache-Control: no-cache\r\n\
             Connection: close\r\n\
             \r\n\
             {}",
            body.len(),
            body
        );
        let _ = stream.write_all(response.as_bytes());
        let _ = stream.flush();
        return Ok(());
    }

    // SPA fallback: the embedded web UI (`web_ui.html`) is a single-page
    // app whose own navigation is JS-driven from `/`. But browsers, PWA
    // launchers, and third parties can legitimately land on other paths —
    // refreshes, bookmarks, stale notification URLs, or a future version of
    // the UI that adopts URL-based routing. Without a fallback those all
    // return a bare JSON 404. Serving the index HTML instead lets the SPA
    // boot; it can then route internally or surface the user on the Home
    // tab, which is strictly better UX than a raw error body.
    //
    // Same pre-auth rationale as `GET /`: the HTML itself is inert and
    // gates every data fetch behind `/api/*` auth.
    //
    // Heuristic: GET whose path is not an API/proxy/WS route and whose
    // final segment has no file extension. Covers current and future SPA
    // routes without an allowlist, while letting unknown static assets
    // (e.g. `/favicon.ico`) fall through to a clean 404.
    if req.method == "GET"
        && !req.path.starts_with("/api/")
        && !req.path.starts_with("/proxy/")
        && req.path != "/ws"
    {
        let last_seg = req.path.rsplit('/').next().unwrap_or("");
        if !last_seg.contains('.') {
            send_html(&mut stream, &get_web_ui());
            return Ok(());
        }
    }

    // Rate limiter: reject early if this IP has too many recent failures.
    if let Some((status, msg)) = ctx.rate_limiter.check(&peer_addr) {
        log::warn!(
            "[Auth] Rate-limited {} {} from {} -> {}",
            req.method,
            req.path,
            peer_addr,
            status
        );
        let body = serde_json::json!({"error": msg}).to_string();
        send_json(&mut stream, status, &body);
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

    // For WebSocket upgrades, the browser API cannot set custom headers.
    // Accept the token as a query parameter (/ws?token=trls_...) instead.
    // URL-decode the token since the browser sends encodeURIComponent().
    let effective_auth: Option<String> = if req.path == "/ws" && req.is_websocket_upgrade {
        req.query
            .get("token")
            .and_then(|t| urlencoding::decode(t).ok())
            .map(|t| format!("Bearer {}", t))
            .or_else(|| req.authorization.clone())
    } else {
        req.authorization.clone()
    };

    let (role, auth_token_id) = match auth::check_auth(
        &ctx.db,
        &peer_addr,
        effective_auth.as_deref(),
        require_strict,
    ) {
        AuthResult::Allow { token_id, role } => {
            // Bump last_used_at on the matched token. Best-effort — a
            // failure here just leaves the timestamp stale, the request
            // still proceeds.
            if let Some(id) = token_id {
                if let Err(e) = ctx.db.touch_api_token(id) {
                    log::warn!("[Auth] Failed to touch token {}: {}", id, e);
                }
            }
            // Successful auth — clear any failure state for this IP.
            ctx.rate_limiter.clear(&peer_addr);
            (role, token_id)
        }
        AuthResult::Deny(status, msg) => {
            // Record the failure for rate limiting.
            ctx.rate_limiter.record_failure(&peer_addr);
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
    };

    // Dashboard WebSocket push: /ws
    // Viewers can connect (read-only push). Auth handled above via
    // query-param or header token; loopback bypass applies.
    if req.path == "/ws" && req.is_websocket_upgrade {
        stream.set_read_timeout(None).ok();
        let ws_key = req.sec_websocket_key.as_deref().unwrap_or("dGhlIHNhbXBsZSBub25jZQ==");
        return handle_dashboard_ws(stream, ws_key, &ctx.ws_broadcaster);
    }

    // Device proxy: `/proxy/{device-id}/{path...}` forwards to the
    // device's embedded HTTP server on :8080 (and WebSocket on :8081).
    // This lets remote users (through a tunnel) reach individual device
    // dashboards without direct LAN access. Viewers are blocked — the
    // proxied dashboard includes command controls that would fail anyway.
    if req.path.starts_with("/proxy/") {
        if role == Role::Viewer {
            let body = serde_json::json!({"error": "This action requires an admin token. Your token has viewer-only access."}).to_string();
            send_json(&mut stream, 403, &body);
            return Ok(());
        }
        return handle_proxy(ctx, &req, &mut stream);
    }

    let (status, body) = route(&req, ctx, role, auth_token_id);

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

// ─── Dashboard WebSocket push ──────────────────────────────────────────────

fn handle_dashboard_ws(
    mut stream: TcpStream,
    ws_key: &str,
    broadcaster: &WsBroadcaster,
) -> Result<(), String> {
    // The HTTP upgrade request was already consumed by parse_request.
    // Manually send the 101 Switching Protocols response, then wrap
    // the stream with tungstenite for framed WS I/O.
    let accept_key = tungstenite::handshake::derive_accept_key(ws_key.as_bytes());
    let response = format!(
        "HTTP/1.1 101 Switching Protocols\r\n\
         Upgrade: websocket\r\n\
         Connection: Upgrade\r\n\
         Sec-WebSocket-Accept: {}\r\n\
         \r\n",
        accept_key
    );
    stream
        .write_all(response.as_bytes())
        .map_err(|e| format!("WS upgrade write: {}", e))?;
    stream.flush().map_err(|e| e.to_string())?;

    // 50ms read timeout so we can interleave broadcast polling
    let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(50)));

    // Wrap in tungstenite for framed WS read/write
    let mut ws = tungstenite::WebSocket::from_raw_socket(
        stream,
        tungstenite::protocol::Role::Server,
        None,
    );

    let rx = broadcaster.subscribe();

    log::info!(
        "[WS] Dashboard client connected ({} total)",
        broadcaster.client_count()
    );

    loop {
        // Drain broadcast messages → send to this client
        loop {
            match rx.try_recv() {
                Ok(msg) => {
                    if ws.send(tungstenite::Message::Text(msg)).is_err() {
                        log::debug!("[WS] Dashboard client send failed, disconnecting");
                        let _ = ws.close(None);
                        return Ok(());
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    let _ = ws.close(None);
                    return Ok(());
                }
            }
        }

        // Read from WS: handle pings, detect disconnect
        match ws.read() {
            Ok(tungstenite::Message::Close(_)) => break,
            Ok(tungstenite::Message::Ping(data)) => {
                let _ = ws.send(tungstenite::Message::Pong(data));
            }
            Ok(_) => {} // Ignore text/binary from clients (read-only push)
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        }
    }

    let _ = ws.close(None);
    log::info!(
        "[WS] Dashboard client disconnected ({} remaining)",
        broadcaster.client_count()
    );
    Ok(())
}

/// Check whether the caller has admin privileges. Returns `Some((403, ...))` if
/// the token is viewer-only, which the caller can return early from the match arm.
fn require_admin(role: Role) -> Option<(u16, String)> {
    if role == Role::Viewer {
        Some(json_error(403, "This action requires an admin token. Your token has viewer-only access."))
    } else {
        None
    }
}

fn route(req: &HttpRequest, ctx: &ApiContext, role: Role, token_id: Option<i64>) -> (u16, String) {
    let path = req.path.as_str();
    let method = req.method.as_str();

    match (method, path) {
        // ─── Auth info ──────────────────────────────────────────────
        ("GET", "/api/auth/whoami") => {
            let role_str = match role {
                Role::Admin => "admin",
                Role::Viewer => "viewer",
            };
            json_ok(&serde_json::json!({
                "role": role_str,
                "token_id": token_id,
            }))
        }

        // ─── Activity feed ───────────────────────────────────────────
        ("GET", "/api/activity") => {
            let limit: u32 = req.query.get("limit").and_then(|l| l.parse().ok()).unwrap_or(30);
            match ctx.db.get_recent_activity(limit) {
                Ok(a) => json_ok(&a),
                Err(e) => json_error(500, &e),
            }
        }

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
            if let Some(denied) = require_admin(role) { return denied; }
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
            // Optional comma-separated severity filter, e.g.
            // `?severity=state,error,warn` for the annotation click-through
            // path which only cares about rows that can produce annotations.
            let severities: Option<Vec<String>> = req.query.get("severity").map(|s| {
                s.split(',')
                    .map(|x| x.trim().to_string())
                    .filter(|x| !x.is_empty())
                    .collect()
            });
            let result = match severities {
                Some(ref list) if !list.is_empty() => {
                    ctx.db.get_logs_filtered(id, limit, Some(list))
                }
                _ => ctx.db.get_logs(id, limit),
            };
            match result {
                Ok(l) => json_ok(&l),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/annotations") => {
            let id = &p["/api/devices/".len()..p.len() - "/annotations".len()];
            // Window matches the metric-chart `hours` query param (1/6/24/168).
            let hours: u32 = req.query.get("hours").and_then(|h| h.parse().ok()).unwrap_or(24);
            match ctx.db.get_annotations(id, hours) {
                Ok(a) => json_ok(&a),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", p) if p.starts_with("/api/devices/") && p.ends_with("/diagnose") => {
            let id = &p["/api/devices/".len()..p.len() - "/diagnose".len()];
            let live_devices = ctx.discovery.get_devices();
            let live = live_devices.iter().find(|d| d.id == id);
            match crate::diagnostics::diagnose(&ctx.db, id, live) {
                Ok(report) => json_ok(&report),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", "/api/diagnostics/fleet") => {
            let live_devices = ctx.discovery.get_devices();
            match crate::diagnostics::diagnose_fleet(&ctx.db, &live_devices) {
                Ok(report) => json_ok(&report),
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
            if let Some(denied) = require_admin(role) { return denied; }
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
            if let Some(denied) = require_admin(role) { return denied; }
            let id = &p["/api/devices/".len()..p.len() - "/group".len()];
            handle_set_device_group(ctx, id, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/devices/") && p.ends_with("/nickname") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id = &p["/api/devices/".len()..p.len() - "/nickname".len()];
            handle_set_nickname(ctx, id, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/devices/") && p.ends_with("/favorite") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id = &p["/api/devices/".len()..p.len() - "/favorite".len()];
            handle_set_device_favorite(ctx, id, &req.body)
        }

        ("GET", "/api/favorites") => {
            match ctx.db.get_favorite_capabilities() {
                Ok(favs) => {
                    let list: Vec<serde_json::Value> = favs.iter().map(|(d, c)| {
                        serde_json::json!({"device_id": d, "capability_id": c})
                    }).collect();
                    json_ok(&list)
                }
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/favorites/toggle") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_toggle_favorite_capability(ctx, &req.body)
        }

        ("PUT", "/api/devices/reorder") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_reorder_devices(ctx, &req.body)
        }

        // ─── Floor plans (multi-floor) ─────────────────────────────
        ("GET", "/api/floor-plans") => {
            match ctx.db.get_floor_plans() {
                Ok(floors) => json_ok(&floors),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/floor-plans") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_floor_plan(ctx, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/floor-plans/") && !p["/api/floor-plans/".len()..].contains('/') => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id_str = &p["/api/floor-plans/".len()..];
            let id: i64 = match id_str.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid floor plan id"),
            };
            handle_update_floor_plan(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/floor-plans/") && !p["/api/floor-plans/".len()..].contains('/') => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id_str = &p["/api/floor-plans/".len()..];
            let id: i64 = match id_str.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid floor plan id"),
            };
            match ctx.db.delete_floor_plan(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Floor plan positions ────────────────────────────────────
        ("GET", "/api/floor-plan") => {
            handle_get_floor_plan(ctx, &req.query)
        }

        ("GET", "/api/floor-plan/positions") => {
            match ctx.db.get_all_device_positions() {
                Ok(positions) => json_ok(&positions),
                Err(e) => json_error(500, &e),
            }
        }

        ("PUT", "/api/floor-plan/position") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_set_device_position(ctx, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/floor-plan/position/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let raw_id = &p["/api/floor-plan/position/".len()..];
            let id = urlencoding::decode(raw_id).unwrap_or_else(|_| raw_id.into());
            match ctx.db.remove_device_position(&id) {
                Ok(()) => json_ok(&serde_json::json!({"removed": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Floor plan rooms ───────────────────────────────────────
        ("GET", "/api/rooms") => {
            match ctx.db.get_all_rooms() {
                Ok(rooms) => json_ok(&rooms),
                Err(e) => json_error(500, &e),
            }
        }

        ("GET", p) if p.starts_with("/api/floor-plans/") && p.ends_with("/rooms") => {
            let floor_str = &p["/api/floor-plans/".len()..p.len() - "/rooms".len()];
            let floor_id: i64 = match floor_str.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid floor plan id"),
            };
            match ctx.db.get_rooms(floor_id) {
                Ok(rooms) => json_ok(&rooms),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", p) if p.starts_with("/api/floor-plans/") && p.ends_with("/rooms") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let floor_str = &p["/api/floor-plans/".len()..p.len() - "/rooms".len()];
            let floor_id: i64 = match floor_str.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid floor plan id"),
            };
            handle_create_room(ctx, floor_id, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/rooms/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/rooms/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid room id"),
            };
            handle_update_room(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/rooms/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/rooms/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "invalid room id"),
            };
            match ctx.db.delete_room(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        ("DELETE", p) if p.starts_with("/api/devices/") && !p["/api/devices/".len()..].contains('/') => {
            if let Some(denied) = require_admin(role) { return denied; }
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

        ("POST", "/api/groups") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_group(ctx, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/groups/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/groups/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid group ID"),
            };
            handle_update_group(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/groups/") => {
            if let Some(denied) = require_admin(role) { return denied; }
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

        ("POST", "/api/schedules") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_schedule(ctx, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/schedules/") => {
            if let Some(denied) = require_admin(role) { return denied; }
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

        ("POST", "/api/rules") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_rule(ctx, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/rules/") => {
            if let Some(denied) = require_admin(role) { return denied; }
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

        ("POST", "/api/webhooks") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_webhook(ctx, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/webhooks/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/webhooks/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid webhook ID"),
            };
            match ctx.db.delete_webhook(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Webhook deliveries ────────────────────────────────────
        ("GET", p) if p.starts_with("/api/webhooks/") && p.ends_with("/deliveries") => {
            let middle = &p["/api/webhooks/".len()..p.len()-"/deliveries".len()];
            let wh_id: i64 = match middle.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid webhook ID"),
            };
            match ctx.db.get_webhook_deliveries(wh_id, 50) {
                Ok(d) => json_ok(&d),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── Scenes ─────────────────────────────────────────────────
        ("GET", "/api/scenes") => {
            match ctx.db.get_scenes() {
                Ok(s) => json_ok(&s),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", "/api/scenes") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_scene(ctx, &req.body)
        }

        ("PUT", p) if p.starts_with("/api/scenes/") && !p["/api/scenes/".len()..].contains('/') => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/scenes/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid scene ID"),
            };
            handle_update_scene(ctx, id, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/scenes/") && !p["/api/scenes/".len()..].contains('/') => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/scenes/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid scene ID"),
            };
            match ctx.db.delete_scene(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        ("POST", p) if p.starts_with("/api/scenes/") && p.ends_with("/run") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id_str = &p["/api/scenes/".len()..p.len() - "/run".len()];
            let id: i64 = match id_str.parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid scene ID"),
            };
            handle_run_scene(ctx, id)
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
            if let Some(denied) = require_admin(role) { return denied; }
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
            if let Some(denied) = require_admin(role) { return denied; }
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
            if let Some(denied) = require_admin(role) { return denied; }
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

        // ─── Sinric Pro ─────────────────────────────────────────────
        ("GET", "/api/settings/sinric") => {
            json_ok(&ctx.sinric_bridge.get_config_public())
        }

        ("PUT", "/api/settings/sinric") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let cfg: SinricConfig = match serde_json::from_str(&req.body) {
                Ok(c) => c,
                Err(e) => return json_error(400, &format!("Invalid Sinric config JSON: {}", e)),
            };
            if let Err(e) = ctx.sinric_bridge.apply_config_from_user(cfg) {
                return json_error(500, &e);
            }
            let mut merged = ctx.sinric_bridge.get_config();
            if let Err(e) = secret_store::encrypt_sinric_secret(
                ctx.secret_store.as_ref(),
                &mut merged,
            ) {
                return json_error(500, &e);
            }
            match serde_json::to_string(&merged) {
                Ok(json) => {
                    if let Err(e) = ctx.db.set_setting("sinric_config", &json) {
                        return json_error(500, &e);
                    }
                }
                Err(e) => return json_error(500, &e.to_string()),
            }
            json_ok(&ctx.sinric_bridge.get_status())
        }

        ("POST", "/api/sinric/clear-secret") => {
            if let Some(denied) = require_admin(role) { return denied; }
            if let Err(e) = ctx.sinric_bridge.clear_secret() {
                return json_error(500, &e);
            }
            let mut cleared = ctx.sinric_bridge.get_config();
            if let Err(e) = secret_store::encrypt_sinric_secret(
                ctx.secret_store.as_ref(),
                &mut cleared,
            ) {
                return json_error(500, &e);
            }
            match serde_json::to_string(&cleared) {
                Ok(json) => {
                    if let Err(e) = ctx.db.set_setting("sinric_config", &json) {
                        return json_error(500, &e);
                    }
                }
                Err(e) => return json_error(500, &e.to_string()),
            }
            json_ok(&ctx.sinric_bridge.get_status())
        }

        ("GET", "/api/sinric/status") => {
            json_ok(&ctx.sinric_bridge.get_status())
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
                    "This setting key is restricted. Use its dedicated endpoint (e.g. /api/settings/mqtt, /api/settings/sinric).",
                );
            }
            match ctx.db.get_setting(key) {
                Ok(Some(v)) => json_ok(&serde_json::json!({"key": key, "value": v})),
                Ok(None) => json_error(404, "Setting not found"),
                Err(e) => json_error(500, &e),
            }
        }

        ("PUT", p) if p.starts_with("/api/settings/") => {
            if let Some(denied) = require_admin(role) { return denied; }
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
        // Viewers can list tokens (see names + timestamps) but cannot
        // create or revoke — prevents privilege escalation where a viewer
        // mints an admin token.
        ("GET", "/api/tokens") => match ctx.db.list_api_tokens() {
            Ok(tokens) => json_ok(&tokens),
            Err(e) => json_error(500, &e),
        },

        ("POST", "/api/tokens") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_create_token(ctx, &req.body)
        }

        ("DELETE", p) if p.starts_with("/api/tokens/") => {
            if let Some(denied) = require_admin(role) { return denied; }
            let id: i64 = match p["/api/tokens/".len()..].parse() {
                Ok(id) => id,
                Err(_) => return json_error(400, "Invalid token ID"),
            };
            match ctx.db.delete_api_token(id) {
                Ok(()) => json_ok(&serde_json::json!({"deleted": true})),
                Err(e) => json_error(500, &e),
            }
        }

        // ─── GitHub OTA ──────────────────────────────────────────────
        ("GET", "/api/github/releases") => {
            let owner = match req.query.get("owner") {
                Some(o) => o.clone(),
                None => return json_error(400, "Missing ?owner= parameter"),
            };
            let repo = match req.query.get("repo") {
                Some(r) => r.clone(),
                None => return json_error(400, "Missing ?repo= parameter"),
            };
            match fetch_github_releases(&owner, &repo) {
                Ok(releases) => json_ok(&releases),
                Err(e) => json_error(502, &e),
            }
        }

        ("POST", "/api/github/ota") => {
            if let Some(denied) = require_admin(role) { return denied; }
            handle_github_ota(ctx, &req.body)
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

fn handle_reorder_devices(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let arr = match v.as_array() {
        Some(a) => a,
        None => return json_error(400, "Expected a JSON array of {id, sort_order} objects"),
    };
    let mut order = Vec::new();
    for item in arr {
        let id = match item["id"].as_str() {
            Some(s) => s.to_string(),
            None => return json_error(400, "Each item must have a string 'id' field"),
        };
        let sort_order = match item["sort_order"].as_i64() {
            Some(n) => n,
            None => return json_error(400, "Each item must have a numeric 'sort_order' field"),
        };
        order.push((id, sort_order));
    }
    match ctx.db.reorder_devices(&order) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
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

fn handle_toggle_favorite_capability(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let device_id = match v["device_id"].as_str() {
        Some(id) => id,
        None => return json_error(400, "missing device_id"),
    };
    let capability_id = match v["capability_id"].as_str() {
        Some(id) => id,
        None => return json_error(400, "missing capability_id"),
    };
    match ctx.db.toggle_favorite_capability(device_id, capability_id) {
        Ok(is_fav) => json_ok(&serde_json::json!({"favorite": is_fav})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_set_device_favorite(ctx: &ApiContext, device_id: &str, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let favorite = v["favorite"].as_bool().unwrap_or(false);

    match ctx.db.set_device_favorite(device_id, favorite) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_get_floor_plan(ctx: &ApiContext, query: &HashMap<String, String>) -> (u16, String) {
    // If floor_id is specified, use it; otherwise fall back to the first floor
    let floor_id: i64 = if let Some(fid) = query.get("floor_id").and_then(|f| f.parse().ok()) {
        fid
    } else {
        match ctx.db.get_floor_plans() {
            Ok(floors) if !floors.is_empty() => floors[0].id,
            Ok(_) => return json_ok(&serde_json::json!({"positions": [], "background": null, "floor_id": null})),
            Err(e) => return json_error(500, &e),
        }
    };
    match ctx.db.get_device_positions(floor_id) {
        Ok(positions) => {
            let bg = ctx.db.get_floor_plans().ok()
                .and_then(|floors| floors.into_iter().find(|f| f.id == floor_id))
                .and_then(|f| f.background);
            json_ok(&serde_json::json!({"positions": positions, "background": bg, "floor_id": floor_id}))
        }
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_floor_plan(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = match v["name"].as_str() {
        Some(n) if !n.trim().is_empty() => n.trim(),
        _ => return json_error(400, "missing or empty name"),
    };
    match ctx.db.create_floor_plan(name) {
        Ok(id) => json_ok(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_update_floor_plan(ctx: &ApiContext, id: i64, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = v["name"].as_str().map(|n| n.trim()).filter(|n| !n.is_empty());
    // background: if key is present and non-empty string → set; if key is present and null/empty → clear; if absent → no change
    let background = if v.get("background").is_some() {
        match v["background"].as_str() {
            Some(bg) if !bg.is_empty() => Some(Some(bg)),
            _ => Some(None),
        }
    } else {
        None
    };
    // Convert Option<Option<&str>> to the right shape for the DB method
    let bg_param: Option<Option<&str>> = background;
    match ctx.db.update_floor_plan(id, name, bg_param) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_create_room(ctx: &ApiContext, floor_id: i64, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = match v["name"].as_str() {
        Some(n) if !n.trim().is_empty() => n.trim().to_string(),
        _ => return json_error(400, "missing or empty name"),
    };
    let color = v["color"].as_str().unwrap_or("#6366f1").to_string();
    let x = v["x"].as_f64().unwrap_or(10.0).clamp(0.0, 100.0);
    let y = v["y"].as_f64().unwrap_or(10.0).clamp(0.0, 100.0);
    let w = v["w"].as_f64().unwrap_or(30.0).clamp(1.0, 100.0);
    let h = v["h"].as_f64().unwrap_or(30.0).clamp(1.0, 100.0);
    // Clamp so a room can't extend past the canvas.
    let w = w.min(100.0 - x);
    let h = h.min(100.0 - y);
    match ctx.db.create_room(floor_id, &name, &color, x, y, w, h) {
        Ok(id) => json_ok(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_update_room(ctx: &ApiContext, id: i64, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = v["name"].as_str().map(|s| s.trim()).filter(|s| !s.is_empty());
    let color = v["color"].as_str().filter(|s| !s.is_empty());
    let x = v["x"].as_f64().map(|n| n.clamp(0.0, 100.0));
    let y = v["y"].as_f64().map(|n| n.clamp(0.0, 100.0));
    let w = v["w"].as_f64().map(|n| n.clamp(1.0, 100.0));
    let h = v["h"].as_f64().map(|n| n.clamp(1.0, 100.0));
    match ctx.db.update_room(id, name, color, x, y, w, h) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_set_device_position(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let device_id = match v["device_id"].as_str() {
        Some(id) => id,
        None => return json_error(400, "missing device_id"),
    };
    let floor_id = match v["floor_id"].as_i64() {
        Some(id) => id,
        None => return json_error(400, "missing floor_id"),
    };
    let x = match v["x"].as_f64() {
        Some(x) => x.clamp(0.0, 100.0),
        None => return json_error(400, "missing x"),
    };
    let y = match v["y"].as_f64() {
        Some(y) => y.clamp(0.0, 100.0),
        None => return json_error(400, "missing y"),
    };
    match ctx.db.set_device_position(device_id, floor_id, x, y) {
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

fn handle_create_scene(ctx: &ApiContext, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = match v["name"].as_str() {
        Some(n) if !n.trim().is_empty() => n,
        _ => return json_error(400, "Scene name is required"),
    };
    let actions = match v["actions"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return json_error(400, "Scene must have at least one action"),
    };
    let parsed: Vec<crate::db::SceneActionInput> = actions.iter().map(|a| {
        crate::db::SceneActionInput {
            device_id: a["device_id"].as_str().unwrap_or("").to_string(),
            capability_id: a["capability_id"].as_str().unwrap_or("").to_string(),
            value: a["value"].as_str().unwrap_or("").to_string(),
        }
    }).collect();
    match ctx.db.create_scene(name, &parsed) {
        Ok(id) => json_created(&serde_json::json!({"id": id})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_update_scene(ctx: &ApiContext, id: i64, body: &str) -> (u16, String) {
    let v: Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };
    let name = match v["name"].as_str() {
        Some(n) if !n.trim().is_empty() => n,
        _ => return json_error(400, "Scene name is required"),
    };
    let actions = match v["actions"].as_array() {
        Some(arr) if !arr.is_empty() => arr,
        _ => return json_error(400, "Scene must have at least one action"),
    };
    let parsed: Vec<crate::db::SceneActionInput> = actions.iter().map(|a| {
        crate::db::SceneActionInput {
            device_id: a["device_id"].as_str().unwrap_or("").to_string(),
            capability_id: a["capability_id"].as_str().unwrap_or("").to_string(),
            value: a["value"].as_str().unwrap_or("").to_string(),
        }
    }).collect();
    match ctx.db.update_scene(id, name, &parsed) {
        Ok(()) => json_ok(&serde_json::json!({"updated": true})),
        Err(e) => json_error(500, &e),
    }
}

fn handle_run_scene(ctx: &ApiContext, id: i64) -> (u16, String) {
    let scene = match ctx.db.get_scene(id) {
        Ok(Some(s)) => s,
        Ok(None) => return json_error(404, "Scene not found"),
        Err(e) => return json_error(500, &e),
    };
    for action in &scene.actions {
        let saved = match ctx.db.get_saved_device(&action.device_id) {
            Ok(Some(d)) => d,
            _ => continue,
        };
        let value: Value = if action.value == "true" {
            Value::Bool(true)
        } else if action.value == "false" {
            Value::Bool(false)
        } else if let Ok(n) = action.value.parse::<f64>() {
            serde_json::json!(n)
        } else {
            Value::String(action.value.clone())
        };
        let cmd = serde_json::json!({
            "command": "set",
            "id": action.capability_id,
            "value": value
        });
        let msg = serde_json::to_string(&cmd).unwrap_or_default();
        let ws_port = saved.port + 1;
        if let Err(e) = ctx.connection_manager.send_to_device(&action.device_id, &saved.ip, ws_port, &msg) {
            log::warn!("[Scene API] Failed to send to {}: {}", action.device_id, e);
        }
    }
    json_ok(&serde_json::json!({"ran": true, "actions": scene.actions.len()}))
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
    let scene_id = v["scene_id"].as_i64();

    match ctx.db.create_schedule(device_id, capability_id, value, cron, label, scene_id) {
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
    let logic = v["logic"].as_str().unwrap_or("and");
    let conditions = v["conditions"].as_str();

    match ctx.db.create_rule(
        source_device_id, source_metric_id, condition, threshold,
        target_device_id, target_capability_id, target_value, label,
        logic, conditions,
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
    let ttl = v["ttl"].as_str().unwrap_or("never");
    let role = v["role"].as_str().unwrap_or("admin");
    if role != "admin" && role != "viewer" {
        return json_error(400, "Invalid role. Must be \"admin\" or \"viewer\".");
    }
    let (plaintext, hash) = auth::generate_token();
    let expires_at = auth::compute_expires_at(ttl);
    match ctx.db.create_api_token(name, &hash, expires_at.as_deref(), role) {
        Ok(id) => {
            // The plaintext is returned ONCE here and never persisted.
            // Once this response is on the wire, the only proof of the
            // token is the SHA-256 digest in `api_tokens.token_hash`.
            let resp = serde_json::json!({
                "id": id,
                "name": name,
                "token": plaintext,
                "role": role,
                "expires_at": expires_at,
                "warning": "Store this token now — it will not be shown again."
            });
            (201, resp.to_string())
        }
        Err(e) => json_error(500, &e),
    }
}

// ─── GitHub OTA handlers ────────────────────────────────────────────────────

fn fetch_github_releases(owner: &str, repo: &str) -> Result<serde_json::Value, String> {
    let url = format!("https://api.github.com/repos/{}/{}/releases", owner, repo);
    let resp = ureq::get(&url)
        .set("User-Agent", "Trellis-Desktop")
        .set("Accept", "application/vnd.github+json")
        .timeout(std::time::Duration::from_secs(10))
        .call()
        .map_err(|e| match e {
            ureq::Error::Status(404, _) => "Repository not found. Check the owner/repo format. Private repositories are not supported.".to_string(),
            ureq::Error::Status(403, _) => "GitHub API rate limit reached (60 requests/hour). Wait a few minutes and try again.".to_string(),
            ureq::Error::Status(code, _) => format!("GitHub returned HTTP {}.", code),
            ureq::Error::Transport(_) => "Could not reach GitHub. Check your internet connection.".to_string(),
        })?;

    let releases: Vec<serde_json::Value> = resp
        .into_json()
        .map_err(|e| format!("JSON parse error: {}", e))?;

    let mut result = Vec::new();
    for rel in releases.iter().take(20) {
        let tag = rel["tag_name"].as_str().unwrap_or("").to_string();
        let name = rel["name"].as_str().unwrap_or(&tag).to_string();
        let published = rel["published_at"].as_str().unwrap_or("").to_string();
        let prerelease = rel["prerelease"].as_bool().unwrap_or(false);

        let mut assets = Vec::new();
        if let Some(arr) = rel["assets"].as_array() {
            for asset in arr {
                let aname = asset["name"].as_str().unwrap_or("");
                if aname.ends_with(".bin") || aname.ends_with(".bin.gz") {
                    assets.push(serde_json::json!({
                        "name": aname,
                        "size": asset["size"].as_i64().unwrap_or(0),
                        "download_url": asset["browser_download_url"].as_str().unwrap_or(""),
                    }));
                }
            }
        }

        if !assets.is_empty() {
            result.push(serde_json::json!({
                "tag": tag,
                "name": name,
                "published_at": published,
                "prerelease": prerelease,
                "assets": assets,
            }));
        }
    }
    Ok(serde_json::json!(result))
}

fn handle_github_ota(ctx: &ApiContext, body: &str) -> (u16, String) {
    let parsed: serde_json::Value = match serde_json::from_str(body) {
        Ok(v) => v,
        Err(e) => return json_error(400, &format!("Invalid JSON: {}", e)),
    };

    let device_id = match parsed["device_id"].as_str() {
        Some(id) => id,
        None => return json_error(400, "Missing device_id"),
    };
    let download_url = match parsed["download_url"].as_str() {
        Some(u) => u,
        None => return json_error(400, "Missing download_url"),
    };
    let release_tag = parsed["release_tag"].as_str().unwrap_or("unknown");
    let asset_name = parsed["asset_name"].as_str().unwrap_or("firmware.bin");

    // Find the device
    let devices = ctx.discovery.get_devices();
    let device = match devices.iter().find(|d| d.id == device_id) {
        Some(d) => d.clone(),
        None => return json_error(404, "Device not found or offline"),
    };

    if !device.online {
        return json_error(400, "Device is offline");
    }

    // Download the firmware with progress broadcast
    log::info!("[OTA] Downloading {} from GitHub for device {}", asset_name, device_id);
    let resp = match ureq::get(download_url)
        .set("User-Agent", "Trellis-Desktop")
        .timeout(std::time::Duration::from_secs(120))
        .call()
    {
        Ok(r) => r,
        Err(e) => return json_error(502, &match e {
            ureq::Error::Status(404, _) => "Firmware file not found — the asset may have been removed from the release.".to_string(),
            ureq::Error::Status(403, _) => "Download blocked by GitHub — rate limit or authentication required.".to_string(),
            ureq::Error::Status(code, _) => format!("Download failed with HTTP {}.", code),
            ureq::Error::Transport(_) => "Download failed — network error. Check your internet connection.".to_string(),
        }),
    };

    let content_length = resp
        .header("Content-Length")
        .and_then(|s| s.parse::<u64>().ok())
        .unwrap_or(0);

    let mut reader = resp.into_reader();
    let mut raw = Vec::with_capacity(
        if content_length > 0 { content_length as usize } else { 512 * 1024 },
    );
    {
        let mut buf = [0u8; 8192];
        let mut downloaded: u64 = 0;
        let mut last_pct: u64 = 0;

        loop {
            let n = match reader.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => n,
                Err(e) => return json_error(502, &format!("Read failed: {}", e)),
            };
            raw.extend_from_slice(&buf[..n]);
            downloaded += n as u64;

            if content_length > 0 {
                let pct = (downloaded * 100 / content_length).min(100);
                if pct >= last_pct + 2 || downloaded >= content_length {
                    let ws_msg = serde_json::json!({
                        "type": "device_event",
                        "device_id": device_id,
                        "event_type": "gh_download_progress",
                        "payload": {
                            "downloaded": downloaded,
                            "total": content_length,
                            "percent": pct,
                        },
                    });
                    ctx.ws_broadcaster.broadcast(ws_msg.to_string());
                    last_pct = pct;
                }
            }
        }
    }

    // Auto-decompress .bin.gz files so the device gets raw firmware
    let data = if asset_name.ends_with(".bin.gz") {
        let mut decoder = flate2::read::GzDecoder::new(&raw[..]);
        let mut decompressed = Vec::new();
        if let Err(e) = decoder.read_to_end(&mut decompressed) {
            return json_error(502, &format!("Gzip decompression failed: {}", e));
        }
        log::info!("[OTA] Decompressed .bin.gz: {} -> {} bytes", raw.len(), decompressed.len());
        decompressed
    } else {
        raw
    };
    let file_size = data.len() as i64;

    // Save to firmware directory (same path as Tauri desktop OTA)
    let fw_dir = match ctx.app_handle.path().app_data_dir() {
        Ok(p) => p.join("firmware"),
        Err(_) => {
            // Fallback if path resolution fails
            std::path::PathBuf::from(
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".to_string()),
            )
            .join(".trellis")
            .join("data")
            .join("firmware")
        }
    };
    let _ = std::fs::create_dir_all(&fw_dir);

    let timestamp = chrono::Utc::now().format("%Y%m%d_%H%M%S").to_string();
    let dest_name = format!("{}_gh_{}_{}.bin", device_id, release_tag, timestamp);
    let dest_path = fw_dir.join(&dest_name);
    if let Err(e) = std::fs::write(&dest_path, &data) {
        return json_error(500, &format!("Failed to save firmware: {}", e));
    }

    let dest_str = dest_path.to_string_lossy().to_string();
    if let Err(e) = ctx.db.store_firmware_record(device_id, release_tag, &dest_str, file_size) {
        return json_error(500, &e);
    }

    // Serve and trigger OTA
    let ws_port = device.port + 1;
    let conn_mgr = ctx.connection_manager.clone();
    let app_handle = ctx.app_handle.clone();
    let did = device_id.to_string();
    let ip = device.ip.clone();

    match crate::ota::serve_firmware(&dest_str, app_handle, did.clone()) {
        Ok((url, _stop_flag)) => {
            let ota_cmd = serde_json::json!({"command": "ota", "url": url});
            let msg = serde_json::to_string(&ota_cmd).unwrap_or_default();
            if let Err(e) = conn_mgr.send_to_device(&did, &ip, ws_port, &msg) {
                return json_error(500, &format!("Failed to send OTA command: {}", e));
            }
            log::info!("[OTA] GitHub OTA triggered for device {} via REST", did);
            json_ok(&serde_json::json!({
                "status": "ota_triggered",
                "device_id": did,
                "release_tag": release_tag,
                "file_size": file_size,
            }))
        }
        Err(e) => json_error(500, &format!("Failed to serve firmware: {}", e)),
    }
}

// ─── Web UI (placeholder — will be replaced in Batch 4) ─────────────────────

fn get_web_ui() -> String {
    include_str!("web_ui.html").to_string()
}
