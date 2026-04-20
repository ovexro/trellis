use std::collections::HashMap;
use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};

use crate::connection::DeviceEvent;
use crate::db::Database;

/// Shared per-device cancellation flags so a running OTA can be aborted
/// from a Tauri command or REST endpoint. The key is the `device_id`; the
/// value is the `stop_flag` that `serve_firmware`'s worker checks in both
/// its accept loop (cancel-before-connect) and its chunked write loop
/// (cancel-mid-transfer). The worker removes its own entry on exit.
/// Registering a new OTA for a device overwrites any stale entry from a
/// prior OTA on the same device (whose worker has already exited).
#[derive(Default)]
pub struct OtaRegistry {
    flags: Mutex<HashMap<String, Arc<Mutex<bool>>>>,
}

impl OtaRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    fn insert(&self, device_id: &str, flag: Arc<Mutex<bool>>) {
        self.flags.lock().unwrap().insert(device_id.to_string(), flag);
    }

    fn remove(&self, device_id: &str) {
        self.flags.lock().unwrap().remove(device_id);
    }

    /// Sets the stop flag for the named device if one is registered.
    /// Returns `true` if a flag was found and set (i.e. there was an
    /// in-flight OTA to cancel), `false` otherwise.
    pub fn cancel(&self, device_id: &str) -> bool {
        if let Some(flag) = self.flags.lock().unwrap().get(device_id).cloned() {
            *flag.lock().unwrap() = true;
            true
        } else {
            false
        }
    }
}

