use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::api::WsBroadcaster;
use crate::connection::ConnectionManager;
use crate::db::Database;
use crate::device::{Device, DeviceInfo, SystemInfo};
use crate::mqtt::MqttBridge;
use crate::sinric::SinricBridge;

const SERVICE_TYPE: &str = "_trellis._tcp.local.";
const DEFAULT_HEALTH_CHECK_SECS: u64 = 30;

#[derive(Debug, Clone, Serialize)]
struct DeviceDiscoveryEvent {
    device: Device,
    event: String, // "found" or "lost"
}

pub struct Discovery {
    devices: Arc<Mutex<HashMap<String, Device>>>,
    connection_manager: Arc<ConnectionManager>,
    mqtt_bridge: Arc<Mutex<Option<Arc<MqttBridge>>>>,
    sinric_bridge: Arc<Mutex<Option<Arc<SinricBridge>>>>,
    ws_broadcaster: Arc<Mutex<Option<Arc<WsBroadcaster>>>>,
    stop_flag: Arc<Mutex<bool>>,
}

impl Discovery {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            connection_manager,
            mqtt_bridge: Arc::new(Mutex::new(None)),
            sinric_bridge: Arc::new(Mutex::new(None)),
            ws_broadcaster: Arc::new(Mutex::new(None)),
            stop_flag: Arc::new(Mutex::new(false)),
        }
    }

    pub fn set_mqtt_bridge(&self, bridge: Arc<MqttBridge>) {
        *self.mqtt_bridge.lock().unwrap() = Some(bridge);
    }

    pub fn set_sinric_bridge(&self, bridge: Arc<SinricBridge>) {
        *self.sinric_bridge.lock().unwrap() = Some(bridge);
    }

    pub fn set_ws_broadcaster(&self, broadcaster: Arc<WsBroadcaster>) {
        *self.ws_broadcaster.lock().unwrap() = Some(broadcaster);
    }

    /// Hydrate the in-memory device map from SQLite at startup so saved devices
    /// (especially cross-subnet ones added by IP that mDNS can't rediscover)
    /// reappear in the desktop UI, REST API, web dashboard, and MQTT bridge
    /// immediately on app launch instead of waiting for the user to manually
    /// re-add them. Devices start as offline; the health check loop's first
    /// tick (now run-immediately, see health_check_loop) will probe each one
    /// and flip them online if reachable.
    pub fn hydrate_from_db(&self, app_handle: &AppHandle) {
        let Some(db) = app_handle.try_state::<Database>() else {
            log::warn!("[Discovery] hydrate_from_db: Database state not available");
            return;
        };
        let saved = match db.get_all_saved_devices() {
            Ok(rows) => rows,
            Err(e) => {
                log::warn!("[Discovery] hydrate_from_db: failed to read SQLite: {}", e);
                return;
            }
        };
        if saved.is_empty() {
            return;
        }
        let mut devs = self.devices.lock().unwrap();
        for s in &saved {
            // Don't overwrite anything mDNS may have already populated in the
            // tiny window between discovery construction and hydration. (In
            // practice this is impossible because hydrate_from_db is called
            // before start_background, but be defensive.)
            if devs.contains_key(&s.id) {
                continue;
            }
            devs.insert(
                s.id.clone(),
                Device {
                    id: s.id.clone(),
                    name: s.name.clone(),
                    ip: s.ip.clone(),
                    port: s.port,
                    firmware: s.firmware.clone(),
                    platform: s.platform.clone(),
                    capabilities: Vec::new(),
                    system: SystemInfo {
                        rssi: 0,
                        heap_free: 0,
                        uptime_s: 0,
                        chip: String::new(),
                        reset_reason: None,
                        nvs_writes: None,
                    },
                    online: false,
                    last_seen: s.last_seen.clone(),
                },
            );
        }
        log::info!("[Discovery] Hydrated {} saved devices from SQLite (offline until reachable)", saved.len());
    }

    /// Start continuous background discovery
    pub fn start_background(&self, app_handle: AppHandle) {
        let devices = self.devices.clone();
        let conn_mgr = self.connection_manager.clone();
        let bridge = self.mqtt_bridge.clone();
        let ws_bc = self.ws_broadcaster.clone();
        let stop_flag = self.stop_flag.clone();
        let handle = app_handle.clone();

        // mDNS continuous browsing thread
        thread::spawn(move || {
            mdns_browse_loop(devices.clone(), conn_mgr.clone(), bridge.clone(), ws_bc.clone(), stop_flag.clone(), handle.clone());
        });

        // Health check thread
        let devices2 = self.devices.clone();
        let conn_mgr2 = self.connection_manager.clone();
        let bridge2 = self.mqtt_bridge.clone();
        let ws_bc2 = self.ws_broadcaster.clone();
        let stop_flag2 = self.stop_flag.clone();
        let handle2 = app_handle;

        thread::spawn(move || {
            health_check_loop(devices2, conn_mgr2, bridge2, ws_bc2, stop_flag2, handle2);
        });
    }

    /// Manually add a device by IP address
    pub fn add_by_ip(&self, ip: &str, port: u16, app_handle: &AppHandle) -> Result<Device, String> {
        let device_info = fetch_device_info(ip, port)?;
        let device = Device {
            id: device_info.id.clone(),
            name: device_info.name,
            ip: ip.to_string(),
            port,
            firmware: device_info.firmware,
            platform: device_info.platform,
            capabilities: device_info.capabilities,
            system: device_info.system,
            online: true,
            last_seen: chrono::Utc::now().to_rfc3339(),
        };

        // Store device in memory + SQLite
        let mut devs = self.devices.lock().unwrap();
        devs.insert(device_info.id.clone(), device.clone());
        persist_device(app_handle, &device);

        // Connect WebSocket
        self.connection_manager
            .connect_device(&device_info.id, ip, port + 1);

        // Publish HA discovery configs (no-op when bridge disabled)
        if let Some(bridge) = self.mqtt_bridge.lock().unwrap().as_ref() {
            bridge.publish_discovery(&device);
        }

        // Notify frontend
        let _ = app_handle.emit(
            "device-discovered",
            DeviceDiscoveryEvent {
                device: device.clone(),
                event: "found".to_string(),
            },
        );

        // Push to :9090 WebSocket dashboard clients
        if let Some(bc) = self.ws_broadcaster.lock().unwrap().as_ref() {
            let msg = serde_json::json!({"type":"device_discovery","event":"found","device":&device});
            bc.broadcast(msg.to_string());
        }

        Ok(device)
    }

    pub fn get_devices(&self) -> Vec<Device> {
        let devs = self.devices.lock().unwrap();
        devs.values().cloned().collect()
    }
}

