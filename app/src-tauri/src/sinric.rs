// Sinric Pro voice assistant bridge for Trellis.
//
// Connects to the Sinric Pro WebSocket server and bridges Trellis devices
// to Alexa and Google Home via the Sinric cloud. Mirrors the MQTT bridge
// architecture: worker thread with reconnect, config in SQLite, secret
// encrypted at rest.
//
// Topology:
//
//   Trellis device  <─ WS ─>  ConnectionManager  <─ events ─>  SinricBridge
//                                                                    |
//                                                                    └─> wss://ws.sinric.pro
//                                                                           |
//                                                                           └─> Alexa / Google Home
//
// Outbound (Trellis → Sinric):
//   State changes from devices (switch toggles, sensor readings) are sent as
//   Sinric events so the cloud updates its shadow state and voice queries
//   return correct values.
//
// Inbound (Sinric → Trellis):
//   Voice commands (e.g. "turn on the light") arrive as Sinric requests and
//   are routed through ConnectionManager::send_to_device.

use std::io;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use base64::Engine;
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::Sha256;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use tauri::Manager;

use crate::connection::ConnectionManager;
use crate::db::Database;
use crate::discovery::Discovery;

type HmacSha256 = Hmac<Sha256>;

const SINRIC_WS_URL: &str = "wss://ws.sinric.pro";

// ─── Config ──────────────────────────────────────────────────────────────────

/// Persisted Sinric Pro bridge configuration. Stored as JSON in the `settings`
/// table under key `sinric_config`. The `api_secret` field is encrypted at
/// rest (see secret_store.rs). All fields have serde defaults so older saved
/// configs parse cleanly when new fields are added.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinricConfig {
    #[serde(default)]
    pub enabled: bool,
    /// Sinric Pro APP_KEY (UUID format). Visible in the Sinric Pro dashboard
    /// under Credentials.
    #[serde(default)]
    pub api_key: String,
    /// Sinric Pro APP_SECRET (min 32 chars). Used to HMAC-sign every message.
    /// Encrypted at rest in SQLite.
    #[serde(default)]
    pub api_secret: String,
    /// Maps Sinric device IDs to Trellis device IDs. Each entry tells the
    /// bridge "when Sinric asks about this device, talk to that Trellis device."
    /// The user creates devices on sinric.pro and copies the IDs here.
    #[serde(default)]
    pub device_mappings: Vec<SinricDeviceMapping>,
}

impl Default for SinricConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_key: String::new(),
            api_secret: String::new(),
            device_mappings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SinricDeviceMapping {
    pub sinric_device_id: String,
    pub trellis_device_id: String,
    /// When set, the mapping targets a specific capability on the Trellis
    /// device. When absent (or empty), the bridge auto-discovers the first
    /// capability of the matching type (backward-compatible default).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub trellis_capability_id: Option<String>,
    /// When set, a `setPowerState(On)` command triggers this scene instead
    /// of dispatching to a device capability. `trellis_device_id` is ignored
    /// when `scene_id` is present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scene_id: Option<i64>,
}

/// Public-facing config view. The api_secret is redacted — same pattern as
/// MqttConfigPublic.
#[derive(Debug, Clone, Serialize)]
pub struct SinricConfigPublic {
    pub enabled: bool,
    pub api_key: String,
    pub has_secret: bool,
    pub device_mappings: Vec<SinricDeviceMapping>,
}

impl From<&SinricConfig> for SinricConfigPublic {
    fn from(c: &SinricConfig) -> Self {
        Self {
            enabled: c.enabled,
            api_key: c.api_key.clone(),
            has_secret: !c.api_secret.is_empty(),
            device_mappings: c.device_mappings.clone(),
        }
    }
}

/// Live status of the bridge.
#[derive(Debug, Clone, Serialize)]
pub struct SinricStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub messages_sent: u64,
    pub messages_received: u64,
}

impl Default for SinricStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            connected: false,
            last_error: None,
            messages_sent: 0,
            messages_received: 0,
        }
    }
}

// ─── Outgoing message types ──────────────────────────────────────────────────

/// Messages sent from other threads (ConnectionManager, Discovery) to the
/// worker thread via an mpsc channel.
enum OutgoingEvent {
    PowerState {
        sinric_device_id: String,
        state: bool,
    },
    Temperature {
        sinric_device_id: String,
        temperature: f64,
        humidity: Option<f64>,
    },
    RangeValue {
        sinric_device_id: String,
        value: i64,
    },
    Color {
        sinric_device_id: String,
        r: u8,
        g: u8,
        b: u8,
    },
}

