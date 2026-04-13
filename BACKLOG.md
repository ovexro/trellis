# Trellis — Backlog

Forward-looking list: candidate next tasks, speculative enhancements, and known follow-ups that aren't blocking current work. Claude appends here during sessions; user reviews on demand. Git log is authoritative for what has shipped — nothing here is a record of completed work.

## Candidate next tasks

Concrete enough to pick up in a future session. Each has scope + what it unblocks. Not a priority order.

- **Desktop `DeviceDetail` React page — remaining parity gap** — replacing Recharts with the hand-rolled SVG renderer. Long-term refactor option, not required for parity.

- **Floor plan / spatial device layout** — visual room layout with drag-drop device placement (Tier 5 roadmap, larger scope).

- **Voice assistant integration research** — investigate Sinric Pro or direct Alexa/Google Home paths. Requires MQTT bridge (already shipped) as prerequisite.

## Known follow-ups

Small tech debt, edge cases, or minor bugs noticed in passing. Not blocking anything.

### UI & frontend

- **`fmtTooltipTime` and `fmtChartTime` treat SQLite UTC timestamps as local time** (pre-existing). Uptime tooltips inherit this through `formatSqliteUtc + fmtTooltipTime`, so displayed times are shifted by the TZ offset but CONSISTENT with chart annotation tooltips and chart x-axis labels. Fix would affect every chart in the app.

- **Uptime strip x-position maps to the full window while chart annotations map to first/last data point** — minor sub-pixel misalignment if data has big leading/trailing gaps. Acceptable for v1.

- **Annotation x-position is linear time-based while data polyline is index-based** — minor visual mismatch if data has gaps. Acceptable for event markers.

- **First transition in window is "online" → leading segment rendered gray "unknown"**. Strictly-correct would be "offline" (transitions only fire on change), but we can't prove the device was being tracked before window-start. Gray is the safe default.

- **Toggle visual-state update uses `classList.toggle('on', value)` after sendCommand** — only handles switches (bool). Sliders/color/text don't have an equivalent post-command DOM patch for the detail panel, so their visual state could go stale after a server-pushed update. Not currently user-visible because those controls read `this.value` at commit time.

- **OTA annotation click-through may silently fail on devices with long firmware history.** OTA annotations come from `firmware_history`, not `device_logs`, so the severity-filter fallback doesn't apply. If the target OTA row is older than what the detail panel rendered, clicking the marker might do nothing.

### Backend & infrastructure

- **Dashboard discovery cache polling interval is ~2 minutes** — commands round-trip instantly via WS, but `GET /api/devices` can lag up to 2 min after a direct device state change. Not a regression.

- **State transitions only captured going forward** from the 2026-04-11 chart annotations commit — no backfill for pre-existing devices.

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
