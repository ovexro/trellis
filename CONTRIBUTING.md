# Contributing to Trellis

## Prerequisites

- **Rust** (latest stable) — [rustup.rs](https://rustup.rs)
- **Node.js** 20+ and npm
- **Tauri 2 system dependencies** (Linux):
  ```bash
  sudo apt install libwebkit2gtk-4.1-dev libappindicator3-dev librsvg2-dev patchelf libssl-dev
  ```
- **Arduino CLI** or **PlatformIO** — for building the library
- **ESP32 or Pico W board** — for testing

## Project Structure

```
trellis/
├── app/                     # Desktop app
│   ├── src-tauri/           #   Rust backend (Tauri 2)
│   │   └── src/             #     discovery, comms, OTA, serial, db
│   ├── src/                 #   React frontend
│   │   ├── pages/           #     Dashboard, DeviceDetail, SerialMonitor, OTA, Settings
│   │   ├── components/      #     DeviceCard, controls/, charts/, layout/
│   │   ├── hooks/           #     useDevices, useWebSocket, useTauri
│   │   ├── stores/          #     Zustand device state
│   │   └── lib/             #     types, protocol definitions
│   └── package.json
├── library/                 # Arduino library
│   ├── src/                 #   C++ source (Trellis.h, Discovery, WebServer, OTA, Telemetry)
│   └── examples/            #   Example sketches
└── docs/                    # Protocol spec, guides
```

## Development — Desktop App

```bash
cd app
npm install
npm run tauri dev      # starts Vite + Tauri in dev mode
```

The app opens with hot-reload for the React frontend. Rust changes trigger a recompile.

```bash
npm run tauri build    # production build
```

## Development — Arduino Library

Install in Arduino IDE or PlatformIO by symlinking:

```bash
# Arduino IDE
ln -s /path/to/trellis/library ~/Arduino/libraries/Trellis

# PlatformIO — add to platformio.ini:
# lib_extra_dirs = /path/to/trellis/library
```

Then open any example sketch and upload to your board.

## Code Style

- **Rust**: `cargo fmt` + `cargo clippy` before committing
- **TypeScript/React**: ESLint + Prettier (config in app/)
- **C++**: Arduino conventions, PascalCase for classes, camelCase for methods

## Commit Messages

Use concise, descriptive commit messages:
- `feat: add mDNS device discovery`
- `fix: handle WebSocket reconnection on device sleep`
- `docs: add protocol specification`

## Testing with Hardware

Always test changes against real hardware before marking a feature complete:
- ESP32 at `/dev/ttyUSB0`
- Pico 2 at `/dev/ttyACM0`
