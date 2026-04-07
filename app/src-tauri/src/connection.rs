use std::collections::HashMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tungstenite::{connect, Message};

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
}

impl ConnectionManager {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(Mutex::new(HashMap::new())),
            app_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn set_app_handle(&self, handle: AppHandle) {
        let mut h = self.app_handle.lock().unwrap();
        *h = Some(handle);
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
        let id = device_id.to_string();
        let url = format!("ws://{}:{}", ip, ws_port);
        let (command_tx, command_rx) = mpsc::channel::<String>();

        let thread = thread::spawn(move || {
            ws_reader_loop(&id, &url, stop_clone, app_handle, command_rx);
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
