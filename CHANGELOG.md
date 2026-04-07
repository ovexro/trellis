# Changelog

All notable changes to Trellis will be documented in this file.

## [0.3.1] — 2026-04-07

### Added — embedded web dashboard polish pass

Three on-device dashboard features that turn the v0.3.0 control panel into a real debug + monitoring console for any phone:

- **Live log viewer panel.** A "Logs" chip in the dashboard header opens a collapsible panel that streams `event:"log"` WebSocket messages from the device in real time. Severity-coloured rows (info / warn / error), monospace formatting, scrollable ring buffer of the last 200 lines, Pause and Clear controls, and an unread-count badge on the chip when the panel is collapsed. The library already broadcast logs via `broadcastLog`/`logInfo`/`logWarn`/`logError` — this pass is purely the consumer side, no C++ changes required.
- **OTA progress overlay.** When a device emits `event:"ota_progress"` (currently the start tick at `percent: 0` and the failure tick at `percent: -1`), a full-screen modal appears with a progress bar, status text, and reboot detection. On a successful update the WebSocket closes when the device reboots; the overlay flips to "Restarting device…" and auto-reloads the page once the device is back so the new firmware version appears immediately. On failure the overlay shows a red error state with a dismiss button.
- **Add to Home Screen hint.** A one-time bottom sheet that appears on mobile viewports (`window.innerWidth < 768` plus iOS or Android UA detection) with platform-specific wording ("Tap Share, then Add to Home Screen" on iOS / "Tap menu (⋮), then Add to Home Screen" on Android). Skipped automatically when already running standalone or after the user dismisses it (stored in localStorage). Paired with `mobile-web-app-capable`, `theme-color`, and a 180×180 `apple-touch-icon` so iOS Safari renders the saved icon properly with no manifest URL.

### Polish pass

- **Header layout reflow** to accommodate the new chip without breaking on narrow viewports. The title block now uses a dedicated `.ttl` flex child with `min-width:0` and ellipsis overflow so the device name truncates instead of pushing the chip off-screen, and the chip is `flex:none` so it always reserves its slot.
- **Defense-in-depth XSS hygiene** for the new code paths: log message bodies render through `textContent`, severity is filtered to a known whitelist before being interpolated into class names, and the OTA progress percent is `Math.max(0,Math.min(100,p|0))` clamped before being used as a CSS width.
- **Latent bug fix**: the `info` global was being implicitly created via `info=d` in `loadInfo()` (declared nowhere, leaked to `window`). Now declared in the IIFE-scope `var` list alongside `caps`/`ws`/etc. Spotted while reading the code for the polish pass.
- **`overflow-x:hidden` on body** as a safety net so any future flex/grid mishap can't trigger horizontal scroll on phones.
- **OTA reset semantics**: `otaShow()` now resets state on every call (clears the timer, removes `.fail`, resets the bar) so a fresh OTA after a previous failure starts cleanly without forcing the user to dismiss the old overlay first.
- **Log unread counter** correctly resets to zero both when the panel is opened *and* when it's resumed from a paused state.
- **Hardware-verified end-to-end** on real ESP32: TestDevice flashed via `/dev/ttyUSB0`, HTTP fetch returns the new 25 KB byte-clean HTML with all three feature markers present, WebSocket round-trip exercises both the existing command path (`set led true` → device acts → update broadcast) and the new log path (`logInfo` from the `onCommand` callback + periodic ticks received). Headless Chrome screenshots at desktop and mobile viewports confirm the chip + responsive grid + PWA hint render correctly.

### Notes

- Headless Chrome (`google-chrome --headless=new`) has a hard minimum viewport width of ~500 px regardless of `--window-size`. Mobile screenshot tests of viewports narrower than that are unreliable and will appear right-clipped — the actual page layout is fine, the screenshot just isn't capturing what the rendering engine reports. Use puppeteer/playwright with `Page.setViewport` (CDP `Emulation.setDeviceMetricsOverride`) for true narrow-viewport tests.
- Embedded HTML grew from ~13 KB to ~25 KB; ESP32 flash usage stays at 82-83 % across all five examples, Pico W at ~22 %.

## [0.3.0] — 2026-04-07

### Added

- **Embedded web dashboard on the device**. Trellis devices now serve a self-contained control panel at `GET /` straight from PROGMEM. Flash any example, open `http://<device-ip>/` from your phone or laptop, and you get a polished dark-theme dashboard with live toggles for switches, sliders for sliders, sensors with units, native color picker, and text input — all driven by the existing `/api/info` + WebSocket protocol. No desktop app required, no install, no cloud, no second device. Verified end-to-end on a real ESP32 (HTTP fetch + WS round-trip + all 5 capability types rendered).
- The dashboard is a single 13 KB vanilla HTML/CSS/JS file (`src/web_ui.html`) embedded as a PROGMEM byte array (`src/TrellisWebUI_html.h`, regenerated by `scripts/build_web_ui_header.py`). Streamed to clients via `WebServer::send_P` so it never lands in RAM. Includes inline SVG favicon, Apple home-screen meta tags, mobile-first responsive grid (single column on phones, two columns on tablet+), a 5-item sensor/heap/uptime/RSSI tile bar, and live WebSocket reconnect with status pill.
- New library API: `Trellis::enableWebUI(bool enabled = true)`. Default-on so existing sketches inherit the feature for free; pass `false` before *or after* `begin()` to disable (the route handler checks the flag at request time, so toggling at runtime works). Saves ~13 KB of flash when disabled.
- All five examples (BasicSwitch, TemperatureSensor, RGBLed, GreenhouseController, AutoConnect) had their header comments updated to mention the embedded dashboard so users discover it without reading source.
- Library version constant fixed: `TRELLIS_VERSION` was stuck at "0.1.5" through several releases — now properly tracks the actual library version and bumps to "0.3.0".

### Polish pass

- **Defense-in-depth XSS escape** for cap labels/units in the embedded JS. The values are sketch-author-controlled at compile time so they're trusted, but the JS now uses `textContent` everywhere it can and a tiny `esc()` helper for the few innerHTML paths (slider min/max ranges, sensor units). Cheap insurance against future paths that might accept untrusted input.
- **Inline SVG favicon** to avoid the spurious `/favicon.ico` 404 (and the wasted ESP32 request handler tick) on every page load.
- **Cache-Control headers**: dashboard HTML is `public, max-age=300` so phones don't re-download 13 KB on every refresh; `/api/info` is `no-store` to keep capability lists fresh.
- **Generator script** (`scripts/build_web_ui_header.py`) replaces the ad-hoc inline-Python generator. Strips POSIX trailing newline from the source HTML so the served body is byte-clean.
- **Cross-platform sanity check**: all five examples compile clean on both ESP32 (~82% flash) and Raspberry Pi Pico W (~21% flash) with the new feature.

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
