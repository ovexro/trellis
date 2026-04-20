# Changelog

All notable changes to Trellis will be documented in this file.

## [Unreleased]

### Added

- **OTA cancellation.** Every running OTA transfer can now be aborted from the Upload Firmware card on the desktop OTA page (new "Cancel transfer" button appears while a transfer is uploading) or from the web dashboard diagnostics `firmware_update` toast (inline Cancel button after the OTA is triggered). New `ota::OtaRegistry` (`HashMap<device_id, Arc<Mutex<bool>>>`, registered as Tauri-managed state) replaces the `_stop_flag` each `serve_firmware` callsite was discarding. `serve_firmware` rewritten into two cancel-responsive loops — a non-blocking `accept()` poll (~200 ms ticks) handles cancel-before-connect, and a 4 KB chunked write loop with a 500 ms `set_write_timeout` handles cancel-mid-transfer (the rtl8xxxu "Send-Q frozen" scenario from the 2026-04-20 v0.15.0 hardware-test session). On cancel the worker persists `delivery_status = "cancelled"` + `delivery_error = "Cancelled by user"` and emits the existing `ota_delivery_failed` event with `error: "cancelled"` — the desktop OTA page routes that into a new calm "cancelled" UI state instead of the alarming "OTA update failed" banner. New Tauri command `cancel_ota(device_id) -> bool` and admin-gated REST endpoint `POST /api/ota/cancel`. `check_ota_success_rate` filter updated to exclude `"cancelled"` rows from the ratio denominator — user aborts don't count as delivery failures. 5 new unit tests (59 → 64 library tests).

## [0.15.0] — 2026-04-20

OTA reliability release. Every OTA upload's outcome now survives a restart, failure categories are captured alongside failed rows and surfaced in a new `ota_success_rate` diagnostics rule, and the delivery-mark path is concurrent-safe so two in-flight OTAs to the same device can never cross-attribute outcomes.

### Added

- **OTA delivery success rate rule.** New `ota_success_rate` rule reads the ratio of successful vs failed OTA uploads over the last 10 recorded outcomes and surfaces `fail` at <50% delivered, `warn` at <80%, else `ok`. Stays at INFO ("N outcomes recorded so far, need 3 for trend") until 3 attempts have been recorded, so a fresh device isn't fail-flagged on a single bad upload. Backed by two new nullable columns on `firmware_history` (`delivery_status`, `delivered_at`) — `ota::serve_firmware` calls `db.mark_firmware_delivery` right before emitting the `ota_delivered` / `ota_delivery_failed` Tauri event, so outcome survives restart. Pre-v0.15.0 rows have NULL `delivery_status` and the rule skips them, only earning trust as new uploads accumulate. Persistence is best-effort: a DB write failure logs warn but never propagates into the OTA flow. 7 new unit tests (36 → 43 diagnostics tests).
- **OTA delivery error persistence.** Third nullable column `delivery_error` on `firmware_history` captures the failure-category string `serve_firmware` was already emitting on `ota_delivery_failed` events (e.g. `"accept: timed out"`, `"body: Broken pipe (os error 32)"`, `"flush: Connection reset by peer"`). `mark_firmware_delivery` signature becomes `(status, error: Option<&str>)`; `record_delivery` passes the error on failure branches and `None` on the delivered branch. The `ota_success_rate` rule appends `Last error: {reason}.` to its detail string when it tips WARN/FAIL (picks the newest `failed` row's error, skips rows with NULL errors to stay backward-compatible), so admins see the failure category inline instead of cross-referencing the firmware history. 2 new unit tests (43 → 45 diagnostics tests).
- **Concurrent-OTA-safe outcome attribution.** `mark_firmware_delivery` previously targeted "most recent NULL `delivery_status` row for the device" via a subquery — safe under the single-in-flight-OTA-per-device UI contract but would cross-attribute outcomes if two OTAs to the same device ever ran concurrently. Signature is now `mark_firmware_delivery(row_id: i64, status, error)` with a direct `WHERE id = ?1`; the three insert-and-serve sites (desktop `upload_firmware`, desktop GitHub OTA, REST `POST /api/github/ota`) capture `store_firmware_record`'s returned rowid and thread it through. `ota::serve_firmware` gains `history_row_id: Option<i64>`; `record_delivery` accepts the option and skips the write entirely on `None`, which is what `rollback_firmware` now passes — rollback reuses an existing firmware_history row and inserts no new one, so under the old code it was quietly flipping a random stale NULL row; now it's correctly a no-op. 59 library tests pass.

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.14.0] — 2026-04-19

Diagnostics gets smarter. The firmware_age rule now offers a one-click upgrade when a newer GitHub release is available for a device's bound repo, and a new error_rate_trend rule distinguishes "errors are still firing right now" from the existing 24h totals rule. Both render identically in the desktop Diagnostics section and the `:9090` web dashboard.

### Added

- **Firmware auto-remediation.** The `firmware_age` rule escalates from INFO to WARN when a newer release is published in the device's bound GitHub repo, and surfaces a one-click "Update to {tag}" button inline in the Diagnostics card. Each device gains a collapsible `owner/repo` binding panel at the top of Diagnostics (desktop + web); binding lives on two new columns on `devices`. The rule engine stays synchronous — callers pre-fetch eligible releases via a blocking `ureq` helper that dedupes by (owner, repo) so a 20-device fleet on one repo makes a single GitHub API call. Version comparison tolerates `v` prefix and `-prerelease` / `+build` suffixes; anything unparseable short-circuits to "skip" so the rule never nags about versions it can't compare. Desktop button invokes `start_github_ota`; web UI POSTs to the existing admin-gated `POST /api/github/ota` endpoint, reusing the blocking download + `ota::serve_firmware` path and the `gh_download_progress` WebSocket broadcast. New findings carry an optional structured `action { label, action_type, data }` slot, opening the door for more self-healing actions later. 22 → 30 diagnostics unit tests.
- **Error rate trend rule.** New `error_rate_trend` rule splits the same `error`/`warn` log set the existing `error_rate` rule watches into last-hour vs preceding 23h, computes a per-hour baseline, and surfaces `fail` at ≥10 events/hr with ≥3× baseline (or any 10+ with zero baseline), `warn` at ≥5/hr with ≥2× baseline, else `ok`. Detail string reads either "N events in last hour vs M.M/h baseline (X.Xx)" or "N events in last hour with no prior events in 23h window." Pure read from `device_logs` — no schema change, no UI change (rule renders generically via both desktop `DeviceDiagnostics.tsx` and web `renderDiagFinding`). Verified live on a real ESP32: with 110 errors + 109 warns over 24h and 0 in the last hour, `error_rate` stays FAIL while `error_rate_trend` correctly reads OK with the baseline printed. 30 → 36 diagnostics unit tests.

### Fixed

- **Web dashboard Diagnostics binding inputs now pre-fill.** The owner/repo inputs on the per-device Diagnostics binding panel were rendering empty on load even when a binding existed in SQLite, forcing admins to retype before each save. Root cause: the panel mounted before the fetch that populated the binding state completed. Fixed by wiring the inputs to read the saved binding on expand.
- **Web dashboard `showToast` → `toast` + double-encoded response bodies.** Renamed the UI helper to match the rest of the dashboard's toast API, and stripped a layer of double-JSON-encoding from the shared `api()` fetch wrapper so error messages render as plain strings instead of quoted strings.

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.13.0] — 2026-04-19

Fleet health release. A top-level widget on the Home page rolls up every device's diagnostics into three buckets, surfaces the most urgent finding inline for each device, and closes the last desktop-only gap in the v0.12.0 Scene-from-room scaffolder.

### Added

