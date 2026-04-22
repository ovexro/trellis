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

use rumqttc::{Client, Connection as MqttConnection, Event, LastWill, MqttOptions, Packet, QoS, Transport};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::connection::ConnectionManager;
use crate::device::Device;
use crate::discovery::Discovery;

const DEFAULT_BASE_TOPIC: &str = "trellis";
const DEFAULT_HA_PREFIX: &str = "homeassistant";
const BRIDGE_AVAILABILITY_SUFFIX: &str = "bridge/availability";
const PAYLOAD_ONLINE: &str = "online";
const PAYLOAD_OFFLINE: &str = "offline";

/// Persisted MQTT bridge configuration. Stored as JSON in the existing
/// `settings` table under key `mqtt_config`. The password field is encrypted
/// at rest with `enc:v1:` prefix (see secret_store.rs); all other fields are
/// plaintext for inspectability.
///
/// Every field has a serde default so partial JSON payloads (e.g. from the
/// REST API or older saved configs missing newer fields) deserialize
/// cleanly into the defaults. This is what makes adding new fields like
/// `tls_enabled` safe — old saved configs from pre-TLS builds parse fine
/// with `tls_enabled = false`.
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
    /// When true, the bridge connects to the broker over TLS (`mqtts://`).
    /// Defaults to plaintext (`mqtt://`) for backwards compatibility with
    /// existing local-broker setups. Most public brokers and any broker
    /// reachable over an untrusted network should have this on.
    #[serde(default)]
    pub tls_enabled: bool,
    /// Optional path to a PEM-encoded CA certificate. When `Some`, rustls
    /// uses ONLY this CA to verify the broker. When `None`, rustls uses the
    /// system trust roots (the same trust store the OS browser uses), which
    /// is the right choice for any broker with a publicly-issued cert. For
    /// self-signed brokers, point this at either the broker's own cert or
    /// the CA that signed it — both work because rustls just builds a chain
    /// to a trusted anchor.
    #[serde(default)]
    pub tls_ca_cert_path: Option<String>,
    /// When true, TLS certificate verification is completely disabled:
    /// expired certs, wrong hostnames, self-signed certs, and even invalid
    /// chains are all accepted. This is deliberately insecure — it's the
    /// MQTT equivalent of `curl -k`. Use it only when you control the
    /// network path to the broker and can't be bothered to supply a CA file
    /// (e.g. a self-signed Mosquitto on your own LAN). Ignored when
    /// `tls_enabled` is false.
    #[serde(default)]
    pub tls_skip_verify: bool,
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
            tls_enabled: false,
            tls_ca_cert_path: None,
            tls_skip_verify: false,
        }
    }
}

/// Public-facing view of `MqttConfig` returned by GET endpoints (Tauri
/// command + REST API). The password is intentionally omitted from the wire
/// shape to avoid leaking it over the LAN — the REST API binds to
/// 0.0.0.0:9090, so anything serialized into a GET response is visible to
/// anyone on the same network. The `has_password` flag tells the UI whether
/// a password is currently stored, so it can show "(unchanged — type to
/// update)" instead of "(none)".
///
/// TLS settings are returned as-is — neither tls_enabled nor the CA file
/// path are sensitive (the CA path is just a filesystem location, and
/// tls_enabled is operational state visible to anyone watching the broker).
#[derive(Debug, Clone, Serialize)]
pub struct MqttConfigPublic {
    pub enabled: bool,
    pub broker_host: String,
    pub broker_port: u16,
    pub username: String,
    pub base_topic: String,
    pub ha_discovery_prefix: String,
    pub ha_discovery_enabled: bool,
    pub client_id: String,
    pub has_password: bool,
    pub tls_enabled: bool,
    pub tls_ca_cert_path: Option<String>,
    pub tls_skip_verify: bool,
}

