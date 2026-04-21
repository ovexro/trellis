use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub ip: String,
    pub port: u16,
    pub firmware: String,
    pub platform: String,
    pub capabilities: Vec<Capability>,
    pub system: SystemInfo,
    pub online: bool,
    pub last_seen: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Capability {
    pub id: String,
    #[serde(rename = "type")]
    pub cap_type: String,
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max: Option<f64>,
    pub value: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemInfo {
    pub rssi: i32,
    pub heap_free: u32,
    pub uptime_s: u64,
    pub chip: String,
    // Populated by library v0.17.0+. Older firmwares omit the field; we
    // accept that with #[serde(default)] and leave None so the power-
    // supply rule downgrades to INFO instead of lying.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reset_reason: Option<String>,
    // Populated by library v0.18.0+ on ESP32 only. Cumulative count of NVS
    // persist operations since boot (RAM-only — resets on reboot). Feeds the
    // flash_wear diagnostic rule.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nvs_writes: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfo {
    pub name: String,
    pub id: String,
    pub firmware: String,
    pub platform: String,
    pub capabilities: Vec<Capability>,
    pub system: SystemInfo,
}