// ─── Bridge ──────────────────────────────────────────────────────────────────

pub struct SinricBridge {
    config: Arc<Mutex<SinricConfig>>,
    status: Arc<Mutex<SinricStatus>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    stop_flag: Arc<Mutex<bool>>,
    outgoing_tx: Arc<Mutex<Option<mpsc::Sender<OutgoingEvent>>>>,
    connection_manager: Arc<ConnectionManager>,
    discovery: Arc<Mutex<Option<Arc<Discovery>>>>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
}

impl SinricBridge {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            config: Arc::new(Mutex::new(SinricConfig::default())),
            status: Arc::new(Mutex::new(SinricStatus::default())),
            worker: Mutex::new(None),
            stop_flag: Arc::new(Mutex::new(false)),
            outgoing_tx: Arc::new(Mutex::new(None)),
            connection_manager,
            discovery: Arc::new(Mutex::new(None)),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_discovery(&self, discovery: Arc<Discovery>) {
        *self.discovery.lock().unwrap() = Some(discovery);
    }

    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().unwrap() = Some(handle);
    }

    pub fn get_config(&self) -> SinricConfig {
        self.config.lock().unwrap().clone()
    }

    pub fn get_config_public(&self) -> SinricConfigPublic {
        SinricConfigPublic::from(&*self.config.lock().unwrap())
    }

    pub fn get_status(&self) -> SinricStatus {
        self.status.lock().unwrap().clone()
    }

    /// Merge an incoming config preserving an existing secret when the
    /// incoming secret is empty (same preserve-blank pattern as MQTT).
    fn merge_preserving_secret(&self, mut incoming: SinricConfig) -> SinricConfig {
        if incoming.api_secret.is_empty() {
            let existing = self.config.lock().unwrap().api_secret.clone();
            if !existing.is_empty() {
                incoming.api_secret = existing;
            }
        }
        incoming
    }

    pub fn clear_secret(&self) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap().clone();
        cfg.api_secret = String::new();
        self.apply_config(cfg)
    }

    pub fn apply_config_from_user(&self, new_config: SinricConfig) -> Result<(), String> {
        let merged = self.merge_preserving_secret(new_config);
        self.apply_config(merged)
    }

    pub fn apply_config(&self, new_config: SinricConfig) -> Result<(), String> {
        self.stop();

        *self.config.lock().unwrap() = new_config.clone();

        if new_config.enabled {
            if new_config.api_key.is_empty() || new_config.api_secret.is_empty() {
                let mut s = self.status.lock().unwrap();
                s.enabled = true;
                s.connected = false;
                s.last_error = Some("API key and secret are required".to_string());
                return Err("API key and secret are required".to_string());
            }
            self.start()?;
        } else {
            let mut s = self.status.lock().unwrap();
            s.enabled = false;
            s.connected = false;
            s.last_error = None;
        }
        Ok(())
    }

    fn start(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return Ok(());
        }

        let (tx, rx) = mpsc::channel();
        *self.outgoing_tx.lock().unwrap() = Some(tx);
        *self.stop_flag.lock().unwrap() = false;

        let status = self.status.clone();
        let stop_flag = self.stop_flag.clone();
        let config = self.config.clone();
        let conn_mgr = self.connection_manager.clone();
        let discovery = self.discovery.clone();
        let app_handle = self.app_handle.clone();

        {
            let mut s = status.lock().unwrap();
            s.enabled = true;
            s.connected = false;
            s.last_error = None;
        }

        let handle = thread::spawn(move || {
            sinric_worker(config, status, stop_flag, rx, conn_mgr, discovery, app_handle);
        });

        *self.worker.lock().unwrap() = Some(handle);
        Ok(())
    }

    pub fn stop(&self) {
        *self.stop_flag.lock().unwrap() = true;
        *self.outgoing_tx.lock().unwrap() = None;

        // Take the handle without holding the lock during join
        let handle = self.worker.lock().unwrap().take();
        if let Some(h) = handle {
            let _ = h.join();
        }

        let mut s = self.status.lock().unwrap();
        s.connected = false;
    }

    /// Called by ConnectionManager when a device state changes. Translates
    /// Trellis state updates into Sinric events.
    pub fn on_state_change(&self, device_id: &str, capability_id: &str, value: &Value) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return;
        }

        // Find Sinric device mappings that should receive this state change.
        // Mappings with an explicit capability_id only fire for that capability;
        // mappings without one (auto) fire for every capability on the device.
        let sinric_ids: Vec<String> = cfg
            .device_mappings
            .iter()
            .filter(|m| {
                m.trellis_device_id == device_id
                    && match &m.trellis_capability_id {
                        Some(cap) if !cap.is_empty() => cap == capability_id,
                        _ => true,
                    }
            })
            .map(|m| m.sinric_device_id.clone())
            .collect();

        if sinric_ids.is_empty() {
            return;
        }

        let tx = match self.outgoing_tx.lock().unwrap().as_ref() {
            Some(tx) => tx.clone(),
            None => return,
        };

        // Look up the capability type from the live device list
        let cap_type = {
            let disc_lock = self.discovery.lock().unwrap();
            let disc = match disc_lock.as_ref() {
                Some(d) => d,
                None => return,
            };
            let devices = disc.get_devices();
            devices
                .iter()
                .find(|d| d.id == device_id)
                .and_then(|d| {
                    d.capabilities
                        .iter()
                        .find(|c| c.id == capability_id)
                        .map(|c| c.cap_type.clone())
                })
        };

        let cap_type = match cap_type {
            Some(t) => t,
            None => return,
        };

        for sinric_id in sinric_ids {
            let event = match cap_type.as_str() {
                "switch" => {
                    let state = value.as_bool().unwrap_or(false);
                    OutgoingEvent::PowerState {
                        sinric_device_id: sinric_id,
                        state,
                    }
                }
                "sensor" => {
                    let temp = match value.as_f64() {
                        Some(v) => v,
                        None => continue,
                    };
                    OutgoingEvent::Temperature {
                        sinric_device_id: sinric_id,
                        temperature: temp,
                        humidity: None,
                    }
                }
                "slider" => {
                    let v = match value.as_i64() {
                        Some(v) => v,
                        None => value.as_f64().map(|f| f as i64).unwrap_or(0),
                    };
                    OutgoingEvent::RangeValue {
                        sinric_device_id: sinric_id,
                        value: v,
                    }
                }
                "color" => {
                    // Trellis color values are "#RRGGBB" hex strings
                    let hex = value.as_str().unwrap_or("#000000");
                    let hex = hex.trim_start_matches('#');
                    let r = u8::from_str_radix(&hex.get(0..2).unwrap_or("00"), 16).unwrap_or(0);
                    let g = u8::from_str_radix(&hex.get(2..4).unwrap_or("00"), 16).unwrap_or(0);
                    let b = u8::from_str_radix(&hex.get(4..6).unwrap_or("00"), 16).unwrap_or(0);
                    OutgoingEvent::Color {
                        sinric_device_id: sinric_id,
                        r,
                        g,
                        b,
                    }
                }
                _ => continue,
            };

            let _ = tx.send(event);
        }
    }

    /// Test connectivity without starting the persistent bridge. Connects,
    /// waits for a response, then disconnects.
    pub fn test_connection(&self, cfg: &SinricConfig) -> Result<(), String> {
        if cfg.api_key.is_empty() || cfg.api_secret.is_empty() {
            return Err("API key and secret are required".to_string());
        }

        let sinric_ids: Vec<&str> = cfg
            .device_mappings
            .iter()
            .map(|m| m.sinric_device_id.as_str())
            .collect();
        let device_ids_header = sinric_ids.join(";");

        let request = tungstenite::http::Request::builder()
            .uri(SINRIC_WS_URL)
            .header("appkey", &cfg.api_key)
            .header("deviceids", &device_ids_header)
            .body(())
            .map_err(|e| format!("Failed to build request: {}", e))?;

        let (mut ws, _) =
            tungstenite::connect(request).map_err(|e| format!("WebSocket connect failed: {}", e))?;

        // Read one frame to verify the connection is accepted. Sinric sends a
        // response that we can check for auth rejection. Without this, wrong
        // APP_KEYs silently appear to succeed.
        set_read_timeout(&mut ws, Duration::from_secs(5));
        match ws.read() {
            Ok(Message::Text(text)) => {
                // Check for an explicit rejection
                if let Ok(msg) = serde_json::from_str::<Value>(&text) {
                    if msg.get("success") == Some(&Value::Bool(false)) {
                        let reason = msg
                            .get("message")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Unknown error");
                        let _ = ws.close(None);
                        return Err(format!("Sinric rejected connection: {}", reason));
                    }
                }
            }
            Ok(Message::Close(frame)) => {
                let reason = frame
                    .map(|f| f.reason.to_string())
                    .unwrap_or_else(|| "Server closed connection".to_string());
                return Err(format!("Connection closed: {}", reason));
            }
            Ok(_) => {} // Ping/pong/binary — connection is alive
            Err(e) => {
                // Timeout is acceptable — no welcome message, but connection is up
                if let tungstenite::Error::Io(ref io_err) = e {
                    if io_err.kind() == io::ErrorKind::WouldBlock
                        || io_err.kind() == io::ErrorKind::TimedOut
                    {
                        // No message within timeout — connection is still valid
                    } else {
                        let _ = ws.close(None);
                        return Err(format!("Read error: {}", e));
                    }
                }
            }
        }
        let _ = ws.close(None);
        Ok(())
    }

    pub fn test_connection_from_user(&self, cfg: SinricConfig) -> Result<(), String> {
        let merged = self.merge_preserving_secret(cfg);
        self.test_connection(&merged)
    }
}

