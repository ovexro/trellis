// MQTT bridge for Trellis.
//
// Mirrors every Trellis device's capabilities to an external MQTT broker so
// Home Assistant, Node-RED, and other MQTT-aware tools can read state and
// send commands. Optional Home Assistant MQTT discovery (the default) makes
// devices auto-appear as HA entities with no manual YAML config.
//
// Topology:
//
//   Trellis device  <─ WS ─>  ConnectionManager  <─ events/cmds ─>  MqttBridge
//                                                                       |
//                                                                       └─> MQTT broker
//                                                                              |
//                                                                              └─> Home Assistant / Node-RED / etc.
//
// State (Trellis -> MQTT):
//   When ConnectionManager emits a "device-event" with an `update` payload,
//   we publish the new value to:
//     <base_topic>/<device_id>/<capability_id>/state
//   ...and (if HA discovery is enabled) the same value goes to the HA
//   discovery state topic so HA shows the correct entity state.
//
// Commands (MQTT -> Trellis):
//   We subscribe to:
//     <base_topic>/<device_id>/<capability_id>/set
//   ...plus the HA-discovery command topic for each capability. Incoming
//   payloads are translated into the Trellis WS protocol and dispatched via
//   ConnectionManager::send_to_device — re-using the same race-free relay
//   path the Tauri commands and REST API use.
//
// Discovery configs are republished whenever a device first appears or its
// capability list changes (firmware bump, manual reflash). On disconnect we
// fall back to a Last Will message that marks the bridge offline so HA can
// flag entities as unavailable.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use rumqttc::{Client, Connection as MqttConnection, Event, LastWill, MqttOptions, Packet, QoS};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::connection::ConnectionManager;
use crate::device::Device;

const DEFAULT_BASE_TOPIC: &str = "trellis";
const DEFAULT_HA_PREFIX: &str = "homeassistant";
const BRIDGE_AVAILABILITY_SUFFIX: &str = "bridge/availability";
const PAYLOAD_ONLINE: &str = "online";
const PAYLOAD_OFFLINE: &str = "offline";

/// Persisted MQTT bridge configuration. Stored as JSON in the existing
/// `settings` table under key `mqtt_config`. None of these fields are
/// encrypted at rest — keep that in mind for the password.
///
/// Every field has a serde default so partial JSON payloads (e.g. from the
/// REST API or older saved configs missing newer fields) deserialize
/// cleanly into the defaults.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MqttConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_broker_host")]
    pub broker_host: String,
    #[serde(default = "default_broker_port")]
    pub broker_port: u16,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub password: String,
    #[serde(default = "default_base_topic")]
    pub base_topic: String,
    #[serde(default = "default_ha_prefix")]
    pub ha_discovery_prefix: String,
    #[serde(default = "default_true")]
    pub ha_discovery_enabled: bool,
    #[serde(default = "default_client_id")]
    pub client_id: String,
}

fn default_broker_host() -> String {
    "localhost".to_string()
}
fn default_broker_port() -> u16 {
    1883
}
fn default_base_topic() -> String {
    DEFAULT_BASE_TOPIC.to_string()
}
fn default_ha_prefix() -> String {
    DEFAULT_HA_PREFIX.to_string()
}
fn default_true() -> bool {
    true
}
fn default_client_id() -> String {
    "trellis-bridge".to_string()
}

impl Default for MqttConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            broker_host: "localhost".to_string(),
            broker_port: 1883,
            username: String::new(),
            password: String::new(),
            base_topic: default_base_topic(),
            ha_discovery_prefix: default_ha_prefix(),
            ha_discovery_enabled: true,
            client_id: default_client_id(),
        }
    }
}

/// Live status of the bridge — used by the Settings UI to show whether the
/// MQTT connection is healthy.
#[derive(Debug, Clone, Serialize)]
pub struct MqttStatus {
    pub enabled: bool,
    pub connected: bool,
    pub last_error: Option<String>,
    pub messages_published: u64,
    pub messages_received: u64,
}

impl Default for MqttStatus {
    fn default() -> Self {
        Self {
            enabled: false,
            connected: false,
            last_error: None,
            messages_published: 0,
            messages_received: 0,
        }
    }
}

