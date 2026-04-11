# Trellis — Backlog

Forward-looking list: candidate next tasks, speculative enhancements, and known follow-ups that aren't blocking current work. Claude appends here during sessions; user reviews on demand. Git log is authoritative for what has shipped — nothing here is a record of completed work.

## Candidate next tasks

Concrete enough to pick up in a future session. Each has scope + what it unblocks. Not a priority order.

- **Desktop `DeviceDetail` React page vs web dashboard detail panel parity** — the `:9090` web dashboard's detail panel shipped 6 major surfaces in v0.4.6 (interactive charts with range picker, chart event annotations, annotation click-through, severity filter chips, uptime ribbon, uptime stat line). The desktop React `DeviceDetail` page is the parallel component on the desktop-app side and has not received these features. High-confidence consistency gap. Scope: port each v0.4.6 detail-panel feature to the React page, or explicitly decide the desktop page is deprecated in favor of the web dashboard. Likely a multi-session effort — or a deliberate deprecation decision, which is faster.

- **Add LED brightness slider polish to AutoConnect.ino** — the brightness slider is now live on the ESP32 but hasn't been hardened. Candidates: (1) persist value across reboots to NVS so brightness resumes, (2) sync initial value to the dashboard on discovery (currently shows whatever PWM duty is active), (3) confirm/document how it shares GPIO 2 with the existing LED switch. Needs an ESP32 re-flash.

- **Uptime timeline polish pass — remaining candidates**:
  - Clustering when >5 transitions collapse to <10px each, render as striped "noisy" bar with expand-on-hover detail view.
  - Extend to the Metrics tab so every device shows its strip side-by-side for at-a-glance fleet health.

- **Polish-pass sweep of the detail panel on a real phone** — filter chip mobile layout, uptime strip mobile layout, uptime stat line mobile wrapping, tab-key focus order, keyboard shortcut to open the panel from the device list, ensure chip/strip/stat-line state clears correctly on device switch. Requires real phone testing.

- **Second skill crystallization: `session-start-gate`** — the kickoff prompt is ~140 lines of mechanical verification. A skill would let the kickoff shrink to "Continuing Trellis. <last-session bullets>" with the verification list living in a skill file. Prerequisite met (first skill at routine, runs=5). **Propose + get explicit go/no-go — DO NOT write autonomously** because it touches the session-start flow.

- **Third skill crystallization: `release-cut`** — the v0.4.6 release walked through 11 steps (hardware test → version bump → Cargo.lock → CHANGELOG → commit+push → CI wait → release-library.sh → manual pio publish → registry verify → rebuild-install skill → FEATURES.md retag). Two steps need the user (hardware test, pio publish) so it wouldn't fire fully autonomously. **Propose + get explicit go/no-go.**

- **Floor plan / spatial device layout** — visual room layout with drag-drop device placement (Tier 5 roadmap, larger scope).

- **Voice assistant integration research** — investigate Sinric Pro or direct Alexa/Google Home paths. Requires MQTT bridge (already shipped) as prerequisite.

## Known follow-ups

Small tech debt, edge cases, or minor bugs noticed in passing. Not blocking anything.

### UI & frontend

- **`fmtTooltipTime` and `fmtChartTime` treat SQLite UTC timestamps as local time** (pre-existing). Uptime tooltips inherit this through `formatSqliteUtc + fmtTooltipTime`, so displayed times are shifted by the TZ offset but CONSISTENT with chart annotation tooltips and chart x-axis labels. Fix would affect every chart in the app.

- **Uptime strip x-position maps to the full window while chart annotations map to first/last data point** — minor sub-pixel misalignment if data has big leading/trailing gaps. Acceptable for v1.

- **Annotation x-position is linear time-based while data polyline is index-based** — minor visual mismatch if data has gaps. Acceptable for event markers.

- **First transition in window is "online" → leading segment rendered gray "unknown"**. Strictly-correct would be "offline" (transitions only fire on change), but we can't prove the device was being tracked before window-start. Gray is the safe default. Power users can infer from the first colored segment.

- **Uptime strip has no legend entry for "unknown"** if no inferred segment lands in the window. Minor.

- **Toggle visual-state update uses `classList.toggle('on', value)` after sendCommand** — only handles switches (bool). Sliders/color/text don't have an equivalent post-command DOM patch for the detail panel, so their visual state could go stale after a server-pushed update. Not currently user-visible because those controls read `this.value` at commit time. Same class of bug as the v0.4.6 switch toggle fix, but for non-boolean controls.

- **Stale-fetch guard inside `openDeviceDetail` is logs-only.** The Recent Logs section sets and checks `currentLogDeviceId` before and after its `await` to prevent a mid-switch race from writing old results into a new panel. The chart fetch path inside the same flow does NOT have the equivalent guard. Race: user opens device A, opens device B before A's chart fetch returns, A's chart data renders into B's panel. Small window; reproduces only if the user switches panels faster than the chart fetch completes.

- **OTA annotation click-through may silently fail on devices with long firmware history.** The annotation click fallback re-fetches `GET /logs?severity=state,error,warn` when the initial 200-log load didn't include the target row. OTA annotations come from `firmware_history`, not `device_logs`, so the fallback doesn't apply. If the target OTA row is older than what the detail panel already rendered, clicking the marker might do nothing. Verification step: check whether `openDeviceDetail` always fetches the full firmware history or slices it; if sliced, add a fallback equivalent for the firmware-history path.

- **`cssEscape` on uptime ribbon segment click not verified.** Annotation click-through uses `cssEscape` to guard the query selector against unusual device IDs. The uptime ribbon segment click re-uses the annotation click-through pattern ("activates State filter chip + flash-highlights matching log row") but it's not confirmed that the segment's DOM-patch path also goes through `cssEscape`. Low-impact until someone uses a device ID with CSS-special characters, but worth a quick audit.

### Backend & infrastructure

- **Dashboard discovery cache polling interval is ~2 minutes** — commands round-trip instantly via WS, but `GET /api/devices` can lag up to 2 min after a direct device state change. Not a regression; surfaced during v0.4.6 hardware test.

- **State transitions only captured going forward** from the 2026-04-11 chart annotations commit — no backfill for pre-existing devices.

- **Data retention scope unknown for `firmware_history` and `alerts` tables.** The `data_retention_days` setting is documented as "metrics and device logs cleanup period." Chart annotations come from a union of `firmware_history` + `device_logs`, so log pruning implicitly trims the annotation view — that's fine. But `firmware_history` and `alerts` tables: are they pruned by the same setting, pruned separately, or never pruned? If never pruned, firmware history and acknowledged alerts grow unboundedly. Verification step: grep the cleanup loop in `db.rs` to see which tables it actually touches.

- **WebSocket push rate limiting parity not verified.** The REST API path (`/api/*`) has per-IP rate limiting with exponential backoff (v0.4.4). The WebSocket push endpoint (`/ws?token=...`) passes through the token auth gate per v0.4.5 but it's unclear whether it also passes through the rate limiter. If a viewer token can open unlimited WS connections without backoff, that's a low-severity DoS vector accessible to any token holder. Verification step: check `api.rs::handle_connection` WS branch against the rate-limit guard.

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
