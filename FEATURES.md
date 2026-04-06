# Trellis — Feature Inventory

Single source of truth for all features. Check items as they are implemented and tested.

## Desktop App

### Discovery & Connection
- [x] mDNS device scanning on local network
- [x] Auto-refresh device list (continuous mDNS + health check)
- [x] Device online/offline detection
- [x] Manual IP entry fallback
- [x] USB serial device detection (listing)

### Dashboard
- [x] Device card grid layout
- [x] Device card: name, status indicator, RSSI, uptime, firmware version, chip type
- [x] Click card → device detail view
- [ ] Device grouping / tagging (e.g., "Kitchen", "Greenhouse")
- [ ] Search / filter devices

### Auto-Generated Controls
- [x] Switch (toggle) — maps to `type: "switch"`
- [x] Slider (range) — maps to `type: "slider"`
- [x] Sensor (read-only gauge/value) — maps to `type: "sensor"`
- [x] Color picker — maps to `type: "color"`
- [x] Text display — maps to `type: "text"`
- [x] Controls update in real-time via WebSocket

### Charts & Metrics
- [x] Time-series chart for sensor data
- [ ] System metrics panel (RSSI, heap, uptime)
- [x] Metric history stored in SQLite
- [x] Configurable chart time range

### Serial Monitor
- [x] List USB serial ports
- [x] Connect to serial port (baud rate selection)
- [x] Send/receive text
- [x] Auto-scroll with pause
- [x] Clear buffer
- [x] Copy to clipboard

### OTA Updates
- [ ] Drag & drop .bin firmware file
- [x] Upload firmware to selected device (ESP32 only)
- [ ] Progress bar during OTA
- [ ] Success/failure notification
- [ ] Firmware version comparison (current vs new)

### Settings & Preferences
- [x] Dark theme (default)
- [ ] Scan interval configuration (persisted)
- [ ] Device nicknames (override reported name)
- [ ] Window state persistence (size, position)
- [ ] Minimize to system tray

### App Shell
- [x] Sidebar navigation
- [x] Header with connection status
- [ ] About dialog with version

## Microcontroller Library

### Core
- [x] `Trellis` class — main entry point
- [x] `begin(ssid, password)` — WiFi connect + start services
- [x] `loop()` — process events
- [x] Capability registry (add at setup time)

### Capability Types
- [x] `addSwitch(id, label, gpio)` — digital output
- [x] `addSensor(id, label, unit)` — read-only value
- [x] `setSensor(id, value)` — update sensor reading
- [x] `addSlider(id, label, min, max, gpio)` — PWM output
- [x] `addColor(id, label)` — RGB value
- [x] `addText(id, label)` — text display/input
- [x] `onCommand(callback)` — custom command handler

### Networking
- [x] WiFi connection with timeout
- [x] mDNS service advertisement (`_trellis._tcp`)
- [x] HTTP server: `GET /api/info` — capability declaration
- [x] WebSocket server — real-time commands & telemetry

### OTA
- [x] HTTP OTA update handler (ESP32)
- [x] OTA progress reporting via WebSocket
- [x] Auto-reboot after successful update

### Telemetry
- [x] RSSI reporting
- [x] Free heap reporting
- [x] Uptime reporting
- [x] Chip model reporting
- [x] Firmware version reporting

### Platform Support
- [x] ESP32 (all variants) — compiled + tested on hardware
- [x] Raspberry Pi Pico W — compiled
- [x] Raspberry Pi Pico 2 W — compiled
- [x] Platform abstraction layer (WiFi, mDNS, OTA)

## Protocol

- [x] `GET /api/info` — JSON capability declaration
- [x] WebSocket — bidirectional messages
- [x] Command: `{"command": "set", "id": "...", "value": ...}`
- [x] Event: `{"event": "update", "id": "...", "value": ...}`
- [x] OTA command: `{"command": "ota", "url": "..."}`
- [x] OTA progress event: `{"event": "ota_progress", "percent": N}`
- [x] Heartbeat: `{"event": "heartbeat"}` (periodic, every 10s)
- [x] Log event: `{"event": "log", "severity": "...", "message": "..."}`

## Infrastructure

- [x] GitHub Actions CI: build app (Linux)
- [x] GitHub Actions CI: compile library examples
- [x] GitHub releases with app binaries
- [ ] Arduino Library Manager submission
- [ ] PlatformIO registry submission