/// Bridge handle that lives in `AppState`. Holds the MQTT client (when
/// running), the current config, the live status, and a worker thread that
/// drains the rumqttc event loop. Methods on `MqttBridge` are cheap to call
/// from anywhere — heavy lifting happens on the worker thread.
pub struct MqttBridge {
    config: Arc<Mutex<MqttConfig>>,
    status: Arc<Mutex<MqttStatus>>,
    client: Arc<Mutex<Option<Client>>>,
    worker: Mutex<Option<thread::JoinHandle<()>>>,
    stop_flag: Arc<Mutex<bool>>,
    /// Track which devices have already had HA discovery configs published,
    /// so we only republish when a device's capability list actually changes.
    discovery_published: Arc<Mutex<HashMap<String, Vec<String>>>>,
    connection_manager: Arc<ConnectionManager>,
}

impl MqttBridge {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            config: Arc::new(Mutex::new(MqttConfig::default())),
            status: Arc::new(Mutex::new(MqttStatus::default())),
            client: Arc::new(Mutex::new(None)),
            worker: Mutex::new(None),
            stop_flag: Arc::new(Mutex::new(false)),
            discovery_published: Arc::new(Mutex::new(HashMap::new())),
            connection_manager,
        }
    }

    /// Get the current configuration (clone — cheap).
    pub fn get_config(&self) -> MqttConfig {
        self.config.lock().unwrap().clone()
    }

    /// Get the current live status.
    pub fn get_status(&self) -> MqttStatus {
        self.status.lock().unwrap().clone()
    }

    /// Apply a new configuration. If the bridge was previously running, it
    /// is stopped and restarted with the new settings. If the new config
    /// has `enabled = false`, the bridge stops and stays stopped.
    pub fn apply_config(&self, mut new_config: MqttConfig) -> Result<(), String> {
        // Defensive: empty topic prefixes would produce malformed topics like
        // `/<id>/<cap>/set`. Trim and fall back to defaults if empty.
        let trimmed_base = new_config.base_topic.trim().trim_end_matches('/').to_string();
        if trimmed_base.is_empty() {
            new_config.base_topic = default_base_topic();
        } else {
            new_config.base_topic = trimmed_base;
        }
        let trimmed_ha = new_config
            .ha_discovery_prefix
            .trim()
            .trim_end_matches('/')
            .to_string();
        if trimmed_ha.is_empty() {
            new_config.ha_discovery_prefix = default_ha_prefix();
        } else {
            new_config.ha_discovery_prefix = trimmed_ha;
        }
        if new_config.client_id.trim().is_empty() {
            new_config.client_id = default_client_id();
        }
        if new_config.broker_port == 0 {
            new_config.broker_port = 1883;
        }

        // Stop any running worker first
        self.stop();

        // Persist the new config in memory
        *self.config.lock().unwrap() = new_config.clone();
        // Reset discovery tracking — new broker means new HA instance, republish
        self.discovery_published.lock().unwrap().clear();

        if new_config.enabled {
            self.start()?;
        } else {
            let mut s = self.status.lock().unwrap();
            s.enabled = false;
            s.connected = false;
            s.last_error = None;
        }
        Ok(())
    }

    /// Start the worker thread that owns the rumqttc Client + EventLoop.
    /// No-op if a worker is already running.
    fn start(&self) -> Result<(), String> {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return Ok(());
        }

        let mut opts = MqttOptions::new(&cfg.client_id, &cfg.broker_host, cfg.broker_port);
        opts.set_keep_alive(Duration::from_secs(30));
        if !cfg.username.is_empty() {
            opts.set_credentials(&cfg.username, &cfg.password);
        }

        let availability_topic = format!("{}/{}", cfg.base_topic, BRIDGE_AVAILABILITY_SUFFIX);
        opts.set_last_will(LastWill::new(
            &availability_topic,
            PAYLOAD_OFFLINE,
            QoS::AtLeastOnce,
            true, // retain
        ));

        let (client, connection) = Client::new(opts, 50);

        // Stash the client so other code can publish through it
        *self.client.lock().unwrap() = Some(client.clone());
        *self.stop_flag.lock().unwrap() = false;

        // Mark availability online (retained) once connected
        let avail_clone = availability_topic.clone();
        let _ = client.publish(&avail_clone, QoS::AtLeastOnce, true, PAYLOAD_ONLINE);

        // Subscribe to the wildcard set topic so we can route inbound commands.
        // Pattern: <base_topic>/+/+/set  -- device_id/cap_id/set
        //
        // We don't need a separate subscription for the HA discovery namespace:
        // the discovery configs we publish set `command_topic` to the plain
        // Trellis topic above, so HA itself publishes to the trellis namespace.
        let set_pattern = format!("{}/+/+/set", cfg.base_topic);
        if let Err(e) = client.subscribe(&set_pattern, QoS::AtLeastOnce) {
            log::warn!("[MQTT] Failed to subscribe to {}: {}", set_pattern, e);
        }

        let status = self.status.clone();
        let stop_flag = self.stop_flag.clone();
        let config_for_worker = self.config.clone();
        let conn_mgr = self.connection_manager.clone();

        {
            let mut s = status.lock().unwrap();
            s.enabled = true;
            s.connected = false;
            s.last_error = None;
        }

        let handle = thread::spawn(move || {
            event_loop(connection, status, stop_flag, config_for_worker, conn_mgr);
        });

        *self.worker.lock().unwrap() = Some(handle);
        Ok(())
    }

    /// Stop the bridge worker, publish the offline availability message, and
    /// drop the client. Safe to call when not running.
    pub fn stop(&self) {
        // Tell the worker to exit
        *self.stop_flag.lock().unwrap() = true;

        // Publish offline availability if we still have a client
        if let Some(client) = self.client.lock().unwrap().as_ref() {
            let cfg = self.config.lock().unwrap().clone();
            let availability_topic = format!("{}/{}", cfg.base_topic, BRIDGE_AVAILABILITY_SUFFIX);
            let _ = client.publish(&availability_topic, QoS::AtLeastOnce, true, PAYLOAD_OFFLINE);
            // Disconnect cleanly so the broker doesn't have to wait for the LWT
            let _ = client.disconnect();
        }
        *self.client.lock().unwrap() = None;

        if let Some(handle) = self.worker.lock().unwrap().take() {
            // Don't block forever — the worker should exit on next iteration
            let _ = handle.join();
        }

        let mut s = self.status.lock().unwrap();
        s.connected = false;
    }

    /// Publish a state update for one capability of one device. Called by
    /// `connection.rs` whenever a `device-event` with an `update` payload
    /// is received from the device's WebSocket.
    pub fn publish_state(&self, device_id: &str, capability_id: &str, value: &Value) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };

        let payload = value_to_mqtt_payload(value);

        // Plain Trellis state topic
        let trellis_topic = format!("{}/{}/{}/state", cfg.base_topic, device_id, capability_id);
        if let Err(e) = client.publish(&trellis_topic, QoS::AtLeastOnce, true, payload.clone()) {
            log::warn!("[MQTT] publish {} failed: {}", trellis_topic, e);
        } else {
            self.status.lock().unwrap().messages_published += 1;
        }
    }

    /// Publish HA discovery configs for every capability of a device. Called
    /// when a device first appears (or its capability list changes). The
    /// payloads cause Home Assistant to create entities automatically.
    pub fn publish_discovery(&self, device: &Device) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };

        // De-dupe: only republish if the capability list changed
        {
            let mut tracker = self.discovery_published.lock().unwrap();
            let new_caps: Vec<String> = device.capabilities.iter().map(|c| c.id.clone()).collect();
            if let Some(existing) = tracker.get(&device.id) {
                if *existing == new_caps {
                    return;
                }
            }
            tracker.insert(device.id.clone(), new_caps);
        }

        let availability_topic = format!("{}/{}", cfg.base_topic, BRIDGE_AVAILABILITY_SUFFIX);

        for cap in &device.capabilities {
            let component = match cap.cap_type.as_str() {
                "switch" => "switch",
                "slider" => "number",
                "sensor" => "sensor",
                "color" => "light",
                "text" => "text",
                _ => continue,
            };

            // unique_id is the entity identity. We also use it as the discovery
            // config topic identifier — MQTT topics permit dashes, so no
            // sanitization is needed even though device IDs from the firmware
            // contain dashes (e.g. "trellis-fccfb7c8").
            let unique_id = format!("trellis_{}_{}", device.id, cap.id);
            let config_topic = format!(
                "{}/{}/{}/config",
                cfg.ha_discovery_prefix, component, unique_id
            );
            let state_topic = format!("{}/{}/{}/state", cfg.base_topic, device.id, cap.id);
            let command_topic = format!("{}/{}/{}/set", cfg.base_topic, device.id, cap.id);

            let mut config = serde_json::json!({
                "name": cap.label,
                "unique_id": unique_id,
                "state_topic": state_topic,
                "availability_topic": availability_topic,
                "payload_available": PAYLOAD_ONLINE,
                "payload_not_available": PAYLOAD_OFFLINE,
                "device": {
                    "identifiers": [format!("trellis_{}", device.id)],
                    "name": device.name,
                    "manufacturer": "Trellis",
                    "model": device.platform,
                    "sw_version": device.firmware,
                },
            });

            // Component-specific fields
            match cap.cap_type.as_str() {
                "switch" => {
                    config["command_topic"] = command_topic.into();
                    config["payload_on"] = "true".into();
                    config["payload_off"] = "false".into();
                    config["state_on"] = "true".into();
                    config["state_off"] = "false".into();
                }
                "slider" => {
                    config["command_topic"] = command_topic.into();
                    if let Some(min) = cap.min {
                        config["min"] = min.into();
                    }
                    if let Some(max) = cap.max {
                        config["max"] = max.into();
                    }
                    config["mode"] = "slider".into();
                }
                "sensor" => {
                    if let Some(unit) = &cap.unit {
                        config["unit_of_measurement"] = unit.clone().into();
                    }
                }
                "color" => {
                    config["command_topic"] = command_topic.into();
                    config["schema"] = "json".into();
                    config["supported_color_modes"] = serde_json::json!(["rgb"]);
                }
                "text" => {
                    config["command_topic"] = command_topic.into();
                    config["mode"] = "text".into();
                }
                _ => {}
            }

            let payload = serde_json::to_string(&config).unwrap_or_default();
            if let Err(e) =
                client.publish(&config_topic, QoS::AtLeastOnce, true, payload.into_bytes())
            {
                log::warn!("[MQTT] discovery publish {} failed: {}", config_topic, e);
            } else {
                self.status.lock().unwrap().messages_published += 1;
            }
        }
    }

    /// Remove a device from the HA discovery tracker so the next call to
    /// `publish_discovery` will re-emit the configs (used when a device
    /// disappears or its firmware is updated).
    pub fn forget_discovery(&self, device_id: &str) {
        self.discovery_published.lock().unwrap().remove(device_id);
    }

    /// Test connectivity to a broker without applying it as the active config.
    /// Used by the Settings page "Test connection" button.
    pub fn test_connection(&self, cfg: &MqttConfig) -> Result<(), String> {
        let mut opts = MqttOptions::new(
            format!("{}-test", cfg.client_id),
            &cfg.broker_host,
            cfg.broker_port,
        );
        opts.set_keep_alive(Duration::from_secs(5));
        if !cfg.username.is_empty() {
            opts.set_credentials(&cfg.username, &cfg.password);
        }

        let (client, mut connection) = Client::new(opts, 10);

        // Drain a few events to see if we get a ConnAck
        for _ in 0..10 {
            match connection.recv_timeout(Duration::from_secs(2)) {
                Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                    let _ = client.disconnect();
                    return Ok(());
                }
                Ok(Ok(_)) => continue,
                Ok(Err(e)) => return Err(format!("MQTT error: {}", e)),
                Err(_) => return Err("Timed out waiting for broker".to_string()),
            }
        }
        Err("No ConnAck received".to_string())
    }
}