/// Best-effort persistence of an OTA outcome on the exact
/// `firmware_history` row identified by `history_row_id`. Logs a warning
/// on DB error but never propagates — OTA delivery is the user-visible
/// outcome and must not depend on this side write. `error` is persisted
/// alongside non-delivered outcomes so the diagnostics rule can surface
/// the failure category (v0.15.0). Callers pass `None` for paths that
/// reuse an existing firmware record (e.g. rollback) and therefore have
/// no new history row to mark — in that case persistence is skipped
/// entirely.
fn record_delivery(
    app_handle: &AppHandle,
    history_row_id: Option<i64>,
    status: &str,
    error: Option<&str>,
) {
    let Some(row_id) = history_row_id else { return };
    if let Some(db) = app_handle.try_state::<Database>() {
        if let Err(e) = db.mark_firmware_delivery(row_id, status, error) {
            log::warn!("[OTA] mark_firmware_delivery(row_id={}, {}) failed: {}", row_id, status, e);
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
///
/// The returned `stop_flag` can be flipped to `true` to cancel an
/// in-flight transfer. The worker checks it both while waiting for an
/// incoming connection (cancel-before-connect, e.g. rtl8xxxu flake
/// prevents the ESP32 from ever reaching the desktop) and between 4 KB
/// chunks while writing the firmware body (cancel-mid-transfer, when
/// the TCP send buffer is frozen with a healthy ESTABLISHED connection).
/// On cancellation the worker persists `delivery_status = "cancelled"`
/// with `delivery_error = "Cancelled by user"` and emits the same
/// `ota_delivery_failed` event the UI already handles — only the error
/// string differs. Cancelled rows are excluded from the
/// `ota_success_rate` diagnostics denominator.
pub fn serve_firmware(
    firmware_path: &str,
    app_handle: AppHandle,
    device_id: String,
    history_row_id: Option<i64>,
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

    // Register the flag so cancel_ota can flip it from a command or
    // REST endpoint without needing a handle to the returned tuple.
    // A device with a stale entry from a prior OTA (whose worker has
    // already unregistered itself on exit) just gets overwritten.
    if let Some(reg) = app_handle.try_state::<OtaRegistry>() {
        reg.insert(&device_id, stop_flag.clone());
    }

    log::info!("[OTA] Serving firmware ({} bytes) at {}", firmware_size, url);

    thread::spawn(move || {
        serve_worker(
            listener,
            firmware_data,
            firmware_size,
            stop_clone,
            app_handle,
            device_id,
            history_row_id,
        );
    });

    Ok((url, stop_flag))
}

/// Worker body: one-shot HTTP server that serves the firmware to the
/// first connecting client and handles cancellation at both the accept
/// stage and mid-write. Extracted from `serve_firmware` so the
/// registry-cleanup path is a single defer-like block at the end and
/// the happy/cancelled/error arms share it.
fn serve_worker(
    listener: TcpListener,
    firmware_data: Vec<u8>,
    firmware_size: usize,
    stop_flag: Arc<Mutex<bool>>,
    app_handle: AppHandle,
    device_id: String,
    history_row_id: Option<i64>,
) {
    // Accept phase — poll in ~200ms ticks so a cancel from the registry
    // aborts even when no device has connected yet (the rtl8xxxu flake
    // scenario from the 2026-04-20 hardware-test session).
    listener.set_nonblocking(true).ok();
    let mut stream_opt = None;
    let accept_timeout = Duration::from_millis(200);
    loop {
        if *stop_flag.lock().unwrap() {
            break;
        }
        match listener.accept() {
            Ok((s, _addr)) => {
                stream_opt = Some(s);
                break;
            }
            Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(accept_timeout);
            }
            Err(e) => {
                let err_str = format!("accept: {}", e);
                log::warn!("[OTA] Accept error: {}", e);
                finish(
                    &app_handle,
                    &device_id,
                    history_row_id,
                    "failed",
                    Some(&err_str),
                    firmware_size,
                    false,
                );
                return;
            }
        }
    }

    let Some(mut stream) = stream_opt else {
        // Cancel fired before a connection arrived.
        log::info!("[OTA] Cancelled before device connected (device={})", device_id);
        finish(
            &app_handle,
            &device_id,
            history_row_id,
            "cancelled",
            Some("Cancelled by user"),
            firmware_size,
            false,
        );
        return;
    };

    let peer = stream.peer_addr().ok();
    // Back to blocking mode with short timeouts so reads/writes are
    // responsive to cancel but don't spin.
    stream.set_nonblocking(false).ok();
    stream.set_read_timeout(Some(Duration::from_secs(5))).ok();
    stream.set_write_timeout(Some(Duration::from_millis(500))).ok();

    // Read the request (we don't care about the contents). Bounded
    // read so a cancel right after connect still makes progress.
    let mut buf = [0u8; 1024];
    let _ = stream.read(&mut buf);

    // Send HTTP response headers.
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        firmware_size
    );
    if let Err(e) = stream.write_all(header.as_bytes()) {
        let err_str = format!("header: {}", e);
        log::warn!("[OTA] Header write to {:?} failed: {}", peer, err_str);
        finish(
            &app_handle,
            &device_id,
            history_row_id,
            "failed",
            Some(&err_str),
            firmware_size,
            false,
        );
        return;
    }

    // Chunked write with cancel + timeout handling. 4 KB chunks + a
    // 500ms write_timeout mean a user-initiated cancel is seen within
    // ~500ms of a frozen send buffer (rtl8xxxu Send-Q pin scenario).
    const CHUNK: usize = 4096;
    let mut offset = 0;
    let delivery_outcome: DeliveryOutcome = loop {
        if *stop_flag.lock().unwrap() {
            break DeliveryOutcome::Cancelled;
        }
        if offset >= firmware_data.len() {
            break DeliveryOutcome::Delivered;
        }
        let end = (offset + CHUNK).min(firmware_data.len());
        match stream.write(&firmware_data[offset..end]) {
            Ok(0) => {
                break DeliveryOutcome::Failed("body: zero write".to_string());
            }
            Ok(n) => {
                offset += n;
            }
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                // Send buffer full or write timed out — loop and
                // re-check stop_flag. This is the exact spot that
                // converts a stuck-at-0% transfer into a responsive
                // cancel: write_timeout fires every 500ms, stop_flag
                // is checked, and the transfer aborts cleanly without
                // needing to kill the process.
                continue;
            }
            Err(e) => {
                break DeliveryOutcome::Failed(format!("body: {}", e));
            }
        }
    };

    let delivery_outcome = match delivery_outcome {
        DeliveryOutcome::Delivered => match stream.flush() {
            Ok(()) => DeliveryOutcome::Delivered,
            Err(e) => DeliveryOutcome::Failed(format!("flush: {}", e)),
        },
        other => other,
    };

    match &delivery_outcome {
        DeliveryOutcome::Delivered => {
            log::info!("[OTA] Firmware served to {:?}", peer);
        }
        DeliveryOutcome::Cancelled => {
            log::info!("[OTA] Firmware delivery to {:?} cancelled by user", peer);
        }
        DeliveryOutcome::Failed(err) => {
            log::warn!("[OTA] Firmware delivery to {:?} failed: {}", peer, err);
        }
    }

    let (status, err_opt, delivered) = match &delivery_outcome {
        DeliveryOutcome::Delivered => ("delivered", None, true),
        DeliveryOutcome::Cancelled => ("cancelled", Some("Cancelled by user".to_string()), false),
        DeliveryOutcome::Failed(e) => ("failed", Some(e.clone()), false),
    };

    finish(
        &app_handle,
        &device_id,
        history_row_id,
        status,
        err_opt.as_deref(),
        firmware_size,
        delivered,
    );
}

