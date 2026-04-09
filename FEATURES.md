# Trellis — Feature Inventory

Single source of truth for all features. Check items as they are implemented and tested.

## Desktop App

### Discovery & Connection
- [x] mDNS device scanning on local network
- [x] Auto-refresh device list (continuous mDNS + health check)
- [x] Device online/offline detection
- [x] Manual IP entry fallback
- [x] USB serial device detection (listing)
- [x] **Saved devices auto-load on app restart (v0.3.3)** — `Discovery::hydrate_from_db` reads `SavedDevice` rows from SQLite into the in-memory map at startup as offline placeholders, so cross-subnet devices added by IP reappear instantly on every consumer (desktop UI, REST API, web dashboard, MQTT bridge). Health check loop restructured to "work, then sleep" so the first probe runs immediately and hydrated devices flip online within ~1 second instead of waiting a 30-second interval.

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
- [x] Drag & drop .bin firmware file
- [x] Upload firmware to selected device (ESP32, native file picker)
- [x] Progress bar during OTA (WebSocket events from device)
- [x] Success/failure notification
- [x] Firmware version tracking (history per device, stored in SQLite)
- [x] Firmware rollback (one-click re-flash previous firmware)
- [x] Firmware version comparison (current vs new)

### Settings & Preferences
- [x] Dark theme (default)
- [x] Scan interval configuration (persisted)
- [x] Device nicknames (inline edit, persisted in SQLite)
- [x] Window state persistence (size, position)
- [x] Minimize to system tray (close hides, tray restores, right-click quit)

### Get Started Wizard (Onboarding)
- [x] **First-run redirect** — new users auto-redirected to `/get-started` on first launch (persisted via `onboarding_completed` setting)
- [x] **4-step guided wizard**: Welcome/Prerequisites → Pick Template → Configure & Flash → Device Appeared
- [x] **5 bundled starter templates**: Blink (LED toggle), Sensor Monitor (analog + text), Smart Relay (switch + timer), Weather Station (temp/humidity/pressure), Greenhouse Controller (soil moisture + pump + grow light)
- [x] **Prerequisite checks**: arduino-cli detection, board core + library dependency check, one-click install for missing deps
- [x] **Template-to-flash pipeline**: select template → customize device name/board/capabilities → compile & flash via USB in one click
- [x] **Device discovery confirmation**: step 4 watches mDNS for the new device, shows success with device info when found, or WiFi provisioning instructions if waiting
- [x] **Dashboard empty state integration**: "Get Started" button alongside "Add by IP" when no devices found
- [x] **Sidebar entry**: always accessible for re-running the wizard
- [x] **Shared sketch generator**: `generateSketch()` extracted to `lib/sketchGenerator.ts`, used by both wizard and New Device page

### Quick Flash (arduino-cli integration)
- [x] Detect arduino-cli installation (version check)
- [x] Compile generated sketch (ESP32 + Pico W FQBN mapping)
- [x] Flash via USB (serial port selector + upload)
- [x] Build output panel (color-coded success/errors)
- [x] Auto-reset on capability/board changes

### App Shell
- [x] Sidebar navigation
- [x] Header with connection status
- [x] About dialog with version

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

### Embedded Web Dashboard (v0.3.0)
- [x] On-device control panel served at `GET /` from PROGMEM (no RAM cost)
- [x] Self-contained: vanilla HTML/CSS/JS, no build step, no external CDN
- [x] Renders all 5 capability types (switch, slider, sensor, color, text)
- [x] Live updates via WebSocket — auto-reconnect on broker drop
- [x] System telemetry tiles (signal, free heap, uptime)
- [x] Mobile-first responsive layout, dark theme, inline SVG favicon
- [x] Default-on, opt-out via `trellis.enableWebUI(false)` (toggleable at runtime)
- [x] Saves ~13 KB of flash when disabled
- [x] Verified end-to-end on real ESP32 hardware

### Embedded Web Dashboard — Polish (v0.3.1)
- [x] Live log viewer panel — collapsible, severity-coloured, ring-buffered (200 lines), pause/clear, unread badge
- [x] OTA progress overlay — start tick, failure tick, reboot detection, auto-reload on success
- [x] Add-to-home-screen hint — mobile-only, iOS/Android-aware wording, dismiss persisted in localStorage
- [x] Apple touch icon (180×180) + theme-color + mobile-web-app-capable meta tags for proper PWA installation

