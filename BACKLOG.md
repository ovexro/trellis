# Trellis — Backlog

Forward-looking list: candidate next tasks, speculative enhancements, and known follow-ups that aren't blocking current work. Claude appends here during sessions; user reviews on demand. Git log is authoritative for what has shipped — nothing here is a record of completed work.

## Candidate next tasks

Concrete enough to pick up in a future session. Each has scope + what it unblocks. Not a priority order.

- **Desktop `DeviceDetail` React page — v0.4.6 detail-panel parity port** — investigated 2026-04-11, deprecation ruled out: `DeviceCard.tsx:34` navigates `/device/:id` on card click (primary desktop flow) and the desktop shell does NOT embed `:9090` in an iframe, so every desktop user lands in the React page and never sees the `:9090` slide-out panel. Port plan below (multi-session).

  **Confirmed gaps** vs `:9090` detail panel (which has all 6 v0.4.6 surfaces):
  1. ~~Chart event annotations (OTA/state/error/warn)~~ — ✅ **shipped** as P1(a); Recharts `<ReferenceLine>` + `<ReferenceDot>` pair per annotation, numeric-time XAxis, kind-present-only legend below chart, native SVG `<title>` tooltip. New Rust `get_device_annotations` Tauri command wraps existing `Database::get_annotations`.
  2. ~~Annotation click-through (marker → scroll+highlight log row)~~ — ✅ **shipped** as P2(d); `MetricChart.tsx` `onAnnotationClick` → `DeviceLogs.tsx` `scrollToLog` with flash-highlight
  3. ~~Recent Logs chip row~~ — ✅ **shipped** as P1(b); 7-chip row (All/Events/State/Error/Warn/Info/Debug) with server-side re-fetch, stale-fetch guard, live-log filter guard, `key={device.id}` remount on device switch for filter reset parity. Rust `get_device_logs` now takes optional comma-separated `severity` arg.
  4. ~~Uptime timeline ribbon~~ — ✅ **shipped** as P1(c); SVG ribbon + stat line + legend via `UptimeTimeline.tsx`
  5. ~~Uptime stat line~~ — ✅ **shipped** as part of P1(c); included in `UptimeTimeline.tsx` stat line row
  6. Chart range picker (`1h/6h/24h/7d`) — ✅ already parity via Recharts

  **P1 — ship-blocker parity sub-tasks** (each a self-contained session):
  - ~~**(a) Chart annotations overlay on `MetricChart.tsx`**~~ — ✅ **shipped**. Added `get_device_annotations` Tauri command (`commands.rs` + `lib.rs` handler). `MetricChart.tsx` switched to `type="number"` XAxis with `time` field (ms), fetches metrics + annotations in parallel via `Promise.all`, renders `<ReferenceLine>` (dashed, opacity 0.7) + `<ReferenceDot>` (custom shape with transparent 6px hit target + 3px visible circle + native `<title>`) per in-window annotation. Legend row below chart shows only kinds present, in stable order (`ota, online, offline, warn, error`). Colors/labels mirror `annColor()`/`annLabel()` in `web_ui.html` exactly. YAxis uses explicit `[yMin - 10%, yMax + 18%]` domain so the marker row at `rawMax + 14%` sits predictably near the top of the plot.
  - ~~**(b) Recent Logs chip row parity in `DeviceLogs.tsx`**~~ — ✅ **shipped**. Severity arg on `get_device_logs` maps to existing `Database::get_logs_filtered`. `DeviceLogs.tsx` rewritten with 7 chips, fetchGenRef stale-fetch guard, filterRef for live-log listener. `DeviceDetail.tsx` passes `key={device.id}` so the component remounts (resetting filter to "all") on device switch, matching the `:9090` reset behavior.
  - ~~**(c) New `<UptimeTimeline>` component**~~ — ✅ **shipped**. `UptimeTimeline.tsx` derives online/offline/unknown segments from annotations (same algorithm as `renderUptimeTimeline()` in `web_ui.html`), renders SVG ribbon with stat line (online %, tracked span, transitions) and legend. Uses existing `get_device_annotations` Tauri command — no new backend work. Slotted into `DeviceDetail.tsx` between System stats and Sensor Charts with its own time range picker. Perf: `memo`, `useMemo`, CSS containment, no closure SVG props, no polling interval.

  **P2 — consistency polish** (batch into one session):
  - ~~**(d) Annotation click-through**~~ — ✅ **shipped**. `MetricChart.tsx` accepts `onAnnotationClick` callback, Recharts `ReferenceDot` onClick + cursor-pointer. `DeviceDetail.tsx` mediates via `useRef<DeviceLogsHandle>`. `DeviceLogs.tsx` converted to `forwardRef` with `useImperativeHandle` exposing `scrollToLog(timestamp, targetFilter)`. Tries current DOM first, then switches filter chip and retries after refetch. Flash-highlight via CSS `annFlash` keyframe (amber 1.5s fade). OTA annotations skipped (firmware history click-through not in scope).
  - ~~**(e) Uptime ribbon segment click**~~ — ✅ **shipped**. `UptimeTimeline.tsx` accepts `onSegmentClick` callback, non-inferred segments carry `annotationTs` from the original annotation. Click activates the `State` filter chip and scrolls to the matching transition log row. Same `scrollToLog` path as annotation click-through.
  - ~~**(f) Visual parity polish**~~ — ✅ **shipped**. Cursor pointer on annotation dots and uptime segments. Hover brightness effect (`filter: brightness(1.3)`) on uptime segments via `.uptime-seg-hover:hover` CSS class.

  **P3 — explicitly out of scope for parity effort:**
  - Top-level Metrics tab (the `:9090` dashboard has one; desktop does not). That's a new feature, not a parity gap. Track separately if pursued.
  - Replacing Recharts with the hand-rolled SVG renderer. Long-term refactor option, not required for parity.

  **Suggested session breakdown:** ~~N+1 = (b) chip row~~ (done). ~~N+2 = (a) chart annotations~~ (done). ~~N+3 = (c) uptime timeline~~ (done). ~~N+4 = (d)+(e)+(f) cleanup~~ (done). **All P1+P2 sub-tasks shipped. Only P3 out-of-scope items remain.**