// ─── Worker thread ───────────────────────────────────────────────────────────

fn sinric_worker(
    config: Arc<Mutex<SinricConfig>>,
    status: Arc<Mutex<SinricStatus>>,
    stop_flag: Arc<Mutex<bool>>,
    rx: mpsc::Receiver<OutgoingEvent>,
    conn_mgr: Arc<ConnectionManager>,
    discovery: Arc<Mutex<Option<Arc<Discovery>>>>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
) {
    let mut backoff = Duration::from_secs(2);

    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }

        let cfg = config.lock().unwrap().clone();
        if cfg.api_key.is_empty() || cfg.api_secret.is_empty() {
            let mut s = status.lock().unwrap();
            s.last_error = Some("API key and secret are required".to_string());
            break;
        }

        let sinric_ids: Vec<&str> = cfg
            .device_mappings
            .iter()
            .map(|m| m.sinric_device_id.as_str())
            .collect();
        let device_ids_header = sinric_ids.join(";");

        let request = match tungstenite::http::Request::builder()
            .uri(SINRIC_WS_URL)
            .header("appkey", &cfg.api_key)
            .header("deviceids", &device_ids_header)
            .body(())
        {
            Ok(r) => r,
            Err(e) => {
                status.lock().unwrap().last_error =
                    Some(format!("Failed to build request: {}", e));
                break;
            }
        };

        match tungstenite::connect(request) {
            Ok((mut ws, _response)) => {
                set_read_timeout(&mut ws, Duration::from_secs(1));

                {
                    let mut s = status.lock().unwrap();
                    s.connected = true;
                    s.last_error = None;
                }
                log::info!("[Sinric] Connected to ws.sinric.pro");

                backoff = Duration::from_secs(2);

                message_loop(
                    &mut ws,
                    &rx,
                    &stop_flag,
                    &status,
                    &config,
                    &conn_mgr,
                    &discovery,
                    &app_handle,
                );

                let _ = ws.close(None);
                status.lock().unwrap().connected = false;
                log::info!("[Sinric] Disconnected from ws.sinric.pro");
            }
            Err(e) => {
                let mut s = status.lock().unwrap();
                s.connected = false;
                s.last_error = Some(format!("Connect failed: {}", e));
                log::warn!("[Sinric] Connect failed: {}", e);
            }
        }

        if *stop_flag.lock().unwrap() {
            break;
        }

        // Backoff before reconnect
        log::info!("[Sinric] Reconnecting in {:?}", backoff);
        thread::sleep(backoff);
        backoff = (backoff * 2).min(Duration::from_secs(30));
    }
}