### Embedded Web Dashboard — Cache Invalidation (v0.3.3)
- [x] **ETag-based conditional GET tied to library version + content hash** (`"<TRELLIS_VERSION>-<sha256-prefix>"`). Replaces the previous `Cache-Control: max-age=300` which caused browsers to serve stale UI for up to 5 minutes after a firmware push. The version part is for human inspection; the hash part is the actual cache key, so HTML changes invalidate even if a release forgets to bump the version macro. `scripts/build_web_ui_header.py` now emits `TRELLIS_WEB_UI_HTML_HASH` alongside the PROGMEM byte array. `_http->collectHeaders()` registers `If-None-Match` so the conditional GET path can fire. Verified end-to-end on real ESP32: 200 + ETag on first GET, 304 on matching `If-None-Match`, 200 on mismatch.

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

### Remote Access (v0.4.0)
- [x] **Settings → Remote Access panel** with two transport cards: Cloudflare Tunnel (recommended) and Tailscale Funnel (no-domain alternative). Each card has a step-by-step setup recipe with inline links to upstream installers. No third-party binaries bundled — the user installs `cloudflared` / `tailscale` themselves.
- [x] **Token-aware embedded web UI.** `web_ui.html` reads `localStorage.trellis_api_token` on every fetch and adds `Authorization: Bearer trls_…` when present. On loopback (no token needed) the dashboard works exactly as before. Through a tunnel, the first `/api/*` 401 pops an inline modal asking for a token; the user pastes it once, the page reloads, and every subsequent request carries the header.
- [x] **`GET /` always allowed** by the auth gate via a pre-auth special case in `api.rs::handle_connection`. The HTML itself is harmless static content; every dynamic surface (`/api/*`) still goes through the v0.3.4 token gate.
- [x] **Reachability probe.** New `probe_remote_url` Tauri command + Settings widget that runs a single `GET /api/devices` from the desktop machine through the user's tunnel and back. Classifies the result into `success`, `auth_failed`, `not_trellis`, `tunnel_down`, `network_error`, `timeout`, or `unexpected` and surfaces a human-friendly explanation. URL is persisted between probes; the token is held in component memory only.
- [x] **Safety check** on the Settings panel: amber warning card if zero API tokens are minted ("the tunnel will be reachable but unusable").
- [x] User guide §16 walks through both transports + the reachability probe, including the "mint a token first" prerequisite and a one-paragraph "why not ngrok" note.

### REST API
- [x] HTTP API server on port 9090 (runs alongside desktop app)
- [x] Full device CRUD (list, get, delete, send command, set nickname/group)
- [x] Metrics, logs, alerts, firmware history endpoints
- [x] Groups, schedules, rules, webhooks CRUD
- [x] Settings read/write endpoints
- [x] CORS support for cross-origin access
- [x] CSV metrics export endpoint
- [x] **Bearer token authentication (v0.3.4)** — every non-loopback request must include `Authorization: Bearer trls_…`. Tokens minted in `Settings → API Tokens`, stored as SHA-256 digests, surfaced in plaintext exactly once at creation. Loopback bypass on by default so the desktop app and local CLI work with zero setup; opt-in `require_auth_localhost` setting for defense in depth on shared machines. Friendly HTML 401 page for browser users instead of bare JSON. New `/api/tokens` CRUD endpoints + `auth.rs` module with 6 unit tests covering token shape, hash stability, scheme parsing, and loopback detection. Closes the LAN-exposure surface that v0.3.3 only partially addressed.

### Web UI Dashboard
- [x] Responsive web dashboard (phone + desktop) at localhost:9090
- [x] Device cards with live status, grouped view
- [x] Interactive controls (switch, slider, sensor, color, text)
- [x] Automation overview (schedules, rules, webhooks)
- [x] Settings management (ntfy, groups)
- [x] Auto-refresh (5s polling)
- [x] Zero external dependencies (single embedded HTML file)

## Scenes

- [x] Multi-action scene creation (set multiple devices at once)
- [x] One-click scene execution (sequential command dispatch)
- [x] Scene persistence (localStorage)
- [x] Device/capability/value selector per action

## MQTT Bridge