fn mdns_browse_loop(
    devices: Arc<Mutex<HashMap<String, Device>>>,
    conn_mgr: Arc<ConnectionManager>,
    mqtt_bridge: Arc<Mutex<Option<Arc<MqttBridge>>>>,
    ws_broadcaster: Arc<Mutex<Option<Arc<WsBroadcaster>>>>,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: AppHandle,
) {
    let mdns = match ServiceDaemon::new() {
        Ok(d) => d,
        Err(e) => {
            log::error!("Failed to create mDNS daemon: {}", e);
            return;
        }
    };

    let receiver = match mdns.browse(SERVICE_TYPE) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Failed to browse mDNS: {}", e);
            return;
        }
    };

    log::info!("[Discovery] Continuous mDNS browsing started");

    // Per-fullname timestamp of the most recent *accepted* ServiceResolved
    // event. The interval between two accepted events for the same service
    // instance is the cadence sample we record — TTL-driven refreshes emit
    // Resolved continuously in steady state, so cadence stretching is a
    // usable health proxy (device dropping off, LAN path flaking).
    //
    // Dedup: mdns-sd emits Resolved once per listening interface (8+ on a
    // machine with docker / bridges). Resolved events that arrive within
    // `RESOLVED_DEBOUNCE` of the last accepted one for the same fullname
    // are treated as the same announcement seen on another interface and
    // dropped without updating the baseline timestamp.
    //
    // Entries are cleared on ServiceRemoved so a device that disappears
    // and comes back starts a fresh cadence measurement. Timeout branch
    // also prunes stale entries (>30 min with no Resolved) to bound the
    // map against churn.
    let mut last_resolved_at: HashMap<String, Instant> = HashMap::new();
    const RESOLVED_DEBOUNCE: Duration = Duration::from_secs(5);
    const RESOLVED_ENTRY_TTL: Duration = Duration::from_secs(30 * 60);

    loop {
        if *stop_flag.lock().unwrap() {
            let _ = mdns.shutdown();
            break;
        }

        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(ServiceEvent::ServiceFound(_, _)) => {
                // ServiceFound is informational — cadence capture needs only
                // Resolved. (Pre-v0.18.0 used this to pair with Resolved for
                // a one-shot resolution-latency sample; that model under-
                // counted because TTL refreshes don't re-emit Found.)
            }
            Ok(ServiceEvent::ServiceResolved(info)) => {
                let fullname = info.get_fullname().to_string();
                let now = Instant::now();
                // Dedup across interfaces + classify event. DropDuplicate
                // = redundant interface fire within the debounce window
                // (skip all downstream work — already handled). FirstSeen
                // = first Resolved for this instance (proceed, no sample
                // yet — next one seeds the first interval). Cadence(ms) =
                // genuine refresh, record the interval.
                enum ResolvedOutcome {
                    DropDuplicate,
                    FirstSeen,
                    Cadence(u32),
                }
                let outcome = match last_resolved_at.get(&fullname) {
                    Some(&prior) => {
                        let elapsed = now.duration_since(prior);
                        if elapsed < RESOLVED_DEBOUNCE {
                            ResolvedOutcome::DropDuplicate
                        } else {
                            last_resolved_at.insert(fullname.clone(), now);
                            ResolvedOutcome::Cadence(
                                elapsed.as_millis().min(u32::MAX as u128) as u32,
                            )
                        }
                    }
                    None => {
                        last_resolved_at.insert(fullname.clone(), now);
                        ResolvedOutcome::FirstSeen
                    }
                };
                if matches!(outcome, ResolvedOutcome::DropDuplicate) {
                    continue;
                }
                let cadence_ms = match outcome {
                    ResolvedOutcome::Cadence(ms) => Some(ms),
                    _ => None,
                };

                let ip = match info.get_addresses_v4().iter().next() {
                    Some(addr) => addr.to_string(),
                    None => continue,
                };
                let port = info.get_port();

                match fetch_device_info(&ip, port) {
                    Ok(device_info) => {
                        let device = Device {
                            id: device_info.id.clone(),
                            name: device_info.name,
                            ip: ip.clone(),
                            port,
                            firmware: device_info.firmware,
                            platform: device_info.platform,
                            capabilities: device_info.capabilities,
                            system: device_info.system,
                            online: true,
                            last_seen: chrono::Utc::now().to_rfc3339(),
                        };

                        let (is_new, old_uptime) = {
                            let mut devs = devices.lock().unwrap();
                            let prior = devs.get(&device_info.id).map(|d| d.system.uptime_s);
                            let existed = prior.is_some();
                            devs.insert(device_info.id.clone(), device.clone());
                            (!existed, prior.unwrap_or(0))
                        };

                        // Persist to SQLite
                        persist_device(&app_handle, &device);

                        // A strict uptime decrease means the device rebooted
                        // since we last saw it; file a row with whatever reset
                        // reason the firmware reported. `old_uptime == 0` is
                        // either a fresh hydrate or truly new — no prior
                        // baseline, skip.
                        maybe_record_reset(&app_handle, &device.id, old_uptime, &device.system);

                        // Record inter-Resolved cadence for this device.
                        // `None` on the first Resolved since (re-)announcement
                        // — no prior timestamp to measure from, seed only.
                        if let Some(ms) = cadence_ms {
                            if let Some(db) = app_handle.try_state::<Database>() {
                                if let Err(e) = db.record_mdns_cadence(&device.id, ms) {
                                    log::warn!("[Discovery] Failed to record mDNS cadence for {}: {}", device.id, e);
                                }
                            }
                        }

                        // Connect WebSocket for live updates
                        conn_mgr.connect_device(&device_info.id, &ip, port + 1);

                        // Publish HA discovery configs (no-op when bridge disabled)
                        if let Some(bridge) = mqtt_bridge.lock().unwrap().as_ref() {
                            bridge.publish_discovery(&device);
                        }

                        // Notify frontend
                        let discovery_event = if is_new { "found" } else { "updated" };
                        let _ = app_handle.emit(
                            "device-discovered",
                            DeviceDiscoveryEvent {
                                device: device.clone(),
                                event: discovery_event.to_string(),
                            },
                        );

                        // Push to :9090 WebSocket dashboard clients
                        if let Some(bc) = ws_broadcaster.lock().unwrap().as_ref() {
                            let msg = serde_json::json!({"type":"device_discovery","event":discovery_event,"device":&device});
                            bc.broadcast(msg.to_string());
                        }

                        if is_new {
                            log::info!("[Discovery] Found device: {} at {}:{}", device_info.id, ip, port);
                        }
                    }
                    Err(e) => {
                        log::warn!("[Discovery] Failed to fetch info from {}:{} — {}", ip, port, e);
                    }
                }
            }
            Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                // Drop the cadence baseline for this instance so a later
                // re-announcement starts a fresh measurement instead of
                // recording a huge "interval" spanning the outage.
                last_resolved_at.remove(&fullname);

                let mut devs = devices.lock().unwrap();
                let lost: Vec<String> = devs
                    .iter()
                    .filter(|(_, d)| fullname.contains(&d.name.to_lowercase().replace(' ', "-")))
                    .map(|(id, _)| id.clone())
                    .collect();

                for id in lost {
                    if let Some(device) = devs.get_mut(&id) {
                        device.online = false;
                        let _ = app_handle.emit(
                            "device-discovered",
                            DeviceDiscoveryEvent {
                                device: device.clone(),
                                event: "lost".to_string(),
                            },
                        );
                        if let Some(bc) = ws_broadcaster.lock().unwrap().as_ref() {
                            let msg = serde_json::json!({"type":"device_discovery","event":"lost","device":&*device});
                            bc.broadcast(msg.to_string());
                        }
                    }
                    conn_mgr.disconnect_device(&id);
                    if let Some(bridge) = mqtt_bridge.lock().unwrap().as_ref() {
                        bridge.forget_discovery(&id);
                    }
                    log::info!("[Discovery] Device lost: {}", id);
                }
            }
            Err(_) => {
                // Timeout — prune last_resolved_at entries for instances
                // we haven't seen Resolved for in a long time. Without a
                // ServiceRemoved (some LAN disappearances don't produce
                // one), the entry would otherwise sit forever and then
                // record a pathological "interval" on re-announcement.
                last_resolved_at.retain(|_, t| t.elapsed() < RESOLVED_ENTRY_TTL);
            }
            _ => {}
        }
    }
}

