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
  trellis.begin("MyWiFi", "password");
}

void loop() {
  trellis.setSensor("temp", readDHT());
  trellis.loop();
}
```

The desktop app discovers your device via mDNS, reads its capability declaration, and renders the right controls — toggle for the pump, gauge for temperature, slider for fan speed.

## Dependencies

The Trellis Arduino library requires two dependencies:

| Library | Author | Purpose |
|---------|--------|---------|
| [ArduinoJson](https://github.com/bblanchon/ArduinoJson) | Benoit Blanchon | JSON parsing for the self-description protocol |
| [WebSockets](https://github.com/Links2004/arduinoWebSockets) | Markus Sattler | Real-time communication with the desktop app |

**PlatformIO** installs these automatically.

**Arduino IDE** does not auto-install dependencies — you need to add them yourself:

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

### Desktop App
- **Auto-discovery** — continuous mDNS scanning, devices appear automatically
- **Live updates** — persistent WebSocket connections, real-time sensor data
- **Device cards** — name, status, RSSI, uptime, firmware version, chip
- **Auto-generated controls** — switches, sliders, sensors, color pickers, text
- **Time-series charts** — sensor data over time with SQLite storage
- **Serial monitor** — full USB serial terminal with live streaming
- **OTA updates** — native file picker, local HTTP firmware server
- **Device persistence** — nicknames, tags, known devices survive restarts
- **Search & filter** — find devices by name, IP, platform, chip
- **Device logs** — severity-filtered log viewer with live streaming
- **Alert rules** — configurable thresholds with desktop notifications
- **System tray** — app runs in background, click to restore
- **Dark theme** — clean, modern UI with green accent

### Microcontroller Library
- **ESP32** — all variants (S2, S3, C3, C6)
- **Raspberry Pi Pico W / Pico 2 W** — full support
- **15 lines to integrate** — drop-in library, minimal boilerplate
- **Self-description protocol** — device declares its own capabilities
- **WebSocket** — real-time bidirectional communication
- **Embedded web dashboard** — open `http://<device-ip>/` from any phone or laptop browser. Auto-renders all your switches, sliders, sensors, color picker and text fields with live WebSocket updates. No desktop app required, no install.
- **Live broadcasts** — periodic sensor values + system telemetry
- **Device logging** — logInfo()/logWarn()/logError() sent to desktop app
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
| ESP8266 | Planned |

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
