use std::fs;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::thread;

/// Serves a firmware file via HTTP on a random port.
/// Returns the URL that devices can use to download the firmware.
pub fn serve_firmware(firmware_path: &str) -> Result<(String, Arc<Mutex<bool>>), String> {
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
        for stream in listener.incoming() {
            if *stop_clone.lock().unwrap() {
                break;
            }

            match stream {
                Ok(mut stream) => {
                    // Read the request (we don't care about the contents)
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);

                    // Send HTTP response with firmware
                    let header = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        firmware_size
                    );

                    if stream.write_all(header.as_bytes()).is_ok() {
                        let _ = stream.write_all(&firmware_data);
                        let _ = stream.flush();
                    }

                    log::info!("[OTA] Firmware served to {:?}", stream.peer_addr());

                    // One-shot: stop after serving
                    *stop_clone.lock().unwrap() = true;
                    break;
                }
                Err(e) => {
                    log::warn!("[OTA] Accept error: {}", e);
                    break;
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
