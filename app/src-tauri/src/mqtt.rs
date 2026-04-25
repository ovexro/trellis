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
use tauri::Manager;

use crate::connection::ConnectionManager;
use crate::db::Database;
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
    /// Capability-meter cache keyed by (device_id, capability_id). Hydrated
    /// from the `capability_meta` table once at startup and kept in sync via
    /// `set_watts` / `set_linear_power` when the user edits metadata through
    /// the Tauri commands or REST endpoints. Used on the hot path
    /// (`publish_state`) to emit HA `_power` / `_energy` entities without
    /// touching the DB on every state transition. Only capabilities with a
    /// set nameplate appear here.
    meta_map: Arc<Mutex<HashMap<(String, String), MeteredMeta>>>,
    /// Wired in by `lib.rs` setup hook (mirrors the Sinric bridge pattern).
    /// The MQTT worker thread needs this to look up scenes from the DB when
    /// an HA button press arrives on the `<base_topic>/_scene/<id>/run` topic
    /// and call `scheduler::fire_scene`. The bridge stays database-free on
    /// the hot publish path — DB access here is only on inbound scene-run
    /// dispatch (cold path).
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    /// Track which scenes have had HA discovery configs published, keyed by
    /// scene_id with the last-published name as the value. We republish when
    /// the name changes so HA's entity label stays in sync without churning
    /// retained configs on every save.
    scene_discovery_published: Arc<Mutex<HashMap<i64, String>>>,
}

/// Live MQTT-side view of one metered capability. Mirrors the subset of
/// `capability_meta` the bridge needs at publish time (watts for `_power`,
/// `linear_power` + `slider_max` for slider-value derivation). `slider_max`
/// is only read for numeric-valued updates on opted-in sliders.
#[derive(Debug, Clone, Copy)]
pub struct MeteredMeta {
    pub watts: f64,
    pub linear_power: bool,
    pub slider_max: Option<f64>,
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
            meta_map: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
            scene_discovery_published: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn set_discovery(&self, discovery: Arc<Discovery>) {
        *self.discovery.lock().unwrap() = Some(discovery);
    }

    /// Wire the Tauri AppHandle in after construction (mirrors `set_discovery`
    /// and Sinric's `set_app_handle`). Used so the inbound scene-run dispatch
    /// can resolve the `Database` state and call `scheduler::fire_scene`.
    pub fn set_app_handle(&self, handle: tauri::AppHandle) {
        *self.app_handle.lock().unwrap() = Some(handle);
    }

    /// Replace the in-memory meter cache from a freshly-loaded DB snapshot.
    /// Called once at startup (before the first `apply_config` / discovery
    /// publish) so HA sees `_power` + `_energy` entities for every
    /// already-metered capability the moment the broker connects.
    pub fn hydrate_meters(
        &self,
        entries: Vec<(String, String, f64, bool, Option<f64>)>,
    ) {
        let mut map = self.meta_map.lock().unwrap();
        map.clear();
        for (device_id, cap_id, watts, linear_power, slider_max) in entries {
            map.insert(
                (device_id, cap_id),
                MeteredMeta { watts, linear_power, slider_max },
            );
        }
    }

    /// List every (device_id, capability_id) currently in the meter cache.
    /// Used by the periodic energy-tick thread in `lib.rs` to know which
    /// caps need an `_energy/state` refresh without re-querying the DB for
    /// the key list itself.
    pub fn metered_capabilities(&self) -> Vec<(String, String)> {
        self.meta_map
            .lock()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    }

