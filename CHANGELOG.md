# Changelog

All notable changes to Trellis will be documented in this file.

## [0.3.3] — 2026-04-08

A four-fix maintenance release covering security, UX, and connectivity follow-ups noticed during the v0.2.0 → v0.3.2 sessions. No new top-level features; existing flows get materially safer and more usable.

### Fixed — saved devices auto-load on app restart

- **The bug.** Cross-subnet devices added via "Add by IP" disappeared from the desktop app on every restart and had to be re-added manually. The `deviceStore` had hydration code that loaded saved devices from SQLite as offline placeholders, but it raced with `refreshDevices()`'s `set({ devices })` clobber: Tauri processes sync IPC commands roughly in queue order, so `get_saved_devices` returned first → React appended the offline placeholders → then `refreshDevices` returned and wiped the array. mDNS rediscovered same-subnet devices within 1-2 seconds via the additive event listener (masking the bug in dev), but cross-subnet devices stayed missing. Discovered during MQTT testing with a cross-subnet ESP32.
- **The fix.** Hydration moves into the Rust backend (`Discovery::hydrate_from_db`) so every consumer benefits — desktop UI, REST API at `:9090`, mobile web dashboard, MQTT bridge. Called from `lib.rs` setup between `init_db` and `start_background`. Capabilities and system info are refetched from the device on the first health-check probe (which now runs immediately at startup instead of waiting a full 30-second interval).
- The `health_check_loop` was restructured to "work, then sleep" instead of "sleep, then work". Cross-subnet hydrated devices flip online within ~1 second of app launch instead of 30+ seconds.
- The React-side hydration block was simplified to just enrich existing devices with React-only metadata (nickname/tags/group_id) since the backend now owns the device list — no more ghost-device manufacturing, no more clobber race.
- **Verified end-to-end.** Cross-subnet ESP32 (`trellis-fccfb7c8` at `192.168.1.108` from a `192.168.2.x` dev machine) reappears in `/api/devices` fully populated with `online: true` and four capabilities within 1 second of `trellis` launch.

### Fixed — MQTT broker password no longer leaked over the LAN, encrypted at rest

This started as "encrypt the password at rest" and grew after discovering a more urgent network leak during the implementation read-through.

- **Network leak (sub-fix A).** The REST API binds to `0.0.0.0:9090` (so the mobile web dashboard can reach it from a phone), and `GET /api/settings/mqtt` was returning the **plaintext** broker password in the response body. Anyone on the same WiFi could `curl` it and walk away with credentials that typically grant control of the user's entire smart home. The Tauri `get_mqtt_config` command had the same shape but is only callable from inside the desktop app — not network-exposed but worth fixing for consistency.
- **The redaction.** Introduced a new `MqttConfigPublic` struct that omits `password` and adds a `has_password: bool` flag. Used by both `get_mqtt_config` (Tauri) and `GET /api/settings/mqtt` (REST). The plaintext is never serialized to a user-facing endpoint.
- **Preserve-blank save semantics.** When the form submits a config with an empty `password` field, the backend now keeps the existing stored password rather than blanking it out — the form loads with empty password (because GET redacts it), and a save round-trip would otherwise wipe it on every Save & apply. New `merge_preserving_password()` + `apply_config_from_user()` / `test_connection_from_user()` wrappers apply the merge; the internal `apply_config()` / `test_connection()` stay raw and are used by the trusted startup-load path.
- **Explicit clear path.** New `clear_mqtt_password` Tauri command and `POST /api/mqtt/clear-password` REST endpoint for the "I really want to remove the stored password" UX. Without this, users could never clear a password (empty field = preserve). Settings UI grows a Clear button next to the password field that appears when a password is currently stored.
- **Sensitive-key blocklist.** `GET` and `PUT` to `/api/settings/<key>` now return 403 for any key in the `SENSITIVE_SETTING_KEYS` allowlist (currently just `mqtt_config`). Stops the generic key-value getter from being used to bypass the typed endpoint's redaction.
- **At-rest encryption (sub-fix B).** `secret_store.rs` wraps an `age` x25519 identity stored in the OS keyring (Linux Secret Service via `libsecret`) with a 0600 key file fallback at `<app_data_dir>/secret.key` for headless setups. Wire format for stored passwords is `enc:v1:<base64>` where the base64 payload is binary age ciphertext. The encryption/decryption boundary is the SQLite write/read for `mqtt_config` — wired into `set_mqtt_config` (Tauri), `PUT /api/settings/mqtt` (REST), `clear_mqtt_password`, and the startup load path in `lib.rs`.
- **Lazy migration.** At startup, if the loaded `mqtt_config` has a plaintext password (from a pre-encryption build), the bridge gets it as-is so it keeps working, and the config is re-saved encrypted. Migration completes on the very first launch of this build with no user action.
- **New deps.** `age 0.10`, `base64 0.22`, `keyring 3` (with `sync-secret-service` + `vendored`). `keyring` pulls in vendored openssl, adding ~30 s to a cold build but keeping the binary self-contained for the deb/rpm/AppImage.
- **Settings UI.** Password input loads empty, placeholder switches between `(none)` and `(unchanged — type to update)` based on `has_password`, Clear button appears when a password is currently stored.
- **Verified end-to-end.** `curl GET /api/settings/mqtt` no longer contains a `password` field; `/api/settings/mqtt_config` returns 403; planted a legacy plaintext blob in SQLite, launched trellis, the blob became `enc:v1:<base64>` automatically; second restart decrypted cleanly; PUT with empty password preserved the existing one; POST clear-password emptied it; the bridge stayed connected through all of it.