- **Uptime timeline polish pass — remaining candidates**:
  - ~~Clustering when >5 transitions collapse to <10px each, render as striped "noisy" bar with expand-on-hover detail view.~~ — ✅ **shipped** (ca8b632). Expand-on-hover deferred — the tooltip shows transition count + timespan which is sufficient.
  - ~~Extend to the Metrics tab so every device shows its strip side-by-side for at-a-glance fleet health.~~ — ✅ **shipped** (4d1a6ca). Compact 10px ribbon per device with online % label, reuses existing annotation fetch.

- ~~**Polish-pass sweep of the detail panel on a real phone**~~ — ✅ **shipped** (9408006). Chart touch-scrolling fix (`touch-action: pan-y pinch-zoom`), mobile touch target bump (`min-height: 2rem`), stat line separator grouping (`white-space: nowrap`). Verified via headless Chrome at 375px and 320px.

- ~~**Tab-key focus trap in detail panel**~~ — ✅ **shipped** (0c6207b). Tab/Shift+Tab trapped inside panel, Escape returns focus to trigger, `aria-hidden` on background, `role="dialog" aria-modal="true"` on panel. Verified via headless Chrome (desktop 1280px + mobile 320px) with 20 automated assertions.

- ~~**Keyboard-accessible Details links**~~ — ✅ **shipped** (c2df230). Device card "Details" link gets `href="#"` (Tab-reachable, Enter opens panel). Metrics tab device header gets `tabindex="0"`, `role="button"`, Enter/Space handler. Companion to focus trap. Verified via headless Chrome with 8 automated assertions.

- ~~**Second skill crystallization: `session-start-gate`**~~ — ✅ **shipped**. `skill_session_start_gate.md` written, confidence `unverified`. Next session fires it for the first time under observation.

- ~~**Third skill crystallization: `release-cut`**~~ — ✅ **shipped**. `skill_release_cut.md` written, confidence `unverified`. Next release fires it for the first time under observation.

- **Floor plan / spatial device layout** — visual room layout with drag-drop device placement (Tier 5 roadmap, larger scope).

- **Voice assistant integration research** — investigate Sinric Pro or direct Alexa/Google Home paths. Requires MQTT bridge (already shipped) as prerequisite.

- ~~**Web dashboard GitHub OTA**~~ — ✅ **shipped** (ad032ec). "Update from GitHub" section in device detail panel (ESP32 + admin only). Repo input, release list, Flash buttons, per-device persistence.

## Known follow-ups

Small tech debt, edge cases, or minor bugs noticed in passing. Not blocking anything.

### UI & frontend

- **`fmtTooltipTime` and `fmtChartTime` treat SQLite UTC timestamps as local time** (pre-existing). Uptime tooltips inherit this through `formatSqliteUtc + fmtTooltipTime`, so displayed times are shifted by the TZ offset but CONSISTENT with chart annotation tooltips and chart x-axis labels. Fix would affect every chart in the app.

