use mdns_sd::{ServiceDaemon, ServiceEvent};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::device::{Device, DeviceInfo};

const SERVICE_TYPE: &str = "_trellis._tcp.local.";
const SCAN_TIMEOUT: Duration = Duration::from_secs(5);

pub struct Discovery {
    devices: Arc<Mutex<HashMap<String, Device>>>,
}

impl Discovery {
    pub fn new() -> Self {
        Self {
            devices: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Scan for devices via mDNS. Fully synchronous — call from spawn_blocking.
    pub fn scan(&self) -> Vec<Device> {
        let mdns = match ServiceDaemon::new() {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to create mDNS daemon: {}", e);
                return self.get_devices();
            }
        };

        let receiver = match mdns.browse(SERVICE_TYPE) {
            Ok(r) => r,
            Err(e) => {
                log::error!("Failed to browse mDNS: {}", e);
                return self.get_devices();
            }
        };

        let deadline = std::time::Instant::now() + SCAN_TIMEOUT;

        while std::time::Instant::now() < deadline {
            match receiver.recv_timeout(Duration::from_millis(500)) {
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
                            let mut devs = self.devices.lock().unwrap();
                            devs.insert(device_info.id, device);
                        }
                        Err(e) => {
                            log::warn!("Failed to fetch info from {}:{} — {}", ip, port, e);
                        }
                    }
                }
                Ok(ServiceEvent::ServiceRemoved(_, fullname)) => {
                    let mut devs = self.devices.lock().unwrap();
                    devs.retain(|_, d| !fullname.contains(&d.name));
                }
                Err(_) => continue,
                _ => {}
            }
        }

        let _ = mdns.shutdown();
        self.get_devices()
    }

    pub fn get_devices(&self) -> Vec<Device> {
        let devs = self.devices.lock().unwrap();
        devs.values().cloned().collect()
    }

    /// Manually add a device by IP address (bypass mDNS)
    pub fn add_by_ip(&self, ip: &str, port: u16) -> Result<Device, String> {
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
        let mut devs = self.devices.lock().unwrap();
        devs.insert(device_info.id, device.clone());
        Ok(device)
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