### Added — TLS broker support (rustls + custom CA)

Builds on the password fixes above. The previous work keeps credentials out of GET responses and SQLite plaintext; this work encrypts them in flight so brokers reachable over the public internet (or any untrusted network) are usable.

- **Two new `MqttConfig` fields**: `tls_enabled: bool` (default `false` for back-compat with existing local-broker setups) and `tls_ca_cert_path: Option<String>` (None = system trust roots, Some = read PEM from this path and use **only** this CA). Both have `serde` defaults so legacy configs from pre-TLS builds parse cleanly.
- **rumqttc 0.24's default feature is `use-rustls`**, so TLS is already linked — no Cargo.toml changes for the TLS code path. New `build_tls_transport()` helper constructs the right `Transport` variant from the `MqttConfig` and is wired into both `MqttBridge::start()` (live bridge) and `::test_connection()` (Settings UI test button).
- **Settings UI** grows a collapsible TLS subsection: enable checkbox + Tauri-dialog file picker for the CA cert (with PEM extension filter) + helper text. Toggling TLS on auto-bumps the broker port from 1883 → 8883 if it was still on the plaintext default.
- **`MqttConfigPublic` exposes the new fields** — they're not sensitive (the CA path is just a filesystem location, `tls_enabled` is operational state).
- **Verified end-to-end.** Public brokers `broker.emqx.io:8883` and `broker.hivemq.com:8883` connect cleanly using system trust roots. Local Mosquitto on `:18883` with a self-signed CA + server cert pair connects when pointed at the CA path, fails with `UnknownIssuer` without it (correct behaviour), and a bogus CA path returns a clean file-not-found error (no panic). TLS config persists across `trellis` restart and the bridge auto-reconnects.
- **Two rustls strictness gotchas worth knowing.** `test.mosquitto.org:8883` fails with `UnsupportedCertVersion` because their cert has a non-RFC-compliant version field — rustls is stricter than OpenSSL. Production brokers (the ones users actually integrate against) work fine. Self-signed certs generated with the default `openssl req -x509` don't include `basicConstraints=CA:TRUE`, so rustls rejects them as a CA with `CaUsedAsEndEntity` — users with self-signed setups need either the basic-constraint extension OR a proper CA + server cert pair.
- **`insecure_skip_verify` was intentionally not implemented.** It's a security footgun and there's no realistic case where it makes a user safer than the CA file path. If you have a workflow that needs it, it's an additive follow-up.

### Fixed — embedded web UI cache invalidates correctly across firmware updates

