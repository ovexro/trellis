# Changelog

All notable changes to Trellis will be documented in this file.

## [0.1.0] — 2026-04-06

### Added

#### Desktop App
- Auto-discovery via continuous mDNS browsing
- Persistent WebSocket connections with auto-reconnect
- Live sensor updates via Tauri event system
- Device cards with RSSI, uptime, firmware version, chip type
- Auto-generated controls: switch, slider, sensor, color picker, text
- Time-series charts with SQLite metric storage (1h/6h/24h/7d)
- Full serial monitor with live streaming, auto-scroll, copy, clear
- OTA firmware updates with native file picker and local HTTP server
- Device persistence: nicknames, tags stored in SQLite
- Search and filter devices on dashboard
- Device logs viewer with severity filtering (info/warn/error)
- Alert rules API (create, toggle, delete)
- System tray with click-to-restore
- Desktop notification support
- Dark theme with green accent
- Manual device add by IP address
- Health check loop (30s, detects offline/online)
- One-command Linux installer (Ubuntu, Mint, Debian, Fedora, Arch)
- GitHub Actions CI: build app + compile Arduino examples
- GitHub Releases with .deb, .rpm, .AppImage packages

#### Arduino Library
- Trellis class with self-description protocol
- 5 capability types: switch, slider, sensor, color, text
- Periodic sensor broadcasts (5s) + system heartbeat (10s)
- Device logging: logInfo(), logWarn(), logError()
- OTA firmware updates (ESP32)
- mDNS service advertisement (_trellis._tcp)
- HTTP API: GET /api/info
- WebSocket: real-time commands and telemetry
- Platform support: ESP32, Pico W, Pico 2 W
- 4 example sketches: BasicSwitch, TemperatureSensor, GreenhouseController, RGBLed
- PlatformIO and Arduino IDE compatible

#### Protocol
- JSON capability declaration
- WebSocket bidirectional messaging
- Set command, update event, heartbeat, log event
- OTA command with progress reporting