fn message_loop(
    ws: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    rx: &mpsc::Receiver<OutgoingEvent>,
    stop_flag: &Arc<Mutex<bool>>,
    status: &Arc<Mutex<SinricStatus>>,
    config: &Arc<Mutex<SinricConfig>>,
    conn_mgr: &Arc<ConnectionManager>,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
) {
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }

        // Read incoming messages (with 1s timeout)
        match ws.read() {
            Ok(Message::Text(text)) => {
                status.lock().unwrap().messages_received += 1;
                handle_incoming(&text, config, conn_mgr, discovery, ws, status, app_handle);
            }
            Ok(Message::Ping(data)) => {
                let _ = ws.send(Message::Pong(data));
            }
            Ok(Message::Close(_)) => {
                log::info!("[Sinric] Server sent close frame");
                break;
            }
            Ok(_) => {} // Binary, Pong — ignore
            Err(tungstenite::Error::Io(ref e))
                if e.kind() == io::ErrorKind::WouldBlock
                    || e.kind() == io::ErrorKind::TimedOut =>
            {
                // Read timeout — no message, proceed to drain outgoing
            }
            Err(e) => {
                log::warn!("[Sinric] Read error: {}", e);
                status.lock().unwrap().last_error = Some(format!("Read error: {}", e));
                break;
            }
        }

        // Drain outgoing events
        while let Ok(event) = rx.try_recv() {
            let cfg = config.lock().unwrap().clone();
            if let Some(msg) = build_outgoing_event(&event, &cfg.api_secret) {
                match ws.send(Message::Text(msg)) {
                    Ok(()) => {
                        status.lock().unwrap().messages_sent += 1;
                    }
                    Err(e) => {
                        log::warn!("[Sinric] Send error: {}", e);
                        status.lock().unwrap().last_error =
                            Some(format!("Send error: {}", e));
                        return;
                    }
                }
            }
        }
    }
}

