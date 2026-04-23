use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tungstenite::{connect, Message};

use crate::api::WsBroadcaster;
use crate::db::Database;
use crate::mqtt::MqttBridge;
use crate::sinric::SinricBridge;
use tauri::Manager;

#[derive(Debug, Clone, Serialize)]
pub struct DeviceEvent {
    pub device_id: String,
    pub event_type: String,
    pub payload: serde_json::Value,
}

struct DeviceConnection {
    device_id: String,
    stop_flag: Arc<Mutex<bool>>,
    thread: Option<thread::JoinHandle<()>>,
    command_tx: mpsc::Sender<String>,
}

pub struct ConnectionManager {
    connections: Arc<Mutex<HashMap<String, DeviceConnection>>>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
    mqtt_bridge: Arc<Mutex<Option<Arc<MqttBridge>>>>,
    sinric_bridge: Arc<Mutex<Option<Arc<SinricBridge>>>>,
    ws_broadcaster: Arc<Mutex<Option<Arc<WsBroadcaster>>>>,
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
            mqtt_bridge: Arc::new(Mutex::new(None)),
            sinric_bridge: Arc::new(Mutex::new(None)),
            ws_broadcaster: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        let mut h = self.app_handle.lock().unwrap();
        *h = Some(handle);
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

    pub fn connect_device(&self, device_id: &str, ip: &str, ws_port: u16) {
        let mut conns = self.connections.lock().unwrap();

        // Don't double-connect
        if conns.contains_key(device_id) {
            return;
        }

        let stop_flag = Arc::new(Mutex::new(false));
        let stop_clone = stop_flag.clone();
        let app_handle = self.app_handle.clone();
        let mqtt_bridge = self.mqtt_bridge.clone();
        let sinric_bridge = self.sinric_bridge.clone();
        let ws_broadcaster = self.ws_broadcaster.clone();
        let id = device_id.to_string();
        let url = format!("ws://{}:{}", ip, ws_port);
        let (command_tx, command_rx) = mpsc::channel::<String>();

        let thread = thread::spawn(move || {
            ws_reader_loop(&id, &url, stop_clone, app_handle, mqtt_bridge, sinric_bridge, ws_broadcaster, command_rx);
        });

        conns.insert(
            device_id.to_string(),
            DeviceConnection {
                device_id: device_id.to_string(),
                stop_flag,
                thread: Some(thread),
                command_tx,
            },
        );

        log::info!("Connected WebSocket to device {}", device_id);
    }

    pub fn disconnect_device(&self, device_id: &str) {
        let mut conns = self.connections.lock().unwrap();
        if let Some(mut conn) = conns.remove(device_id) {
            *conn.stop_flag.lock().unwrap() = true;
            if let Some(thread) = conn.thread.take() {
                let _ = thread.join();
            }
            log::info!("Disconnected WebSocket from device {}", device_id);
        }
    }

    pub fn send_to_device(
        &self,
        device_id: &str,
        ip: &str,
        ws_port: u16,
        message: &str,
    ) -> Result<(), String> {
        // Preferred path: push through the persistent connection's command channel.
        // The reader loop will write the frame on the same WebSocket it uses for
        // events, so the device never sees a short-lived connection that could
        // race with WStype_TEXT dispatch.
        {
            let conns = self.connections.lock().unwrap();
            if let Some(conn) = conns.get(device_id) {
                conn.command_tx
                    .send(message.to_string())
                    .map_err(|e| format!("Command channel send: {}", e))?;
                return Ok(());
            }
        }

        // Fallback: no persistent connection yet (e.g. just-added device, called
        // before discovery's connect_device ran). Open a one-shot connection,
        // send, and give the device time to dispatch the frame before closing.
        let url = format!("ws://{}:{}", ip, ws_port);
        let (mut socket, _) =
            connect(&url).map_err(|e| format!("WebSocket connect: {}", e))?;
        socket
            .send(Message::Text(message.to_string()))
            .map_err(|e| format!("WebSocket send: {}", e))?;
        // Let the device's WebSocketsServer.loop() dispatch the frame to
        // processCommand before we tear down the connection. Without this
        // delay, the close arrives before WStype_TEXT fires and the frame
        // is dropped on disconnect teardown.
        thread::sleep(Duration::from_millis(200));
        let _ = socket.close(None);
        Ok(())
    }

    pub fn is_connected(&self, device_id: &str) -> bool {
        let conns = self.connections.lock().unwrap();
        conns.contains_key(device_id)
    }
}

