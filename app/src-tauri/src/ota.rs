use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

use tauri::{AppHandle, Emitter, Manager};

use crate::connection::DeviceEvent;
use crate::db::Database;

/// Best-effort persistence of an OTA outcome on the most recent
/// `firmware_history` row for `device_id` that has no `delivery_status`
/// yet. Logs a warning on DB error but never propagates — OTA delivery
/// is the user-visible outcome and must not depend on this side write.
/// `error` is persisted alongside "failed" outcomes so the diagnostics
/// rule can surface the failure category (v0.15.0).
fn record_delivery(
    app_handle: &AppHandle,
    device_id: &str,
    status: &str,
    error: Option<&str>,
) {
    if let Some(db) = app_handle.try_state::<Database>() {
        if let Err(e) = db.mark_firmware_delivery(device_id, status, error) {
            log::warn!("[OTA] mark_firmware_delivery({}, {}) failed: {}", device_id, status, e);
        }
    }
}

/// Serves a firmware file via HTTP on a random port.
/// Returns the URL that devices can use to download the firmware.
///
/// Once the device fetches the firmware and the bytes are flushed (or
/// delivery fails), emits a `device-event` with `event_type` either
/// `"ota_delivered"` or `"ota_delivery_failed"` so the UI can switch
/// from "stuck at 0%" to a "delivered, waiting for reboot" state. This
/// matters because the device's WebSocket drops the moment OTA starts,
/// so the streaming `ota_progress` events from the library never arrive.
pub fn serve_firmware(
    firmware_path: &str,
    app_handle: AppHandle,
    device_id: String,
) -> Result<(String, Arc<Mutex<bool>>), String> {
    let path = PathBuf::from(firmware_path);
    if !path.exists() {
        return Err(format!("Firmware file not found: {}", firmware_path));
    }

    let firmware_data = fs::read(&path).map_err(|e| format!("Failed to read firmware: {}", e))?;
    let firmware_size = firmware_data.len();

    // Bind to random available port
    // Bind to local IP only (not 0.0.0.0) to limit exposure
    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let listener = TcpListener::bind(format!("{}:0", local_ip))
        .or_else(|_| TcpListener::bind("0.0.0.0:0"))
        .map_err(|e| format!("Failed to bind: {}", e))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("Failed to get addr: {}", e))?
        .port();

    // Get local IP
    let local_ip = get_local_ip().unwrap_or_else(|| "127.0.0.1".to_string());
    let url = format!("http://{}:{}/firmware.bin", local_ip, port);

    let stop_flag = Arc::new(Mutex::new(false));
    let stop_clone = stop_flag.clone();

    log::info!("[OTA] Serving firmware ({} bytes) at {}", firmware_size, url);

    thread::spawn(move || {
        // Set timeout so we can check stop_flag
        listener
            .set_nonblocking(false)
            .ok();

        // Serve a single request then stop
        if let Some(stream) = listener.incoming().next() {
            if !*stop_clone.lock().unwrap() {
                match stream {
                    Ok(mut stream) => {
                        let peer = stream.peer_addr().ok();

                        // Read the request (we don't care about the contents)
                        let mut buf = [0u8; 1024];
                        let _ = stream.read(&mut buf);

                        // Send HTTP response with firmware
                        let header = format!(
                            "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            firmware_size
                        );

                        let mut delivered = false;
                        let mut delivery_error: Option<String> = None;
                        match stream.write_all(header.as_bytes()) {
                            Ok(()) => match stream.write_all(&firmware_data) {
                                Ok(()) => match stream.flush() {
                                    Ok(()) => {
                                        delivered = true;
                                    }
                                    Err(e) => delivery_error = Some(format!("flush: {}", e)),
                                },
                                Err(e) => delivery_error = Some(format!("body: {}", e)),
                            },
                            Err(e) => delivery_error = Some(format!("header: {}", e)),
                        }

                        if delivered {
                            log::info!("[OTA] Firmware served to {:?}", peer);
                        } else {
                            log::warn!(
                                "[OTA] Firmware delivery to {:?} failed: {}",
                                peer,
                                delivery_error.as_deref().unwrap_or("unknown")
                            );
                        }

                        // Persist outcome before emitting so a UI subscriber
                        // that immediately re-queries firmware_history sees
                        // the recorded status (v0.15.0).
                        record_delivery(
                            &app_handle,
                            &device_id,
                            if delivered { "delivered" } else { "failed" },
                            if delivered { None } else { delivery_error.as_deref() },
                        );

                        // Emit event so the UI can leave the "stuck at 0%" state.
                        let event_type = if delivered {
                            "ota_delivered"
                        } else {
                            "ota_delivery_failed"
                        };
                        let payload = if delivered {
                            serde_json::json!({ "bytes": firmware_size })
                        } else {
                            serde_json::json!({
                                "bytes": firmware_size,
                                "error": delivery_error.unwrap_or_else(|| "unknown".to_string()),
                            })
                        };
                        let _ = app_handle.emit(
                            "device-event",
                            DeviceEvent {
                                device_id: device_id.clone(),
                                event_type: event_type.to_string(),
                                payload,
                            },
                        );

                        // One-shot: stop after serving
                        *stop_clone.lock().unwrap() = true;
                    }
                    Err(e) => {
                        let err_str = format!("accept: {}", e);
                        log::warn!("[OTA] Accept error: {}", e);
                        record_delivery(&app_handle, &device_id, "failed", Some(&err_str));
                        let _ = app_handle.emit(
                            "device-event",
                            DeviceEvent {
                                device_id: device_id.clone(),
                                event_type: "ota_delivery_failed".to_string(),
                                payload: serde_json::json!({ "error": err_str }),
                            },
                        );
                    }
                }
            }
        }

        log::info!("[OTA] Server stopped");
    });

    Ok((url, stop_flag))
}

fn get_local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}
