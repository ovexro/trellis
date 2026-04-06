use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::connection::ConnectionManager;
use crate::db::Database;
use crate::device::{Device, DeviceInfo};

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
    stop_flag: Arc<Mutex<bool>>,
}

impl Discovery {
    pub fn new(connection_manager: Arc<ConnectionManager>) -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
            connection_manager,
            stop_flag: Arc::new(Mutex::new(false)),
        }
    }

    /// Start continuous background discovery
    pub fn start_background(&self, app_handle: AppHandle) {
        let devices = self.devices.clone();
        let conn_mgr = self.connection_manager.clone();
        let stop_flag = self.stop_flag.clone();
        let handle = app_handle.clone();

        // mDNS continuous browsing thread
        thread::spawn(move || {
            mdns_browse_loop(devices.clone(), conn_mgr.clone(), stop_flag.clone(), handle.clone());
        });

        // Health check thread
        let devices2 = self.devices.clone();
        let conn_mgr2 = self.connection_manager.clone();
        let stop_flag2 = self.stop_flag.clone();
        let handle2 = app_handle;

        thread::spawn(move || {
            health_check_loop(devices2, conn_mgr2, stop_flag2, handle2);
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

        // Notify frontend
        let _ = app_handle.emit(
            "device-discovered",
            DeviceDiscoveryEvent {
                device: device.clone(),
                event: "found".to_string(),
            },
        );

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

    loop {
        if *stop_flag.lock().unwrap() {
            let _ = mdns.shutdown();
            break;
        }

        match receiver.recv_timeout(Duration::from_secs(2)) {
            Ok(ServiceEvent::ServiceResolved(info)) => {
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

                        let is_new = {
                            let mut devs = devices.lock().unwrap();
                            let existed = devs.contains_key(&device_info.id);
                            devs.insert(device_info.id.clone(), device.clone());
                            !existed
                        };

                        // Persist to SQLite
                        persist_device(&app_handle, &device);

                        // Connect WebSocket for live updates
                        conn_mgr.connect_device(&device_info.id, &ip, port + 1);

                        // Notify frontend
                        let _ = app_handle.emit(
                            "device-discovered",
                            DeviceDiscoveryEvent {
                                device,
                                event: if is_new {
                                    "found".to_string()
                                } else {
                                    "updated".to_string()
                                },
                            },
                        );

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
                let mut devs = devices.lock().unwrap();
                let lost: Vec<String> = devs
                    .iter()
                    .filter(|(_, d)| fullname.contains(&d.name.to_lowercase().replace(' ', "-")))
                    .map(|(id, _)| id.clone())
                    .collect();

                for id in lost {
                    if let Some(mut device) = devs.get_mut(&id) {
                        device.online = false;
                        let _ = app_handle.emit(
                            "device-discovered",
                            DeviceDiscoveryEvent {
                                device: device.clone(),
                                event: "lost".to_string(),
                            },
                        );
                    }
                    conn_mgr.disconnect_device(&id);
                    log::info!("[Discovery] Device lost: {}", id);
                }
            }
            Err(_) => continue, // Timeout — just loop
            _ => {}
        }
    }
}

fn health_check_loop(
    devices: Arc<Mutex<HashMap<String, Device>>>,
    conn_mgr: Arc<ConnectionManager>,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: AppHandle,
) {
    loop {
        // Read configurable scan interval from DB settings, default to 30s
        let interval_secs = app_handle
            .try_state::<crate::db::Database>()
            .and_then(|db| db.get_setting("scan_interval").ok().flatten())
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(DEFAULT_HEALTH_CHECK_SECS);
        thread::sleep(Duration::from_secs(interval_secs));

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
                        device.online = true;
                        device.name = info.name;
                        device.firmware = info.firmware;
                        device.platform = info.platform;
                        device.system = info.system;
                        device.capabilities = info.capabilities;
                        device.last_seen = chrono::Utc::now().to_rfc3339();

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
                            log::info!("[Health] Device {} came back online", id);
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
                            log::info!("[Health] Device {} went offline", id);
                        }
                    }
                }
            }
        }
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
