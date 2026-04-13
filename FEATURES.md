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

### Home Overview (post-v0.6.0)
- [x] **Home page as default landing view** — new Home page in both desktop app (React, `/` route) and web dashboard (`:9090`, first tab). Shows system status strip (online/offline device counts, MQTT/Sinric bridge status dots), live sensor readings grid (all sensor capabilities across all devices with current values), quick controls (functional inline switches, sliders, color pickers across all devices — offline devices dimmed), and recent activity feed (last 30 cross-device events from `GET /api/activity` endpoint, severity color-coded, clickable to navigate to device). Real-time updates via Zustand store (desktop) and WebSocket push (web dashboard). Activity feed backed by new `idx_logs_timestamp` index for efficient cross-device queries.
- [x] **Per-capability favorites / pinning (post-v0.6.0)** — star toggle on every sensor and control card in the Home page (desktop app and web dashboard). Clicking the star pins that specific capability to a "Favorites" section at the top of the Home page; non-favorited capabilities remain in the regular "Live Readings" and "Quick Controls" sections below (no duplication). Favorites persisted in SQLite (`favorite_capabilities` table with device_id + capability_id composite key). Tauri command `toggle_favorite_capability` + REST API `POST /api/favorites/toggle` and `GET /api/favorites` endpoints. Admin-only toggle (viewers see stars but cannot click). Optimistic UI with revert on failure in both desktop (Zustand store) and web dashboard. Amber filled star for active, gray outline for inactive.

### Floor Plan (post-v0.6.0)
- [x] **Spatial device layout** — new Floor Plan page in both desktop app (React, `/floor-plan` route, Map icon in sidebar) and web dashboard (`:9090`, new tab). Freeform canvas where devices are dragged from a sidebar panel and placed at percentage-based (x, y) positions. Placed nodes show live status dot (online/offline), primary capability value (sensor reading, switch state), and device name. Click a node to open an inline popup with all sensor readings and interactive controls (switches, sliders) — sends commands via existing `send_command` pipeline. Drag nodes to reposition; positions persisted in SQLite (`device_positions` table). Background image support per floor. Server-side x/y clamping to 0–100. Web dashboard includes touch support for mobile drag. Device deletion cascades to positions and favorites. Viewers can see the floor plan but cannot move devices or change background.
- [x] **Multi-floor support (post-v0.7.0)** — new `floor_plans` table (id, name, sort_order, background). Tab bar above the canvas lists all floors; click to switch, `+` button to add, right-click for rename/delete context menu. Each floor has its own device positions and background image. Sidebar shows only unplaced devices (not on any floor). Background moved from global settings to per-floor storage. Seamless migration: existing positions and background move to an auto-created "Floor 1". REST API: `GET /api/floor-plans`, `POST /api/floor-plans` (admin), `PUT /api/floor-plans/{id}` (admin), `DELETE /api/floor-plans/{id}` (admin, cascades positions). `GET /api/floor-plan` accepts `?floor_id=N` (defaults to first floor). `PUT /api/floor-plan/position` now requires `floor_id`. Both desktop and web dashboard.
- [x] **Snap-to-grid toggle (post-v0.7.0)** — toggle button in the sidebar snaps device positions to a 4% grid (aligned with the 32px dot pattern). Grid dots turn trellis-accent when enabled and remain visible over background images. Applied to sidebar drops, node drags (mouse + touch). Both desktop and web dashboard.
- [x] **Compact labels toggle (post-v0.7.0)** — toggle in the sidebar switches placed nodes between expanded (name + value, default) and compact (value only, smaller padding). Reduces clutter on crowded floor plans. Both desktop and web dashboard.
- [x] **Undo last move (post-v0.7.0)** — Ctrl+Z / Cmd+Z reverts the last floor plan action: new placement (undo removes), move (undo restores previous position), or removal (undo restores device). Single-level. Both desktop and web dashboard.