fn health_check_loop(
    devices: Arc<Mutex<HashMap<String, Device>>>,
    conn_mgr: Arc<ConnectionManager>,
    mqtt_bridge: Arc<Mutex<Option<Arc<MqttBridge>>>>,
    ws_broadcaster: Arc<Mutex<Option<Arc<WsBroadcaster>>>>,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: AppHandle,
) {
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }

        let device_list: Vec<(String, String, u16)> = {
            let devs = devices.lock().unwrap();
            devs.values()
                .map(|d| (d.id.clone(), d.ip.clone(), d.port))
                .collect()
        };

        for (id, ip, port) in device_list {
            match fetch_device_info(&ip, port) {
                Ok(info) => {
                    let mut devs = devices.lock().unwrap();
                    if let Some(device) = devs.get_mut(&id) {
                        let was_offline = !device.online;
                        let old_uptime = device.system.uptime_s;
                        device.online = true;
                        device.name = info.name;
                        device.firmware = info.firmware;
                        device.platform = info.platform;
                        device.system = info.system;
                        device.capabilities = info.capabilities;
                        device.last_seen = chrono::Utc::now().to_rfc3339();

                        // Attribute a reboot to the exact moment discovery
                        // sees uptime go backwards. Must happen while we still
                        // hold the read of `device.system` — relying on the
                        // just-assigned reset_reason.
                        maybe_record_reset(&app_handle, &id, old_uptime, &device.system);

                        // Persist updated info to SQLite
                        persist_device(&app_handle, device);

                        if was_offline {
                            conn_mgr.connect_device(&id, &ip, port + 1);
                            let _ = app_handle.emit(
                                "device-discovered",
                                DeviceDiscoveryEvent {
                                    device: device.clone(),
                                    event: "found".to_string(),
                                },
                            );
                            if let Some(bc) = ws_broadcaster.lock().unwrap().as_ref() {
                                let msg = serde_json::json!({"type":"device_discovery","event":"found","device":&*device});
                                bc.broadcast(msg.to_string());
                            }
                            // Persist the transition so chart annotations
                            // can draw a vertical marker at this timestamp.
                            if let Some(db) = app_handle.try_state::<Database>() {
                                let _ = db.store_log(&id, "state", "online");
                            }
                            log::info!("[Health] Device {} came back online", id);
                        }

                        // Republish HA discovery (handles capability changes after firmware updates)
                        if let Some(bridge) = mqtt_bridge.lock().unwrap().as_ref() {
                            bridge.publish_discovery(device);
                        }
                    }
                }
                Err(_) => {
                    let mut devs = devices.lock().unwrap();
                    if let Some(device) = devs.get_mut(&id) {
                        if device.online {
                            device.online = false;
                            conn_mgr.disconnect_device(&id);
                            let _ = app_handle.emit(
                                "device-discovered",
                                DeviceDiscoveryEvent {
                                    device: device.clone(),
                                    event: "lost".to_string(),
                                },
                            );
                            if let Some(bc) = ws_broadcaster.lock().unwrap().as_ref() {
                                let msg = serde_json::json!({"type":"device_discovery","event":"lost","device":&*device});
                                bc.broadcast(msg.to_string());
                            }
                            // Persist the transition so chart annotations
                            // can draw a vertical marker at this timestamp.
                            if let Some(db) = app_handle.try_state::<Database>() {
                                let _ = db.store_log(&id, "state", "offline");
                            }
                            log::info!("[Health] Device {} went offline", id);
                        }
                    }
                }
            }
        }

        // Sleep AFTER the work, not before. This makes the first probe run
        // immediately on app startup so cross-subnet hydrated devices flip
        // online within seconds instead of waiting a full interval.
        let interval_secs = app_handle
            .try_state::<crate::db::Database>()
            .and_then(|db| db.get_setting("scan_interval").ok().flatten())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_HEALTH_CHECK_SECS);
        thread::sleep(Duration::from_secs(interval_secs));
    }
}

