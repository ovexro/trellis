# Trellis — Backlog

Forward-looking list: candidate next tasks, speculative enhancements, and known follow-ups that aren't blocking current work. Claude appends here during sessions; user reviews on demand. Git log is authoritative for what has shipped — nothing here is a record of completed work.

## Candidate next tasks

Concrete enough to pick up in a future session. Each has scope + what it unblocks. Not a priority order.

- **Add LED brightness slider polish to AutoConnect.ino** — the brightness slider is now live on the ESP32 but hasn't been hardened. Candidates: (1) persist value across reboots to NVS so brightness resumes, (2) sync initial value to the dashboard on discovery (currently shows whatever PWM duty is active), (3) confirm/document how it shares GPIO 2 with the existing LED switch. Needs an ESP32 re-flash.

- **Uptime timeline polish pass — remaining candidates**:
  - Clustering when >5 transitions collapse to <10px each, render as striped "noisy" bar with expand-on-hover detail view.
  - Extend to the Metrics tab so every device shows its strip side-by-side for at-a-glance fleet health.

- **Polish-pass sweep of the detail panel on a real phone** — filter chip mobile layout, uptime strip mobile layout, uptime stat line mobile wrapping, tab-key focus order, keyboard shortcut to open the panel from the device list, ensure chip/strip/stat-line state clears correctly on device switch. Requires real phone testing.

- **Second skill crystallization: `session-start-gate`** — the kickoff prompt is ~140 lines of mechanical verification. A skill would let the kickoff shrink to "Continuing Trellis. <last-session bullets>" with the verification list living in a skill file. Prerequisite met (first skill at routine, runs=5). **Propose + get explicit go/no-go — DO NOT write autonomously** because it touches the session-start flow.

- **Third skill crystallization: `release-cut`** — the v0.4.6 release walked through 11 steps (hardware test → version bump → Cargo.lock → CHANGELOG → commit+push → CI wait → release-library.sh → manual pio publish → registry verify → rebuild-install skill → FEATURES.md retag). Two steps need the user (hardware test, pio publish) so it wouldn't fire fully autonomously. **Propose + get explicit go/no-go.**

- **Manual gap-detection pass** — one-shot audit of FEATURES.md against the codebase looking for incomplete implementations, consistency gaps across parallel components, edge cases not handled. Report findings, do not act. Evaluate whether the output is useful enough to crystallize into a recurring skill.

- **Topical split of `project_overview.md`** — currently 239 lines stacking version history back to v0.3.0. Per `feedback_memory_split.md`, extract v0.3.x and older into `project_history_archive.md` with bidirectional `**See also:**` pointers. Optional — the file isn't broken.

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

- **Toggle visual-state update uses `classList.toggle('on', value)` after sendCommand** — only handles switches (bool). Sliders/color/text don't have an equivalent post-command DOM patch for the detail panel, so their visual state could go stale after a server-pushed update. Not currently user-visible because those controls read `this.value` at commit time.

### Backend & infrastructure

- **Dashboard discovery cache polling interval is ~2 minutes** — commands round-trip instantly via WS, but `GET /api/devices` can lag up to 2 min after a direct device state change. Not a regression; surfaced during v0.4.6 hardware test.

- **State transitions only captured going forward** from the 2026-04-11 chart annotations commit — no backfill for pre-existing devices.

- **OTA 100% duplicate event** — `httpUpdate.onProgress` fires 100% twice. Cosmetic, not fixed.

- **Old tags v0.1.8 → v0.3.1 still ship bloated zip on Arduino LM** (immutable index entries). Only v0.3.2+ is lean.

### Not tested yet

- **Arduino LM indexer pickup of each release** — verify at next session start. Don't re-investigate unless stale >7 days per `feedback_arduino_lm_indexer.md`.
- **Real Cloudflare Tunnel / Tailscale Funnel end-to-end test** — transport code shipped, never exercised against a real tunnel.
- **PWA install flow on a real phone** — not tested.
- **Browser notifications on mobile** — not tested.
- **Uptime strip + stat line on a real phone** — 30px height + legend at ≤640px relies on SVG `width:100%`; stat line uses `flex-wrap:wrap`. Should work, unverified.
- **DnD device card reorder** — only tested with 1 device.
- **WebSocket push through a tunnel** — not tested.

### External

- **Arduino forum thread active** (`reference_forum.md`) — monitor for new user reports.