// ─── Incoming message handling ───────────────────────────────────────────────

/// Envelope for incoming Sinric messages. Uses `RawValue` for the payload so
/// we can verify the HMAC against the exact bytes from the wire (not a
/// re-serialized copy that might differ in key order or whitespace).
#[derive(Deserialize)]
struct SinricEnvelope {
    #[allow(dead_code)]
    header: Value,
    payload: Box<serde_json::value::RawValue>,
    signature: Option<SinricSignature>,
}

#[derive(Deserialize)]
struct SinricSignature {
    #[serde(rename = "HMAC")]
    hmac: String,
}

fn handle_incoming(
    text: &str,
    config: &Arc<Mutex<SinricConfig>>,
    conn_mgr: &Arc<ConnectionManager>,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
    ws: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    status: &Arc<Mutex<SinricStatus>>,
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
) {
    let envelope: SinricEnvelope = match serde_json::from_str(text) {
        Ok(v) => v,
        Err(e) => {
            log::warn!("[Sinric] Failed to parse message: {}", e);
            return;
        }
    };

    // Verify HMAC signature before acting on the message
    let cfg = config.lock().unwrap().clone();
    if let Some(ref sig) = envelope.signature {
        let payload_raw = envelope.payload.get();
        let expected = hmac_sign(&cfg.api_secret, payload_raw);
        if sig.hmac != expected {
            log::warn!("[Sinric] HMAC verification failed — rejecting message");
            return;
        }
    }

    let payload: Value = match serde_json::from_str(envelope.payload.get()) {
        Ok(v) => v,
        Err(_) => return,
    };

    let msg_type = payload
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if msg_type != "request" {
        return; // We only handle incoming requests
    }

    let action = payload
        .get("action")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let device_id = payload
        .get("deviceId")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reply_token = payload
        .get("replyToken")
        .and_then(|v| v.as_str())
        .unwrap_or("");

    if device_id.is_empty() || action.is_empty() {
        return;
    }

    // Look up the Trellis device (and optional explicit capability) for this
    // Sinric device.
    let mapping = cfg
        .device_mappings
        .iter()
        .find(|m| m.sinric_device_id == device_id);

    let (trellis_device_id, explicit_cap_id) = match mapping {
        Some(m) => {
            let cap = match &m.trellis_capability_id {
                Some(id) if !id.is_empty() => Some(id.clone()),
                _ => None,
            };
            (m.trellis_device_id.clone(), cap)
        }
        None => {
            log::warn!(
                "[Sinric] No mapping for Sinric device {}",
                device_id
            );
            return;
        }
    };

    // Scene-mapped device: setPowerState(On) triggers the scene.
    let scene_id = mapping.and_then(|m| m.scene_id);
    if let Some(sid) = scene_id {
        if action == "setPowerState" {
            let state_str = payload
                .get("value")
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("Off");
            let on = state_str == "On";

            let (success, response_value) = if on {
                match run_scene_from_sinric(sid, conn_mgr, app_handle) {
                    Ok(()) => {
                        log::info!("[Sinric] Ran scene {} for Sinric device {}", sid, device_id);
                        (true, json!({"state": "On"}))
                    }
                    Err(e) => {
                        log::warn!("[Sinric] Failed to run scene {}: {}", sid, e);
                        (false, json!({"state": "On"}))
                    }
                }
            } else {
                // "Off" is a no-op for scenes — respond success so Alexa doesn't complain
                (true, json!({"state": "Off"}))
            };

            let response = build_response(
                action, device_id, reply_token, success, &response_value, &cfg.api_secret,
            );
            if let Some(resp_text) = response {
                match ws.send(Message::Text(resp_text)) {
                    Ok(()) => { status.lock().unwrap().messages_sent += 1; }
                    Err(e) => { log::warn!("[Sinric] Failed to send response: {}", e); }
                }
            }
            return;
        }
        // Non-setPowerState actions don't apply to scenes — ignore
        return;
    }

    // Check that the device is online before trying to dispatch
    let device_online = conn_mgr.is_connected(&trellis_device_id);

    let (success, response_value) = match action {
        "setPowerState" => {
            let state_str = payload
                .get("value")
                .and_then(|v| v.get("state"))
                .and_then(|v| v.as_str())
                .unwrap_or("Off");
            let on = state_str == "On";

            if !device_online {
                log::warn!("[Sinric] Trellis device {} is offline", trellis_device_id);
                (false, json!({"state": state_str}))
            } else {
                let cap_id = resolve_capability_id(
                    &explicit_cap_id, &trellis_device_id, "switch", discovery,
                );
                match cap_id {
                    Some(cap_id) => {
                        let cmd = json!({
                            "command": "set",
                            "id": cap_id,
                            "value": on,
                        });
                        let msg = match serde_json::to_string(&cmd) {
                            Ok(m) => m,
                            Err(e) => {
                                log::warn!("[Sinric] Failed to serialize command: {}", e);
                                return;
                            }
                        };
                        match conn_mgr.send_to_device(&trellis_device_id, "", 0, &msg) {
                            Ok(()) => (true, json!({"state": if on { "On" } else { "Off" }})),
                            Err(e) => {
                                log::warn!("[Sinric] Failed to dispatch setPowerState to {}: {}", trellis_device_id, e);
                                (false, json!({"state": state_str}))
                            }
                        }
                    }
                    None => {
                        log::warn!(
                            "[Sinric] No switch capability on Trellis device {}",
                            trellis_device_id
                        );
                        (false, json!({"state": state_str}))
                    }
                }
            }
        }
        "currentTemperature" | "targetTemperature" => {
            // Temperature query — use explicit capability if it's a sensor,
            // else fall back to the name-hint heuristic.
            let temp = resolve_sensor_value(
                &explicit_cap_id, &trellis_device_id, "temp", discovery,
            );
            let humidity = find_sensor_value(&trellis_device_id, "humid", discovery);
            (
                true,
                json!({
                    "temperature": temp.unwrap_or(0.0),
                    "humidity": humidity.unwrap_or(-1.0)
                }),
            )
        }
        "setRangeValue" | "adjustRangeValue" => {
            let range_value = payload
                .get("value")
                .and_then(|v| v.get("rangeValue"))
                .and_then(|v| v.as_i64())
                .unwrap_or(0);

            if !device_online {
                log::warn!("[Sinric] Trellis device {} is offline", trellis_device_id);
                (false, json!({"rangeValue": range_value}))
            } else {
                let cap_id = resolve_capability_id(
                    &explicit_cap_id, &trellis_device_id, "slider", discovery,
                );
                match cap_id {
                    Some(cap_id) => {
                        let cmd = json!({
                            "command": "set",
                            "id": cap_id,
                            "value": range_value,
                        });
                        let msg = match serde_json::to_string(&cmd) {
                            Ok(m) => m,
                            Err(e) => {
                                log::warn!("[Sinric] Failed to serialize command: {}", e);
                                return;
                            }
                        };
                        match conn_mgr.send_to_device(&trellis_device_id, "", 0, &msg) {
                            Ok(()) => (true, json!({"rangeValue": range_value})),
                            Err(e) => {
                                log::warn!("[Sinric] Failed to dispatch setRangeValue to {}: {}", trellis_device_id, e);
                                (false, json!({"rangeValue": range_value}))
                            }
                        }
                    }
                    None => {
                        log::warn!(
                            "[Sinric] No slider capability on Trellis device {}",
                            trellis_device_id
                        );
                        (false, json!({"rangeValue": range_value}))
                    }
                }
            }
        }
        "setColor" => {
            let empty = json!({});
            let color = payload
                .get("value")
                .and_then(|v| v.get("color"))
                .unwrap_or(&empty);
            let r = color.get("r").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let g = color.get("g").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let b = color.get("b").and_then(|v| v.as_u64()).unwrap_or(0) as u8;
            let hex = format!("#{:02x}{:02x}{:02x}", r, g, b);

            if !device_online {
                log::warn!("[Sinric] Trellis device {} is offline", trellis_device_id);
                (false, json!({"color": {"r": r, "g": g, "b": b}}))
            } else {
                let cap_id = resolve_capability_id(
                    &explicit_cap_id, &trellis_device_id, "color", discovery,
                );
                match cap_id {
                    Some(cap_id) => {
                        let cmd = json!({
                            "command": "set",
                            "id": cap_id,
                            "value": hex,
                        });
                        let msg = match serde_json::to_string(&cmd) {
                            Ok(m) => m,
                            Err(e) => {
                                log::warn!("[Sinric] Failed to serialize command: {}", e);
                                return;
                            }
                        };
                        match conn_mgr.send_to_device(&trellis_device_id, "", 0, &msg) {
                            Ok(()) => (true, json!({"color": {"r": r, "g": g, "b": b}})),
                            Err(e) => {
                                log::warn!("[Sinric] Failed to dispatch setColor to {}: {}", trellis_device_id, e);
                                (false, json!({"color": {"r": r, "g": g, "b": b}}))
                            }
                        }
                    }
                    None => {
                        log::warn!(
                            "[Sinric] No color capability on Trellis device {}",
                            trellis_device_id
                        );
                        (false, json!({"color": {"r": r, "g": g, "b": b}}))
                    }
                }
            }
        }
        _ => {
            log::info!("[Sinric] Unhandled action: {}", action);
            (false, json!({}))
        }
    };

    // Send response
    let response = build_response(
        action,
        device_id,
        reply_token,
        success,
        &response_value,
        &cfg.api_secret,
    );

    if let Some(resp_text) = response {
        match ws.send(Message::Text(resp_text)) {
            Ok(()) => {
                status.lock().unwrap().messages_sent += 1;
            }
            Err(e) => {
                log::warn!("[Sinric] Failed to send response: {}", e);
            }
        }
    }
}

