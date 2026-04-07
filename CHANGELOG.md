# Changelog

All notable changes to Trellis will be documented in this file.

## [0.2.1] — 2026-04-07

### Fixed (MQTT bridge — caught by HA Docker bonus test)

The HA Docker integration test surfaced two related bugs in the MQTT bridge that v0.2.0's polish pass missed: rumqttc's internal reconnect does NOT replay either retained publishes or subscriptions, so a broker restart left the bridge "connected but functionally broken".

- **Republish retained `online` availability on every ConnAck** (polish #4). When the broker restarts (or the network blips long enough for the TCP connection to drop), Mosquitto fires our LWT (`offline`) on the availability topic. rumqttc reconnects under us and the bridge keeps publishing state — but the availability topic still reads `offline` until something forces a republish, so HA marks every entity unavailable. The fix re-asserts `online` in the ConnAck handler.
- **Re-subscribe to `<base_topic>/+/+/set` on every ConnAck** (polish #5). The original `start()` calls `client.subscribe()` once at startup. rumqttc reconnects automatically when the connection drops, but it does NOT replay subscriptions, so after a broker restart the bridge is "connected" yet deaf — HA toggles never reach the device, even though the messages are visible to other subscribers. The fix re-asserts the subscription in the ConnAck handler. Idempotent.

Both fixes live alongside polish #2 (republish HA discovery configs) in the same `event_loop` ConnAck branch, since they all need to fire on the same trigger.

## [0.2.0] — 2026-04-07

### Added

- **MQTT bridge with Home Assistant discovery**. Trellis now ships an in-app MQTT bridge that mirrors every device's capabilities to a user-configured broker. When enabled, switches/sliders/sensors/colors/text capabilities are auto-published as MQTT topics under `<base_topic>/<device_id>/<cap_id>/state` and accept commands at `…/set`. With Home Assistant MQTT discovery enabled (default), devices appear in HA's UI as native entities with no YAML — switches map to `switch`, sliders to `number`, sensors to `sensor` (with units), colors to RGB `light`, and text to `text`.
- New `Settings → MQTT bridge` panel: broker host/port, username/password, base topic, HA discovery prefix + toggle, enable toggle, "Test connection" button, live status indicator (connected / enabled-but-disconnected / disabled, with last-error message and pub/sub counters).
- New Tauri commands: `get_mqtt_config`, `set_mqtt_config`, `get_mqtt_status`, `test_mqtt_connection`.
- New REST endpoints on `:9090`: `GET /api/settings/mqtt`, `PUT /api/settings/mqtt`, `GET /api/mqtt/status`. The web dashboard (and any external script) can now drive the bridge config without the desktop UI.
- **Last-will availability**: when the bridge connects it publishes `online` (retained) to `<base_topic>/bridge/availability`, and the broker auto-publishes `offline` if Trellis crashes or disconnects. HA uses this to mark entities as unavailable.

### Architecture notes

- The bridge runs as a worker thread that owns the rumqttc Client + EventLoop, started/stopped from `MqttBridge::apply_config`. When config changes the worker is cleanly stopped (offline retain message + disconnect + thread join) before a new one is spawned.
- Inbound commands are routed through the existing race-free `ConnectionManager::send_to_device` path (the post-v0.1.6 fix), so MQTT-driven commands are subject to the same correctness guarantees as the REST and Tauri command paths.
- HA discovery configs are deduped — they only republish when a device's capability list actually changes (firmware update / capability add). This avoids spamming the broker on every health-check tick.
- Empty/whitespace `base_topic` and `ha_discovery_prefix` fall back to defaults; trailing slashes are stripped. Multi-segment base topics (e.g. `home/iot/trellis`) are supported via prefix-stripping rather than naive segment counting.
- Password is stored in the SQLite settings table as plain text (same security model as the rest of the app's local-only state). TLS connections to the broker are not yet supported — MVP scope.

### Polish pass

- **Instant HA discovery on bridge enable** — `apply_config` now immediately publishes discovery configs for all currently-known devices instead of waiting for the next 30s health-check tick. Toggling the bridge on in Settings → MQTT bridge populates HA within ~1 second.
- **Republish HA discovery on broker reconnect** — the worker thread re-emits discovery configs for every known device on every successful `ConnAck`. Handles broker restarts (where retained configs are lost), transient network drops, and the laptop sleeping/waking. Idempotent: the dedupe tracker is cleared first so even already-tracked devices re-announce.
- **HA sensors for device system telemetry** — every Trellis device now gets three extra HA sensor entities (Signal strength, Free heap, Uptime) in the `diagnostic` entity category. The bridge listens for `heartbeat` events on the device WebSocket and publishes the values to `<base_topic>/<device_id>/_sys/<field>/state`. HA users can graph weak-signal warnings and memory leaks without needing the Trellis desktop app open.

### Known limitations

- The Settings UI doesn't yet show the running config diff vs the saved config; clicking "Save & apply" applies whatever is currently in the form.

## [0.1.8] — 2026-04-07

### Changed (BREAKING — repo layout)

- The Arduino library files now live at the **repository root** instead of inside `library/`. The Arduino Library Manager indexer requires `library.properties` to sit at the repo root, so this is the only way to be indexed without splitting the project into a separate repo.
- New layout at root: `src/`, `examples/`, `library.properties`, `library.json`, `LICENSE`, `README.md`. The desktop app still lives under `app/`.
- Local Arduino IDE users developing against the source: re-symlink as `ln -s /path/to/trellis ~/Arduino/libraries/Trellis` (was `…/trellis/library`).
- Added `.gitattributes` with `export-ignore` directives so `git archive` tarballs (used by some Library Manager tooling) exclude `app/`, `docs/`, `screenshots/`, `.github/`, `install.sh`, and project-meta markdowns from the library distribution.
- Updated CI (`.github/workflows/build.yml`) to compile examples from the new `examples/` path and symlink the repo root into `~/Arduino/libraries/Trellis`.
- Updated `CONTRIBUTING.md` and `README.md` repo trees + dev install instructions.

This is a no-op for desktop app users — `Trellis_0.1.8_amd64.deb` is identical in behavior to `0.1.7`. The change only affects the Arduino library publishing path.

## [0.1.7] — 2026-04-07

### Added
- `library/LICENSE` — MIT license bundled inside the Arduino library directory so it ships with installs from Library Manager.
- `library.properties`: `includes=Trellis.h` — lets Arduino IDE auto-add the include statement on install.
- `library.json`: `AutoConnect` example registered for PlatformIO (was missing).

### Changed
- `library.json`: `frameworks` is now an array (`["arduino"]`) per PlatformIO schema preference.

These cleanups make the library pass `arduino-lint --library-manager submit` with zero errors and zero warnings, in preparation for Arduino Library Manager and PlatformIO Registry submissions.

## [0.1.6] — 2026-04-07

### Fixed
- **Critical**: desktop command relay race that dropped switch/slider/OTA commands. `send_to_device` opened a short-lived WebSocket per command and called `socket.close()` before the device's `WebSocketsServer.loop()` could dispatch the text frame to `processCommand()`. The frame was sitting in the device buffer when the disconnect tore it down, so commands appeared "sent" to the desktop but never landed on the device. Reproducible across **all** capability types (switch, slider, color, text, OTA). Discovered during hardware-test gate that should have run before v0.1.5.

### Changed
- `send_to_device` now pushes commands through an `mpsc::channel` into the existing persistent `ws_reader_loop`, which writes them on the same WebSocket it reads events from. Eliminates the short-lived-connection race entirely. A one-shot fallback (with a 200ms hold-off before close) is preserved for the brief race window before discovery establishes the persistent connection.
- Reader loop's socket read timeout dropped from 2s to 50ms so outbound commands are flushed promptly.

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
