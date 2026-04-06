use serde::Serialize;
use serialport::{available_ports, SerialPort};
use std::collections::HashMap;
use std::io::Read;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;
use tauri::{AppHandle, Emitter};

#[derive(Debug, Clone, Serialize)]
pub struct SerialPortInfo {
    pub name: String,
    pub port_type: String,
    pub vid: Option<u16>,
    pub pid: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SerialData {
    pub port: String,
    pub data: String,
}

struct OpenPort {
    stop_flag: Arc<Mutex<bool>>,
    thread: Option<thread::JoinHandle<()>>,
    writer: Arc<Mutex<Box<dyn SerialPort>>>,
}

pub struct SerialManager {
    open_ports: Arc<Mutex<HashMap<String, OpenPort>>>,
}

impl SerialManager {
    pub fn new() -> Self {
        Self {
            open_ports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn list_ports() -> Vec<SerialPortInfo> {
        match available_ports() {
            Ok(ports) => ports
                .into_iter()
                .map(|p| {
                    let (port_type, vid, pid) = match &p.port_type {
                        serialport::SerialPortType::UsbPort(info) => (
                            format!(
                                "USB: {}",
                                info.product.as_deref().unwrap_or("Unknown")
                            ),
                            Some(info.vid),
                            Some(info.pid),
                        ),
                        serialport::SerialPortType::PciPort => ("PCI".to_string(), None, None),
                        serialport::SerialPortType::BluetoothPort => {
                            ("Bluetooth".to_string(), None, None)
                        }
                        serialport::SerialPortType::Unknown => {
                            ("Unknown".to_string(), None, None)
                        }
                    };
                    SerialPortInfo {
                        name: p.port_name,
                        port_type,
                        vid,
                        pid,
                    }
                })
                .collect(),
            Err(_) => vec![],
        }
    }

    pub fn open(
        &self,
        port_name: &str,
        baud_rate: u32,
        app_handle: AppHandle,
    ) -> Result<(), String> {
        let mut ports = self.open_ports.lock().unwrap();

        if ports.contains_key(port_name) {
            return Err(format!("Port {} is already open", port_name));
        }

        let port = serialport::new(port_name, baud_rate)
            .timeout(Duration::from_millis(100))
            .open()
            .map_err(|e| format!("Failed to open {}: {}", port_name, e))?;

        let writer = Arc::new(Mutex::new(port.try_clone().map_err(|e| e.to_string())?));
        let stop_flag = Arc::new(Mutex::new(false));
        let stop_clone = stop_flag.clone();
        let name = port_name.to_string();

        let thread = thread::spawn(move || {
            serial_reader_loop(port, &name, stop_clone, app_handle);
        });

        ports.insert(
            port_name.to_string(),
            OpenPort {
                stop_flag,
                thread: Some(thread),
                writer,
            },
        );

        log::info!("[Serial] Opened {} at {} baud", port_name, baud_rate);
        Ok(())
    }

    pub fn close(&self, port_name: &str) -> Result<(), String> {
        let mut ports = self.open_ports.lock().unwrap();
        if let Some(mut port) = ports.remove(port_name) {
            *port.stop_flag.lock().unwrap() = true;
            if let Some(thread) = port.thread.take() {
                let _ = thread.join();
            }
            log::info!("[Serial] Closed {}", port_name);
            Ok(())
        } else {
            Err(format!("Port {} is not open", port_name))
        }
    }

    pub fn write(&self, port_name: &str, data: &str) -> Result<(), String> {
        let ports = self.open_ports.lock().unwrap();
        if let Some(port) = ports.get(port_name) {
            let mut writer = port.writer.lock().unwrap();
            writer
                .write_all(data.as_bytes())
                .map_err(|e| format!("Write error: {}", e))?;
            writer
                .write_all(b"\r\n")
                .map_err(|e| format!("Write error: {}", e))?;
            Ok(())
        } else {
            Err(format!("Port {} is not open", port_name))
        }
    }
}

fn serial_reader_loop(
    mut port: Box<dyn SerialPort>,
    port_name: &str,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: AppHandle,
) {
    let mut buf = [0u8; 1024];
    let mut line_buf = String::new();

    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }

        match port.read(&mut buf) {
            Ok(n) if n > 0 => {
                let text = String::from_utf8_lossy(&buf[..n]);
                line_buf.push_str(&text);

                // Emit complete lines
                while let Some(pos) = line_buf.find('\n') {
                    let line = line_buf[..pos].trim_end_matches('\r').to_string();
                    line_buf = line_buf[pos + 1..].to_string();

                    let _ = app_handle.emit(
                        "serial-data",
                        SerialData {
                            port: port_name.to_string(),
                            data: line,
                        },
                    );
                }

                // Emit partial data if buffer is getting large (no newline)
                if line_buf.len() > 512 {
                    let _ = app_handle.emit(
                        "serial-data",
                        SerialData {
                            port: port_name.to_string(),
                            data: line_buf.clone(),
                        },
                    );
                    line_buf.clear();
                }
            }
            Ok(_) => {} // No data
            Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
                continue;
            }
            Err(e) => {
                log::warn!("[Serial] Read error on {}: {}", port_name, e);
                let _ = app_handle.emit(
                    "serial-data",
                    SerialData {
                        port: port_name.to_string(),
                        data: format!("[ERROR] {}", e),
                    },
                );
                break;
            }
        }
    }

    // Flush remaining buffer
    if !line_buf.is_empty() {
        let _ = app_handle.emit(
            "serial-data",
            SerialData {
                port: port_name.to_string(),
                data: line_buf,
            },
        );
    }
}
