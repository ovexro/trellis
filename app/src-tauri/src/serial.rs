use serde::Serialize;
use serialport::available_ports;

#[derive(Debug, Clone, Serialize)]
pub struct SerialPortInfo {
    pub name: String,
    pub port_type: String,
}

pub fn list_ports() -> Vec<SerialPortInfo> {
    match available_ports() {
        Ok(ports) => ports
            .into_iter()
            .map(|p| SerialPortInfo {
                name: p.port_name,
                port_type: match p.port_type {
                    serialport::SerialPortType::UsbPort(info) => {
                        format!(
                            "USB: {}",
                            info.product.unwrap_or_else(|| "Unknown".to_string())
                        )
                    }
                    serialport::SerialPortType::PciPort => "PCI".to_string(),
                    serialport::SerialPortType::BluetoothPort => "Bluetooth".to_string(),
                    serialport::SerialPortType::Unknown => "Unknown".to_string(),
                },
            })
            .collect(),
        Err(_) => vec![],
    }
}