- **Fleet Health widget on Home.** At-a-glance rollup of per-device diagnostics on the Home page (both desktop React app and `:9090` web dashboard). Three color-coded tiles (`Healthy` / `Attention` / `Unhealthy`) with device counts; click any non-empty tile to expand a list of the devices in that bucket. Widget defaults to the most-urgent non-empty bucket on load. New `diagnose_fleet` Tauri command and `GET /api/diagnostics/fleet` REST endpoint (viewer-safe) reuse the per-device `diagnose` engine — zero new rules, zero schema changes. Results sorted most-urgent-first; per-device errors skip silently so one bad row can't hide the rest.
- **Fleet Health drill-in — top finding inline.** Each device row in the Fleet Health widget now surfaces its most urgent finding (e.g. `Connection stability · 13 reconnect events in last 24h.`) severity-colored, without requiring a click-through. Pick semantics: first FAIL in rule-evaluation order, else first WARN, else none. Three new unit tests cover the selection rules (22 diagnostics tests total). Desktop (`FleetHealth.tsx`) and web UI (`renderFleetHealth`) parity.
- **Scene-from-room scaffolder in the web dashboard.** Ports the v0.12.0 desktop Scene-from-room feature to `:9090`. When Devices is filtered to a single room, admins see a "Scene from {Room}" chip that opens three template cards (Switches / Sliders / Colors) identical to the desktop version. Uses the existing `POST /api/scenes` endpoint; tab-switches to Scenes on success.

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.12.0] — 2026-04-18

Device diagnostics release. One-click health check rolls up eight signals per device, plus room-filtered views and scene scaffolding from a filtered room.

### Added

- **Device diagnostics — one-click health check.** New Diagnostics section on the device detail page rolls up eight rules into a single overall status (good / attention / unhealthy) with per-finding explanations: online status, RSSI health, low heap, heap trend, uptime percent, reconnect count, error rate, firmware age. Rule engine in `src-tauri/src/diagnostics.rs` with 18 unit tests. New Tauri command `diagnose_device` and REST endpoint `GET /api/devices/:id/diagnose`. Renders in both desktop app (`DeviceDiagnostics.tsx`) and embedded web UI.
- **Room-filtered dashboard view.** Filter the dashboard by room to show only devices placed in the selected room region on the floor plan. Desktop and web dashboard.
- **Scene from filtered room.** When filtered by a room, "Create Scene" scaffolds a scene from only that room's devices — one click to capture a room-level scene (e.g. "Kitchen Evening").

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.11.0] — 2026-04-16

Floor Plan v2 release. Rooms as named spatial regions, device-room association, and deep-link-safe web dashboard.

### Added

- **Floor plan rooms.** Named rectangular regions on the floor plan canvas. Add Room button spawns a colored rectangle; drag to move, resize via SE handle, rename inline, pick from 8-color palette. New `floor_plan_rooms` SQLite table (cascades on floor delete). REST endpoints for full CRUD. Both desktop React app and embedded web UI.
- **Device room indicator.** Devices placed inside a room rectangle show the room name as a subtle label beneath the device name. Derived property computed on each render — no schema change. Updates instantly on drag or room resize. Hidden in compact mode.

### Fixed

- **SPA fallback for deep links.** Non-API/proxy/ws GET requests with no file extension now serve the embedded index HTML instead of returning a JSON 404. Prevents broken bookmarks, browser reloads, and notification click-throughs. `/favicon.ico` still 404s cleanly.

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.10.1] — 2026-04-15

Targeted polish release. Fixes a latent timezone bug in the embedded web dashboard so chart tooltips and x-axis labels show the viewer's local time correctly.

### Fixed

- **Web dashboard chart times displayed in wrong timezone.** `fmtChartTime` and `fmtTooltipTime` in the on-device chart code parsed SQLite UTC timestamps as local time, so tooltips and x-axis labels were shifted by the viewer's timezone offset. Now both functions parse the timestamp as UTC (by appending `Z`) and call `getHours()` / `getMinutes()` to render the viewer's local wall-clock time. Affects every chart in the `:9090` web dashboard: metric charts, uptime tooltips, annotation tooltips, and chart x-axis labels.

## [0.10.0] — 2026-04-14

Dashboard experience, automation, and webhook reliability release. Inline controls on device cards, compound rule conditions with AND/OR logic, and webhook retry with delivery logging.

### Added

- **Inline controls and live values on device cards.** Desktop cards now show interactive switch toggles, compact sliders, sensor values, color swatches, and text previews directly — no need to navigate to the detail page for quick actions. Web dashboard collapsed cards show a capability preview row with inline switch toggles. Offline devices show dimmed, disabled controls. Outer card element changed from `<button>` to `<div role="button">` for valid nested interactive elements.
- **Compound rule conditions with AND/OR logic.** Rules support multiple conditions combined with AND or OR. Each condition can reference a different device and sensor, enabling cross-device rules like "if temp > 30 AND humidity < 40, turn on fan". Four operators: above, below, equals, not_equals. Multi-condition form in desktop app with add/remove condition rows and AND/OR toggle. New `logic` and `conditions` (JSON) columns on the rules table. Config import/export preserves compound conditions. Backward compatible — existing single-condition rules continue to work.
- **Webhook retry with exponential backoff.** Failed webhook deliveries retry up to 3 times with exponential backoff (2s, 4s, 8s). Each attempt is logged in a new `webhook_deliveries` SQLite table.
- **Webhook delivery log.** Desktop app shows an expandable delivery history per webhook with status codes, retry indicators, timestamps, and error messages. REST endpoint `GET /api/webhooks/{id}/deliveries`.
- **Webhook test button.** Send a test POST to any webhook and see the result immediately. Available in both the desktop app and web dashboard.

### Library

- No library changes in this release (all features are desktop/web-only).

## [0.9.0] — 2026-04-13

Scene management release. Scene editing, config export v2, web dashboard scene CRUD, and Sinric voice scene triggering.

### Added

- **Scene editing.** Edit existing scenes (rename + add/remove/reorder actions). `PUT /api/scenes/{id}` REST endpoint and `update_scene` Tauri command. Edit button on scene cards in both desktop app and web dashboard.
- **Config import/export v2.** Export now includes backend-backed scenes, floor plans, device positions, and favorite capabilities. Import remaps IDs across cross-referenced tables. Config version bumped to 2; backward-compatible with v1 exports.
- **Web dashboard scene CRUD.** Full scene management in the web dashboard: create, edit, and delete scenes with inline forms. Previously only the desktop app could create/delete scenes.
- **Sinric scene triggering.** Map a Sinric Pro virtual device to a Trellis scene so "Alexa, turn on Movie Mode" fires all scene actions. `setPowerState(On)` runs the scene; `setPowerState(Off)` is a no-op. Settings UI has a Device/Scene type toggle per mapping row.

### Library

- No library changes in this release.

## [0.8.0] — 2026-04-13

Floor Plan enhancements and backend-backed scenes release. Multi-floor support, snap-to-grid, compact labels, undo, and scenes moved from localStorage to SQLite with scheduling support.

### Added

- **Multi-floor support.** New `floor_plans` table. Tab bar above the canvas lists all floors; click to switch, `+` to add, right-click for rename/delete. Each floor has its own device positions and background image. Seamless migration from single-floor. REST API: `GET/POST /api/floor-plans`, `PUT/DELETE /api/floor-plans/{id}`. Both desktop and web dashboard.
- **Snap-to-grid toggle.** Sidebar button snaps device positions to a 4% grid. Grid dots turn trellis-accent when active and remain visible over backgrounds. Applied to sidebar drops and node drags.
- **Compact labels toggle.** Sidebar button switches placed nodes between expanded (name + value) and compact (value only, smaller padding) for dense floor plans.
- **Undo last move.** Ctrl+Z / Cmd+Z reverts the last floor plan action: placement (removes), move (restores position), or removal (restores device). Single-level.
- **Backend-backed scenes.** Scenes persisted in SQLite (`scenes` + `scene_actions` tables) instead of localStorage. Full CRUD via Tauri commands and REST API (`GET/POST /api/scenes`, `DELETE /api/scenes/{id}`, `POST /api/scenes/{id}/run`). Scene execution moved to backend so both desktop app and web dashboard can trigger scenes. New "Scenes" tab in web dashboard.
- **Scene scheduling.** Schedules can fire entire scenes on a cron schedule. New `scene_id` column on `schedules` table. Schedule creation form has a type toggle (Single Action / Scene). Scene name shown in schedule cards in both desktop app and web dashboard.