- **The bug.** The on-device dashboard sent `Cache-Control: public, max-age=300`, meaning browsers cached on the old HTML wouldn't pull new HTML for up to 5 minutes after an OTA push. Hot-reload during library development had the same friction — every `web_ui.html` edit needed a hard-reload or a 5-minute wait.
- **The fix.** ETag-based conditional GET tied to a content hash:
    `"<TRELLIS_VERSION>-<sha256-prefix-of-HTML>"`
  e.g. `"0.3.3-c443bd0afb4c2bfd"`. The version prefix is for human inspection (curl the / endpoint, see what firmware you're talking to). The content-hash suffix is the actual cache key — if the embedded HTML changes, the hash changes, the ETag changes, browsers pull the new content. **Critically, this means a release that forgets to bump `TRELLIS_VERSION` still gets correct cache invalidation as long as the HTML actually changed.** Belt and suspenders.
- `scripts/build_web_ui_header.py` emits a new `TRELLIS_WEB_UI_HTML_HASH` constant alongside the existing PROGMEM byte array — first 16 hex chars of `sha256(html)`. 64 bits is collision-negligible for ETag purposes and keeps the header compact.
- `TrellisWebServer::begin` now calls `_http->collectHeaders()` with `If-None-Match` before `_http->begin()`. The Arduino `WebServer` library drops unregistered request headers silently — without this, the conditional GET path never fires and there's no error to debug.
- `Cache-Control` becomes `no-cache, must-revalidate` (browser must revalidate every load, but can reuse the cached body when the server says 304).
- **Verified end-to-end on real ESP32**: first GET → 200 + ETag + 25668-byte body; GET with correct `If-None-Match` → 304 + empty body + ETag header still set; GET with wrong `If-None-Match` → 200 + full body. All five examples × ESP32/Pico W still compile clean (sizes match v0.3.1 baseline; ETag code adds <200 bytes). `arduino-lint --library-manager update` clean.

### Fixed — `TRELLIS_VERSION` macro in sync with the published library version

- **The bug.** The v0.3.2 release left `TRELLIS_VERSION` in `src/Trellis.h` on `"0.3.1"` because `reference_build.md`'s procedural recipe omitted it from the version-bump checklist (even though `feedback_release_sync.md` had it listed). The published v0.3.2 library binary reported the wrong version internally — and now that the embedded UI ETag depends on this macro, a mismatch would skip cache invalidation entirely.
- **The fix.** Bumped the macro to `"0.3.3"` for this release. The release procedure documentation has been updated to make `src/Trellis.h` the sixth file in the version-bump list (was five). The content-hash half of the ETag is a backstop that catches HTML changes even if you forget the version bump, but it's not a substitute for keeping the macro in sync.

### Notes

- No new user-facing features. Five existing flows (saved-devices restore, MQTT password handling, MQTT TLS, web-UI cache, version macro) get materially safer or more usable.
- All four follow-up tasks were verified end-to-end on real hardware: cross-subnet ESP32 at `192.168.1.108` (saved-devices fix), local Mosquitto on `127.0.0.1:18883` with a self-signed CA pair (TLS), `test/TestDevice` flashed via `/dev/ttyUSB0` (ETag round-trip), and the encrypted MQTT password migration was exercised against the live SQLite store.

## [0.3.2] — 2026-04-07

### Release infrastructure

- **Lean Arduino Library Manager tarball.** The published `Trellis-X.Y.Z.zip` on Arduino LM drops from ~740 KB / 122 files (entire monorepo, including the Tauri desktop app source, both lockfiles, and 530 KB of screenshots) to ~50 KB / 25 files (library only). No library code changes — same `src/`, same examples, same API. Achieved by tagging future releases from a lean orphan `library-release` branch (managed by `scripts/release-library.sh`) instead of from `main`. The desktop CI still builds from main's tree by reading a `main-sha:` line embedded in the tag annotation.
- **Why this was needed.** The Arduino Library Manager indexer (`arduino/libraries-repository-engine`) walks the cloned repo with `filepath.Walk()` and only excludes SCCS dirs, symlinks, and dotfiles — it does **not** honor `.gitattributes export-ignore`. Our `git archive` produced a clean ~50 KB tarball already, but the indexer ignored it. Forcing the indexer to see only library files required a separate, lean commit at the tag.
- **Old tags unchanged.** v0.1.8 → v0.3.1 stay bloated in the LM index (immutable index entries). Only v0.3.2 onward will be lean. PIO is unaffected — it has always honored `library.json export.include`.
- **Releases must now use the script.** `./scripts/release-library.sh vX.Y.Z` is the only supported way to tag a release; raw `git tag && git push` will fail loudly because `release.yml` requires the `main-sha:` line in the tag annotation.

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