/// Worker thread function: drains the rumqttc event loop, dispatches inbound
/// commands to the ConnectionManager, and updates the live status.
fn event_loop(
    mut connection: MqttConnection,
    status: Arc<Mutex<MqttStatus>>,
    stop_flag: Arc<Mutex<bool>>,
    config: Arc<Mutex<MqttConfig>>,
    conn_mgr: Arc<ConnectionManager>,
) {
    log::info!("[MQTT] Worker started");
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }
        match connection.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                log::info!("[MQTT] Connected to broker");
                let mut s = status.lock().unwrap();
                s.connected = true;
                s.last_error = None;
            }
            Ok(Ok(Event::Incoming(Packet::Publish(p)))) => {
                let topic = p.topic.clone();
                let payload = String::from_utf8_lossy(&p.payload).to_string();
                status.lock().unwrap().messages_received += 1;
                let cfg = config.lock().unwrap().clone();
                handle_inbound(&topic, &payload, &cfg, &conn_mgr);
            }
            Ok(Ok(Event::Incoming(Packet::Disconnect))) => {
                log::warn!("[MQTT] Broker sent disconnect");
                status.lock().unwrap().connected = false;
            }
            Ok(Ok(_)) => {}
            Ok(Err(e)) => {
                log::warn!("[MQTT] Connection error: {}", e);
                let mut s = status.lock().unwrap();
                s.connected = false;
                s.last_error = Some(e.to_string());
                // Brief pause before next iteration to avoid spinning
                thread::sleep(Duration::from_secs(2));
            }
            Err(_) => {
                // Timeout — check stop_flag and loop
                continue;
            }
        }
    }
    log::info!("[MQTT] Worker stopped");
}