### Library

- No library changes in this release (floor plan and scenes are desktop-only).

## [0.7.0] — 2026-04-13

Home experience release. New Home landing page, per-capability favorites, and Floor Plan spatial layout — all in both the desktop app and the web dashboard.

### Added

- **Home overview page.** New default landing view in the desktop app and web dashboard. Shows a system status strip (online/offline device counts, MQTT and Sinric bridge status), live sensor readings grid, quick controls (functional inline switches, sliders, color pickers — offline devices dimmed), and a recent activity feed (last 30 cross-device events, severity color-coded, clickable to navigate to device). New `GET /api/activity` endpoint backed by `idx_logs_timestamp` index for efficient cross-device queries.
- **Per-capability favorites.** Star toggle on every sensor and control card in the Home page. Pinned capabilities appear in a "Favorites" section at top; non-favorited remain in regular sections (no duplication). New `favorite_capabilities` table (device_id + capability_id composite key). REST API: `POST /api/favorites/toggle`, `GET /api/favorites`. Admin-only with optimistic UI and revert on failure. Works in both desktop app and web dashboard.
- **Floor Plan page.** Freeform canvas for spatial device placement. Drag devices from a sidebar panel onto percentage-based positions. Placed nodes show live status dot, primary capability value, and device name. Click a node for an inline popup with all sensor readings and interactive controls. Background image support (room photo or blueprint). New `device_positions` table. REST API: `GET /api/floor-plan`, `PUT /api/floor-plan/position`, `DELETE /api/floor-plan/position/{id}`, `PUT /api/floor-plan/background`. All admin-only. Touch support for mobile web dashboard. Device deletion cascades to positions and favorites.

### Library

- No library changes in this release (Home, favorites, and floor plan are desktop-only).

## [0.6.0] — 2026-04-13

Sinric Pro voice assistant bridge. Control your Trellis devices with Alexa and Google Home via the Sinric Pro cloud.

### Added

- **Sinric Pro bridge.** WebSocket bridge to `wss://ws.sinric.pro` with HMAC-SHA256 message signing. Runs as a worker thread inside the desktop app, mirrors the MQTT bridge architecture (config in SQLite, secret encrypted at rest, exponential-backoff reconnect).
- **Bidirectional capability mapping.** Switches map to `setPowerState`, sliders to `setRangeValue`/`adjustRangeValue`, color pickers to `setColor`, sensors report `currentTemperature`. Inbound voice commands route through `ConnectionManager::send_to_device`; outbound state changes update the Sinric cloud shadow.
- **Per-capability mapping.** Optional `trellis_capability_id` field on each device mapping lets users target a specific capability instead of relying on auto-discovery (first-match heuristic). Type-safe resolution: the bridge validates that the explicit capability matches the expected action type before using it, falling back to auto-discovery on mismatch.
- **Settings panel** (desktop app). API key, secret (encrypted at rest), device mappings with Trellis device and capability dropdowns, connection test, status indicator.
- **Web dashboard Sinric section.** Read-only connection status, message counters, and mapping breakdown (per-cap vs auto) in the Settings tab at `:9090`.
- **REST API endpoints.** `GET/PUT /api/settings/sinric`, `GET /api/sinric/status`, `POST /api/sinric/clear-secret`.
- **Tauri commands.** `get_sinric_config`, `set_sinric_config`, `get_sinric_status`, `test_sinric_connection`, `clear_sinric_secret`.
- **User guide §22.** Complete Sinric Pro walkthrough: account setup, device creation, bridge configuration, per-capability mapping, sensor naming conventions, monitoring, and limitations.

### Library

- No library changes in this release (Sinric bridge is desktop-only).

## [0.5.0] — 2026-04-13

Desktop Metrics page release. New top-level monitoring overview in the desktop app, plus polish fixes for chart loading states and CSV exports.

### Added

- **Metrics page.** Top-level monitoring overview in the desktop app showing all devices with uptime ribbon, RSSI/heap/sensor charts in a 2-column grid, global time range picker (1h/6h/24h/7d), device status indicators, and "Details" links to device detail. Reuses existing MetricChart and UptimeTimeline components via new `externalHours` prop. Matches the `:9090` web dashboard Metrics tab layout.
- **Online device count in Metrics header.** Subtitle now shows "N devices · M online" with green highlight.
- **Device name in CSV export filenames.** Exported CSVs now include the device name (e.g. `MyESP32_rssi_24h.csv`) instead of bare metric ID.

### Fixed

- **Unreachable empty-state message in MetricChart.** The "Waiting for data from device..." message was dead code — `loading` was derived from `data.length === 0` inside the `data.length === 0` branch, so it was always `true`. Replaced with a `fetchedOnce` flag that correctly distinguishes initial load from no-data.
- **Offline device context in empty charts.** Charts for offline devices with no data in the selected range now show "Device is offline — no data in this range" instead of spinning on "Loading..." indefinitely.

## [0.4.9] — 2026-04-13

GitHub OTA polish release. Download progress, friendlier error messages, pre-release filtering, and asset name filtering — all surfaced in both the desktop app and web dashboard.

### Added

- **GitHub OTA download progress bar.** Chunked downloads emit per-2% progress events via Tauri events and WebSocket broadcast. Animated progress bar in both the desktop OTA page and the `:9090` web dashboard device detail panel.
- **Pre-release filtering toggle.** Hidden by default; a checkbox appears when pre-releases exist. Amber badge distinguishes pre-releases in the list.
- **Asset name filter.** Substring match input for filtering firmware assets within a release. Per-device persistence. Releases with zero matching assets are hidden.

### Improved

- **User-friendly GitHub OTA error messages.** 404/403/network errors mapped to plain-English explanations instead of raw HTTP status codes. Web dashboard `api()` helper reads JSON error bodies for richer feedback.

### Docs

- Updated screenshots and README for v0.4.8 release.
- Cleaned shipped items from BACKLOG.md.

## [0.4.8] — 2026-04-12

The GitHub OTA + accessibility release. OTA firmware updates can now be pulled directly from any public GitHub repository's Releases page — both in the desktop app and the web dashboard. Accessibility improvements bring focus trapping, keyboard navigation, and mobile touch polish to the web dashboard. Several internal audits close out low-priority backlog items.

### Added

- **OTA firmware updates from GitHub Releases.** New "Update from GitHub Release" section on the desktop OTA page. Enter a GitHub `owner/repo` (or full URL), the app fetches releases via the GitHub API and shows `.bin` and `.bin.gz` firmware assets with tag, date, and file size. One-click Flash downloads the asset and pushes it through the existing OTA pipeline. `.bin.gz` files are auto-decompressed via `flate2` before serving. Per-device repo binding persisted in the settings table. Two new Tauri commands (`check_github_releases`, `start_github_ota`) and two REST API endpoints (`GET /api/github/releases`, `POST /api/github/ota`).
- **Web dashboard GitHub OTA.** "Update from GitHub" section in the `:9090` device detail panel (ESP32 + admin + online only). Repo input, release dropdown with Flash buttons, version comparison against current firmware, per-device repo persistence. Confirmation dialog before flashing. Uses the same REST API endpoints as the desktop app.

### Fixed

