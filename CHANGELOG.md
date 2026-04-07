# Changelog

All notable changes to Trellis will be documented in this file.

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