- [x] In-app MQTT client (rumqttc), worker-thread design
- [x] Settings UI: broker host/port/username/password, base topic, HA discovery prefix, enable + test connection
- [x] Tauri commands: get_mqtt_config, set_mqtt_config, clear_mqtt_password, get_mqtt_status, test_mqtt_connection
- [x] REST API: GET/PUT /api/settings/mqtt, POST /api/mqtt/clear-password, GET /api/mqtt/status
- [x] Last-will availability (`<base_topic>/bridge/availability` retained)
- [x] Bidirectional state sync (Trellis → MQTT and MQTT → device commands)
- [x] Home Assistant MQTT discovery — auto-creates entities for switch/slider/sensor/color/text
- [x] HA diagnostic sensors per device — Signal strength (dBm), Free heap (B), Uptime (s, total_increasing)
- [x] Instant discovery on bridge enable (no 30s wait)
- [x] Republish discovery on broker reconnect (handles broker restart)
- [x] Heartbeat → MQTT mirroring (device telemetry visible in HA without Trellis desktop app open)
- [x] Multi-segment base topics (e.g. `home/iot/trellis`) supported via prefix-strip
- [x] **TLS/encrypted broker connection (v0.3.3)** — `tls_enabled` + optional `tls_ca_cert_path` (PEM, custom CA or system trust roots), wired into both `start()` and `test_connection()`. Settings UI has a CA file picker and auto-bumps port to 8883 when TLS is enabled. Verified end-to-end on broker.emqx.io / broker.hivemq.com (system trust roots) and a local Mosquitto + self-signed CA pair.
- [x] **Password redacted in GET endpoints + sensitive-key blocklist (v0.3.3)** — `MqttConfigPublic` strips `password` from the wire shape with a `has_password` flag for the UI; generic `/api/settings/<key>` returns 403 for `mqtt_config`. Stops the LAN-exposed REST API on `:9090` from leaking the broker password to anyone on the same network.
- [x] **Encrypted password at rest with age (v0.3.3)** — `secret_store.rs` wraps an x25519 identity in the OS keyring (with 0600 file fallback). Wire format `enc:v1:<base64>` for stored passwords. Lazy migration of legacy plaintext blobs on first launch.
- [x] **Empty-password-preserves on save + explicit Clear (v0.3.3)** — `merge_preserving_password()` so the form load+save round-trip doesn't blank the stored password; `clear_mqtt_password` Tauri/REST endpoint and a Clear button in the Settings UI for the explicit-clear UX.

## Automation

- [x] Scheduled actions (cron-based: "turn on pump at 6am daily")
- [x] Conditional rules ("if temp > 30, turn on fan")
- [x] Rule evaluation engine (checks on sensor updates, 30s debounce)
- [x] Webhooks (POST to URL on device_offline, device_online, alert_triggered, sensor_update)
- [x] Device templates (save/load capability configs for firmware generator)
- [x] CSV data export (download sensor history from charts)
- [x] Integrated terminal (run shell commands, arrow-key history)

## Data Management

- [x] Config import/export (full backup of devices, scenes, schedules, rules, webhooks, alerts, templates, groups)
- [x] Automatic data retention cleanup (metrics + logs older than 30 days)
- [x] Device health diagnostics (RSSI warnings, heap warnings)

## Microcontroller Library — Additional API

- [x] `setSwitch(id, value)` — update switch state from firmware
- [x] `setText(id, value)` — update text value from firmware
- [x] `setColor(id, value)` — update color value from firmware
- [x] `getSensor(id)` / `getSwitch(id)` — read current capability values
- [x] `setFirmwareVersion(version)` — custom firmware version string
- [x] `log(severity, message)` — send structured logs to desktop app
- [x] `logInfo()` / `logWarn()` / `logError()` — convenience log methods
- [x] `beginAutoConnect(timeout)` — WiFi provisioning via captive portal AP

## Infrastructure

- [x] GitHub Actions CI: build app (Linux)
- [x] GitHub Actions CI: compile library examples
- [x] GitHub releases with app binaries
- [x] Arduino Library Manager submission (arduino/library-registry#8088 merged 2026-04-07)
- [x] PlatformIO registry submission (nubiraorg/Trellis v0.1.8 published 2026-04-07)
- [x] Lean Arduino LM tarball — `library-release` orphan branch + `scripts/release-library.sh` + `.release-main-sha` dotfile resolver in `release.yml`. Drops the published Trellis-X.Y.Z.zip from ~740 KB / 122 files (entire monorepo) to ~50 KB / 25 files (library only) for v0.3.2 onward. Validated end-to-end at v0.3.2 (after fixing two release-infra bugs the prior session's dry-run missed: the tagged tree had no `.github/workflows/release.yml` so GitHub Actions couldn't fire on the tag, and reading the main SHA from the tag annotation didn't survive `actions/checkout@v4`'s fetch shape — switched to a `.release-main-sha` file). Old tags v0.1.8 → v0.3.1 stay bloated in the LM index (immutable).