- **Mobile touch polish on web dashboard.** Chart containers now use `touch-action: pan-y pinch-zoom` so touch-scrolling works alongside chart interaction. Touch targets bumped to `min-height: 2rem`. Stat line separators wrapped with `white-space: nowrap` to prevent awkward mid-separator line breaks on narrow viewports.
- **Focus trap in device detail panel.** Tab / Shift+Tab trapped inside the panel when open, Escape returns focus to the trigger element, `aria-hidden` on background content, `role="dialog" aria-modal="true"` on the panel.
- **Keyboard-accessible Details links.** Device card "Details" link is now Tab-reachable (`href="#"`, Enter opens panel). Metrics tab device headers gain `tabindex="0"`, `role="button"`, and Enter/Space handlers.
- **Stale-fetch guard for charts, firmware, and sparklines.** Added device-ID checks after async fetches in `openDeviceDetail` and `loadDetailCharts` to prevent stale data from a previous device being written into a newly opened panel.
- **OTA page subtitle updated** to mention GitHub Releases as a firmware source.

### Audited (no changes needed)

- `cssEscape` usage on uptime ribbon segment click — both paths correctly escape timestamps.
- Data retention scope — `firmware_history` and `alerts` tables confirmed to not need pruning (negligible automatic growth).
- WebSocket push rate limiting — `/ws` upgrade hits the same `rate_limiter.check()` as REST endpoints; full parity confirmed.

### Verified

- Hardware test on real ESP32 (greenhouse-controller, 192.168.1.108). LED toggle round-trip confirmed, telemetry flowing (RSSI, heap, temp, humidity), MQTT bridge connected. Library code byte-identical to v0.4.7 — no re-flash needed.
- GitHub OTA tested with arendst/Tasmota (140 `.bin` assets) and Aircoookie/WLED (`.bin` + `.bin.gz`).

## [0.4.7] — 2026-04-12

The DeviceDetail parity + persistence release. The React desktop page now matches every observability surface from the `:9090` web dashboard: chart annotations, severity filter chips, uptime timeline with stat line, annotation click-through, and uptime segment click. Dense transition regions on the uptime ribbon collapse into striped cluster bars. The Arduino library gains NVS persistence — slider and switch values survive reboots on ESP32. Hardware-tested on a real ESP32 (NVS round-trips for both switches and sliders confirmed across multiple reboots).

### Added

- **DeviceDetail React page — chart annotations overlay.** Recharts `<ReferenceLine>` + `<ReferenceDot>` pairs for OTA/online/offline/warn/error events, matching the `:9090` dashboard's annotation system. Numeric-time XAxis, kind-present-only legend below chart, native SVG `<title>` tooltip. New `get_device_annotations` Tauri command wraps the existing `Database::get_annotations`.
- **DeviceDetail React page — Recent Logs severity filter chips.** 7-chip row (All / Events / State / Error / Warn / Info / Debug) with server-side re-fetch via optional `severity` arg on `get_device_logs` Tauri command. Stale-fetch guard, live-log filter guard, `key={device.id}` remount on device switch for filter reset parity.
- **DeviceDetail React page — UptimeTimeline component.** SVG ribbon + stat line + legend in a new `UptimeTimeline.tsx`. Derives online/offline/unknown segments from annotations (same algorithm as `web_ui.html`), renders with its own time range picker. Stat line shows online %, tracked span, and transition count. Perf: `memo`, `useMemo`, CSS containment.
- **DeviceDetail React page — annotation + uptime click-through.** Clicking a chart annotation dot or an uptime segment scrolls to and flash-highlights the matching log row. `DeviceLogs.tsx` converted to `forwardRef` with `useImperativeHandle` exposing `scrollToLog(timestamp, targetFilter)`. Flash highlight via CSS `annFlash` keyframe (amber 1.5s fade). Cursor pointer and hover brightness on interactive elements.
- **Uptime timeline clustering.** When 3+ consecutive segments would each render narrower than 6px, they collapse into a single diagonal-stripe bar (green/red SVG pattern). Tooltip shows transition count + timespan. Click activates the State filter chip. Stat line uses pre-clustered data so percentages stay accurate. Applied to both the React page and the `:9090` web dashboard.
- **NVS persistence for slider values (ESP32).** `addSlider()` restores the last user-set value from ESP32 NVS on boot and applies PWM immediately, so hardware state matches before the first client connects. New `setSlider()` public API method for parity with `setSwitch()`.
- **NVS persistence for switch values (ESP32).** `addSwitch()` restores the last user-set value from NVS on boot and applies GPIO immediately. Both switch and slider persistence share the `trellis_cap` NVS namespace.
- **AutoConnect example gains brightness slider.** `addSlider("brightness", "LED Brightness", 0, 100, 4)` on GPIO 4, demonstrating the new NVS persistence alongside the existing LED switch on GPIO 2.

### Fixed

- **Scroll jank on multi-sensor DeviceDetail pages.** `MetricChart` was re-rendering all charts on every annotation/metric fetch. Added `useMemo` for annotation filtering and chart data prep to eliminate 10s stalls on devices with 4+ sensor capabilities.

### Verified

- Hardware test GATE on real ESP32 (greenhouse-controller, 192.168.1.108). NVS slider persistence: set 75 → reboot → 75, set 30 → reboot → 30. NVS switch persistence: ON → reboot → ON, OFF → reboot → OFF. All 5 examples compile clean on ESP32, AutoConnect compiles on Pico W. Device online with 4 capabilities (LED switch, brightness slider, temp sensor, humidity sensor). MQTT bridge connected.

## [0.4.6] — 2026-04-11

The observability release. Ten dashboard features land on top of v0.4.5 plus a stale-state bug in the device detail panel. The metric chart grows cursor-anchored tooltips, drag-to-zoom, and event annotations drawn from real state transitions and log entries; annotations click-through to the exact log or firmware row that generated them. A new Metrics tab gives a fleet-wide monitoring overview. Recent Logs gains severity filter chips. The device detail panel gets an uptime timeline strip with a summary stat line above it. Hardware-tested on a real ESP32 (temp/humidity/LED/brightness all round-tripping, 51 state transitions feeding the ribbon, 161 annotations in the last 24h).

### Added