// ─── Scene execution from Sinric ─────────────────────────────────────────────

fn run_scene_from_sinric(
    scene_id: i64,
    conn_mgr: &Arc<ConnectionManager>,
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
) -> Result<(), String> {
    let handle = app_handle.lock().unwrap();
    let handle = handle.as_ref().ok_or("App handle not available")?;
    let db = handle.try_state::<Database>()
        .ok_or("Database not available")?;

    let scene = db.get_scene(scene_id)?
        .ok_or_else(|| format!("Scene {} not found", scene_id))?;

    crate::scheduler::fire_scene(handle, conn_mgr.as_ref(), &scene)
}

// ─── Outgoing message construction ───────────────────────────────────────────

fn build_outgoing_event(event: &OutgoingEvent, secret: &str) -> Option<String> {
    let (action, device_id, value) = match event {
        OutgoingEvent::PowerState {
            sinric_device_id,
            state,
        } => (
            "setPowerState",
            sinric_device_id.as_str(),
            json!({"state": if *state { "On" } else { "Off" }}),
        ),
        OutgoingEvent::Temperature {
            sinric_device_id,
            temperature,
            humidity,
        } => {
            let mut val = json!({"temperature": temperature});
            if let Some(h) = humidity {
                val["humidity"] = json!(h);
            }
            ("currentTemperature", sinric_device_id.as_str(), val)
        }
        OutgoingEvent::RangeValue {
            sinric_device_id,
            value,
        } => (
            "setRangeValue",
            sinric_device_id.as_str(),
            json!({"rangeValue": value}),
        ),
        OutgoingEvent::Color {
            sinric_device_id,
            r,
            g,
            b,
        } => (
            "setColor",
            sinric_device_id.as_str(),
            json!({
                "color": {
                    "r": r,
                    "g": g,
                    "b": b,
                }
            }),
        ),
    };

    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let payload = json!({
        "action": action,
        "cause": {"type": "PHYSICAL_INTERACTION"},
        "createdAt": now,
        "deviceAttributes": [],
        "deviceId": device_id,
        "type": "event",
        "value": value
    });

    let payload_json = serde_json::to_string(&payload).ok()?;
    let signature = hmac_sign(secret, &payload_json);

    let message = json!({
        "header": {
            "payloadVersion": 2,
            "signatureVersion": 1
        },
        "payload": payload,
        "signature": {
            "HMAC": signature
        }
    });

    serde_json::to_string(&message).ok()
}