### Dashboard
- [x] Device card grid layout
- [x] **Drag-and-drop card reordering (post-v0.4.4)** — device cards can be reordered via drag-and-drop in both the desktop app and the `:9090` web dashboard. Order persists in SQLite (`sort_order` column on `devices` table). `PUT /api/devices/reorder` REST endpoint (admin-only). Viewers see cards in the same order but cannot drag. HTML5 DnD with visual feedback (opacity on drag, accent ring on drop target, grip handle icon).
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
- [x] **Metrics page (post-v0.4.9)** — top-level monitoring overview in the desktop app showing all devices with uptime ribbon, RSSI/heap/sensor charts in a 2-column grid, global time range picker (1h/6h/24h/7d), device status indicators, and "Details" links. Reuses existing MetricChart and UptimeTimeline components via new `externalHours` prop that locks the range and hides per-chart pickers. Matches the `:9090` web dashboard Metrics tab layout.

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
- [x] **OTA from GitHub Releases (post-v0.4.7)** — point to any GitHub repo (`owner/repo` or full URL), app fetches releases via GitHub API, shows .bin firmware assets with tag/date/size, one-click download + flash via existing OTA pipeline. Per-device repo binding persisted in settings. Version comparison highlights the release matching the device's current firmware. REST API endpoints (`GET /api/github/releases`, `POST /api/github/ota`) enable the same flow from the web dashboard. Completes Tier 4 "Firmware OTA from GitHub".
- [x] **GitHub OTA polish (post-v0.4.8)** — download progress bar (chunked reads with per-2% events, shown in both desktop app and web dashboard via WS broadcast), user-friendly error messages (404/403/network mapped to plain-English explanations, web dashboard `api()` reads JSON error bodies), pre-release filtering toggle (hidden by default, checkbox when pre-releases exist, amber badge), asset name filter input (substring match, per-device persistence, hides releases with zero matching assets). All four surfaces: desktop OTA page + web dashboard detail panel.

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
- [x] `setSlider(id, value)` — update slider value + apply PWM
- [x] `addColor(id, label)` — RGB value
- [x] `addText(id, label)` — text display/input
- [x] `onCommand(callback)` — custom command handler

### NVS Persistence (v0.4.7, ESP32 only)
- [x] Switch values persist across reboots — GPIO state applied on boot before first client connects
- [x] Slider values persist across reboots — PWM duty applied on boot before first client connects
- [x] Shared `trellis_cap` NVS namespace, keyed by capability ID (max 15 chars)
- [x] Pico W degrades gracefully — values start at defaults, no persistence

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
- [x] **Real-time OTA progress + delivery confirmation (post-v0.4.4)** — `httpUpdate.onProgress` streams real progress percentages (every 5%) over WebSocket during firmware download. After successful write, device sends explicit `ota_delivered` event before rebooting. Desktop and embedded dashboards show live progress bar and a "Firmware received" confirmation tick. Broadcaster callback decouples OTA from WebSocket library.

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

### Per-Device Dashboard Proxy
- [x] **Reverse proxy at `/proxy/{device-id}/`** on :9090 — forwards HTTP requests to the device's embedded :8080 web server. Passes through the existing Bearer token auth gate so remote users (via Cloudflare Tunnel / Tailscale Funnel) can reach individual device dashboards without direct LAN access.
- [x] **WebSocket bridge at `/proxy/{device-id}/ws`** — detects `Upgrade: websocket` and bridges raw TCP to the device's WS port (:8081). Two-thread copy loop, long-lived, auto-closes when either side drops.
- [x] **HTML rewriting** — proxied root HTML is rewritten so `fetch("/api/info")` becomes relative (resolves through proxy) and the WebSocket URL routes through `/proxy/{id}/ws` instead of `host:port+1`. Protocol-aware (`ws://` or `wss://`) for tunnel compatibility.
- [x] **"Device Dashboard" link** in the desktop app device detail page (online devices only) and in the :9090 web dashboard device cards.

### REST API
- [x] HTTP API server on port 9090 (runs alongside desktop app)
- [x] Full device CRUD (list, get, delete, send command, set nickname/group)
- [x] Metrics, logs, alerts, firmware history endpoints
- [x] Groups, schedules, rules, webhooks CRUD
- [x] Settings read/write endpoints
- [x] CORS support for cross-origin access
- [x] CSV metrics export endpoint
- [x] **Bearer token authentication (v0.3.4)** — every non-loopback request must include `Authorization: Bearer trls_…`. Tokens minted in `Settings → API Tokens`, stored as SHA-256 digests, surfaced in plaintext exactly once at creation. Loopback bypass on by default so the desktop app and local CLI work with zero setup; opt-in `require_auth_localhost` setting for defense in depth on shared machines. Friendly HTML 401 page for browser users instead of bare JSON. New `/api/tokens` CRUD endpoints + `auth.rs` module with 13 unit tests covering token shape, hash stability, scheme parsing, loopback detection, expiry, and rate limiting. Closes the LAN-exposure surface that v0.3.3 only partially addressed.
- [x] **Token expiry / TTL (post-v0.4.3)** — optional `expires_at` on API tokens. TTL options: 1h, 24h, 7d, 30d, 90d, or never (default, backward-compatible). Auth gate rejects expired tokens with a distinct error message. Settings UI has a TTL dropdown in the create form and an Expires column with color-coded status (red=expired, amber=<24h). REST API `POST /api/tokens` accepts optional `ttl` field.
- [x] **Rate limiting + failed-auth backoff (post-v0.4.3)** — per-IP in-memory rate limiter with exponential backoff. After 3 grace failures, requests are rejected with 429 (1s → 2s → 4s → ... capped at 60s). Auto-resets after 15 minutes of silence or on successful auth. Loopback exempt. 5 unit tests.
- [x] **Role-based access control (post-v0.4.4)** — each API token carries a role: `admin` (full access) or `viewer` (read-only). Viewers can read devices, metrics, status, schedules, rules, webhooks, and token metadata but cannot send commands, trigger OTA, manage tokens, change settings, or access the device proxy. Existing tokens default to admin (backward-compatible). `Role` enum in `auth.rs`, `require_admin()` guard on 17 mutating endpoints + proxy. Settings UI has a role dropdown in the create form, role column in the token table, and role badge in the created-token modal. Web dashboard at `:9090` calls `GET /api/auth/whoami` on load to detect viewer tokens: disables toggles/sliders/color pickers/text inputs, hides group/ntfy write controls, and shows a "Read-only" badge in the header. 14 unit tests (including role parsing).