- **Device detail panel polish.** Firmware version history block with upload timestamps. Sparkline overlays on the system metric grid (RSSI, free heap, uptime) rendered from the last 24h of metric samples. Mobile layout breakpoint: at ≤640px the slide-out takes the full viewport width, the 2-column stat grid collapses to single-column, and the close button enlarges for touch. All controls render through the same `renderControl` path as the card list so toggles/sliders/color/text stay identical.
- **Notification sound option.** A new "Play sound on notification" toggle in the Browser Notifications settings section. When on, the web dashboard plays a short Web Audio beep (220 Hz → 880 Hz sweep, 200ms) alongside the system notification fire. Disabled by default to stay quiet on first install. Persists to `localStorage` next to the other notification preferences.
- **Configurable data retention period.** A new `data_retention_days` setting (default 30) controls how long historical metrics, logs, firmware events, and annotations are kept in SQLite. A background task prunes rows older than the window on a daily tick. Settings UI has a numeric input with a "Apply retention" button that runs the prune immediately. Valid range 1–3650 days; the input validates client-side.
- **Interactive metric charts in the device detail panel.** Per-metric charts (temperature, humidity, and any other numeric capability) now have: (1) a cursor-anchored tooltip showing the exact timestamp + value of the nearest sample, (2) drag-to-zoom via `setChartRange(startTs, endTs)` with a "Reset zoom" chip that clears the range, and (3) a range selector (1h / 6h / 24h / 7d) that re-fetches with the corresponding `hours` query param. All three features share one `renderChart` pipeline so each metric gets them for free.
- **Metrics tab — fleet monitoring overview.** New top-level tab beside Devices/Automation/Settings that aggregates metric charts across every device in one scrolling page. Each device's numeric capabilities render as a row of mini-charts sharing the tab-level range selector. `loadMetrics()` fetches all device metrics in parallel and renders them through the same `renderChart` pipeline as the detail panel, so cursor tooltips, annotations, and click-through all work at fleet level too.
- **Chart event annotations.** Vertical markers drawn on every metric chart for state transitions (online/offline), warnings, and errors. Pulled from `GET /api/devices/{id}/annotations?hours=N` which traverses the logs table and joins in firmware history rows. Color-coded by severity (green for online, gray for offline, amber for warn, red for error, blue for firmware). Legend rendered below each chart with counts per category. Annotations are fetched once per range change and rendered on top of the data polyline.
- **Annotation click-through to log/firmware row.** Clicking any chart annotation jumps to the exact log entry or firmware history row that generated it. The Recent Logs list inside the device detail panel scrolls the matching row into view and runs a short `annotation-flash` animation. The row is matched by `data-ann-ts` (timestamp) attribute set on both ends; `cssEscape` guards the query selector against unusual characters. Works across zoomed ranges because the annotation data carries its own timestamp, not a chart-space index.
- **Severity filter chips in Recent Logs.** Toggle chips above the Recent Logs list (inside the detail panel) for state / error / warn / info — click to include/exclude each severity. Selection is per-device via `currentLogDeviceId`, preserved through range changes but reset on device switch. Server-side filtering via `?severity=state,error,warn` on `GET /api/devices/{id}/logs`, so the chip state translates to a query param rather than a client-side filter. `logFilterToSevParam()` is the translator.
- **Uptime timeline in the device detail panel.** A horizontal SVG strip above the Recent Logs list that renders the device's on/offline history as alternating green/gray segments built from state transition log rows. The leading inferred segment (before the first captured transition in the window) is rendered in dim gray "unknown" because we can't prove the device was being tracked before window-start. Segment tooltips show the start/end timestamp and duration. Shares the range selector with the chart block so the strip and the charts always cover the same window. Segment colors computed in `renderUptimeTimeline`, SVG class `uptime-segment`.
- **Uptime summary stat line.** A one-line quantitative roll-up rendered above the uptime ribbon: `NN.N% online · Xd Yh tracked · N transitions`. Shares the segment array with the ribbon (zero extra fetches). Edge cases: ≥99.95% pins to "100%", sub-0.05% pins to "<0.1%", a single transition shows singular "1 transition", `knownMs=0` routes to an italicized "No tracked uptime in this window". The percentage denominator excludes the leading inferred "unknown" segment so it reflects only the span we actually observed transitions for. Auto-updates on `setChartRange`.

### Fixed

- **Switch toggle in the device detail panel no longer repeats stale value.** The `'switch'` case in `renderControl` was hardcoding `${!cap.value}` directly into the button's `onclick` string at render time. When the user toggled from the detail panel, `sendCommand` updated local state and called `renderDevices()` — which only rebuilds the card list, never the open detail panel. The panel's button therefore kept its original `onclick` and sent the same value on every click: first click turned the LED on, every subsequent click sent "on" again instead of "off". Fixed with three edits: (1) `renderControl` removes the precomputed value and adds `data-trellis-device` + `data-trellis-cap` attributes on the button, (2) `sendToggle(event, deviceId, capId)` reads `cap.value` live from the `devices` state array at click time and sends `!cap.value`, (3) `sendCommand` patches any matching `.toggle[data-trellis-device=...][data-trellis-cap=...]` button's `.on` class in the DOM after server-side confirm. Card-list toggles were never affected because `renderDevices()` rebuilt them with a fresh `onclick` each render. Verified end-to-end on the real ESP32.

### Library

- **Library code is byte-identical to v0.4.5 except the `TRELLIS_VERSION` macro.** No microcontroller behavior changes. The Arduino LM and PIO releases exist purely so all three distribution endpoints stay in lockstep with the desktop app version.

### Verified

- Hardware test GATE on real ESP32 (greenhouse-controller, 192.168.1.108) before tagging. Command round-trip (LED switch + brightness slider) confirmed in both directions; metrics flowing (temp 2615 rows, humidity 2607 rows in last 24h); annotations flowing (161 in last 24h); 51 state transitions (30 online + 21 offline) feeding the uptime ribbon; MQTT bridge connected and pushing telemetry to the broker; embedded device dashboard at `:8080` reachable. Switch-toggle detail-panel fix confirmed end-to-end last session.

## [0.4.5] — 2026-04-10

A large feature release spanning real-time infrastructure, security hardening, and UX polish for the `:9090` web dashboard. Twelve feature commits landed between v0.4.4 and the tag — the biggest batch since the v0.4.0 remote-access release. Hardware-tested on a real ESP32 with OTA round-trip verification.

### Added

- **MQTT `tls_skip_verify` option.** New boolean in MQTT settings that disables TLS certificate validation via the rustls `dangerous()` API with a custom `NoVerifier`. Connection is still encrypted but broker identity is not checked — equivalent of `curl -k`. Settings UI shows a checkbox inside the TLS section with an amber security warning when active. Both `start()` and `test_connection()` honor the flag. Useful for self-signed broker certs where providing a CA PEM is impractical.
- **1 MB body size limit on REST API.** All incoming requests are checked against `MAX_BODY_SIZE` (1,048,576 bytes) before allocation. Returns 413 Payload Too Large with a descriptive error. OTA firmware uploads go directly to device `:8080`, not through the REST API, so they're unaffected.
- **Role-based access control (RBAC).** Each API token now carries a role: `admin` (full access) or `viewer` (read-only). Viewers can read devices, metrics, status, schedules, rules, webhooks, and token metadata but cannot send commands, trigger OTA, manage tokens, change settings, or access the device proxy. Existing tokens default to admin (backward-compatible). `Role` enum in `auth.rs`, `require_admin()` guard on 20 mutating endpoints + proxy. Settings UI has a role dropdown in the create form, role column in the token table, and role badge in the created-token modal. 14 unit tests.
- **`GET /api/auth/whoami` endpoint.** Returns `{ role, token_id }` for the authenticated caller. Loopback callers get `{ role: "admin", token_id: null }`. The `:9090` web dashboard calls this on load to detect viewer tokens: disables toggles/sliders/color pickers/text inputs, hides group/ntfy write controls, and shows a "Read-only" badge in the header.
- **Drag-and-drop device card reordering.** Device cards in both the desktop app and the `:9090` web dashboard can be reordered via HTML5 DnD. Order persists in SQLite (`sort_order` column on `devices` table). `PUT /api/devices/reorder` REST endpoint (admin-only). Visual feedback: opacity on drag, accent ring on drop target, grip handle icon. Viewers see cards in the saved order but cannot drag.
- **WebSocket push for the web dashboard.** Persistent `/ws` connection replaces 5-second polling with instant event delivery. Device state changes, heartbeats, logs, and discovery events (online/offline) arrive in real time. `WsBroadcaster` fan-out in Rust feeds all connected browser clients. Query-param token auth (`/ws?token=trls_...`) for remote access since the browser WebSocket API can't set custom headers. Loopback bypass applies. Polling auto-resumes as fallback on WS disconnect. Green/gray connection indicator dot in header.
- **PWA support.** Web app manifest (`/manifest.json`) with standalone display mode, themed SVG icons, and dark background. Service worker (`/sw.js`) caches the HTML shell for offline display with network-first strategy. Mobile install prompt banner for "Add to Home Screen" with persistent dismiss. Both routes served pre-auth.
- **PWA polish.** Card flash animation on WS update, reconnect pulsing dot, offline banner (`navigator.onLine`), and browser Notification API integration for device offline/online/error events.
- **Browser notification preferences.** "Browser Notifications" settings section with three toggles: device offline (default on), device online (default off), and error logs (default off). Persisted to `localStorage`. Permission status label with color coding. Permission requested on first toggle-on. Viewers see toggles but cannot change them.
- **Per-device notification filtering.** "Per-Device Overrides" section below global toggles. Each device gets three buttons (Offline/Online/Errors) that cycle through inherit → on → off. Overrides stored in `localStorage`, checked via `shouldNotify()` before firing browser notifications. Absent overrides follow the global setting.
- **Device detail slide-out panel.** Click "Details" on any device card to open a right-side panel showing: device info (status, ID, IP, firmware), system metrics (RSSI, free heap, uptime, chip) in a 2-column stat grid, interactive controls, link to per-device embedded dashboard, and 20 most recent log entries with severity coloring. Closes on overlay click, close button, or Escape key.