fn ws_reader_loop(
    device_id: &str,
    url: &str,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: Arc<Mutex<Option<AppHandle>>>,
    mqtt_bridge: Arc<Mutex<Option<Arc<MqttBridge>>>>,
    sinric_bridge: Arc<Mutex<Option<Arc<SinricBridge>>>>,
    ws_broadcaster: Arc<Mutex<Option<Arc<WsBroadcaster>>>>,
    command_rx: mpsc::Receiver<String>,
) {
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }

        log::info!("[WS] Connecting to {} for device {}", url, device_id);

        match connect(url) {
            Ok((mut socket, _)) => {
                // Short read timeout so we can interleave channel polling and
                // stop_flag checks without blocking.
                if let Some(stream) = socket.get_ref().as_tcp_stream() {
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(50)));
                }

                loop {
                    if *stop_flag.lock().unwrap() {
                        let _ = socket.close(None);
                        return;
                    }

                    // Drain any pending outbound commands and write them on the
                    // same socket. Sharing the persistent connection eliminates
                    // the short-lived-connection race that drops frames on
                    // disconnect teardown.
                    let mut write_failed = false;
                    loop {
                        match command_rx.try_recv() {
                            Ok(msg) => {
                                if let Err(e) = socket.send(Message::Text(msg)) {
                                    log::warn!(
                                        "[WS] Send error for device {}: {}",
                                        device_id,
                                        e
                                    );
                                    write_failed = true;
                                    break;
                                }
                            }
                            Err(mpsc::TryRecvError::Empty) => break,
                            Err(mpsc::TryRecvError::Disconnected) => {
                                // No more senders — manager dropped this connection
                                let _ = socket.close(None);
                                return;
                            }
                        }
                    }
                    if write_failed {
                        break;
                    }

                    match socket.read() {
                        Ok(Message::Text(text)) => {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&text) {
                                let event_type = json
                                    .get("event")
                                    .and_then(|e| e.as_str())
                                    .unwrap_or("unknown")
                                    .to_string();

                                // Mirror state updates and heartbeats to MQTT
                                // (no-op when bridge disabled).
                                if event_type == "update" {
                                    if let (Some(cap_id), Some(value)) = (
                                        json.get("id").and_then(|v| v.as_str()),
                                        json.get("value"),
                                    ) {
                                        if let Some(bridge) =
                                            mqtt_bridge.lock().unwrap().as_ref()
                                        {
                                            bridge.publish_state(device_id, cap_id, value);
                                        }
                                        if let Some(bridge) =
                                            sinric_bridge.lock().unwrap().as_ref()
                                        {
                                            bridge.on_state_change(device_id, cap_id, value);
                                        }
                                        // Persist capability transitions so the energy
                                        // rollup can integrate them. Switches go through
                                        // log_switch_state (bool state). Sliders that have
                                        // opted in to linear_power (phase 2) are rounded
                                        // to i64 and logged via
                                        // log_slider_value_if_linear, which no-ops if the
                                        // capability hasn't enabled the opt-in.
                                        if let Some(handle) =
                                            app_handle.lock().unwrap().as_ref()
                                        {
                                            if let Some(db) =
                                                handle.try_state::<Database>()
                                            {
                                                let mut logged = false;
                                                if let Some(on) = value.as_bool() {
                                                    let _ = db.log_switch_state(
                                                        device_id, cap_id, on,
                                                    );
                                                    logged = true;
                                                } else if let Some(n) = value.as_f64() {
                                                    let _ = db.log_slider_value_if_linear(
                                                        device_id, cap_id, n as i64,
                                                    );
                                                    logged = true;
                                                }
                                                // Companion `_energy` publish on
                                                // every transition that hit
                                                // capability_state_log. Gated on
                                                // bridge opt-in so non-metered
                                                // caps skip the DB read.
                                                if logged {
                                                    if let Some(bridge) =
                                                        mqtt_bridge.lock().unwrap().as_ref()
                                                    {
                                                        let metered = bridge
                                                            .metered_capabilities()
                                                            .iter()
                                                            .any(|(d, c)| {
                                                                d == device_id
                                                                    && c == cap_id
                                                            });
                                                        if metered {
                                                            if let Ok(wh) = db
                                                                .get_capability_lifetime_wh(
                                                                    device_id, cap_id,
                                                                )
                                                            {
                                                                bridge.publish_energy(
                                                                    device_id,
                                                                    cap_id,
                                                                    wh,
                                                                );
                                                            }
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                } else if event_type == "heartbeat" {
                                    // Polish #3: mirror device telemetry
                                    // (rssi, heap_free, uptime_s) to MQTT so HA
                                    // can graph device health.
                                    if let Some(system) = json.get("system") {
                                        if let Some(bridge) =
                                            mqtt_bridge.lock().unwrap().as_ref()
                                        {
                                            bridge.publish_heartbeat(device_id, system);
                                        }
                                    }
                                }

                                // Push to :9090 WebSocket dashboard clients
                                if let Some(broadcaster) = ws_broadcaster.lock().unwrap().as_ref() {
                                    let ws_msg = serde_json::json!({
                                        "type": "device_event",
                                        "device_id": device_id,
                                        "event_type": &event_type,
                                        "payload": &json,
                                    });
                                    broadcaster.broadcast(ws_msg.to_string());
                                }

                                if let Some(handle) = app_handle.lock().unwrap().as_ref() {
                                    let _ = handle.emit(
                                        "device-event",
                                        DeviceEvent {
                                            device_id: device_id.to_string(),
                                            event_type,
                                            payload: json,
                                        },
                                    );
                                }
                            }
                        }
                        Ok(Message::Close(_)) => {
                            log::info!("[WS] Device {} closed connection", device_id);
                            break;
                        }
                        Err(tungstenite::Error::Io(ref e))
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            // Timeout — just loop and check stop_flag/commands
                            continue;
                        }
                        Err(e) => {
                            log::warn!("[WS] Read error for device {}: {}", device_id, e);
                            break;
                        }
                        _ => {}
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "[WS] Failed to connect to {} for device {}: {}",
                    url,
                    device_id,
                    e
                );
            }
        }

        if *stop_flag.lock().unwrap() {
            break;
        }

        // Reconnect delay
        log::info!("[WS] Reconnecting to device {} in 5s...", device_id);
        thread::sleep(Duration::from_secs(5));
    }
}

// Helper trait to get the inner TCP stream for setting timeouts
trait AsTcpStream {
    fn as_tcp_stream(&self) -> Option<&std::net::TcpStream>;
}

impl AsTcpStream for tungstenite::stream::MaybeTlsStream<std::net::TcpStream> {
    fn as_tcp_stream(&self) -> Option<&std::net::TcpStream> {
        match self {
            tungstenite::stream::MaybeTlsStream::Plain(s) => Some(s),
            _ => None,
        }
    }
}