### Web UI Dashboard
- [x] Responsive web dashboard (phone + desktop) at localhost:9090
- [x] Device cards with live status, grouped view
- [x] Interactive controls (switch, slider, sensor, color, text)
- [x] Automation overview (schedules, rules, webhooks)
- [x] Settings management (ntfy, groups)
- [x] Auto-refresh (5s polling, fallback only)
- [x] Zero external dependencies (single embedded HTML file)
- [x] **WebSocket push (post-v0.4.4)** — persistent `/ws` connection replaces polling with instant device event delivery. Device state changes, heartbeats, logs, and discovery events (online/offline) arrive in real time. `WsBroadcaster` fan-out in Rust feeds all connected browser clients. Query-param token auth (`/ws?token=trls_...`) for remote access since browser WebSocket API can't set custom headers. Loopback bypass applies. Polling auto-resumes as fallback on WS disconnect. Green/gray connection indicator dot in header.
- [x] **PWA support (post-v0.4.4)** — web app manifest (`/manifest.json`) with standalone display mode, themed SVG icons, and dark background. Service worker (`/sw.js`) caches the HTML shell for offline display with network-first strategy. Mobile install prompt banner for "Add to Home Screen". Both routes served pre-auth (same as `GET /`).
- [x] **Notification preferences (post-v0.4.4)** — "Browser Notifications" settings section with three toggles: device offline (default on), device online (default off), and error logs (default off). Persisted to `localStorage`. Permission status label (Allowed/Blocked/Not yet requested) with color coding. Permission requested on first toggle-on. Viewers see toggles but cannot change them. Log error events from WS push now handled and can trigger notifications.
- [x] **Per-device notification filtering (post-v0.4.4)** — "Per-Device Overrides" section below global toggles. Each device gets three buttons (Offline/Online/Errors) that cycle through inherit (gray dashed border) → on (green) → off (red). Overrides stored in `localStorage` (`trellis_notif_device_prefs`), checked via `shouldNotify(key, deviceId)` before firing browser notifications. Absent overrides follow the global setting. Viewers see buttons but cannot change them.
- [x] **Device detail panel (post-v0.4.4)** — click "Details" on any device card to open a right-side slide-out panel showing: device info (status, ID, IP, firmware), system metrics (RSSI, free heap, uptime, chip) in a 2-column stat grid, interactive controls (same as card expansion), link to per-device embedded dashboard, and 20 most recent log entries with severity coloring. Closes on overlay click, close button, or Escape key.
- [x] **Device detail panel polish (v0.4.6)** — firmware history timeline (up to 5 past uploads with version, date, file size), inline SVG sparklines for RSSI and free heap (last 1h of stored metrics, 120×24px), and mobile-responsive layout (full-width panel, single-column stat grid, tighter spacing at ≤640px). All async fetches run in parallel.
- [x] **Notification sound (v0.4.6)** — optional audio chime when a browser notification fires. Two-tone sine wave (880Hz → 1175Hz, 0.4s) generated via Web Audio API — no external audio file. Toggle in Settings → Browser Notifications, default off. Respects viewer/unsupported disabled states.
- [x] **Interactive metric charts (v0.4.6)** — full SVG time-series charts in the device detail panel for WiFi Signal (amber), Free Heap (blue), and all sensor capabilities (green). Time range picker (1h / 6h / 24h / 7d), hover tooltips with crosshair + value + timestamp, touch support for mobile, auto-scaled Y-axis with grid lines, smart X-axis labels (HH:mm for ≤24h, Mon D HH:mm for 7d). Bucket downsampling caps at 200 points for smooth rendering with large datasets. Zero external dependencies.
- [x] **Metrics tab (v0.4.6)** — top-level monitoring overview showing all devices with 2-column chart grids, global time range picker (1h / 6h / 24h / 7d), online/offline status dots, and "Details" links to the slide-out panel. Reuses the interactive SVG chart renderer. Mobile-responsive (single column at ≤640px).
- [x] **Chart event annotations (v0.4.6)** — point-in-time event markers overlaid on every metric chart: OTA firmware uploads (blue), online/offline state transitions (green/red), device-reported errors and warnings (amber). Markers render as color-coded vertical dashed lines with circular hover targets; SVG `<title>` provides a native browser tooltip with the kind, label, and exact timestamp. A legend row appears below the chart listing only the kinds present in the current window. State transitions are persisted from `discovery.rs` into `device_logs` with `severity="state"` so they survive across restarts. New `GET /api/devices/{id}/annotations?hours=N` endpoint unions `firmware_history` + `device_logs` (state/error/warn), sorted oldest-first, capped at 200 per request. Fetched once per device in parallel with metrics and reused across all charts in the device detail panel and Metrics tab.
- [x] **Annotation click-through (v0.4.6)** — clicking any event marker on a metric chart opens the device detail panel and scrolls to + flash-highlights the underlying log row (state/error/warn) or firmware history row (ota). Each marker carries its kind, timestamp, and label as data-* attributes plus a 6px transparent hit target above the 3px visible circle for tap-friendly mobile use. Detail panel logs section now fetches up to 200 entries (was 20) and renders the full firmware history (was first 5). For noisy devices where info logs displace older annotation rows from the recent-200 window, a fallback fetch hits `GET /api/devices/{id}/logs?limit=200&severity=state,error,warn` and re-renders the section so the matching row is guaranteed to be present. New `severity` query param on `/logs` is additive — existing call sites and the Tauri `get_device_logs` command keep their unchanged signatures via a new `Database::get_logs_filtered` method.
- [x] **Recent Logs severity filter chips (v0.4.6)** — device detail panel's Recent Logs section now has a chip row above the list: **All / Events / State / Error / Warn / Info / Debug**. Chips reuse the `.chart-range-btn` style for visual consistency with the chart range picker and inherit its mobile tuning. 'Events' maps to `state,error,warn` (the same set that backs chart annotations, matching the Metrics tab legend). Clicking a chip re-fetches `/logs?limit=200&severity=...` and re-renders the list in place; clicking 'All' restores the unfiltered view. Shared `renderLogsList` helper dedupes the rendering logic between `openDeviceDetail` and `setLogFilter`. Stale-fetch guard via `currentLogDeviceId` check before and after the `await` prevents a mid-switch race from writing old results into the new panel. The annotation click-through fallback now calls `setLogFilter('events')` instead of doing a hidden inline re-render — the user sees the active chip update, making the filter state explicit and reversible.
- [x] **Uptime timeline (v0.4.6)** — device detail panel has a new **Uptime History** section between System metrics and Controls that renders a horizontal ribbon of online/offline segments over the current chart time range. Green = online, red = offline, gray = leading "unknown" segment before the first recorded transition (transitions only fire on state change, so the prior state is inferred but left gray because we can't prove the device was being tracked before window-start). The last segment always extends to "now" using the most recent transition's kind. Segments derive client-side from the same `/api/devices/{id}/annotations?hours=N` set already fetched for chart markers — no new backend work and no duplicate fetch. Native SVG `<title>` tooltips show `Online/Offline for Xh Ym (start → end)`; clicking a colored segment activates the **State** filter chip and flash-highlights the matching log row in Recent Logs (re-uses the annotation click-through pattern). Strip aligns horizontally with the chart data area (`pad.left=42, pad.right=10`) so visual scrubbing matches the metric charts below it. Empty-window case renders a single gray strip labeled "No state transitions in this window". Re-renders automatically on chart range change because `loadDetailCharts` calls `renderUptimeTimeline` after each fetch. A one-line summary above the ribbon rolls the segments into quantitative stats — `NN.N% online · Xd Yh tracked · N transitions` — where the denominator excludes the leading inferred "unknown" segment so the percentage reflects only the span we actually observed. Empty windows fall through to an italicized "No tracked uptime in this window".

## Scenes

- [x] Multi-action scene creation (set multiple devices at once)
- [x] One-click scene execution (sequential command dispatch)
- [x] ~~Scene persistence (localStorage)~~ Replaced by SQLite backend below
- [x] Device/capability/value selector per action
- [x] **Backend-backed scenes (post-v0.7.0)** — scenes persisted in SQLite (`scenes` + `scene_actions` tables) instead of localStorage. Full CRUD via Tauri commands (`create_scene`, `get_scenes`, `delete_scene`, `run_scene`) and REST API (`GET/POST /api/scenes`, `DELETE /api/scenes/{id}`, `POST /api/scenes/{id}/run`). Scene execution moved to backend (ConnectionManager sends commands to each device) so both desktop app and web dashboard can trigger scenes. New "Scenes" tab in `:9090` web dashboard with Run button per scene. Admin-only for create/delete/run.

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
- [x] **Skip certificate verification (post-v0.4.4)** — `tls_skip_verify` boolean disables TLS cert validation when enabled. Connection is still encrypted but broker identity is not checked — equivalent of `curl -k`. Implemented via rustls `dangerous()` API with a `NoVerifier` that accepts any cert. Settings UI shows checkbox inside the TLS section with an amber security warning when active. Tested live against broker.emqx.io:8883.
- [x] **Password redacted in GET endpoints + sensitive-key blocklist (v0.3.3)** — `MqttConfigPublic` strips `password` from the wire shape with a `has_password` flag for the UI; generic `/api/settings/<key>` returns 403 for `mqtt_config`. Stops the LAN-exposed REST API on `:9090` from leaking the broker password to anyone on the same network.
- [x] **Encrypted password at rest with age (v0.3.3)** — `secret_store.rs` wraps an x25519 identity in the OS keyring (with 0600 file fallback). Wire format `enc:v1:<base64>` for stored passwords. Lazy migration of legacy plaintext blobs on first launch.
- [x] **Empty-password-preserves on save + explicit Clear (v0.3.3)** — `merge_preserving_password()` so the form load+save round-trip doesn't blank the stored password; `clear_mqtt_password` Tauri/REST endpoint and a Clear button in the Settings UI for the explicit-clear UX.

## Sinric Pro Bridge (v0.6.0)

- [x] WebSocket bridge to `wss://ws.sinric.pro` with HMAC-SHA256 signing, worker-thread design (mirrors MQTT bridge architecture)
- [x] Settings UI: API key, secret (encrypted at rest), device mappings with capability selector, test connection
- [x] Tauri commands: get_sinric_config, set_sinric_config, clear_sinric_secret, get_sinric_status, test_sinric_connection
- [x] REST API: GET/PUT /api/settings/sinric, POST /api/sinric/clear-secret, GET /api/sinric/status
- [x] Bidirectional switch mapping (setPowerState)
- [x] Bidirectional slider mapping (setRangeValue / adjustRangeValue)
- [x] Bidirectional color mapping (setColor)
- [x] Outbound sensor reporting (currentTemperature, with humidity auto-discovery)
- [x] Per-capability mapping — optional explicit capability targeting with type-safe resolution (falls back to auto-discovery on type mismatch)
- [x] Secret encrypted at rest (same `enc:v1:` format as MQTT password)
- [x] Device-online check before dispatching inbound voice commands
- [x] Web dashboard Sinric status section (connection dot, message counters, mapping breakdown)
- [x] User guide §22 — complete setup walkthrough

## Automation

- [x] Scheduled actions (cron-based: "turn on pump at 6am daily") — supports both single device/capability actions and full scene execution
- [x] **Scene scheduling (post-v0.7.0)** — schedules can fire entire scenes on a cron schedule. Schedule creation form has a type toggle (Single Action / Scene). `scene_id` column on `schedules` table. Scheduler loads all scene actions and executes sequentially. Scene name shown in schedule cards in both desktop app and web dashboard.
- [x] Conditional rules ("if temp > 30, turn on fan")
- [x] Rule evaluation engine (checks on sensor updates, 30s debounce)
- [x] Webhooks (POST to URL on device_offline, device_online, alert_triggered, sensor_update)
- [x] Device templates (save/load capability configs for firmware generator)
- [x] CSV data export (download sensor history from charts)
- [x] Integrated terminal (run shell commands, arrow-key history)

## Data Management

- [x] Config import/export (full backup of devices, scenes, schedules, rules, webhooks, alerts, templates, groups)
- [x] **Configurable data retention (v0.4.6)** — metrics and device logs cleanup period selectable from Settings: 7 days, 30 days (default, backward-compatible), 90 days, 1 year, or forever (disables cleanup). Cleanup thread reads the setting each hourly cycle. Dropdown in both the desktop Settings page and the `:9090` web dashboard. Viewers can see but not change the setting.
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
