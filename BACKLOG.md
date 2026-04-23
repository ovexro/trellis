# Trellis — Backlog

Forward-looking list: candidate next tasks, speculative enhancements, and known follow-ups that aren't blocking current work. Claude appends here during sessions; user reviews on demand. Git log is authoritative for what has shipped — nothing here is a record of completed work.

## Candidate next tasks

Concrete enough to pick up in a future session. Each has scope + what it unblocks. Not a priority order.

- **Dashboard card inline color picker** — color capabilities currently show a read-only swatch on cards; could add an inline color picker that opens without navigating to detail page. Low priority.
- **Offline-aware scene scaffolder** — Scene from room currently relies on live in-memory capabilities, which are empty for offline devices. Could fall back to a cached-capabilities source (last known descriptor from `device_templates` or equivalent) so offline rooms still get useful scaffolding. Defer until a real user hits this.
- **Energy tracking — sliders phase 3 (non-linear lookup)** — phase 2 shipped post-v0.21.0 as the linear opt-in (design (a) in the original sketch: `nameplate_watts × (value / max)` gated by a `linear_power` checkbox + `slider_max` column on `capability_meta`). Phase 3 would layer a 0-100% → watts lookup table for LED drivers with CC/CV curves, motor torque curves, etc. Only worth shipping when a user files a concrete case where linear is visibly wrong — heater/incandescent loads cover most practical setups today.
- **Energy tracking — hardware meters (INA219/PZEM)** — long tail. Library-side addition: new capability type `energy_meter` with live W / Wh reporting. Desktop side reuses the existing `DeviceEnergy` renderer but sources from metric rows instead of derived on-time.
- **rtl8xxxu WiFi dongle causes transient desktop↔ESP32 unreachability** — during 2026-04-20 OTA investigation the desktop occasionally gets "No route to host" for 192.168.1.108 (the ESP32); the stale negative entry clears after `sudo ip route flush cache` and the ESP32 is fully reachable again. Router can reach the device even when desktop can't — it's the second-hop timing (desktop→router→br-lan→rtl8xxxu→device) that occasionally drops. Per `~/.claude/projects/-home-ovidiu/memory/project_router_setup.md` this is a known rtl8xxxu limitation; mitigation is replacing the TP-Link RTL8192EU dongle with a MediaTek (mt76) adapter. Nothing to fix in Trellis code — but if the session-start GATE ever reports ESP32 unreachable even though the router can see it, try the route-cache flush before investigating anything else. Also: the pre-v0.15.0 theory that `ota::serve_firmware`'s `get_local_ip()` (UDP-connect to 8.8.8.8) bound to an unroutable IP is FALSIFIED — tcpdump on the router during a live OTA shows the ESP32 completing multi-KB TCP exchanges with the desktop cleanly; the binding works. Do not re-open that investigation.

## Known follow-ups

Small tech debt, edge cases, or minor bugs noticed in passing. Not blocking anything.

### UI & frontend

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
- **PWA install flow on a real phone** — not tested *from a phone*. Headless Chrome smoke on 2026-04-15 confirmed: manifest has required fields (name, start_url=/, display=standalone, theme/background colors, 192+512 icons), SW registers and controls `/`, `beforeinstallprompt` handler present in page, script calls `Notification.requestPermission`, offline banner hidden when `navigator.onLine` is true. Only the final Android/iOS install-prompt UX is unverified.
- **Browser notifications on mobile** — see PWA note above; API + wiring verified in headless, only the real mobile permission prompt path unverified.
- **Uptime strip + stat line on a real phone** — 30px height + legend at ≤640px relies on SVG `width:100%`; stat line uses `flex-wrap:wrap`. Embedded web UI smoke passed at 375/640/1280 (no horizontal overflow, no console errors). Uptime strip itself only renders on per-device detail views in the React desktop app (not the embedded web UI), so phone-visibility of the strip is specifically the React path when wrapped in a tunnel/PWA launch.
- **WebSocket push through a tunnel** — not tested.

- **Real Cloudflare Tunnel / Tailscale Funnel end-to-end test** — transport code shipped, never exercised against a real tunnel. Numbered recipe (run at your convenience, ~10 min):

  **A. Prepare**
  1. Open Trellis → **Settings → API Tokens** → click **Create token** → name it `tunnel-smoke` → copy the `trls_…` value.
  2. In **Settings → Remote Access**, note the **Test reachability** widget — you'll use it in steps B5 and C4.

  **B. Cloudflare Tunnel (quick, no domain needed — uses `trycloudflare.com`)**
  1. `curl -L --output /tmp/cloudflared https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-linux-amd64 && chmod +x /tmp/cloudflared` (skip if `cloudflared` is already installed).
  2. `/tmp/cloudflared tunnel --url http://localhost:9090` — it prints `https://<random>.trycloudflare.com` on stdout. Copy that URL.
  3. In a **second terminal**: `curl -s -H "Authorization: Bearer trls_…" https://<random>.trycloudflare.com/api/devices | head -c 300` — expect a JSON array starting with your real device.
  4. Open the URL from step B2 in your **phone's browser**; at the token prompt paste the `trls_…` from A1. Dashboard should load with live data.
  5. Back on desktop, in **Settings → Remote Access → Test reachability**: paste the tunnel URL + token, click **Test**. Expect green `success` with a latency number.
  6. Stop cloudflared (`Ctrl-C` in terminal from B2). Optional: for a named tunnel with your own domain, follow `docs/guide.md §Cloudflare Tunnel`.

  **C. Tailscale Funnel (one-liner if Tailscale is set up)**
  1. `tailscale status` — confirm this machine is on your tailnet. If not: `sudo tailscale up` and sign in.
  2. `sudo tailscale funnel 9090 on` — prints `https://<hostname>.<tailnet>.ts.net/`. Copy that.
  3. `curl -s -H "Authorization: Bearer trls_…" https://<hostname>.<tailnet>.ts.net/api/devices | head -c 300` — expect JSON array.
  4. In **Settings → Remote Access → Test reachability**: paste URL + token, click **Test**. Expect `success`.
  5. `sudo tailscale funnel 9090 off` to tear down.

  **D. Record findings**
  - If both B5 and C4 return `success` in the UI probe, remove this item from "Not tested yet" and tick `feedback_hardware_test.md` for the next release.
  - If either fails with `auth_failed`, re-mint the token (step A1) and retry.
  - If it fails with `tunnel_down` / `timeout`, verify Trellis is running (`curl http://localhost:9090/api/mqtt/status`) and retry the `curl` in B3/C3 first.
  - Report back any result other than `success` so the probe categories (`tunnel_down`, `not_trellis`, etc.) can be re-checked against real error shapes.

### External

- **Arduino forum thread active** (`reference_forum.md`) — monitor for new user reports.