### Fixed

- **OTA 0% progress deduplication.** The `httpUpdate.onProgress` callback fires 0% twice (download start + write start). Added `lastPct` guard in `TrellisOTA.cpp` so only the first 0% event is forwarded over WebSocket. **Hardware-tested**: exactly 1 zero-percent event confirmed, clean 5% increments through 100%.

### Library

- **Real-time OTA progress + delivery confirmation.** `httpUpdate.onProgress` streams real progress percentages (every 5%) over WebSocket during firmware download. After successful write, device sends explicit `ota_delivered` event before rebooting via `rebootOnUpdate(false)` + controlled shutdown. Desktop and embedded dashboards show live progress bar and a "Firmware received" confirmation tick. Broadcaster callback decouples OTA from WebSocket library.
- **Dependencies section added to README** for Arduino IDE users (WebSockets, ArduinoJson, ESPAsyncWebServer, AsyncTCP).

### Verified

- Full OTA round-trip on real ESP32 hardware (greenhouse-controller, 192.168.1.108). Compiled `test/TestDevice/TestDevice.ino` with updated 0.4.5 library, flashed via USB, triggered OTA via direct WebSocket command. Results: exactly 1 zero-percent event, clean 5% increments (0→5→10→...→95→100), `ota_delivered` event received, device rebooted to "1.0.1-ota-test" firmware. Restored to original "1.0.0" firmware via USB. ETag on device: `"0.4.5-fb4c53df7924588f"`.

## [0.4.4] — 2026-04-09

The onboarding and hardening release. New users get a 4-step guided wizard that goes from zero to a working device in one flow. The security surface gets token expiry, per-IP rate limiting, and a reverse proxy that lets remote users reach individual device dashboards through a tunnel. Internal refactor splits the monolithic Settings page into 8 focused modules.

### Added

- **Get Started onboarding wizard.** 4-step guided flow (Welcome/Prerequisites → Pick Template → Configure & Flash → Device Appeared) that takes new users from zero to a working device. 5 bundled starter templates: Blink (LED toggle), Sensor Monitor (analog + text), Smart Relay (switch + timer), Weather Station (temp/humidity/pressure), Greenhouse Controller (soil moisture + pump + grow light). Prerequisite checks detect `arduino-cli`, board cores, and library dependencies with one-click install for missing deps. Template-to-flash pipeline: select template → customize device name/board/capabilities → compile & flash via USB in one click. Device discovery confirmation watches mDNS for the new device. Dashboard empty state integration ("Get Started" button alongside "Add by IP"). Always accessible from the sidebar.
- **Per-device dashboard proxy at `/proxy/{device-id}/`.** Reverse proxy on `:9090` that forwards HTTP to the device's embedded `:8080` web server and bridges WebSocket to `:8081`. Passes through the existing Bearer token auth gate so remote users (via Cloudflare Tunnel / Tailscale Funnel) can reach individual device dashboards without direct LAN access. HTML rewriting makes `fetch("/api/info")` and WebSocket URLs route through the proxy. Protocol-aware (`ws://` or `wss://`) for tunnel compatibility. "Device Dashboard" link in the desktop app device detail page and in the `:9090` web dashboard device cards.
- **Token expiry / TTL.** Optional `expires_at` on API tokens so they can auto-expire instead of being valid until manually revoked. TTL options: 1h, 24h, 7d, 30d, 90d, or never (default, backward-compatible). Auth gate rejects expired tokens with a distinct error message. Settings UI has a TTL dropdown in the create form and an Expires column with color-coded status (red=expired, amber=<24h, dim=never).
- **Per-IP rate limiting with exponential backoff.** In-memory rate limiter tracks consecutive failed auth attempts per source IP. After 3 grace failures, subsequent requests are rejected with 429 Too Many Requests before the auth check runs. Backoff doubles each attempt (1s → 2s → 4s → ... capped at 60s). Auto-resets after 15 minutes of silence or on successful auth. Loopback exempt. 5 unit tests.

### Fixed

- **Existing users no longer redirected to onboarding wizard.** The first-run redirect checked only the `onboarding_completed` setting, which was never set for users who had devices before the wizard was added. Now queries `saved_devices` from SQLite alongside the setting — if either indicates a returning user, the wizard is bypassed and the flag is auto-set. Also adds a visible "Skip to dashboard" link.
- **6 onboarding wizard UX fixes.** "View Device" marks onboarding complete, device discovery snapshot taken at flash time, `checkDeps` error state surfaced with retry, build failure retry button, stale build output cleared on template change, responsive grid on Configure & Flash step.

### Refactored

- **Settings.tsx split into 8 focused modules.** The 1362-line monolith is replaced by `app/src/pages/settings/` with dedicated files for each section (Config, Notifications, MQTT, API Tokens, Remote Access, Diagnostics/About) plus shared types. Cross-section dependency (token count for the Remote Access zero-token warning) wired via a callback prop through a thin `index.tsx` shell. Zero behavior change.

### Library

- **Library code is byte-identical to v0.4.3 except the `TRELLIS_VERSION` macro.** No microcontroller behavior changes. The Arduino LM and PIO releases exist purely so all three distribution endpoints stay in lockstep with the desktop app version.

## [0.4.3] — 2026-04-08

A desktop UX patch release that fixes the OTA progress bar's "stuck at 0%" symptom — a real bug that bit the v0.4.2 hardware test cycle and could easily have been mistaken for a delivery failure when the OTA had actually succeeded.

### Fixed

- **Desktop OTA UI no longer sits on "Downloading firmware to device... 0%" forever.** The previous flow relied on the device sending `ota_progress` WebSocket events back to the desktop, but the device's WS connection drops the moment OTA flashing starts — so progress=100 never arrives, the success path never fires, and the user has no way to tell whether the bytes were delivered, the device crashed, or the desktop is just not reporting anything. Two changes fix this:
    - **`serve_firmware()` now emits a `device-event` with `event_type: "ota_delivered"`** (or `"ota_delivery_failed"`) the moment `write_all` and `flush` complete on the firmware HTTP listener. This is the point where the bytes have provably left the desktop's send buffer. The Tauri event carries the `device_id` so multiple in-flight OTAs (theoretical — the UI only allows one at a time) wouldn't cross-pollute.
    - **`OtaManager.tsx` adds a "delivered, waiting for reboot" UI phase** that triggers on the new event, plus a reboot watcher that compares the in-flight device's `system.uptime_s` against a baseline captured at click time. When the next heartbeat arrives carrying an uptime *lower* than the baseline (which can only happen after a reboot, since uptime is monotonic per boot), the UI flips to a green "Firmware update complete. The device has rebooted." card. A 60-second soft-success timeout prevents the UI from sitting on "delivered" forever if the heartbeat path is slow to recover (the OTA bytes were delivered, after all — the device just rebooted to somewhere we can't see).
- Validated end-to-end on real ESP32 hardware (greenhouse-controller, 192.168.1.108) by pushing `test/TestDevice/TestDevice.ino` compiled against the v0.4.2-snapshot library. The cross-subnet path was unusually slow this session (~270 B/s sustained, ~3 minutes for 1 MB) which made the in-flight phase visibly long — but that itself was a useful test: the new "delivered" overlay appeared the instant `write_all` returned, and the reboot watcher caught the uptime drop within ~5 seconds of the device reconnecting.