impl From<&MqttConfig> for MqttConfigPublic {
    fn from(c: &MqttConfig) -> Self {
        Self {
            enabled: c.enabled,
            broker_host: c.broker_host.clone(),
            broker_port: c.broker_port,
            username: c.username.clone(),
            base_topic: c.base_topic.clone(),
            ha_discovery_prefix: c.ha_discovery_prefix.clone(),
            ha_discovery_enabled: c.ha_discovery_enabled,
            client_id: c.client_id.clone(),
            has_password: !c.password.is_empty(),
            tls_enabled: c.tls_enabled,
            tls_ca_cert_path: c.tls_ca_cert_path.clone(),
            tls_skip_verify: c.tls_skip_verify,
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
    /// Set after construction (avoids a circular new() arg). Used by polish #1
    /// (instant discovery on enable) and polish #2 (republish on broker
    /// reconnect) to look up the live device list.
    discovery: Arc<Mutex<Option<Arc<Discovery>>>>,
    /// Nameplate watts cache keyed by (device_id, capability_id). Hydrated
    /// from the `capability_meta` table once at startup and kept in sync via
    /// `set_watts` when the user edits watts through the Tauri command or
    /// REST endpoint. Used on the hot path (`publish_state`) to emit a
    /// per-switch HA `sensor.<...>_power` entity without touching the DB on
    /// every state transition. Only switches with a set nameplate appear
    /// here.
    watts_map: Arc<Mutex<HashMap<(String, String), f64>>>,
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
            discovery: Arc::new(Mutex::new(None)),
            watts_map: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_discovery(&self, discovery: Arc<Discovery>) {
        *self.discovery.lock().unwrap() = Some(discovery);
    }

    /// Replace the in-memory watts cache from a freshly-loaded DB snapshot.
    /// Called once at startup (before the first `apply_config` / discovery
    /// publish) so HA sees `sensor.<cap>_power` entities for every
    /// already-metered switch the moment the broker connects.
    pub fn hydrate_watts(&self, entries: Vec<(String, String, f64)>) {
        let mut map = self.watts_map.lock().unwrap();
        map.clear();
        for (device_id, cap_id, watts) in entries {
            map.insert((device_id, cap_id), watts);
        }
    }

    /// Update the watts cache for a single capability and re-emit HA
    /// discovery for its device so the `sensor.<cap>_power` entity appears
    /// (when Some) or is removed (when None) without a bridge restart.
    ///
    /// When watts are cleared we publish a zero-length retained payload to
    /// the `sensor.<...>_power` discovery config topic, which is HA's
    /// idiomatic way of asking it to remove the entity. The primary switch
    /// entity and the three diagnostic system sensors are unaffected.
    pub fn set_watts(&self, device_id: &str, capability_id: &str, watts: Option<f64>) {
        let key = (device_id.to_string(), capability_id.to_string());
        match watts {
            Some(w) => {
                self.watts_map.lock().unwrap().insert(key, w);
            }
            None => {
                self.watts_map.lock().unwrap().remove(&key);
                // Actively remove the stale HA entity (retain='' on the
                // discovery config topic).
                let cfg = self.config.lock().unwrap().clone();
                if cfg.enabled && cfg.ha_discovery_enabled {
                    let client = self.client.lock().unwrap().as_ref().cloned();
                    if let Some(client) = client {
                        let unique_id =
                            format!("trellis_{}_{}_power", device_id, capability_id);
                        let config_topic = format!(
                            "{}/sensor/{}/config",
                            cfg.ha_discovery_prefix, unique_id
                        );
                        let _ = client.publish(
                            &config_topic,
                            QoS::AtLeastOnce,
                            true,
                            Vec::<u8>::new(),
                        );
                    }
                }
            }
        }
        // The dedupe tracker keys off the cap list, which is unchanged — so
        // force a re-publish by dropping this device from the tracker first.
        self.forget_discovery(device_id);
        let device = match self.discovery.lock().unwrap().as_ref() {
            Some(d) => d.get_devices().into_iter().find(|d| d.id == device_id),
            None => None,
        };
        if let Some(device) = device {
            self.publish_discovery(&device);
        }
    }

    /// Get the current configuration (clone — cheap).
    /// **Internal use only.** Contains the plaintext password — never serialize
    /// this directly to a network-facing endpoint. Use `get_config_public`
    /// for anything user-visible.
    pub fn get_config(&self) -> MqttConfig {
        self.config.lock().unwrap().clone()
    }

    /// Get a network-safe view of the current configuration. The password is
    /// stripped and replaced by a `has_password: bool` flag. Safe to return
    /// from any GET endpoint, including the LAN-exposed REST API on
    /// 0.0.0.0:9090.
    pub fn get_config_public(&self) -> MqttConfigPublic {
        MqttConfigPublic::from(&*self.config.lock().unwrap())
    }

    /// Merge an incoming config with the in-memory config such that an empty
    /// `password` field in the incoming side means "preserve the existing
    /// password" rather than "blank it out". This is the counterpart to the
    /// password-redaction in GET responses: the UI loads the config without
    /// the password, the user edits other fields, and a save round-trip would
    /// otherwise wipe the stored password. Empty-means-preserve fixes that.
    ///
    /// To explicitly clear a password, callers must use `clear_password()`
    /// rather than submitting an empty string here.
    fn merge_preserving_password(&self, mut incoming: MqttConfig) -> MqttConfig {
        if incoming.password.is_empty() {
            let existing = self.config.lock().unwrap().password.clone();
            if !existing.is_empty() {
                incoming.password = existing;
            }
        }
        incoming
    }

    /// Explicitly clear the stored MQTT broker password. Used by the
    /// Settings UI's "Clear password" button. The bridge is restarted with
    /// the cleared config so the new auth state takes effect immediately.
    pub fn clear_password(&self) -> Result<(), String> {
        let mut cfg = self.config.lock().unwrap().clone();
        cfg.password = String::new();
        self.apply_config(cfg)
    }

    /// Get the current live status.
    pub fn get_status(&self) -> MqttStatus {
        self.status.lock().unwrap().clone()
    }

    /// Apply a new configuration that came from a user-facing endpoint
    /// (Tauri `set_mqtt_config` or REST `PUT /api/settings/mqtt`). An empty
    /// `password` in the incoming config is interpreted as "keep the
    /// existing stored password" rather than "clear it" — see
    /// `merge_preserving_password` for the rationale. To explicitly clear,
    /// the UI calls `clear_password()` instead.
    pub fn apply_config_from_user(&self, new_config: MqttConfig) -> Result<(), String> {
        let merged = self.merge_preserving_password(new_config);
        self.apply_config(merged)
    }

    /// Test connectivity using a user-supplied config. Same preserve-blank
    /// rule as `apply_config_from_user`: if the user didn't re-type the
    /// password, fall back to the stored one so the test exercises the same
    /// auth state the live bridge would use.
    pub fn test_connection_from_user(&self, cfg: MqttConfig) -> Result<(), String> {
        let merged = self.merge_preserving_password(cfg);
        self.test_connection(&merged)
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
            // Polish #1: publish HA discovery configs for all currently-known
            // devices immediately, instead of waiting for the next 30s
            // health-check tick. The first ConnAck inside the worker thread
            // will also trigger a republish via polish #2 — that's idempotent
            // because we clear the dedupe tracker first.
            //
            // Tiny sleep so the rumqttc client has a moment to connect before
            // we start queuing publishes; the publishes still queue regardless
            // but the broker sees them after the ConnAck this way.
            thread::sleep(Duration::from_millis(200));
            self.publish_all_discovery();
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
        if cfg.tls_enabled {
            opts.set_transport(build_tls_transport(&cfg)?);
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
        let discovery_for_worker = self.discovery.clone();
        let client_for_worker = self.client.clone();
        let tracker_for_worker = self.discovery_published.clone();
        let watts_for_worker = self.watts_map.clone();

        {
            let mut s = status.lock().unwrap();
            s.enabled = true;
            s.connected = false;
            s.last_error = None;
        }

        let handle = thread::spawn(move || {
            event_loop(
                connection,
                status,
                stop_flag,
                config_for_worker,
                conn_mgr,
                discovery_for_worker,
                client_for_worker,
                tracker_for_worker,
                watts_for_worker,
            );
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

        // Companion power sensor: if this capability has a nameplate watts
        // value and the update is a bool (only switches publish bools here,
        // sliders publish numbers), mirror it to the `_power` state topic
        // as W when ON, 0 when OFF. HA users see live wattage without the
        // desktop app computing anything — the firmware-side nameplate is
        // the sole source of truth, matching `get_device_energy`'s model.
        let watts = self
            .watts_map
            .lock()
            .unwrap()
            .get(&(device_id.to_string(), capability_id.to_string()))
            .copied();
        if let Some(watts) = watts {
            if let Some(power_payload) = compute_power_payload(value, watts) {
                let power_topic = format!(
                    "{}/{}/{}/_power/state",
                    cfg.base_topic, device_id, capability_id
                );
                if let Err(e) =
                    client.publish(&power_topic, QoS::AtLeastOnce, true, power_payload.into_bytes())
                {
                    log::warn!("[MQTT] power publish {} failed: {}", power_topic, e);
                } else {
                    self.status.lock().unwrap().messages_published += 1;
                }
            }
        }
    }

    /// Publish HA discovery configs for every capability of a device, plus
    /// the three synthetic system sensors (RSSI, free heap, uptime). Called
    /// when a device first appears, when its capability list changes, and
    /// (since polish #2) when the broker reconnects.
    pub fn publish_discovery(&self, device: &Device) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        publish_discovery_for_device(
            device,
            &cfg,
            &client,
            &self.discovery_published,
            &self.status,
            &self.watts_map,
        );
    }

    /// Iterate the live device list (via the wired-in `Discovery` handle)
    /// and publish HA discovery configs for every device. Used by polish #1
    /// (instant discovery on bridge enable) and polish #2 (republish on
    /// broker reconnect — see `event_loop` ConnAck branch).
    pub fn publish_all_discovery(&self) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let devices = match self.discovery.lock().unwrap().as_ref() {
            Some(d) => d.get_devices(),
            None => return,
        };
        // Force a republish (the dedupe tracker would otherwise skip devices
        // we've already announced — but on reconnect / enable we want HA to
        // see fresh configs).
        self.discovery_published.lock().unwrap().clear();
        for device in &devices {
            publish_discovery_for_device(
                device,
                &cfg,
                &client,
                &self.discovery_published,
                &self.status,
                &self.watts_map,
            );
        }
    }

    /// Publish a heartbeat (system telemetry: rssi, heap_free, uptime_s)
    /// to MQTT state topics so HA can graph the device's health. Called by
    /// `connection.rs` whenever a `heartbeat` event arrives over the WS.
    pub fn publish_heartbeat(&self, device_id: &str, system: &Value) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };

        let mut count = 0u64;
        for (field, suffix) in [
            ("rssi", "rssi"),
            ("heap_free", "heap_free"),
            ("uptime_s", "uptime_s"),
        ] {
            let value = match system.get(field) {
                Some(v) => v,
                None => continue,
            };
            let topic = format!("{}/{}/_sys/{}/state", cfg.base_topic, device_id, suffix);
            let payload = value_to_mqtt_payload(value);
            if let Err(e) = client.publish(&topic, QoS::AtMostOnce, false, payload) {
                log::warn!("[MQTT] heartbeat publish {} failed: {}", topic, e);
            } else {
                count += 1;
            }
        }
        if count > 0 {
            self.status.lock().unwrap().messages_published += count;
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
        if cfg.tls_enabled {
            opts.set_transport(build_tls_transport(cfg)?);
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

/// Build a rumqttc `Transport::Tls(...)` from an MqttConfig with TLS enabled.
///
/// - If `tls_ca_cert_path` is None, uses the system trust roots
///   (rustls-native-certs reads /etc/ssl/certs and friends). This is the
///   right choice for any broker with a publicly-issued cert.
/// - If `tls_ca_cert_path` is Some, reads the file as PEM and uses ONLY
///   that CA to verify the broker. Works for self-signed brokers (point
///   at the broker's own cert or its CA) and private PKI setups.
///
/// Returns Err if `tls_enabled` is true but the CA file path is set and
/// the file can't be read or parsed.
fn build_tls_transport(cfg: &MqttConfig) -> Result<Transport, String> {
    if cfg.tls_skip_verify {
        log::warn!("[MQTT] TLS certificate verification DISABLED — connection is encrypted but NOT authenticated");
        let tls_config = tokio_rustls::rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(std::sync::Arc::new(NoVerifier))
            .with_no_client_auth();
        return Ok(Transport::tls_with_config(
            rumqttc::TlsConfiguration::Rustls(std::sync::Arc::new(tls_config)),
        ));
    }

    match &cfg.tls_ca_cert_path {
        None => {
            // System trust roots — what test.mosquitto.org and any public
            // broker need. rumqttc's default config calls
            // `load_native_certs()` internally.
            Ok(Transport::tls_with_default_config())
        }
        Some(path) => {
            let ca_pem = std::fs::read(path).map_err(|e| {
                format!("Failed to read TLS CA cert at {}: {}", path, e)
            })?;
            // Empty file is almost certainly user error — fail loudly
            // rather than letting rustls produce a confusing parse error.
            if ca_pem.is_empty() {
                return Err(format!("TLS CA cert file is empty: {}", path));
            }
            // Transport::tls(ca, client_auth=None, alpn=None) builds a
            // TlsConfiguration::Simple that rustls will parse as PEM.
            Ok(Transport::tls(ca_pem, None, None))
        }
    }
}

/// A rustls ServerCertVerifier that accepts any certificate without validation.
/// Used when `tls_skip_verify` is true — the connection is still encrypted
/// (TLS handshake completes normally) but the broker's identity is not verified.
/// This is the equivalent of `curl --insecure` / Go's `InsecureSkipVerify`.
#[derive(Debug)]
struct NoVerifier;

impl tokio_rustls::rustls::client::danger::ServerCertVerifier for NoVerifier {
    fn verify_server_cert(
        &self,
        _end_entity: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _intermediates: &[tokio_rustls::rustls::pki_types::CertificateDer<'_>],
        _server_name: &tokio_rustls::rustls::pki_types::ServerName<'_>,
        _ocsp_response: &[u8],
        _now: tokio_rustls::rustls::pki_types::UnixTime,
    ) -> Result<
        tokio_rustls::rustls::client::danger::ServerCertVerified,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::ServerCertVerified::assertion())
    }

    fn verify_tls12_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn verify_tls13_signature(
        &self,
        _message: &[u8],
        _cert: &tokio_rustls::rustls::pki_types::CertificateDer<'_>,
        _dss: &tokio_rustls::rustls::DigitallySignedStruct,
    ) -> Result<
        tokio_rustls::rustls::client::danger::HandshakeSignatureValid,
        tokio_rustls::rustls::Error,
    > {
        Ok(tokio_rustls::rustls::client::danger::HandshakeSignatureValid::assertion())
    }

    fn supported_verify_schemes(&self) -> Vec<tokio_rustls::rustls::SignatureScheme> {
        use tokio_rustls::rustls::SignatureScheme;
        vec![
            SignatureScheme::RSA_PKCS1_SHA256,
            SignatureScheme::RSA_PKCS1_SHA384,
            SignatureScheme::RSA_PKCS1_SHA512,
            SignatureScheme::ECDSA_NISTP256_SHA256,
            SignatureScheme::ECDSA_NISTP384_SHA384,
            SignatureScheme::RSA_PSS_SHA256,
            SignatureScheme::RSA_PSS_SHA384,
            SignatureScheme::RSA_PSS_SHA512,
            SignatureScheme::ED25519,
        ]
    }
}

/// Build and publish HA discovery configs for one device's regular
/// capabilities AND its synthetic system telemetry sensors (rssi, heap, uptime).
/// Free function so both `MqttBridge::publish_discovery` (per-device) and
/// `MqttBridge::publish_all_discovery` (bulk on enable / reconnect) can call
/// it without re-locking the bridge state.
fn publish_discovery_for_device(
    device: &Device,
    cfg: &MqttConfig,
    client: &Client,
    discovery_published: &Mutex<HashMap<String, Vec<String>>>,
    status: &Mutex<MqttStatus>,
    watts_map: &Mutex<HashMap<(String, String), f64>>,
) {
    // De-dupe: only republish if the capability list changed. Callers that
    // want a forced republish (broker reconnect, bridge enable) clear the
    // tracker before calling us, so the first iteration always proceeds.
    {
        let mut tracker = discovery_published.lock().unwrap();
        let new_caps: Vec<String> = device.capabilities.iter().map(|c| c.id.clone()).collect();
        if let Some(existing) = tracker.get(&device.id) {
            if *existing == new_caps {
                return;
            }
        }
        tracker.insert(device.id.clone(), new_caps);
    }

    let availability_topic = format!("{}/{}", cfg.base_topic, BRIDGE_AVAILABILITY_SUFFIX);
    let device_block = serde_json::json!({
        "identifiers": [format!("trellis_{}", device.id)],
        "name": device.name,
        "manufacturer": "Trellis",
        "model": device.platform,
        "sw_version": device.firmware,
    });
    let mut published_count = 0u64;

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
            "device": device_block,
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
            published_count += 1;
        }
    }

    // Polish #3: synthetic system telemetry sensors. Each device gets three
    // extra HA sensor entities (RSSI, free heap, uptime) so HA users can graph
    // device health and trigger alerts on weak signal / low memory.
    for (suffix, friendly, unit, device_class, state_class) in [
        ("rssi", "Signal strength", Some("dBm"), Some("signal_strength"), Some("measurement")),
        ("heap_free", "Free heap", Some("B"), None, Some("measurement")),
        ("uptime_s", "Uptime", Some("s"), Some("duration"), Some("total_increasing")),
    ] {
        let unique_id = format!("trellis_{}_sys_{}", device.id, suffix);
        let config_topic = format!(
            "{}/sensor/{}/config",
            cfg.ha_discovery_prefix, unique_id
        );
        let state_topic = format!("{}/{}/_sys/{}/state", cfg.base_topic, device.id, suffix);

        let mut config = serde_json::json!({
            "name": friendly,
            "unique_id": unique_id,
            "state_topic": state_topic,
            "availability_topic": availability_topic,
            "payload_available": PAYLOAD_ONLINE,
            "payload_not_available": PAYLOAD_OFFLINE,
            "device": device_block,
            "entity_category": "diagnostic",
        });
        if let Some(u) = unit {
            config["unit_of_measurement"] = u.into();
        }
        if let Some(dc) = device_class {
            config["device_class"] = dc.into();
        }
        if let Some(sc) = state_class {
            config["state_class"] = sc.into();
        }

        let payload = serde_json::to_string(&config).unwrap_or_default();
        if let Err(e) =
            client.publish(&config_topic, QoS::AtLeastOnce, true, payload.into_bytes())
        {
            log::warn!("[MQTT] system sensor publish {} failed: {}", config_topic, e);
        } else {
            published_count += 1;
        }
    }

    // Per-switch power sensor: one `sensor.<cap>_power` entity per switch
    // that has a nameplate watts value set. The primary switch entity is
    // still published above; this is an additive companion sensor so HA
    // users get a live power readout (device_class=power, unit=W) and can
    // graph/alert on it. State topic is published from `publish_state`
    // whenever the switch toggles (W when ON, 0 when OFF).
    let watts_for_device: Vec<(String, f64)> = {
        let map = watts_map.lock().unwrap();
        device
            .capabilities
            .iter()
            .filter(|c| c.cap_type == "switch")
            .filter_map(|c| {
                map.get(&(device.id.clone(), c.id.clone()))
                    .map(|w| (c.id.clone(), *w))
            })
            .collect()
    };
    for (cap_id, _watts) in &watts_for_device {
        let cap_label = device
            .capabilities
            .iter()
            .find(|c| c.id == *cap_id)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| cap_id.clone());

        let unique_id = format!("trellis_{}_{}_power", device.id, cap_id);
        let config_topic =
            format!("{}/sensor/{}/config", cfg.ha_discovery_prefix, unique_id);
        let state_topic =
            format!("{}/{}/{}/_power/state", cfg.base_topic, device.id, cap_id);

        let config = serde_json::json!({
            "name": format!("{} Power", cap_label),
            "unique_id": unique_id,
            "state_topic": state_topic,
            "availability_topic": availability_topic,
            "payload_available": PAYLOAD_ONLINE,
            "payload_not_available": PAYLOAD_OFFLINE,
            "device": device_block,
            "device_class": "power",
            "state_class": "measurement",
            "unit_of_measurement": "W",
        });

        let payload = serde_json::to_string(&config).unwrap_or_default();
        if let Err(e) =
            client.publish(&config_topic, QoS::AtLeastOnce, true, payload.into_bytes())
        {
            log::warn!("[MQTT] power sensor config publish {} failed: {}", config_topic, e);
        } else {
            published_count += 1;
        }

        // Initial retained state so HA has a value before the first toggle.
        // Conservative default is "0" (OFF) — matches `get_device_energy`'s
        // bootstrap assumption and avoids a false "ON" spike if HA attaches
        // before the first real transition.
        if let Err(e) = client.publish(&state_topic, QoS::AtLeastOnce, true, "0".as_bytes()) {
            log::warn!("[MQTT] power sensor state publish {} failed: {}", state_topic, e);
        } else {
            published_count += 1;
        }
    }

    if published_count > 0 {
        status.lock().unwrap().messages_published += published_count;
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
    discovery: Arc<Mutex<Option<Arc<Discovery>>>>,
    client: Arc<Mutex<Option<Client>>>,
    discovery_published: Arc<Mutex<HashMap<String, Vec<String>>>>,
    watts_map: Arc<Mutex<HashMap<(String, String), f64>>>,
) {
    log::info!("[MQTT] Worker started");
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }
        match connection.recv_timeout(Duration::from_secs(1)) {
            Ok(Ok(Event::Incoming(Packet::ConnAck(_)))) => {
                log::info!("[MQTT] Connected to broker");
                {
                    let mut s = status.lock().unwrap();
                    s.connected = true;
                    s.last_error = None;
                }
                // Polish #4 (post-v0.2.0): republish the retained `online`
                // availability message on every ConnAck. The broker fires
                // our LWT (offline) when it sees the TCP drop on a
                // restart/network blip. rumqttc reconnects under us and the
                // bridge keeps publishing state, but the availability topic
                // would still read `offline` until something forced a
                // republish. HA marks every entity unavailable when that
                // happens. Re-asserting `online` here keeps entities live
                // across broker hiccups.
                let cfg_snapshot = config.lock().unwrap().clone();
                if let Some(c) = client.lock().unwrap().as_ref() {
                    // Polish #4: republish retained `online` availability so
                    // entities don't stay marked unavailable after a broker
                    // restart that fired our LWT.
                    let availability_topic =
                        format!("{}/{}", cfg_snapshot.base_topic, BRIDGE_AVAILABILITY_SUFFIX);
                    if let Err(e) = c.publish(
                        &availability_topic,
                        QoS::AtLeastOnce,
                        true,
                        PAYLOAD_ONLINE,
                    ) {
                        log::warn!("[MQTT] availability republish failed: {}", e);
                    }
                    // Polish #5: re-subscribe to the command topic. rumqttc
                    // does NOT automatically replay subscriptions across its
                    // internal reconnects, so a broker restart leaves us
                    // connected but deaf — HA toggles never reach the device.
                    // Re-asserting the subscription on every ConnAck is cheap
                    // and idempotent.
                    let set_pattern = format!("{}/+/+/set", cfg_snapshot.base_topic);
                    if let Err(e) = c.subscribe(&set_pattern, QoS::AtLeastOnce) {
                        log::warn!(
                            "[MQTT] resubscribe {} failed: {}",
                            set_pattern,
                            e
                        );
                    }
                }
                // Polish #2: republish HA discovery configs whenever the
                // broker accepts a fresh connection. Handles broker restarts
                // (where retained configs were lost) and reconnects after
                // transient network drops. Idempotent — discovery_published
                // is cleared first so even tracked devices get re-announced.
                if cfg_snapshot.enabled && cfg_snapshot.ha_discovery_enabled {
                    let devices = discovery
                        .lock()
                        .unwrap()
                        .as_ref()
                        .map(|d| d.get_devices())
                        .unwrap_or_default();
                    if !devices.is_empty() {
                        if let Some(c) = client.lock().unwrap().as_ref() {
                            discovery_published.lock().unwrap().clear();
                            for device in &devices {
                                publish_discovery_for_device(
                                    device,
                                    &cfg_snapshot,
                                    c,
                                    &discovery_published,
                                    &status,
                                    &watts_map,
                                );
                            }
                            log::info!(
                                "[MQTT] Republished discovery for {} device(s) on reconnect",
                                devices.len()
                            );
                        }
                    }
                }
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

/// Format a watts value for the `_power/state` topic: integer-like when the
/// value is a whole number (avoids HA parsing "60.0" into a float graph axis
/// with a trailing ".0"), otherwise trimmed to 2 decimals.
fn format_watts_payload(watts: f64) -> String {
    if (watts - watts.round()).abs() < 1e-9 {
        format!("{}", watts.round() as i64)
    } else {
        format!("{:.2}", watts)
    }
}

/// Pure helper for the companion `_power/state` payload. Returns `None` if
/// the capability's current value is not a boolean (non-switch updates —
/// sliders and sensors — never drive a power sensor because we only
/// nameplate switches). Returns `Some("<watts>")` when ON and `Some("0")`
/// when OFF.
fn compute_power_payload(value: &Value, watts: f64) -> Option<String> {
    let on = value.as_bool()?;
    Some(format_watts_payload(if on { watts } else { 0.0 }))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::ConnectionManager;

    #[test]
    fn format_watts_payload_whole_number() {
        assert_eq!(format_watts_payload(60.0), "60");
        assert_eq!(format_watts_payload(1500.0), "1500");
        assert_eq!(format_watts_payload(0.0), "0");
    }

    #[test]
    fn format_watts_payload_decimal() {
        assert_eq!(format_watts_payload(12.5), "12.50");
        assert_eq!(format_watts_payload(0.33), "0.33");
        // Source is not a whole number so we stay on the 2-decimal branch
        // even though the formatter rounds it to 100.00 — HA parses that
        // the same as 100. Accepting it avoids flickering between "99" and
        // "100" for near-whole inputs.
        assert_eq!(format_watts_payload(99.999), "100.00");
    }

    #[test]
    fn compute_power_on_off() {
        assert_eq!(
            compute_power_payload(&serde_json::Value::Bool(true), 60.0),
            Some("60".to_string())
        );
        assert_eq!(
            compute_power_payload(&serde_json::Value::Bool(false), 60.0),
            Some("0".to_string())
        );
    }

    #[test]
    fn compute_power_skips_non_bool() {
        // Sliders publish numeric updates; sensors publish numbers; neither
        // should drive the switch power sensor.
        assert_eq!(
            compute_power_payload(&serde_json::json!(42), 60.0),
            None
        );
        assert_eq!(
            compute_power_payload(&serde_json::json!("on"), 60.0),
            None
        );
        assert_eq!(
            compute_power_payload(&serde_json::Value::Null, 60.0),
            None
        );
    }

    #[test]
    fn hydrate_and_set_watts_update_map() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge.hydrate_watts(vec![
            ("dev1".to_string(), "led".to_string(), 60.0),
            ("dev1".to_string(), "heater".to_string(), 1500.0),
        ]);
        {
            let map = bridge.watts_map.lock().unwrap();
            assert_eq!(map.len(), 2);
            assert_eq!(
                map.get(&("dev1".to_string(), "led".to_string())).copied(),
                Some(60.0)
            );
        }

        // Setter updates an existing entry.
        bridge.set_watts("dev1", "led", Some(75.0));
        assert_eq!(
            bridge
                .watts_map
                .lock()
                .unwrap()
                .get(&("dev1".to_string(), "led".to_string()))
                .copied(),
            Some(75.0)
        );

        // Clearing to None removes the entry entirely so the discovery
        // loop stops emitting a power sensor for that cap.
        bridge.set_watts("dev1", "led", None);
        assert!(!bridge
            .watts_map
            .lock()
            .unwrap()
            .contains_key(&("dev1".to_string(), "led".to_string())));
        // Other caps untouched.
        assert_eq!(
            bridge
                .watts_map
                .lock()
                .unwrap()
                .get(&("dev1".to_string(), "heater".to_string()))
                .copied(),
            Some(1500.0)
        );
    }

    #[test]
    fn hydrate_watts_replaces_existing() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge.hydrate_watts(vec![(
            "old-device".to_string(),
            "cap".to_string(),
            10.0,
        )]);
        // Fresh hydrate (e.g. on re-init) wipes the old contents.
        bridge.hydrate_watts(vec![(
            "new-device".to_string(),
            "cap".to_string(),
            20.0,
        )]);
        let map = bridge.watts_map.lock().unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&("new-device".to_string(), "cap".to_string())));
    }
}