fn build_response(
    action: &str,
    device_id: &str,
    reply_token: &str,
    success: bool,
    value: &Value,
    secret: &str,
) -> Option<String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let payload = json!({
        "action": action,
        "clientId": "trellis",
        "createdAt": now,
        "deviceId": device_id,
        "message": if success { "OK" } else { "FAILED" },
        "replyToken": reply_token,
        "success": success,
        "type": "response",
        "value": value
    });

    let payload_json = serde_json::to_string(&payload).ok()?;
    let signature = hmac_sign(secret, &payload_json);

    let message = json!({
        "header": {
            "payloadVersion": 2,
            "signatureVersion": 1
        },
        "payload": payload,
        "signature": {
            "HMAC": signature
        }
    });

    serde_json::to_string(&message).ok()
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn hmac_sign(secret: &str, payload: &str) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret.as_bytes()).expect("HMAC accepts any key length");
    mac.update(payload.as_bytes());
    let result = mac.finalize().into_bytes();
    base64::engine::general_purpose::STANDARD.encode(&result)
}

/// Set a read timeout on the underlying TCP stream so the worker loop can
/// drain outgoing messages periodically instead of blocking forever on read.
fn set_read_timeout(
    ws: &mut WebSocket<MaybeTlsStream<std::net::TcpStream>>,
    timeout: Duration,
) {
    match ws.get_mut() {
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.get_mut().set_read_timeout(Some(timeout));
        }
        MaybeTlsStream::Plain(tcp) => {
            let _ = tcp.set_read_timeout(Some(timeout));
        }
        _ => {}
    }
}