### Library

- **Library code is byte-identical to v0.4.2 except the `TRELLIS_VERSION` macro.** No microcontroller behavior changes. Same situation as v0.4.1 vs v0.4.0: the Arduino LM and PIO releases exist purely so all three distribution endpoints stay in lockstep with the desktop app version (per the release-sync rule).

## [0.4.2] — 2026-04-08

A small patch release that ships two fixes both surfaced by an end-to-end OTA test on real ESP32 hardware: a clippy `never_loop` cleanup in the desktop OTA delivery code, and a previously-undiscovered library crash on the no-WiFi path that affected every example sketch in this repo.

### Fixed

- **Library: `Trellis::loop()` no longer crashes when WiFi never came up.** Devices that call `Trellis::begin(SSID, PASS)`, ignore its return value, and then call `trellis.loop()` from their main `loop()` — which is exactly what every example sketch in this repo does — would crash with `Guru Meditation: LoadProhibited (EXCVADDR=0x08)` within ~50ms of the `[Trellis] WiFi connection timeout` message any time WiFi was unreachable. Root cause: `Trellis::begin()` only allocates `_webServer = new TrellisWebServer(this)` *after* WiFi connects; on timeout it returns false and `_webServer` stays nullptr. The periodic broadcast block in `Trellis::loop()` then dereferenced the null `_webServer`, entered `TrellisWebServer::broadcastUpdate(const char*, bool)` with `this == nullptr`, and read `_ws` at offset 0x08 of the null struct. Fix is a single early-return guard at the top of `Trellis::loop()` if `_webServer == nullptr`. The two now-redundant inner `if (_webServer)` guards are removed. Validated on real hardware: a sketch with intentionally-bad credentials and a 5-second WiFi timeout now produces `[Trellis] WiFi connection timeout` followed by silence — no panic dump, no reboot loop. The `test/TestDevice/TestDevice.ino` sketch was already immune via its own `while (true) delay(1000);` halt-on-failure pattern.
- **Desktop: cleared the `cargo clippy --lib` `never_loop` deny error in `app/src-tauri/src/ota.rs`.** The OTA HTTP server in `serve_firmware()` has always been a one-shot accept-then-stop, expressed as a `for stream in listener.incoming() { ... break; }` loop — clippy's `never_loop` lint is a deny-by-default error on this shape. Pure structural rewrite to `if let Some(stream) = listener.incoming().next()`, no behavior change. Validated end-to-end by an actual OTA push to a real ESP32: the device's panic-dump ELF SHA256 prefix exactly matched the locally-built `.elf`, proving the served bytes landed and the device booted the new image.

### Why these landed together

Both fixes came out of the same hardware test cycle. The desktop clippy fix was the original ~10-minute task; cutting a real OTA push to validate it surfaced the library crash, which is the more serious of the two — anyone running an example sketch with WiFi off (or wrong credentials, or out of range) would hit it. The library fix is the reason this is a patch release rather than waiting for the next minor.

### Library

- **Library code now contains a real behavior fix** (the `Trellis::loop()` early-return guard) on top of the v0.4.1 byte-identical-except-`TRELLIS_VERSION` baseline. Anyone consuming Trellis via Arduino Library Manager or PlatformIO Registry should upgrade — the v0.4.1 (and earlier) library will crash on the no-WiFi path; v0.4.2 will not.

## [0.4.1] — 2026-04-08

A cosmetic patch release that fixes the version string in the desktop app's About dialog and Sidebar badge — and rewires the underlying mechanism so it can't drift again.

### Fixed

- **Stale version strings in the desktop app UI.** The Sidebar badge button, the Sidebar's About modal, and the Settings → About panel were all hardcoded — they showed `v0.1.5` and `Trellis v0.2.0` long after the project had moved past those versions. The strings were missing from `feedback_release_sync.md`'s "six files to bump" list because nobody noticed they were React literals, so every release after 0.2.0 silently shipped the wrong number to users.
- **Wired the React version display to `package.json` so it can't drift again.** Vite's `define` feature injects `__APP_VERSION__` at build time, sourced from `app/package.json` via `fs.readFileSync` at config load. Declaration in `app/src/vite-env.d.ts` keeps the strict `tsc -b` happy. The three call sites now use `v{__APP_VERSION__}` and `Trellis v{__APP_VERSION__}`. Future version bumps no longer need to remember to update React strings — bumping `app/package.json` is enough.

### Library

- **Library code is byte-identical to v0.4.0 except the `TRELLIS_VERSION` macro.** No microcontroller behavior changes. The Arduino LM and PIO releases exist purely so all three distribution endpoints stay in lockstep with the desktop app version (per the release-sync rule).

## [0.4.0] — 2026-04-08

The remote-access release. The v0.3.4 token gate was the prerequisite that made it safe to expose `:9090` beyond the LAN — this release ships everything you need to actually do that, with first-class support for two transports and a token-aware web dashboard that survives the trip through a tunnel.

### Added — Remote access via Cloudflare Tunnel and Tailscale Funnel

