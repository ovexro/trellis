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
- [x] Device grouping / tagging (nicknames, tags, pills on cards)
- [x] Search / filter devices (by name, nickname, IP, platform, chip, tags)
- [x] Device rooms/groups (create groups, assign devices, grouped dashboard view)
- [x] Group management UI (create, edit, delete, color palette)

### Auto-Generated Controls
- [x] Switch (toggle) — maps to `type: "switch"`
- [x] Slider (range) — maps to `type: "slider"`
- [x] Sensor (read-only gauge/value) — maps to `type: "sensor"`
- [x] Color picker — maps to `type: "color"`
- [x] Text display — maps to `type: "text"`
- [x] Controls update in real-time via WebSocket

### Charts & Metrics
- [x] Time-series chart for sensor data
- [x] System metrics panel (RSSI, heap charts — shown for all devices)
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
- [x] Upload firmware to selected device (ESP32, native file picker)
- [x] Progress bar during OTA (WebSocket events from device)
- [x] Success/failure notification
- [x] Firmware version tracking (history per device, stored in SQLite)
- [x] Firmware rollback (one-click re-flash previous firmware)
- [ ] Firmware version comparison (current vs new)

### Settings & Preferences
- [x] Dark theme (default)
- [ ] Scan interval configuration (persisted)
- [x] Device nicknames (inline edit, persisted in SQLite)
- [ ] Window state persistence (size, position)
- [x] Minimize to system tray (close hides, tray restores, right-click quit)

### Quick Flash (arduino-cli integration)
- [x] Detect arduino-cli installation (version check)
- [x] Compile generated sketch (ESP32 + Pico W FQBN mapping)
- [x] Flash via USB (serial port selector + upload)
- [x] Build output panel (color-coded success/errors)
- [x] Auto-reset on capability/board changes

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

### Push Notifications
- [x] ntfy.sh integration (send alerts to phone, no app account needed)
- [x] Push on alert trigger (sensor threshold exceeded)
- [x] Push on device offline
- [x] Settings UI for ntfy topic configuration
- [x] Test notification button

### REST API
- [x] HTTP API server on port 9090 (runs alongside desktop app)
- [x] Full device CRUD (list, get, delete, send command, set nickname/group)
- [x] Metrics, logs, alerts, firmware history endpoints
- [x] Groups, schedules, rules, webhooks CRUD
- [x] Settings read/write endpoints
- [x] CORS support for cross-origin access
- [x] CSV metrics export endpoint

### Web UI Dashboard
- [x] Responsive web dashboard (phone + desktop) at localhost:9090
- [x] Device cards with live status, grouped view
- [x] Interactive controls (switch, slider, sensor, color, text)
- [x] Automation overview (schedules, rules, webhooks)
- [x] Settings management (ntfy, groups)
- [x] Auto-refresh (5s polling)
- [x] Zero external dependencies (single embedded HTML file)

## Automation

- [x] Scheduled actions (cron-based: "turn on pump at 6am daily")
- [x] Conditional rules ("if temp > 30, turn on fan")
- [x] Rule evaluation engine (checks on sensor updates, 30s debounce)
- [x] Webhooks (POST to URL on device_offline, device_online, alert_triggered, sensor_update)
- [x] Device templates (save/load capability configs for firmware generator)
- [x] CSV data export (download sensor history from charts)
- [x] Integrated terminal (run shell commands, arrow-key history)

## Infrastructure

- [x] GitHub Actions CI: build app (Linux)
- [x] GitHub Actions CI: compile library examples
- [x] GitHub releases with app binaries
- [ ] Arduino Library Manager submission
- [ ] PlatformIO registry submission