- **Uptime strip x-position maps to the full window while chart annotations map to first/last data point** — minor sub-pixel misalignment if data has big leading/trailing gaps. Acceptable for v1.

- **Annotation x-position is linear time-based while data polyline is index-based** — minor visual mismatch if data has gaps. Acceptable for event markers.

- **First transition in window is "online" → leading segment rendered gray "unknown"**. Strictly-correct would be "offline" (transitions only fire on change), but we can't prove the device was being tracked before window-start. Gray is the safe default. Power users can infer from the first colored segment.

- **Uptime strip has no legend entry for "unknown"** if no inferred segment lands in the window. Minor.

- **Toggle visual-state update uses `classList.toggle('on', value)` after sendCommand** — only handles switches (bool). Sliders/color/text don't have an equivalent post-command DOM patch for the detail panel, so their visual state could go stale after a server-pushed update. Not currently user-visible because those controls read `this.value` at commit time. Same class of bug as the v0.4.6 switch toggle fix, but for non-boolean controls.

- ~~**Stale-fetch guard inside `openDeviceDetail` is logs-only.**~~ — ✅ **fixed** (fb0cd0b). Added `id !== currentLogDeviceId` check after `Promise.all` in `openDeviceDetail` (covers logs, firmware, sparklines) and `id !== detailChartDeviceId` check after fetches in `loadDetailCharts` (covers metric charts and uptime ribbon). `closeDeviceDetail` now resets `detailChartDeviceId` too.

- **OTA annotation click-through may silently fail on devices with long firmware history.** The annotation click fallback re-fetches `GET /logs?severity=state,error,warn` when the initial 200-log load didn't include the target row. OTA annotations come from `firmware_history`, not `device_logs`, so the fallback doesn't apply. If the target OTA row is older than what the detail panel already rendered, clicking the marker might do nothing. Verification step: check whether `openDeviceDetail` always fetches the full firmware history or slices it; if sliced, add a fallback equivalent for the firmware-history path.

- ~~**`cssEscape` on uptime ribbon segment click not verified.**~~ — ✅ **audited** (2026-04-12). Both paths escape the timestamp via `cssEscape(ts)` in the selector; `deviceId` never enters a `querySelector` in either path. No issue.

### Backend & infrastructure

- **Dashboard discovery cache polling interval is ~2 minutes** — commands round-trip instantly via WS, but `GET /api/devices` can lag up to 2 min after a direct device state change. Not a regression; surfaced during v0.4.6 hardware test.

- **State transitions only captured going forward** from the 2026-04-11 chart annotations commit — no backfill for pre-existing devices.

- ~~**Data retention scope unknown for `firmware_history` and `alerts` tables.**~~ — ✅ **audited** (2026-04-12). Cleanup loop (`lib.rs:167-183`) prunes `metrics` + `device_logs` only. `firmware_history` grows at ~1 row/OTA/device (negligible) and pruning it would break rollback + OTA annotations. `alerts` are user-created rule definitions, not data — zero automatic growth. Current scope is correct; no changes needed.

- ~~**WebSocket push rate limiting parity not verified.**~~ — ✅ **audited** (2026-04-12). The `/ws` upgrade hits the same `rate_limiter.check()` at `api.rs:411` and the same auth gate as every REST endpoint. Success clears failure state; denial records failure with exponential backoff. Full parity confirmed.

- **OTA 100% duplicate event** — `httpUpdate.onProgress` fires 100% twice. Cosmetic, not fixed.

- **Old tags v0.1.8 → v0.3.1 still ship bloated zip on Arduino LM** (immutable index entries). Only v0.3.2+ is lean.

### Not tested yet

- **Arduino LM indexer pickup of each release** — verify at next session start. Don't re-investigate unless stale >7 days per `feedback_arduino_lm_indexer.md`.
- **Real Cloudflare Tunnel / Tailscale Funnel end-to-end test** — transport code shipped, never exercised against a real tunnel.
- **MQTT `tls_skip_verify` end-to-end with a real self-signed broker** — shipped in v0.4.5 (uses rustls `dangerous()` API with a custom `NoVerifier`) but never exercised against an actual self-signed cert. Code path looks right, needs a hardware-adjacent smoke test.
- **PWA install flow on a real phone** — not tested.
- **Browser notifications on mobile** — not tested.
- **Uptime strip + stat line on a real phone** — 30px height + legend at ≤640px relies on SVG `width:100%`; stat line uses `flex-wrap:wrap`. Should work, unverified.
- **DnD device card reorder** — only tested with 1 device.
- **WebSocket push through a tunnel** — not tested.

### External

- **Arduino forum thread active** (`reference_forum.md`) — monitor for new user reports.