- **The user demand.** "I want to flip a switch from outside the house" was the #1 unsolved request after v0.3.4 closed the LAN-exposure surface. Now that the auth boundary exists, layering a tunnel on top is the obvious next step. v0.4.0 picks two transports, documents both end-to-end, and ships the small client-side and server-side changes needed to make the existing dashboard usable through them — without bundling any third-party agents.
- **Cloudflare Tunnel (recommended).** Free, no inbound port, terminates TLS at the edge, brandable URL on your own domain (`https://trellis.<your-domain>`). Composes with Cloudflare Access for free SSO if you want defense in depth on top of the token gate. Settings → Remote Access has a step-by-step recipe.
- **Tailscale Funnel (no-domain alternative).** Three commands (`tailscale up`, `tailscale funnel 9090 on`, done). URL is `*.ts.net`. Personal use is free.
- **No bundled agents.** The user installs `cloudflared` or `tailscale` themselves; Trellis just walks them through it and verifies the result. Keeps the `.deb` lean and avoids re-vendoring third-party software.
- **Token-aware embedded web UI.** The thick-client dashboard at `:9090/` (the same `web_ui.html` that's been there since v0.2.0) now picks up an API token from `localStorage`. On loopback no token is needed and the dashboard works exactly as before. Through a tunnel, the first `/api/*` fetch returns 401, an inline modal pops up asking for a token, the user pastes it once, the page reloads, and every subsequent fetch carries `Authorization: Bearer trls_…`. The token is stored in the browser's `localStorage` and never leaves the client. Wrong-prefix tokens get rejected client-side before round-tripping.
- **`GET /` always allowed by the auth gate.** Pre-auth special case in `api.rs::handle_connection`. The HTML itself contains no secrets and no device data — every dynamic surface (`/api/*`) still goes through the v0.3.4 token gate and is unchanged. This is what makes the dashboard reachable through a tunnel without breaking the rest of the security model.
- **Reachability probe.** New `probe_remote_url` Tauri command. The Settings panel includes a "Test reachability" widget that takes a public URL + a token and runs a single `GET /api/devices` from the desktop machine through the user's tunnel and back. Classifies the result into `success`, `auth_failed`, `not_trellis`, `tunnel_down`, `network_error`, `timeout`, or `unexpected`, and surfaces a human-friendly explanation. Lets users verify the setup before pulling out their phone — no copy-paste curl gymnastics. The token is held in component memory only; only the URL is persisted (as a convenience between probes).
- **Safety check on the Settings panel.** If the user has zero API tokens minted and tries to set up remote access, the panel shows an amber warning card explaining that the tunnel would be reachable but completely unusable without a token. Points them at the API Tokens section above.
- **`require_auth_localhost` interaction.** The strict-loopback opt-in from v0.3.4 still applies to `/api/*`, but the new `GET /` bypass is unconditional — the page itself is harmless static HTML. If you have strict-loopback on, the dashboard at `localhost:9090/` will still load, but its first API fetch will 401 and pop the auth modal locally. Same behaviour as the remote case, intentionally.
- **`auth_required_html` removed.** The friendly HTML 401 page that v0.3.4 served at non-loopback `GET /` is gone — the dashboard itself now handles the "I need to authenticate" UX with the inline modal. Removing dead code.
- **CHANGELOG explicitly out of scope:** WebSocket reverse-proxy through `:9090` (would let per-device `:8080` dashboards be remotely accessible too), multi-user RBAC, token expiry/TTL, rate limiting, ngrok support (free tier rotates URLs, paid is worse than the two options above).

### Documentation

- New §16 **Remote Access** in `docs/guide.md` walks through both transports end-to-end, the test-reachability widget, the "mint a token first" prerequisite, and a one-paragraph "why not ngrok" note. Subsequent sections renumbered.
- §17 (Web Dashboard) updated to mention the inline auth modal as the new auth UX through tunnels.
- FEATURES.md gains a Remote Access section.

### Verified

- `cargo build --release` clean (only the 2 pre-existing dead-code warnings from `connection.rs`).
- `cargo test --lib` 6/6 auth tests pass (no regressions; v0.4.0 is a no-op on the auth boundary itself).
- Frontend `tsc --noEmit` and `vite build` clean.
- Library code is **byte-identical** to v0.3.4 except the version macro (3-line diff in `library.json`, `library.properties`, `Trellis.h`). Per the project's hardware-test rule, no ESP32 re-flash needed for this release — the `.bin` would be provably identical.
- Embedded binary contains the new symbols (`authModal`, `TRELLIS_TOKEN_KEY`, `saveAuthToken`, `probe_remote_url`) — verified via `strings` against the release binary.

### Out of scope (deliberately deferred)

- **Per-device `:8080` dashboards via the tunnel.** Each ESP32 has its own embedded dashboard that uses its own WebSocket on `port+1`. Proxying just the HTML through `:9090` would give a static-looking page with dead controls — fixing it requires WebSocket-aware reverse-proxying through `:9090`, which the current hand-rolled HTTP server doesn't support. Punted to v0.5.0 if there's user demand. The central `:9090` web UI already aggregates every device, so for remote use it's strictly better anyway.
- **Multi-user / RBAC.** Currently every token has full admin access. Tokens have names but no scopes.
- **Token TTL / expiry.** Same model as v0.3.4: valid until revoked.
- **Rate limiting + failed-auth backoff.** Separate hardening pass.
- **Bundling `cloudflared` or `tailscale`.** Out of scope on principle — re-vendoring third-party agents grows the binary, complicates the supply chain, and adds an upgrade burden. Users install them via their distribution's package manager or the upstream installer.

## [0.3.4] — 2026-04-08

A focused security release that finishes closing the LAN-exposure surface that v0.3.3 only partially addressed. The REST API on port 9090 is now token-gated for every non-loopback request.

### Added — REST API authentication tokens

- **The risk model.** v0.3.3 patched one specific leak (the MQTT broker password was being returned in `GET /api/settings/mqtt` over the LAN), but the underlying problem was untouched: the REST API binds to `0.0.0.0:9090` with **zero authentication** for any of its ~30 endpoints. Anyone on the same WiFi could `curl /api/devices/foo/command` to flip switches, drain sensor history, mint webhooks, or read every setting key. The MQTT redaction was a band-aid; this release closes the wound.
- **Bearer tokens, scoped to the REST API.** New `api_tokens` SQLite table stores `(name, sha256-hex digest, created_at, last_used_at)` rows. Tokens are minted in plaintext, returned to the user **exactly once** at creation, and immediately hashed for storage. A stolen database cannot be used to make authenticated requests — the attacker would need to brute-force a 256-bit secret.
- **Token format.** `trls_<43 chars of base64url-no-pad>` (32 bytes from `OsRng`, total length 48). The `trls_` prefix mirrors `ghp_` etc. — greppable in logs and trivially distinguishable from other secrets in a config file.
- **Auth gate logic in `api.rs`.** Every request runs through `auth::check_auth` before route dispatch. CORS preflight (`OPTIONS`) is always allowed; loopback requests (127.0.0.1, ::1) are bypassed by default so the desktop app's embedded WebView and any local CLI work with zero setup; non-loopback requests **always** require a valid `Authorization: Bearer trls_…` header. There is no opt-out and no read-only fallback — if you want LAN access, you mint a token. The first-time user gets a distinct, helpful error message ("open Settings → API Tokens and click Create") instead of the generic "missing Authorization header".
- **Strict-loopback opt-in.** New `require_auth_localhost` setting (off by default). When on, even loopback requests must present a token — defense in depth against malicious local processes on a shared machine. The desktop app authenticates over Tauri IPC, not HTTP, so it's unaffected, but local CLI tools and the embedded `localhost:9090/` web dashboard will then also need a token.
- **Friendly HTML for browser users.** A non-loopback `GET /` (the embedded web dashboard, which is a thick client polling `/api/*` from the same origin) returns a styled HTML page explaining what happened and how to authenticate, instead of a bare JSON 401 dumped onto the user's screen. Points users at the Trellis desktop app, the per-device dashboard at `<device-ip>:8080/`, or the curl token flow.
- **Token management.** Three new Tauri commands (`list_api_tokens`, `create_api_token`, `revoke_api_token`) and three new REST endpoints (`GET/POST/DELETE /api/tokens`) — the REST endpoints are themselves gated by the same auth check, so you can't mint a token from outside without already having one (or being on loopback). The Settings UI grows an **API Tokens** section with a name input, a one-shot "your token is" modal that surfaces the plaintext exactly once with copy + curl-snippet hints, a list of existing tokens with `created_at` / `last_used_at` columns, and a per-row revoke button. Revocation is immediate — the next request bearing that token gets 401.
- **`auth.rs` module.** Token generation, SHA-256 hashing, Bearer header parsing, loopback detection, and the `check_auth` middleware live in their own module so the Tauri command layer and the HTTP middleware share one implementation. 6 unit tests cover token shape, hash stability, scheme parsing, and loopback detection — `cargo test --lib auth::` is the gate.
- **New deps.** `sha2 = "0.10"`, `rand = "0.8"`. Both are tiny, audited, and battle-tested.
- **Verified end-to-end** on the cross-subnet ESP32 at `192.168.1.108`. From a non-loopback IP: `curl http://desktop-pc:9090/api/devices` → 401 with "Authentication required" body; mint a token in Settings → curl with `Authorization: Bearer trls_…` → 200 with the device list; `last_used_at` updates on the next list refresh; revoke the token → next request → 401. From loopback: everything continues to work without setup. The friendly HTML page renders correctly for browser navigation to `http://desktop-pc:9090/`. Toggling strict-loopback mode flips the localhost behaviour as expected.
- **Upgrading from v0.3.3.** The behavior change is loud: any consumer that was talking to the REST API from a non-loopback IP needs a token. The desktop app and the localhost dashboard continue to work with zero changes. The user guide now has a full "Authentication" subsection in §15 with the curl recipe.

### Out of scope (deliberately deferred)

- **Multi-user / role-based access control.** This release gates the existing single-user surface; full RBAC is the next layer up.
- **Token expiry / TTL.** v1 is "valid until revoked" like GitHub PATs. Add later if a workflow needs it.
- **Rate limiting / IP allowlists / failed-auth backoff.** Separate hardening pass.
- **Cookie/session auth, OAuth, JWT.** Bearer tokens are enough for a programmatic API.
- **Auth on the embedded device's own dashboard at `:8080/`.** That's served by the Arduino library, on the device itself, and is a separate trust boundary. Untouched here.

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