/// Route an inbound MQTT message to the appropriate Trellis device. We only
/// recognize the plain Trellis topic shape:
///   <base_topic>/<device_id>/<cap_id>/set
/// `base_topic` may contain slashes (e.g. "home/iot/trellis"), so we use
/// prefix-stripping rather than naive segment counting.
fn handle_inbound(
    topic: &str,
    payload: &str,
    cfg: &MqttConfig,
    conn_mgr: &ConnectionManager,
) {
    let plain_prefix = format!("{}/", cfg.base_topic);
    if let Some(rest) = topic.strip_prefix(&plain_prefix) {
        if let Some(without_set) = rest.strip_suffix("/set") {
            let parts: Vec<&str> = without_set.split('/').collect();
            if parts.len() == 2 && !parts[0].is_empty() && !parts[1].is_empty() {
                dispatch_set(parts[0], parts[1], payload, conn_mgr);
                return;
            }
        }
    }
    log::debug!("[MQTT] Unhandled topic: {}", topic);
}

/// Build a Trellis WS `set` command from an MQTT payload and send it via
/// ConnectionManager.
fn dispatch_set(
    device_id: &str,
    cap_id: &str,
    payload: &str,
    conn_mgr: &ConnectionManager,
) {
    // Parse the payload — bool, number, or string. We accept all three.
    let value: Value = if payload == "true" || payload == "false" {
        Value::Bool(payload == "true")
    } else if let Ok(n) = payload.parse::<f64>() {
        serde_json::json!(n)
    } else if let Ok(parsed) = serde_json::from_str::<Value>(payload) {
        parsed
    } else {
        Value::String(payload.to_string())
    };

    let cmd = serde_json::json!({
        "command": "set",
        "id": cap_id,
        "value": value,
    });
    let msg = serde_json::to_string(&cmd).unwrap_or_default();

    // We don't have ip/port at hand, but send_to_device prefers the
    // persistent connection by device_id and only needs ip/port for the
    // fallback path. Pass empty strings — they're unused on the happy path.
    if let Err(e) = conn_mgr.send_to_device(device_id, "", 0, &msg) {
        log::warn!(
            "[MQTT] Failed to dispatch {}={} to {}: {}",
            cap_id,
            payload,
            device_id,
            e
        );
    } else {
        log::info!("[MQTT] {} {}={}", device_id, cap_id, payload);
    }
}

/// Convert a JSON value into the textual MQTT payload Trellis publishes.
/// Booleans become `"true"`/`"false"`, numbers become their string form,
/// strings are passed through, anything else is JSON-encoded.
fn value_to_mqtt_payload(value: &Value) -> Vec<u8> {
    match value {
        Value::Bool(b) => b.to_string().into_bytes(),
        Value::Number(n) => n.to_string().into_bytes(),
        Value::String(s) => s.clone().into_bytes(),
        other => serde_json::to_string(other).unwrap_or_default().into_bytes(),
    }
}
