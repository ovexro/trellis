# Trellis

**The easiest way to deploy and control ESP32 and Pico W devices.**

Trellis is a desktop app + microcontroller library that makes your boards feel like real products. Plug in a device, it appears in the app, and you get auto-generated controls — no config files, no cloud, no YAML.

> **No cloud.** No account. No subscription. Your data never leaves your network.
> **No config.** Devices describe themselves. Controls render automatically.
> **No complexity.** One command to install. 15 lines to integrate.
>
> Read the full story: **[What is Trellis?](ABOUT.md)** | **[User Guide](docs/guide.md)**

![Device Detail — live controls, sensor charts, system stats](screenshots/device-detail.png)

## How it works

1. Drop the Trellis library into your Arduino sketch
2. Declare what your device can do (switches, sensors, sliders)
3. Open the Trellis desktop app — your device appears automatically
4. Control it, monitor it, update its firmware — all from one place

```cpp
#include <Trellis.h>

Trellis trellis("Greenhouse Controller");

void setup() {
  trellis.addSwitch("pump", "Water Pump", 13);
  trellis.addSensor("temp", "Temperature", "C");
  trellis.addSlider("fan", "Fan Speed", 0, 100, 25);
  trellis.beginAutoConnect();   // captive-AP WiFi provisioning, no creds in source
}

void loop() {
  trellis.setSensor("temp", readDHT());
  trellis.loop();
}
```

The desktop app discovers your device via mDNS, reads its capability declaration, and renders the right controls — toggle for the pump, gauge for temperature, slider for fan speed.

**Don't want to write this by hand?** The desktop app and embedded `:9090` web dashboard ship a [**Sketch Generator**](#sketch-generator) — pick capabilities from a form, get a ready-to-flash `.ino`. Save the spec to a portable JSON file, share it, restore it on any install.

## Dependencies

The Trellis Arduino library requires two dependencies:

| Library | Author | Purpose |
|---------|--------|---------|
| [ArduinoJson](https://github.com/bblanchon/ArduinoJson) | Benoit Blanchon | JSON parsing for the self-description protocol |
| [WebSockets](https://github.com/Links2004/arduinoWebSockets) | Markus Sattler | Real-time communication with the desktop app |

**PlatformIO** installs these automatically.

**Arduino IDE Library Manager** will offer to install these when you install Trellis — click **"INSTALL ALL"** in the dependency dialog.

If you installed Trellis manually (e.g. via "Add .ZIP Library…"), you need to install the dependencies yourself:

1. Open **Sketch → Include Library → Manage Libraries…**
2. Search for **ArduinoJson** and click Install
3. Search for **WebSockets** (by Markus Sattler) and click Install

## Install

### Linux (one command)

```bash
curl -fsSL https://raw.githubusercontent.com/ovexro/trellis/main/install.sh | bash
```

Works on Ubuntu, Linux Mint, Debian, Fedora, Arch, and derivatives. Installs dependencies, downloads the app, creates a desktop entry, and optionally installs Arduino CLI.

### Manual download

Download from [GitHub Releases](https://github.com/ovexro/trellis/releases):
- **Ubuntu/Mint/Debian** → `.deb`
- **Fedora/RHEL** → `.rpm`
- **Any Linux** → `.AppImage`

## Features

### Sketch Generator

The flagship of recent releases. Skip the boilerplate — build a device from a form.

- **Form to `.ino`** — pick a board (ESP32 / Pico W), add capabilities (switch, slider, color, sensor, text), set GPIOs and labels, get a complete Arduino sketch with `#include <Trellis.h>`, the constructor, an `onCommand` handler, captive-AP WiFi provisioning, and one `addX` call per capability
- **Live preview** — every change re-renders the source pane; backend validation surfaces inline ("Sketch can't be generated: …")
- **Compile & Flash from the app** — `arduino-cli` integration handles board cores + library deps; click once to compile, click again to flash to a USB-connected board
- **Library version pin (v0.29.0)** — the desktop binary embeds a `lib_manifest.json` declaring exactly which Trellis library version it was built against. The wizard pins `arduino-cli lib install Trellis@<version>` so a wizard-compiled sketch always binds to the same API the manifest advertises. If the installed library drifts from the manifest, an amber banner with a one-click "Update" appears
- **Portable templates (v0.29.0)** — save your spec to a `.trellis-template.json` file, share it (forum, Slack, Git), import it back on any install. The wrapper format carries a manifest version stamp so an older template imported into a newer Trellis surfaces a "saved against X.Y.Z, you're on A.B.C" warning instead of a silent mismatch
- **Two surfaces** — both the desktop wizard and the embedded `:9090` web dashboard's Sketch tab call the same backend, so onboarding and standalone generation emit byte-identical sketches

### Desktop App
- **Auto-discovery** — continuous mDNS scanning, devices appear automatically
- **Live updates** — persistent WebSocket connections, real-time sensor data
- **Device cards** — name, status, RSSI, uptime, firmware version, drag-and-drop reorder
- **Auto-generated controls** — switches, sliders, sensors, color pickers, text
- **Interactive charts** — time-series sensor data with range picker, hover tooltips, event annotations (OTA/online/offline markers), click-through to logs
- **Uptime timeline** — visual ribbon showing online/offline history with stats
- **Energy tracking** — per-switch nameplate watts + WS-captured on/off transitions yield estimated kWh; offline-aware (won't credit a switch that was on while the device was unreachable). Optional cost-per-kWh + currency renders a $-cost line. 24h / 7d / 30d windows
- **Floor plans** — drag your devices onto a 2D plan of your space; per-room chips group, filter, and scope automation. Build a "Scene from {Room}" with one click — auto-scaffolds a scene targeting every controllable capability in the filtered room
- **Severity-filtered logs** — chip row (All/Events/State/Error/Warn/Info/Debug)
- **Serial monitor** — full USB serial terminal with live streaming
- **OTA updates** — local file upload or pull firmware from any GitHub Release (.bin/.bin.gz)
- **Onboarding wizard** — 4-step guided setup with 5 bundled starter templates and Quick Flash; uses the same backend as the standalone Sketch Generator
- **MQTT bridge + Home Assistant integration** — mirror every device to any MQTT broker with full HA auto-discovery: switches, sliders, sensors (incl. `binary_sensor` opt-in with device classes), sliders routed to HA `cover` for blinds/positions, color caps + brightness sliders combined into a single HA `light` entity with rgb + brightness, and every Trellis scene appears as an HA `button` entity
- **Voice control via Sinric Pro** — Alexa + Google Assistant work out of the box. Map a Trellis device + capability to a Sinric voice device with one dropdown
- **Remote access** — reach your devices from anywhere via Cloudflare Tunnel or Tailscale Funnel; token-aware web UI, reachability probe
- **API tokens + RBAC** — Bearer token auth, admin/viewer roles, rate limiting, token TTL
- **Automation** — scheduled actions (cron), conditional rules with `if device.metric > threshold then ...`, scenes (multi-action, multi-device), webhooks (POST to URL on real events: device.online, device.offline, ota_applied, alert.triggered, sensor.update — fired by the backend, not the browser, so they keep working when the desktop is closed)
- **Alerts + push notifications** — threshold alerts evaluated server-side (debounced), ntfy.sh push, desktop notifications, webhook fan-out — all from one rule
- **Device persistence** — nicknames, tags, known devices survive restarts
- **Search & filter** — find devices by name, IP, platform, chip, tags
- **Web dashboard** — responsive UI at `:9090` with WebSocket push, works on phones; mirrors every desktop feature including the Sketch Generator
- **System tray** — app runs in background, click to restore
- **Dark theme** — clean, modern UI with green accent

### Microcontroller Library
- **ESP32** — all variants (S2, S3, C3, C6)
- **Raspberry Pi Pico W / Pico 2 W** — full support
- **15 lines to integrate** — drop-in library, minimal boilerplate
- **Self-description protocol** — device declares its own capabilities
- **WebSocket** — real-time bidirectional communication
- **Embedded web dashboard** — open `http://<device-ip>/` from any phone or laptop browser. Auto-renders all your switches, sliders, sensors, color picker and text fields with live WebSocket updates. No desktop app required, no install.
- **NVS persistence** — switch and slider values survive reboots on ESP32
- **WiFi provisioning** — `beginAutoConnect()` opens a captive-portal AP on first boot; user picks SSID + password from any phone; credentials stored in NVS for subsequent boots
- **Live broadcasts** — periodic sensor values + system telemetry
- **Device logging** — `logInfo()` / `logWarn()` / `logError()` sent to desktop app
- **OTA ready** — firmware updates from the desktop app (ESP32)
- **System metrics** — RSSI, free heap, uptime reported automatically

## Screenshots

| | |
|---|---|
| ![Dashboard — device cards with live status](screenshots/dashboard.png) | ![Firmware Generator + Quick Flash](screenshots/new-device.png) |
| ![Automation — schedules, rules, webhooks](screenshots/automation.png) | ![OTA Firmware Updates — drag & drop](screenshots/ota.png) |
| ![Settings — config, notifications, diagnostics](screenshots/settings.png) | ![Serial Monitor](screenshots/serial.png) |

## Architecture

```
trellis/
├── app/          # Tauri 2 desktop app (Rust + React)
├── src/          # Arduino library source (ESP32 + Pico W)
├── examples/     # Arduino library examples
└── docs/         # Protocol spec + guides
```

The repo is a monorepo: the root doubles as the Arduino library (so Library Manager can index `src/`, `examples/`, `library.properties` at the repo root) while `app/` holds the desktop app and `docs/` holds protocol/guides.

**Desktop App**: Tauri 2 (Rust backend + React frontend). Local-first, no cloud dependency. SQLite for device history and metrics.

**Library**: Arduino-compatible C++ library. Works on ESP32 and Pico W/Pico 2 W with the same sketch. Handles WiFi, mDNS, HTTP, WebSocket, OTA internally.

**Protocol**: Devices serve a JSON capability declaration at `/api/info` and communicate in real-time over WebSocket. The app renders controls based on what the device reports.

## Supported Boards

| Board | Status |
|-------|--------|
| ESP32 (all variants) | Supported |
| Raspberry Pi Pico W | Supported |
| Raspberry Pi Pico 2 W | Supported |
| ESP8266 | Not yet supported |

## Tech Stack

- **App backend**: Rust (Tauri 2)
- **App frontend**: React, TypeScript, Tailwind CSS
- **App database**: SQLite
- **Library**: C++ (Arduino framework)
- **Discovery**: mDNS / DNS-SD
- **Communication**: HTTP + WebSocket
- **Build**: Vite, Cargo

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions.

## Support

If you find Trellis useful, consider supporting development:

[![PayPal](https://img.shields.io/badge/PayPal-Donate-blue?logo=paypal)](https://www.paypal.com/paypalme/ovexro)

## License

[MIT](LICENSE) — Joshua-Ovidiu Drobota