/// Find the first capability of a given type on a Trellis device.
fn find_first_capability(
    trellis_device_id: &str,
    cap_type: &str,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
) -> Option<String> {
    let disc_lock = discovery.lock().unwrap();
    let disc = disc_lock.as_ref()?;
    let devices = disc.get_devices();
    devices
        .iter()
        .find(|d| d.id == trellis_device_id)
        .and_then(|d| {
            d.capabilities
                .iter()
                .find(|c| c.cap_type == cap_type)
                .map(|c| c.id.clone())
        })
}

/// Read the current value of the first sensor whose ID contains `hint`
/// (e.g. "temp", "humid") on a Trellis device.
fn find_sensor_value(
    trellis_device_id: &str,
    hint: &str,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
) -> Option<f64> {
    let disc_lock = discovery.lock().unwrap();
    let disc = disc_lock.as_ref()?;
    let devices = disc.get_devices();
    devices
        .iter()
        .find(|d| d.id == trellis_device_id)
        .and_then(|d| {
            d.capabilities
                .iter()
                .find(|c| c.cap_type == "sensor" && c.id.contains(hint))
                .and_then(|c| c.value.as_f64())
        })
}

/// Resolve which capability ID to use for a given action type. If the mapping
/// has an explicit `trellis_capability_id` AND that capability matches
/// `expected_type`, use it. Otherwise fall back to auto-discovering the first
/// capability of the expected type.
///
/// This prevents type mismatches: if a user maps to a switch but Sinric sends
/// a setRangeValue request, the switch cap is skipped and auto-discovery
/// finds the first slider instead.
fn resolve_capability_id(
    explicit: &Option<String>,
    trellis_device_id: &str,
    expected_type: &str,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
) -> Option<String> {
    if let Some(ref cap_id) = explicit {
        let disc_lock = discovery.lock().unwrap();
        let matches = disc_lock.as_ref().map_or(false, |disc| {
            disc.get_devices()
                .iter()
                .find(|d| d.id == trellis_device_id)
                .and_then(|d| d.capabilities.iter().find(|c| c.id == *cap_id))
                .map_or(false, |c| c.cap_type == expected_type)
        });
        if matches {
            return Some(cap_id.clone());
        }
    }
    find_first_capability(trellis_device_id, expected_type, discovery)
}

/// Resolve a sensor value with type validation. If the explicit capability
/// is a sensor, read its value. Otherwise fall back to the name-hint
/// heuristic (e.g. "temp", "humid").
fn resolve_sensor_value(
    explicit: &Option<String>,
    trellis_device_id: &str,
    hint: &str,
    discovery: &Arc<Mutex<Option<Arc<Discovery>>>>,
) -> Option<f64> {
    if let Some(ref cap_id) = explicit {
        let disc_lock = discovery.lock().unwrap();
        let val = disc_lock.as_ref().and_then(|disc| {
            disc.get_devices()
                .iter()
                .find(|d| d.id == trellis_device_id)
                .and_then(|d| {
                    d.capabilities
                        .iter()
                        .find(|c| c.id == *cap_id && c.cap_type == "sensor")
                        .and_then(|c| c.value.as_f64())
                })
        });
        if val.is_some() {
            return val;
        }
    }
    find_sensor_value(trellis_device_id, hint, discovery)
}