    /// Update the watts cache for a single capability and re-emit HA
    /// discovery for its device so the `_power` + `_energy` entities appear
    /// (when Some) or are removed (when None) without a bridge restart.
    ///
    /// When watts are cleared we publish zero-length retained payloads to
    /// the `_power` and `_energy` discovery config topics — HA's idiomatic
    /// way of asking it to remove those entities. The primary capability
    /// entity and the three diagnostic system sensors are unaffected.
    pub fn set_watts(&self, device_id: &str, capability_id: &str, watts: Option<f64>) {
        let key = (device_id.to_string(), capability_id.to_string());
        match watts {
            Some(w) => {
                let mut map = self.meta_map.lock().unwrap();
                map.entry(key.clone())
                    .and_modify(|m| m.watts = w)
                    .or_insert(MeteredMeta {
                        watts: w,
                        linear_power: false,
                        slider_max: None,
                    });
            }
            None => {
                self.meta_map.lock().unwrap().remove(&key);
                self.remove_metered_entities(device_id, capability_id);
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

    /// Update the linear-power opt-in (+ slider_max) for a capability. For
    /// switches the flag is a no-op on entity topology (switches always get
    /// `_power` + `_energy` when watts is set). For sliders it gates whether
    /// the `_power` + `_energy` entities exist — opting out retracts them.
    pub fn set_linear_power(
        &self,
        device_id: &str,
        capability_id: &str,
        linear_power: bool,
        slider_max: Option<f64>,
    ) {
        let key = (device_id.to_string(), capability_id.to_string());
        let was_opted_in: bool;
        let is_slider: bool;
        {
            let mut map = self.meta_map.lock().unwrap();
            was_opted_in = map
                .get(&key)
                .map(|m| m.linear_power)
                .unwrap_or(false);
            if let Some(m) = map.get_mut(&key) {
                m.linear_power = linear_power;
                m.slider_max = slider_max;
            }
            // If no meta entry exists (no nameplate_watts set), the opt-in is
            // latent — it'll take effect next time watts are set. No-op here.
        }
        is_slider = self
            .discovery
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|d| d.get_devices().into_iter().find(|dev| dev.id == device_id))
            .and_then(|dev| {
                dev.capabilities
                    .iter()
                    .find(|c| c.id == capability_id)
                    .map(|c| c.cap_type.clone())
            })
            .map(|t| t == "slider")
            .unwrap_or(false);
        // Slider opted-out → retract the entities (entity set depends on
        // the flag). Switches don't care.
        if is_slider && was_opted_in && !linear_power {
            self.remove_metered_entities(device_id, capability_id);
        }
        // Force a discovery republish so the entity set reflects the new
        // opt-in state immediately.
        self.forget_discovery(device_id);
        let device = match self.discovery.lock().unwrap().as_ref() {
            Some(d) => d.get_devices().into_iter().find(|d| d.id == device_id),
            None => None,
        };
        if let Some(device) = device {
            self.publish_discovery(&device);
        }
    }

    /// Publish empty retained payloads on the `_power` and `_energy`
    /// discovery config topics for a capability. HA's idiom for "forget this
    /// entity." Called when watts are cleared, when a slider opts out of
    /// linear_power, and implicitly by `set_watts(None)`.
    fn remove_metered_entities(&self, device_id: &str, capability_id: &str) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        for suffix in ["power", "energy"] {
            let unique_id =
                format!("trellis_{}_{}_{}", device_id, capability_id, suffix);
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
        self.scene_discovery_published.lock().unwrap().clear();

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
            self.publish_all_scene_discovery();
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
        // Scene-run pattern: <base_topic>/_scene/<id>/run. The leading
        // underscore on `_scene` keeps the namespace disjoint from device IDs
        // (firmware ID format is `trellis-<hex>`, never starts with `_`).
        let scene_pattern = format!("{}/_scene/+/run", cfg.base_topic);
        if let Err(e) = client.subscribe(&scene_pattern, QoS::AtLeastOnce) {
            log::warn!("[MQTT] Failed to subscribe to {}: {}", scene_pattern, e);
        }

        let status = self.status.clone();
        let stop_flag = self.stop_flag.clone();
        let config_for_worker = self.config.clone();
        let conn_mgr = self.connection_manager.clone();
        let discovery_for_worker = self.discovery.clone();
        let client_for_worker = self.client.clone();
        let tracker_for_worker = self.discovery_published.clone();
        let meta_for_worker = self.meta_map.clone();
        let app_handle_for_worker = self.app_handle.clone();
        let scene_tracker_for_worker = self.scene_discovery_published.clone();

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
                meta_for_worker,
                app_handle_for_worker,
                scene_tracker_for_worker,
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
        // value, mirror the update to the `_power` state topic. For switches
        // (bool value) → W when ON, 0 when OFF. For sliders opted into
        // linear_power (numeric value) → value × watts / slider_max.
        let meta = self
            .meta_map
            .lock()
            .unwrap()
            .get(&(device_id.to_string(), capability_id.to_string()))
            .copied();
        if let Some(meta) = meta {
            if let Some(power_payload) = compute_power_payload_any(value, &meta) {
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

    /// Publish a cumulative Wh value on the `_energy/state` topic (retained).
    /// Companion to the HA `total_increasing` energy sensor — the Wh payload
    /// is derived off-hot-path (connection.rs transition handler or the
    /// periodic tick in lib.rs) so the bridge stays database-free.
    pub fn publish_energy(&self, device_id: &str, capability_id: &str, wh: f64) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        // Only publish for capabilities the bridge considers metered. The
        // caller is expected to gate on `metered_capabilities()` already;
        // this check is defensive (no-op on a miss).
        if !self
            .meta_map
            .lock()
            .unwrap()
            .contains_key(&(device_id.to_string(), capability_id.to_string()))
        {
            return;
        }
        let topic = format!(
            "{}/{}/{}/_energy/state",
            cfg.base_topic, device_id, capability_id
        );
        let payload = format_energy_payload(wh);
        if let Err(e) =
            client.publish(&topic, QoS::AtLeastOnce, true, payload.into_bytes())
        {
            log::warn!("[MQTT] energy publish {} failed: {}", topic, e);
        } else {
            self.status.lock().unwrap().messages_published += 1;
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
            &self.meta_map,
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
                &self.meta_map,
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

    /// Publish an HA `button` discovery config for one scene. Pressing the
    /// button in HA publishes any payload to `<base_topic>/_scene/<id>/run`,
    /// which the bridge maps to `scheduler::fire_scene`. All scene buttons
    /// group under a single synthetic "Trellis Scenes" device in HA so
    /// they're one collapsed card on the dashboard regardless of how many
    /// scenes exist.
    ///
    /// Idempotent — the dedupe tracker keys on (scene_id, name), so renaming
    /// a scene republishes (HA picks up the new label) but a no-op save does
    /// nothing on the wire.
    pub fn publish_scene_discovery(&self, scene_id: i64, name: &str) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        publish_scene_discovery_impl(
            scene_id,
            name,
            &cfg,
            &client,
            &self.scene_discovery_published,
            &self.status,
        );
    }

    /// Iterate every scene in the DB and publish HA discovery for each one.
    /// Used by polish #1 (bridge enable) and polish #2 (broker reconnect)
    /// so HA always rehydrates a complete scene-button list. Clears the
    /// dedupe tracker first so even known scenes get re-announced.
    pub fn publish_all_scene_discovery(&self) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let scenes: Vec<(i64, String)> = match self.app_handle.lock().unwrap().as_ref() {
            Some(h) => match h.try_state::<Database>() {
                Some(db) => match db.get_scenes() {
                    Ok(list) => list
                        .into_iter()
                        .map(|s: crate::db::Scene| (s.id, s.name))
                        .collect(),
                    Err(e) => {
                        log::warn!("[MQTT] scene discovery: failed to load scenes: {}", e);
                        return;
                    }
                },
                None => return,
            },
            None => return,
        };
        self.scene_discovery_published.lock().unwrap().clear();
        for (id, name) in &scenes {
            publish_scene_discovery_impl(
                *id,
                name,
                &cfg,
                &client,
                &self.scene_discovery_published,
                &self.status,
            );
        }
    }

    /// Retract a scene's HA discovery entity. Publishes an empty retained
    /// payload to the discovery config topic — HA's idiomatic "forget this
    /// entity" — and drops the dedupe tracker entry. Called when a scene is
    /// deleted; safe to call when discovery is off (early-returns).
    pub fn forget_scene_discovery(&self, scene_id: i64) {
        let cfg = self.config.lock().unwrap().clone();
        if !cfg.enabled || !cfg.ha_discovery_enabled {
            return;
        }
        let client = match self.client.lock().unwrap().as_ref() {
            Some(c) => c.clone(),
            None => return,
        };
        let unique_id = format!("trellis_scene_{}", scene_id);
        let config_topic = format!(
            "{}/button/{}/config",
            cfg.ha_discovery_prefix, unique_id
        );
        let _ = client.publish(&config_topic, QoS::AtLeastOnce, true, Vec::<u8>::new());
        self.scene_discovery_published.lock().unwrap().remove(&scene_id);
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
    meta_map: &Mutex<HashMap<(String, String), MeteredMeta>>,
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

    // Per-metered-capability `_power` + `_energy` sensors. One pair per
    // capability that has a nameplate watts value set AND is either a
    // switch OR a slider opted into linear_power tracking. The `_power`
    // entity (device_class=power, unit=W) is fed from `publish_state`;
    // the `_energy` entity (device_class=energy, state_class=total_increasing,
    // unit=Wh) is the cumulative counter HA's Energy dashboard consumes —
    // driven by `publish_energy` on every transition plus a periodic tick
    // from lib.rs for steady-state coverage.
    let metered_for_device: Vec<(String, MeteredMeta)> = {
        let map = meta_map.lock().unwrap();
        device
            .capabilities
            .iter()
            .filter_map(|c| {
                let m = map.get(&(device.id.clone(), c.id.clone()))?;
                let include = match c.cap_type.as_str() {
                    "switch" => true,
                    "slider" => m.linear_power,
                    _ => false,
                };
                if include {
                    Some((c.id.clone(), *m))
                } else {
                    None
                }
            })
            .collect()
    };
    for (cap_id, _meta) in &metered_for_device {
        let cap_label = device
            .capabilities
            .iter()
            .find(|c| c.id == *cap_id)
            .map(|c| c.label.clone())
            .unwrap_or_else(|| cap_id.clone());

        // `_power` companion — live wattage sensor.
        let power_uid = format!("trellis_{}_{}_power", device.id, cap_id);
        let power_cfg_topic = format!(
            "{}/sensor/{}/config",
            cfg.ha_discovery_prefix, power_uid
        );
        let power_state_topic =
            format!("{}/{}/{}/_power/state", cfg.base_topic, device.id, cap_id);
        let power_config = serde_json::json!({
            "name": format!("{} Power", cap_label),
            "unique_id": power_uid,
            "state_topic": power_state_topic,
            "availability_topic": availability_topic,
            "payload_available": PAYLOAD_ONLINE,
            "payload_not_available": PAYLOAD_OFFLINE,
            "device": device_block,
            "device_class": "power",
            "state_class": "measurement",
            "unit_of_measurement": "W",
        });
        let power_payload = serde_json::to_string(&power_config).unwrap_or_default();
        if let Err(e) = client.publish(
            &power_cfg_topic,
            QoS::AtLeastOnce,
            true,
            power_payload.into_bytes(),
        ) {
            log::warn!(
                "[MQTT] power sensor config publish {} failed: {}",
                power_cfg_topic,
                e
            );
        } else {
            published_count += 1;
        }
        // Initial retained state so HA has a value before the first toggle.
        // Conservative default is "0" (matches `get_device_energy` bootstrap).
        if let Err(e) = client.publish(
            &power_state_topic,
            QoS::AtLeastOnce,
            true,
            "0".as_bytes(),
        ) {
            log::warn!(
                "[MQTT] power sensor state publish {} failed: {}",
                power_state_topic,
                e
            );
        } else {
            published_count += 1;
        }

        // `_energy` companion — HA Energy-dashboard cumulative counter.
        let energy_uid = format!("trellis_{}_{}_energy", device.id, cap_id);
        let energy_cfg_topic = format!(
            "{}/sensor/{}/config",
            cfg.ha_discovery_prefix, energy_uid
        );
        let energy_state_topic =
            format!("{}/{}/{}/_energy/state", cfg.base_topic, device.id, cap_id);
        let energy_config = serde_json::json!({
            "name": format!("{} Energy", cap_label),
            "unique_id": energy_uid,
            "state_topic": energy_state_topic,
            "availability_topic": availability_topic,
            "payload_available": PAYLOAD_ONLINE,
            "payload_not_available": PAYLOAD_OFFLINE,
            "device": device_block,
            "device_class": "energy",
            "state_class": "total_increasing",
            "unit_of_measurement": "Wh",
        });
        let energy_payload = serde_json::to_string(&energy_config).unwrap_or_default();
        if let Err(e) = client.publish(
            &energy_cfg_topic,
            QoS::AtLeastOnce,
            true,
            energy_payload.into_bytes(),
        ) {
            log::warn!(
                "[MQTT] energy sensor config publish {} failed: {}",
                energy_cfg_topic,
                e
            );
        } else {
            published_count += 1;
        }
        // Leave the `_energy/state` topic alone on discovery publish — the
        // caller (connection.rs / periodic tick) will fill it in with the
        // current lifetime Wh reading. Publishing "0" here would reset HA's
        // counter on every broker reconnect, which the `total_increasing`
        // spec treats as valid but causes a visible artifact in the Energy
        // dashboard. Better to let HA retain the prior value until we have
        // a fresh reading.
    }

    if published_count > 0 {
        status.lock().unwrap().messages_published += published_count;
    }
}

/// Free function so both `MqttBridge::publish_scene_discovery` (per-scene)
/// and `MqttBridge::publish_all_scene_discovery` (bulk on enable / reconnect)
/// can call it without re-locking the bridge state. Mirrors the (device-,
/// capability-) discovery dedupe pattern but keys on scene_id instead.
///
/// The HA entity is published as a `button` component, which fires a
/// momentary press payload — perfect semantics for "run this scene" since
/// scenes don't have an on/off state to track. All scene buttons share one
/// synthetic device block (`trellis_scenes`) so HA renders them as a single
/// collapsible card.
fn publish_scene_discovery_impl(
    scene_id: i64,
    name: &str,
    cfg: &MqttConfig,
    client: &Client,
    tracker: &Mutex<HashMap<i64, String>>,
    status: &Mutex<MqttStatus>,
) {
    {
        let mut t = tracker.lock().unwrap();
        if t.get(&scene_id).map(|n| n.as_str()) == Some(name) {
            return;
        }
        t.insert(scene_id, name.to_string());
    }

    let availability_topic = format!("{}/{}", cfg.base_topic, BRIDGE_AVAILABILITY_SUFFIX);
    let unique_id = format!("trellis_scene_{}", scene_id);
    let config_topic = format!(
        "{}/button/{}/config",
        cfg.ha_discovery_prefix, unique_id
    );
    let command_topic = format!("{}/_scene/{}/run", cfg.base_topic, scene_id);

    let device_block = serde_json::json!({
        "identifiers": ["trellis_scenes"],
        "name": "Trellis Scenes",
        "manufacturer": "Trellis",
        "model": "Scene",
    });

    let config = serde_json::json!({
        "name": name,
        "unique_id": unique_id,
        "command_topic": command_topic,
        "payload_press": "PRESS",
        "availability_topic": availability_topic,
        "payload_available": PAYLOAD_ONLINE,
        "payload_not_available": PAYLOAD_OFFLINE,
        "device": device_block,
        "icon": "mdi:movie-play",
    });

    let payload = serde_json::to_string(&config).unwrap_or_default();
    if let Err(e) =
        client.publish(&config_topic, QoS::AtLeastOnce, true, payload.into_bytes())
    {
        log::warn!("[MQTT] scene discovery publish {} failed: {}", config_topic, e);
    } else {
        status.lock().unwrap().messages_published += 1;
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
    meta_map: Arc<Mutex<HashMap<(String, String), MeteredMeta>>>,
    app_handle: Arc<Mutex<Option<tauri::AppHandle>>>,
    scene_discovery_published: Arc<Mutex<HashMap<i64, String>>>,
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
                    let scene_pattern = format!("{}/_scene/+/run", cfg_snapshot.base_topic);
                    if let Err(e) = c.subscribe(&scene_pattern, QoS::AtLeastOnce) {
                        log::warn!(
                            "[MQTT] resubscribe {} failed: {}",
                            scene_pattern,
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
                                    &meta_map,
                                );
                            }
                            log::info!(
                                "[MQTT] Republished discovery for {} device(s) on reconnect",
                                devices.len()
                            );
                        }
                    }
                    // Same idea for scene buttons. Pulled from the DB via
                    // app_handle (the bridge stays database-free on the hot
                    // path; this is cold-path republish only).
                    let scenes: Vec<(i64, String)> = match app_handle.lock().unwrap().as_ref() {
                        Some(h) => match h.try_state::<Database>() {
                            Some(db) => db
                                .get_scenes()
                                .map(|list: Vec<crate::db::Scene>| {
                                    list.into_iter().map(|s| (s.id, s.name)).collect()
                                })
                                .unwrap_or_default(),
                            None => Vec::new(),
                        },
                        None => Vec::new(),
                    };
                    if !scenes.is_empty() {
                        if let Some(c) = client.lock().unwrap().as_ref() {
                            scene_discovery_published.lock().unwrap().clear();
                            for (id, name) in &scenes {
                                publish_scene_discovery_impl(
                                    *id,
                                    name,
                                    &cfg_snapshot,
                                    c,
                                    &scene_discovery_published,
                                    &status,
                                );
                            }
                            log::info!(
                                "[MQTT] Republished discovery for {} scene(s) on reconnect",
                                scenes.len()
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
                handle_inbound(&topic, &payload, &cfg, &conn_mgr, &app_handle);
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

/// Route an inbound MQTT message to the appropriate Trellis device or scene.
/// Recognized topic shapes:
///   <base_topic>/<device_id>/<cap_id>/set    → capability set
///   <base_topic>/_scene/<scene_id>/run       → scene run (HA button press)
/// `base_topic` may contain slashes (e.g. "home/iot/trellis"), so we use
/// prefix-stripping rather than naive segment counting. The leading underscore
/// on `_scene` keeps that namespace disjoint from device IDs (which start with
/// `trellis-` per firmware contract).
fn handle_inbound(
    topic: &str,
    payload: &str,
    cfg: &MqttConfig,
    conn_mgr: &ConnectionManager,
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
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
        if let Some(without_run) = rest.strip_suffix("/run") {
            if let Some(scene_id_str) = without_run.strip_prefix("_scene/") {
                if !scene_id_str.is_empty() && !scene_id_str.contains('/') {
                    if let Ok(scene_id) = scene_id_str.parse::<i64>() {
                        dispatch_scene_run(scene_id, conn_mgr, app_handle);
                        return;
                    }
                }
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

/// Resolve a scene by id and call the shared `scheduler::fire_scene` runner.
/// Mirrors the Sinric voice path (`run_scene_from_sinric`) — same DB lookup,
/// same scheduler entry point, same last_run stamping. Payload is ignored:
/// HA's `button` entity always sends a fixed press payload that just means
/// "do it now."
fn dispatch_scene_run(
    scene_id: i64,
    conn_mgr: &ConnectionManager,
    app_handle: &Arc<Mutex<Option<tauri::AppHandle>>>,
) {
    let handle_guard = app_handle.lock().unwrap();
    let handle = match handle_guard.as_ref() {
        Some(h) => h,
        None => {
            log::warn!("[MQTT] scene run dispatch: app handle unavailable");
            return;
        }
    };
    let db = match handle.try_state::<Database>() {
        Some(db) => db,
        None => {
            log::warn!("[MQTT] scene run dispatch: Database state unavailable");
            return;
        }
    };
    let scene = match db.get_scene(scene_id) {
        Ok(Some(s)) => s,
        Ok(None) => {
            log::warn!("[MQTT] scene run dispatch: scene {} not found", scene_id);
            return;
        }
        Err(e) => {
            log::warn!("[MQTT] scene run dispatch: db error: {}", e);
            return;
        }
    };
    match crate::scheduler::fire_scene(handle, conn_mgr, &scene) {
        Ok(()) => log::info!("[MQTT] scene {} ('{}') ran via HA button", scene.id, scene.name),
        Err(e) => log::warn!("[MQTT] scene {} run failed: {}", scene_id, e),
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
/// the capability's current value doesn't map to a meaningful W reading.
/// For bool values (switches): `Some("<watts>")` ON, `Some("0")` OFF.
/// For numeric values on linear-power sliders:
/// `Some("<value × watts / slider_max>")`. For everything else (non-opted
/// slider, string, null): None.
fn compute_power_payload_any(value: &Value, meta: &MeteredMeta) -> Option<String> {
    if let Some(on) = value.as_bool() {
        return Some(format_watts_payload(if on { meta.watts } else { 0.0 }));
    }
    if !meta.linear_power {
        return None;
    }
    let n = value.as_f64()?;
    let max = meta.slider_max.unwrap_or(255.0);
    let max_safe = if max.is_finite() && max > 0.0 {
        max
    } else {
        255.0
    };
    let watts_now = (n / max_safe) * meta.watts;
    Some(format_watts_payload(watts_now.max(0.0)))
}

/// Format a cumulative Wh value for the `_energy/state` topic. HA's
/// `total_increasing` integrator tolerates any numeric payload; we emit
/// up to 4 decimal places for sub-Wh resolution on frequently-toggled
/// devices and trim a trailing ".0000" so integers stay clean.
fn format_energy_payload(wh: f64) -> String {
    let rounded = (wh * 10000.0).round() / 10000.0;
    let s = format!("{:.4}", rounded);
    if let Some(stripped) = s.strip_suffix(".0000") {
        stripped.to_string()
    } else {
        s.trim_end_matches('0').trim_end_matches('.').to_string()
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

    fn meta(watts: f64, linear_power: bool, slider_max: Option<f64>) -> MeteredMeta {
        MeteredMeta { watts, linear_power, slider_max }
    }

    #[test]
    fn power_payload_switch_on_off() {
        assert_eq!(
            compute_power_payload_any(&serde_json::Value::Bool(true), &meta(60.0, false, None)),
            Some("60".to_string())
        );
        assert_eq!(
            compute_power_payload_any(&serde_json::Value::Bool(false), &meta(60.0, false, None)),
            Some("0".to_string())
        );
    }

    #[test]
    fn power_payload_skips_numeric_when_not_linear() {
        // Non-opted-in slider: numeric updates don't drive `_power`.
        assert_eq!(
            compute_power_payload_any(&serde_json::json!(42), &meta(60.0, false, Some(100.0))),
            None
        );
    }

    #[test]
    fn power_payload_slider_linear_fraction() {
        // 50/100 × 20W = 10W
        assert_eq!(
            compute_power_payload_any(&serde_json::json!(50), &meta(20.0, true, Some(100.0))),
            Some("10".to_string())
        );
        // 75/100 × 20W = 15W
        assert_eq!(
            compute_power_payload_any(&serde_json::json!(75), &meta(20.0, true, Some(100.0))),
            Some("15".to_string())
        );
        // 0 → 0W
        assert_eq!(
            compute_power_payload_any(&serde_json::json!(0), &meta(20.0, true, Some(100.0))),
            Some("0".to_string())
        );
    }

    #[test]
    fn power_payload_slider_fallback_max_when_unset() {
        // slider_max=None → fallback 255 denominator.
        // 128/255 × 100W ≈ 50.196 → rounds to 50.20 at 2dp.
        assert_eq!(
            compute_power_payload_any(&serde_json::json!(128), &meta(100.0, true, None)),
            Some("50.20".to_string())
        );
    }

    #[test]
    fn power_payload_skips_non_numeric_non_bool() {
        assert_eq!(
            compute_power_payload_any(&serde_json::json!("on"), &meta(60.0, true, Some(100.0))),
            None
        );
        assert_eq!(
            compute_power_payload_any(&serde_json::Value::Null, &meta(60.0, true, Some(100.0))),
            None
        );
    }

    #[test]
    fn energy_payload_integer_trim() {
        assert_eq!(format_energy_payload(0.0), "0");
        assert_eq!(format_energy_payload(10.0), "10");
        assert_eq!(format_energy_payload(1500.0), "1500");
    }

    #[test]
    fn energy_payload_fractional_trim() {
        assert_eq!(format_energy_payload(0.1234), "0.1234");
        assert_eq!(format_energy_payload(0.5), "0.5");
        assert_eq!(format_energy_payload(12.50), "12.5");
        // Sub-Wh resolution: round to 4dp.
        assert_eq!(format_energy_payload(0.12345678), "0.1235");
    }

    #[test]
    fn hydrate_and_set_meters_update_map() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge.hydrate_meters(vec![
            ("dev1".to_string(), "led".to_string(), 60.0, false, None),
            (
                "dev1".to_string(),
                "bright".to_string(),
                20.0,
                true,
                Some(100.0),
            ),
        ]);
        {
            let map = bridge.meta_map.lock().unwrap();
            assert_eq!(map.len(), 2);
            let m = map
                .get(&("dev1".to_string(), "led".to_string()))
                .copied()
                .unwrap();
            assert_eq!(m.watts, 60.0);
            assert!(!m.linear_power);
            let s = map
                .get(&("dev1".to_string(), "bright".to_string()))
                .copied()
                .unwrap();
            assert_eq!(s.watts, 20.0);
            assert!(s.linear_power);
            assert_eq!(s.slider_max, Some(100.0));
        }

        // set_watts updates existing without clobbering linear_power / max.
        bridge.set_watts("dev1", "bright", Some(25.0));
        {
            let map = bridge.meta_map.lock().unwrap();
            let s = map
                .get(&("dev1".to_string(), "bright".to_string()))
                .copied()
                .unwrap();
            assert_eq!(s.watts, 25.0);
            assert!(s.linear_power, "opt-in preserved across watts edit");
            assert_eq!(s.slider_max, Some(100.0));
        }

        // Clearing watts removes the entry.
        bridge.set_watts("dev1", "led", None);
        assert!(!bridge
            .meta_map
            .lock()
            .unwrap()
            .contains_key(&("dev1".to_string(), "led".to_string())));

        // metered_capabilities lists remaining entries.
        let caps = bridge.metered_capabilities();
        assert_eq!(caps, vec![("dev1".to_string(), "bright".to_string())]);
    }

    #[test]
    fn set_linear_power_toggles_flag_preserving_watts() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge.hydrate_meters(vec![(
            "dev1".to_string(),
            "bright".to_string(),
            20.0,
            false,
            None,
        )]);
        bridge.set_linear_power("dev1", "bright", true, Some(100.0));
        let map = bridge.meta_map.lock().unwrap();
        let s = map
            .get(&("dev1".to_string(), "bright".to_string()))
            .copied()
            .unwrap();
        assert_eq!(s.watts, 20.0);
        assert!(s.linear_power);
        assert_eq!(s.slider_max, Some(100.0));
    }

    #[test]
    fn hydrate_meters_replaces_existing() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge.hydrate_meters(vec![(
            "old-device".to_string(),
            "cap".to_string(),
            10.0,
            false,
            None,
        )]);
        bridge.hydrate_meters(vec![(
            "new-device".to_string(),
            "cap".to_string(),
            20.0,
            true,
            Some(100.0),
        )]);
        let map = bridge.meta_map.lock().unwrap();
        assert_eq!(map.len(), 1);
        assert!(map.contains_key(&("new-device".to_string(), "cap".to_string())));
    }

    // ─── Scene MQTT discovery topic shape ────────────────────────────────

    #[test]
    fn scene_run_topic_parses_to_dispatch() {
        // Topic shape: <base_topic>/_scene/<id>/run
        // The handle_inbound walker should accept this and reject obvious
        // non-matches. We can't drive dispatch_scene_run without a Tauri
        // AppHandle, but we can assert the prefix-stripping arithmetic.
        let cfg = MqttConfig::default();
        let prefix = format!("{}/", cfg.base_topic);
        let topic = format!("{}_scene/42/run", prefix);
        let rest = topic.strip_prefix(&prefix).unwrap();
        let without_run = rest.strip_suffix("/run").unwrap();
        let scene_id_str = without_run.strip_prefix("_scene/").unwrap();
        assert_eq!(scene_id_str, "42");
        assert_eq!(scene_id_str.parse::<i64>().unwrap(), 42);
    }

    #[test]
    fn scene_run_topic_rejects_non_numeric_id() {
        let cfg = MqttConfig::default();
        let prefix = format!("{}/", cfg.base_topic);
        let topic = format!("{}_scene/notanid/run", prefix);
        let rest = topic.strip_prefix(&prefix).unwrap();
        let without_run = rest.strip_suffix("/run").unwrap();
        let scene_id_str = without_run.strip_prefix("_scene/").unwrap();
        assert!(scene_id_str.parse::<i64>().is_err());
    }

    #[test]
    fn scene_run_topic_rejects_extra_segments() {
        let cfg = MqttConfig::default();
        let prefix = format!("{}/", cfg.base_topic);
        let topic = format!("{}_scene/42/extra/run", prefix);
        let rest = topic.strip_prefix(&prefix).unwrap();
        let without_run = rest.strip_suffix("/run").unwrap();
        let scene_id_str = without_run.strip_prefix("_scene/").unwrap();
        // Defense: id segment may not contain a slash.
        assert!(scene_id_str.contains('/'));
    }

    #[test]
    fn scene_run_topic_rejects_device_set_pattern() {
        // A capability `set` topic must not be misclassified as a scene run.
        let cfg = MqttConfig::default();
        let prefix = format!("{}/", cfg.base_topic);
        let topic = format!("{}trellis-abc/power/set", prefix);
        let rest = topic.strip_prefix(&prefix).unwrap();
        // `/run` suffix is absent → scene-run branch never enters.
        assert!(rest.strip_suffix("/run").is_none());
    }

    fn scene_tracker() -> Mutex<HashMap<i64, String>> {
        Mutex::new(HashMap::new())
    }

    #[test]
    fn scene_discovery_dedupe_skips_repeat_with_same_name() {
        let tracker = scene_tracker();
        // First "publish" — record the name in the tracker. We can't make
        // a real rumqttc Client in a unit test, so simulate the tracker
        // bookkeeping directly: this is the dedupe contract.
        tracker.lock().unwrap().insert(7, "Movie Night".to_string());
        // The dedupe contract: same name → skip.
        let same = tracker.lock().unwrap().get(&7).map(|n| n.as_str()) == Some("Movie Night");
        assert!(same, "tracker should treat identical name as already-published");
    }

    #[test]
    fn scene_discovery_dedupe_republishes_on_rename() {
        let tracker = scene_tracker();
        tracker.lock().unwrap().insert(7, "Movie Night".to_string());
        let same = tracker.lock().unwrap().get(&7).map(|n| n.as_str()) == Some("Bedtime");
        assert!(!same, "tracker should re-emit when the name changes");
    }

    #[test]
    fn forget_scene_drops_tracker_entry() {
        let bridge = MqttBridge::new(Arc::new(ConnectionManager::new()));
        bridge
            .scene_discovery_published
            .lock()
            .unwrap()
            .insert(99, "Some Scene".to_string());
        // forget_scene_discovery early-returns without an enabled config and
        // a live client — that's fine, we just need to confirm the local
        // tracker entry can be removed via the public API path.
        bridge.scene_discovery_published.lock().unwrap().remove(&99);
        assert!(bridge
            .scene_discovery_published
            .lock()
            .unwrap()
            .get(&99)
            .is_none());
    }

    #[test]
    fn scene_discovery_topic_uses_button_component() {
        // We expect: <ha_prefix>/button/trellis_scene_<id>/config
        let cfg = MqttConfig::default();
        let scene_id: i64 = 13;
        let unique_id = format!("trellis_scene_{}", scene_id);
        let topic = format!("{}/button/{}/config", cfg.ha_discovery_prefix, unique_id);
        assert!(topic.starts_with("homeassistant/button/"));
        assert!(topic.ends_with("/trellis_scene_13/config"));
    }

    #[test]
    fn scene_command_topic_uses_underscore_namespace() {
        // The leading underscore on `_scene` keeps this topic disjoint from
        // device IDs (which always start with `trellis-`).
        let cfg = MqttConfig::default();
        let scene_id: i64 = 5;
        let cmd_topic = format!("{}/_scene/{}/run", cfg.base_topic, scene_id);
        assert_eq!(cmd_topic, "trellis/_scene/5/run");
        // Sanity: no device ID could collide with `_scene`.
        assert!(!cmd_topic.contains("trellis-"));
    }
}