/// Persist device to SQLite
fn persist_device(app_handle: &AppHandle, device: &Device) {
    if let Some(db) = app_handle.try_state::<Database>() {
        let _ = db.upsert_device(
            &device.id,
            &device.name,
            &device.ip,
            device.port,
            &device.firmware,
            &device.platform,
        );
    }
}

/// Blocking HTTP fetch of device info
fn fetch_device_info(ip: &str, port: u16) -> Result<DeviceInfo, String> {
    let url = format!("http://{}:{}/api/info", ip, port);
    let resp = ureq::get(&url)
        .timeout(Duration::from_secs(3))
        .call()
        .map_err(|e| format!("HTTP error: {}", e))?;
    let info: DeviceInfo = resp
        .into_json()
        .map_err(|e| format!("JSON parse error: {}", e))?;
    Ok(info)
}

/// Append a `device_reset_history` row when the device's reported uptime
/// has regressed since the last observation — a monotonic break we treat
/// as "the device rebooted while we weren't watching its uptime crawl."
/// `old == 0` is the hydrated-from-DB baseline (no prior live reading) and
/// is deliberately excluded: otherwise every cold start of the desktop
/// would mis-record a reboot against every device. Firmwares older than
/// v0.17.0 omit `reset_reason`; we persist "unknown" so the rule can still
/// count the reboot without needing to distinguish.
fn maybe_record_reset(app_handle: &AppHandle, device_id: &str, old_uptime_s: u64, new_sys: &SystemInfo) {
    if old_uptime_s == 0 || new_sys.uptime_s >= old_uptime_s {
        return;
    }
    let reason = new_sys.reset_reason.as_deref().unwrap_or("unknown");
    if let Some(db) = app_handle.try_state::<Database>() {
        if let Err(e) = db.record_reset(device_id, reason) {
            log::warn!("[Health] Failed to record reset for {}: {}", device_id, e);
            return;
        }
        log::info!(
            "[Health] Device {} rebooted (reset_reason={}, old_uptime={}s → new={}s)",
            device_id, reason, old_uptime_s, new_sys.uptime_s
        );
    }
}
