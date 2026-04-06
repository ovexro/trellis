# Trellis — Feature Inventory

Single source of truth for all features. Check items as they are implemented and tested.

## Desktop App

### Discovery & Connection
- [ ] mDNS device scanning on local network
- [ ] Auto-refresh device list (periodic + on-demand)
- [ ] Device online/offline detection
- [ ] Manual IP entry fallback
- [ ] USB serial device detection

### Dashboard
- [ ] Device card grid layout
- [ ] Device card: name, status indicator, RSSI, uptime, firmware version, chip type
- [ ] Click card → device detail view
- [ ] Device grouping / tagging (e.g., "Kitchen", "Greenhouse")
- [ ] Search / filter devices

### Auto-Generated Controls
- [ ] Switch (toggle) — maps to `type: "switch"`
- [ ] Slider (range) — maps to `type: "slider"`
- [ ] Sensor (read-only gauge/value) — maps to `type: "sensor"`
- [ ] Color picker — maps to `type: "color"`
- [ ] Text display — maps to `type: "text"`
- [ ] Controls update in real-time via WebSocket

### Charts & Metrics
- [ ] Time-series chart for sensor data
- [ ] System metrics panel (RSSI, heap, uptime)
- [ ] Metric history stored in SQLite
- [ ] Configurable chart time range

### Serial Monitor
- [ ] List USB serial ports
- [ ] Connect to serial port (baud rate selection)
- [ ] Send/receive text
- [ ] Auto-scroll with pause
- [ ] Clear buffer
- [ ] Copy to clipboard

### OTA Updates
- [ ] Drag & drop .bin firmware file
- [ ] Upload firmware to selected device
- [ ] Progress bar during OTA
- [ ] Success/failure notification
- [ ] Firmware version comparison (current vs new)

### Settings & Preferences
- [ ] Dark theme (default)
- [ ] Scan interval configuration
- [ ] Device nicknames (override reported name)
- [ ] Window state persistence (size, position)
- [ ] Minimize to system tray

### App Shell
- [ ] Sidebar navigation
- [ ] Header with connection status
- [ ] About dialog with version

## Microcontroller Library

### Core
- [ ] `Trellis` class — main entry point
- [ ] `begin(ssid, password)` — WiFi connect + start services
- [ ] `loop()` — process events
- [ ] Capability registry (add/remove at runtime)

### Capability Types
- [ ] `addSwitch(id, label, gpio)` — digital output
- [ ] `addSensor(id, label, unit)` — read-only value
- [ ] `setSensor(id, value)` — update sensor reading
- [ ] `addSlider(id, label, min, max, gpio)` — PWM output
- [ ] `addColor(id, label)` — RGB value
- [ ] `addText(id, label)` — text display/input
- [ ] `onCommand(callback)` — custom command handler

### Networking
- [ ] WiFi connection with auto-reconnect
- [ ] mDNS service advertisement (`_trellis._tcp`)
- [ ] HTTP server: `GET /api/info` — capability declaration
- [ ] WebSocket server: `/ws` — real-time commands & telemetry

### OTA
- [ ] HTTP OTA update listener
- [ ] Progress reporting via WebSocket
- [ ] Auto-reboot after successful update

### Telemetry
- [ ] RSSI reporting
- [ ] Free heap reporting
- [ ] Uptime reporting
- [ ] Chip model reporting
- [ ] Firmware version reporting

### Platform Support
- [ ] ESP32 (all variants)
- [ ] Raspberry Pi Pico W
- [ ] Raspberry Pi Pico 2 W
- [ ] Platform abstraction layer (WiFi, mDNS, OTA)

## Protocol

- [ ] `GET /api/info` — JSON capability declaration
- [ ] WebSocket `/ws` — bidirectional messages
- [ ] Command: `{"command": "set", "id": "...", "value": ...}`
- [ ] Event: `{"event": "update", "id": "...", "value": ...}`
- [ ] OTA command: `{"command": "ota", "url": "..."}`
- [ ] OTA progress event: `{"event": "ota_progress", "percent": N}`
- [ ] Heartbeat: `{"event": "heartbeat"}` (periodic)

## Infrastructure

- [ ] GitHub Actions CI: build app (Linux)
- [ ] GitHub Actions CI: compile library examples
- [ ] GitHub releases with app binaries
- [ ] Arduino Library Manager submission
- [ ] PlatformIO registry submission