enum DeliveryOutcome {
    Delivered,
    Cancelled,
    Failed(String),
}

/// Persists the outcome, emits the appropriate `device-event`, and
/// unregisters the device's stop_flag. Centralised so every exit path
/// in `serve_worker` has identical side-effects.
fn finish(
    app_handle: &AppHandle,
    device_id: &str,
    history_row_id: Option<i64>,
    status: &str,
    error: Option<&str>,
    firmware_size: usize,
    delivered: bool,
) {
    record_delivery(app_handle, history_row_id, status, error);

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
            "error": error.unwrap_or("unknown"),
        })
    };
    let _ = app_handle.emit(
        "device-event",
        DeviceEvent {
            device_id: device_id.to_string(),
            event_type: event_type.to_string(),
            payload,
        },
    );

    if let Some(reg) = app_handle.try_state::<OtaRegistry>() {
        reg.remove(device_id);
    }

    log::info!("[OTA] Server stopped (device={}, status={})", device_id, status);
}

fn get_local_ip() -> Option<String> {
    let socket = std::net::UdpSocket::bind("0.0.0.0:0").ok()?;
    socket.connect("8.8.8.8:80").ok()?;
    let addr = socket.local_addr().ok()?;
    Some(addr.ip().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_cancel_returns_true_when_present_and_flips_flag() {
        let reg = OtaRegistry::new();
        let flag = Arc::new(Mutex::new(false));
        reg.insert("dev-a", flag.clone());

        assert!(reg.cancel("dev-a"));
        assert!(*flag.lock().unwrap());
    }

    #[test]
    fn registry_cancel_returns_false_when_absent() {
        let reg = OtaRegistry::new();
        assert!(!reg.cancel("never-registered"));
    }

    #[test]
    fn registry_remove_clears_entry() {
        let reg = OtaRegistry::new();
        let flag = Arc::new(Mutex::new(false));
        reg.insert("dev-b", flag.clone());
        reg.remove("dev-b");

        // After remove, cancel finds nothing and leaves the (orphaned)
        // flag untouched — the worker has already exited so flipping it
        // would be meaningless anyway.
        assert!(!reg.cancel("dev-b"));
        assert!(!*flag.lock().unwrap());
    }

    #[test]
    fn registry_insert_overwrites_stale_entry() {
        let reg = OtaRegistry::new();
        let old = Arc::new(Mutex::new(false));
        let new = Arc::new(Mutex::new(false));
        reg.insert("dev-c", old.clone());
        reg.insert("dev-c", new.clone());

        assert!(reg.cancel("dev-c"));
        // Only the newest flag is flipped — the previous OTA's flag
        // would already be meaningless because its worker exited.
        assert!(*new.lock().unwrap());
        assert!(!*old.lock().unwrap());
    }
}
