# Trellis User Guide

A complete walkthrough of every feature. From installing the app to automating your devices.

---

## Table of Contents

1. [Install the Desktop App](#1-install-the-desktop-app)
2. [Set Up Your First Device](#2-set-up-your-first-device)
3. [WiFi Provisioning (No Hardcoded Passwords)](#3-wifi-provisioning)
4. [The Dashboard](#4-the-dashboard)
5. [Device Detail Page](#5-device-detail-page)
6. [Controls](#6-controls)
7. [Charts & Metrics](#7-charts--metrics)
8. [Alerts](#8-alerts)
9. [Device Groups & Rooms](#9-device-groups--rooms)
10. [Scenes](#10-scenes)
11. [Automation: Schedules, Rules, Webhooks](#11-automation)
12. [Serial Monitor](#12-serial-monitor)
13. [OTA Firmware Updates](#13-ota-firmware-updates)
14. [Quick Flash (Compile & Upload)](#14-quick-flash)
15. [REST API](#15-rest-api)
16. [Remote Access](#16-remote-access)
17. [Web Dashboard](#17-web-dashboard)
18. [Push Notifications (ntfy.sh)](#18-push-notifications)
19. [Settings & Config Backup](#19-settings--config-backup)
20. [System Tray](#20-system-tray)
21. [Arduino Library Reference](#21-arduino-library-reference)
22. [Sinric Pro (Alexa / Google Home)](#22-sinric-pro-alexa--google-home)
23. [Troubleshooting](#23-troubleshooting)

---

## 1. Install the Desktop App

### One-command install (Linux)

```bash
curl -fsSL https://raw.githubusercontent.com/ovexro/trellis/main/install.sh | bash
```

This installs the `.deb` or `.rpm` depending on your distro, adds it to your app menu, and optionally sets up Arduino CLI.

### Manual install

Download from [GitHub Releases](https://github.com/ovexro/trellis/releases):
- **Ubuntu / Linux Mint / Debian** → `.deb` file, install with `sudo dpkg -i Trellis_*.deb`
- **Fedora / RHEL** → `.rpm` file, install with `sudo rpm -i Trellis-*.rpm`
- **Any Linux** → `.AppImage` file, make executable and run

After installing, open "Trellis" from your app menu. The app starts scanning for devices immediately.

---

## 2. Set Up Your First Device

### What you need

- An **ESP32** or **Raspberry Pi Pico W** board
- **Arduino IDE** or **arduino-cli** installed
- The **Trellis** and **ArduinoJson** and **WebSockets** libraries installed

### Install the libraries

In Arduino IDE: *Sketch > Include Library > Manage Libraries*, search for:
- `Trellis`
- `ArduinoJson`
- `WebSockets`

Or with arduino-cli:
```bash
arduino-cli lib install Trellis ArduinoJson WebSockets
```

### Write a minimal sketch

```cpp
#include <Trellis.h>

Trellis trellis("My First Device");

void setup() {
  trellis.addSwitch("led", "Built-in LED", 2);
  trellis.addSensor("temp", "Temperature", "C");
  trellis.begin("YourWiFiName", "YourWiFiPassword");
}

void loop() {
  trellis.setSensor("temp", analogRead(34) * 0.1);
  trellis.loop();
  delay(2000);
}
```

### Flash it

Upload to your board. Open the Serial Monitor at 115200 baud — you'll see:

```
[Trellis] Connecting to YourWiFiName...
[Trellis] Connected! IP: 192.168.1.42
[Trellis] My First Device ready at http://192.168.1.42:8080
```

### Open the Trellis app

Your device appears automatically on the dashboard within seconds. Click it to see the controls.

---

## 3. WiFi Provisioning

Hardcoding WiFi credentials is fine for testing, but for a "real" device you want the user to enter their own WiFi details. Trellis has a built-in captive portal for this.

Replace `trellis.begin("ssid", "password")` with:

```cpp
void setup() {
  // ... add capabilities ...

  if (!trellis.beginAutoConnect()) {
    Serial.println("WiFi failed! Restarting...");
    delay(3000);
    ESP.restart();
  }
}
```

**First boot**: The device creates a WiFi hotspot named `Trellis-My-First-Device`. Connect to it with your phone or laptop. A captive portal page opens where you enter your WiFi name and password. The credentials are saved — on next boot, it connects automatically.

**To reset WiFi**: Flash the sketch again, or erase the flash memory.

---

## 4. The Dashboard

The main page shows all your devices as cards.

**Each card shows:**
- Device name (or nickname if you set one)
- Online/Offline status with a colored badge
- IP address and port
- Chip type (ESP32, RP2040, etc.)
- Firmware version
- WiFi signal strength (RSSI)
- Uptime
- Tags (if assigned)
- Number of controls
- When offline: "Last seen: 2h ago" instead of stale metrics

**Features:**
- **Search bar** — filter by name, nickname, IP, platform, chip, or tags
- **Add by IP** — if mDNS doesn't work (e.g., cross-subnet), manually enter an IP address
- **Groups** — organize devices into named groups with colors (see [Groups](#9-device-groups--rooms))

Click any device card to open the [Device Detail](#5-device-detail-page) page.

---

## 5. Device Detail Page

Shows everything about one device:

- **Header**: name (editable nickname), IP, chip, firmware version, online status
- **Controls**: auto-generated from the device's capability declarations
- **System stats**: live RSSI, free heap, uptime (when online)
- **Sensor charts**: time-series graphs for each sensor
- **System charts**: historical WiFi signal and heap usage
- **Alerts**: threshold rules that trigger notifications
- **Logs**: messages sent from the device via `trellis.logInfo()` etc.
- **Tags**: custom labels (e.g., "kitchen", "outdoor", "test")
- **Remove device**: deletes all stored data for this device

---

## 6. Controls

Controls are auto-generated based on what the device declares. You don't design them — the device says "I have a switch called Pump" and the app renders a toggle.

| Type | What It Looks Like | Arduino Code |
|------|-------------------|--------------|
| **Switch** | Toggle on/off | `addSwitch("pump", "Water Pump", 13)` |
| **Sensor** | Read-only number with unit | `addSensor("temp", "Temperature", "C")` |
| **Slider** | Range control | `addSlider("fan", "Fan Speed", 0, 100, 25)` |
| **Color** | Color picker with hex value | `addColor("led", "LED Color")` |
| **Text** | Text display | `addText("status", "Status Message")` |

**Switches** toggle GPIO pins directly. **Sliders** apply PWM via `analogWrite()`. **Sensors** are read-only — update them from your firmware with `trellis.setSensor("temp", value)`.

To handle commands with custom logic (not just GPIO), register a callback:

```cpp
void onCommand(const char* id, JsonVariant value) {
  if (strcmp(id, "pump") == 0) {
    bool on = value.as<bool>();
    // your custom logic here
  }
}

void setup() {
  trellis.onCommand(onCommand);
  // ...
}
```

---

## 7. Charts & Metrics

Every sensor value is stored in SQLite. The Device Detail page shows time-series charts for each sensor.

- **Time ranges**: 1 hour, 6 hours, 24 hours, 7 days
- **Auto-refresh**: charts update every 10 seconds
- **CSV export**: click the download icon to export data as CSV
- **System metrics**: WiFi signal (RSSI) and free heap are charted automatically for every device

Data older than 30 days is automatically cleaned up.

---

## 8. Alerts

Create threshold-based alerts on the Device Detail page, under the "Alerts" section.

**Example**: "If temperature goes above 35, notify me."

1. Click **Add rule**
2. Name it: "High temp warning"
3. Select the sensor: Temperature
4. Set condition: above 35
5. Click **Create alert**

When triggered:
- A **desktop notification** appears
- If configured, a **push notification** is sent to your phone via ntfy.sh (see [Push Notifications](#17-push-notifications))

Alerts have a 60-second debounce — they won't fire repeatedly for the same condition.

---

## 9. Device Groups & Rooms

Organize devices into named groups — like rooms in a house.

1. On the dashboard, click **Manage Groups**
2. Create a group: name it "Living Room", pick a color
3. On each device card, click the small colored dot (bottom-right) to assign it to a group

The dashboard switches to a grouped view — devices sorted under their group headers, collapsible, with the group's color dot. Ungrouped devices appear at the bottom.

---

## 10. Scenes

Scenes let you control multiple devices with one button. Like "Good Night" = turn off all lights + set thermostat to 18.

1. Go to **Scenes** in the sidebar
2. Click **New Scene**
3. Name it: "Good Night"
4. Click **+ Add action** for each device you want to control
5. Pick the device, capability, and target value
6. Click **Create**

Hit **Run** to execute all actions in sequence. Scenes are stored locally.

---

## 11. Automation

Go to **Automation** in the sidebar. Three tabs:

### Schedules (cron-based)

Run actions at specific times. Examples:
- "Turn on pump at 6:00 AM every day" → cron: `0 6 * * *`
- "Turn off lights at midnight" → cron: `0 0 * * *`
- "Check sensor every 5 minutes" → cron: `*/5 * * * *`

Create a schedule: pick a device, capability, value, and cron expression.

### Rules (if/then)

Trigger actions based on sensor readings. Example:
- "If greenhouse temperature above 30, turn on fan"

Create a rule: pick a source sensor, condition (above/below), threshold, then a target device and what to set it to. Rules are checked on every sensor update with a 30-second debounce.

### Webhooks

Send HTTP POST requests to external services when events happen.

Supported events:
- `device_offline` — a device went unreachable
- `device_online` — a device came back
- `alert_triggered` — a threshold alert fired
- `sensor_update` — a sensor value changed

Use this to integrate with Slack, Discord, Telegram bots, or any HTTP endpoint.

---

## 12. Serial Monitor

A built-in USB serial terminal. Go to **Serial** in the sidebar.

1. Select a port (e.g., `/dev/ttyUSB0`)
2. Pick a baud rate (default: 115200)
3. Click **Connect**

Features:
- Send text commands (type and press Enter)
- Color-coded output: your commands in green, device output in white, system messages in gray
- Auto-scroll toggle
- Copy all output to clipboard
- Clear buffer
- 5000-line history

---

## 13. OTA Firmware Updates

Update your ESP32's firmware over WiFi — no USB cable needed.

1. Go to **OTA** in the sidebar
2. Select the target device (must be online)
3. Either **drag & drop** a `.bin` file onto the drop zone, or click **Browse**
4. Click **Upload Firmware**

The app starts a temporary HTTP server on your PC, tells the device to download the firmware from it, and shows a progress bar. The device reboots automatically when done.

**Firmware history**: every upload is saved. You can **rollback** to a previous firmware version with one click. The currently running firmware is marked with a "current" badge.

> OTA is currently ESP32-only. Pico W OTA support is planned.

---

## 14. Quick Flash

Compile and flash directly from the Trellis app — no need to open Arduino IDE.

1. Go to **New Device** in the sidebar
2. Pick capabilities (switch, sensor, slider, etc.)
3. The generated sketch appears on the right
4. Expand the **Quick Flash** panel at the bottom
5. Select your serial port
6. Click **Compile & Flash**

Requirements:
- `arduino-cli` must be installed
- Board cores and libraries are auto-detected; click **Install** if anything is missing

You can also **Save .ino** to disk or **Copy to clipboard** to paste into Arduino IDE.

---

## 15. REST API

Trellis runs an HTTP API on port **9090** alongside the desktop app. Use it for scripting, integration, or building your own tools.

**Base URL**: `http://localhost:9090/api`

### Authentication (v0.3.4+)

The REST API binds to `0.0.0.0:9090` so other machines on your LAN can reach it, but every non-loopback request requires a Bearer token. Loopback (`127.0.0.1`, `::1`) is allowed without a token by default so the desktop app and any local scripts keep working with no setup.

**Mint a token:**

1. Open the Trellis desktop app
2. Go to **Settings → API Tokens**
3. Type a memorable name (e.g. `homeassistant`, `phone`, `ci`) and click **Create token**
4. Copy the token from the dialog — it's shown **exactly once** and cannot be recovered afterwards (only the SHA-256 digest is stored)

**Use it in your requests:**

```bash
TOKEN="trls_..."   # paste the token you just copied

# From the same machine (loopback) — token is optional
curl http://localhost:9090/api/devices

# From another machine on your LAN — token is required
curl -H "Authorization: Bearer $TOKEN" http://desktop-pc:9090/api/devices
```

**Revoke a token** at any time from **Settings → API Tokens** — click the trash icon next to the token name. Revocation is immediate; the next request bearing that token gets a 401.

**Strict-loopback mode** (defense in depth): if you're on a multi-user machine and want to require a token even for `127.0.0.1` requests, tick **"Require token even for localhost requests"** in the API Tokens section. The desktop app's embedded WebView authenticates over Tauri IPC, not HTTP, so it's unaffected — but local CLI tools and the embedded web dashboard at `localhost:9090/` will then also need a token.

> **Upgrading from v0.3.3 or earlier:** the old behavior was "wide open over the LAN" — anything on your WiFi could curl `/api/devices/foo/command` and flip switches. v0.3.4 closes that. After upgrading, mint a token before any non-loopback automation breaks. The desktop app and the localhost web dashboard continue to work with zero changes.

### Key endpoints

```bash
# List all devices
curl http://localhost:9090/api/devices

# Get one device
curl http://localhost:9090/api/devices/trellis-aabbccdd

# Send a command to a device
curl -X POST http://localhost:9090/api/devices/trellis-aabbccdd/command \
  -H "Content-Type: application/json" \
  -d '{"command":"set","id":"pump","value":true}'

# Get sensor metrics (last 24 hours)
curl "http://localhost:9090/api/devices/trellis-aabbccdd/metrics/temp?hours=24"

# Export metrics as CSV
curl "http://localhost:9090/api/devices/trellis-aabbccdd/metrics/temp/csv?hours=24"

# List schedules
curl http://localhost:9090/api/schedules

# List groups
curl http://localhost:9090/api/groups

# Token management (for scripting — same operations as the Settings UI)
curl -H "Authorization: Bearer $TOKEN" http://localhost:9090/api/tokens
curl -X POST -H "Authorization: Bearer $TOKEN" \
  -H "Content-Type: application/json" \
  -d '{"name":"my-script"}' \
  http://localhost:9090/api/tokens
curl -X DELETE -H "Authorization: Bearer $TOKEN" \
  http://localhost:9090/api/tokens/3
```

Full CRUD is available for devices, groups, schedules, rules, webhooks, alerts, templates, firmware history, and settings. CORS headers are included for browser access.

---

## 16. Remote Access

Trellis listens on your LAN by default. To reach it from outside your home — flipping a switch from the bus, checking sensors on holiday — you run a tunnel that exposes port `9090` to the internet, then authenticate with the same Bearer tokens you mint for the REST API.

The desktop app's **Settings → Remote Access** panel walks you through both supported transports and includes a **Test reachability** button that does a single round-trip from your machine to the tunnel and back. Use it to verify the setup before pulling out your phone.

> **Pre-requisite — mint a token first.** Remote access is built on the v0.3.4 token gate. If you haven't created any API tokens, every request through the tunnel will hit a 401 and the dashboard will be reachable but unusable. Open **Settings → API Tokens** and create one named e.g. `phone` before setting up the tunnel. The Settings panel will warn you loudly if you skip this step.

### Cloudflare Tunnel (recommended)

Free, no inbound port, branded URL on your own domain. Composes with Cloudflare Access for free SSO if you want defense in depth on top of the token gate.

1. Add a domain to your Cloudflare account (free plan is fine)
2. Install [`cloudflared`](https://developers.cloudflare.com/cloudflare-one/connections/connect-networks/downloads/)
3. `cloudflared tunnel login`
4. `cloudflared tunnel create trellis`
5. `cloudflared tunnel route dns trellis trellis.<your-domain>`
6. `cloudflared tunnel run --url http://localhost:9090 trellis`

Then open `https://trellis.<your-domain>` on your phone. The dashboard's first `/api/*` call returns 401, an inline auth modal pops up asking for a token, you paste it once, and the browser remembers it in `localStorage`. Subsequent loads are seamless.

To run `cloudflared` permanently, install it as a systemd service: `sudo cloudflared service install`.

### Tailscale Funnel (no domain needed)

Three commands. URL is `*.ts.net`. Personal use is free up to 100 devices.

1. [Install Tailscale](https://tailscale.com/kb/1031/install-linux)
2. `sudo tailscale up`
3. `sudo tailscale funnel 9090 on`

Tailscale prints your funnel URL on stdout. Open it on your phone and paste your token at the auth prompt.

### Test reachability before relying on it

The **Test reachability** widget in Settings → Remote Access asks for the public URL and one of your tokens, then runs a single `GET /api/devices` from this machine through the tunnel and back. It distinguishes between:

- **Success**: tunnel + token both work end-to-end
- **HTTP 401**: tunnel reached, token rejected — mint a fresh one
- **HTTP 404**: reached an HTTP server but it's not Trellis (wrong host or port)
- **HTTP 502/503/504**: tunnel up, but the desktop app isn't running on `:9090`
- **DNS / connection / TLS errors**: the tunnel itself isn't reachable

The token you paste into the test widget is held in component memory only — it is never persisted. Only the URL is remembered between probes (as a convenience so you don't retype the hostname).

### Why not ngrok?

The free tier rotates your subdomain on every restart, which doesn't work for "set it once, my phone uses this URL all year". Stable URLs cost $8+/mo per user. Both options above are strictly better and free.

### What's NOT exposed by the tunnel

The tunnel forwards `:9090` only. The per-device `:8080` dashboards (hosted on each ESP32) stay LAN-only — for remote use, the central dashboard at `:9090` already aggregates every device, so you don't need to reach individual devices. If you want per-device remote access, file an issue: it would require WebSocket-aware reverse proxying through `:9090`, which is on the v0.5.0 roadmap if there's demand.

---

## 17. Web Dashboard

A responsive web UI is served at `http://localhost:9090` — open it on your phone, tablet, or any browser on your network. Through a remote-access tunnel (see §16) it's also reachable from outside your LAN, with an inline auth modal that prompts for your token on the first 401 and persists it in `localStorage`.

Features:
- Device cards with live status
- Grouped view (if you have groups)
- Interactive controls (toggle switches, move sliders, pick colors)
- Automation overview (schedules, rules, webhooks)
- Settings (ntfy topic, groups)
- Auto-refresh every 5 seconds

The web dashboard is a single HTML file with zero external dependencies — no React, no npm, no CDN. It works offline once loaded.

---

## 18. Push Notifications

Get alerts on your phone when a sensor threshold is crossed or a device goes offline.

Uses [ntfy.sh](https://ntfy.sh) — a free, open-source notification service. No account needed.

### Setup

1. Install the **ntfy** app on your phone ([Android](https://play.google.com/store/apps/details?id=io.heckel.ntfy) / [iOS](https://apps.apple.com/app/ntfy/id1625396347))
2. Subscribe to a topic name (e.g., `my-trellis-alerts`)
3. In Trellis, go to **Settings**
4. Under "Push Notifications", enter the same topic name
5. Click **Save**, then **Test** to verify

Now you'll get push notifications for:
- Alert rules firing (sensor thresholds)
- Devices going offline

---

## 19. Settings & Config Backup

### Scan Interval

Under **Settings > Discovery**, choose how often Trellis checks if devices are still online: 10s, 30s (default), 60s, or 120s. Lower values detect changes faster but use more network traffic.

### Config Export / Import

**Export** saves everything to a JSON file:
- Device nicknames and tags
- Groups
- Scenes
- Schedules, rules, webhooks
- Alert rules
- Device templates

**Import** restores from a backup file — useful when moving to a new PC.

### Diagnostics

The Settings page shows warnings for any device with:
- Weak WiFi signal (RSSI below -80 dBm)
- Low free heap (below 20 KB — possible memory leak)

---

## 20. System Tray

Trellis runs in the system tray. When you close the window, the app keeps running in the background — devices stay connected, schedules keep firing, webhooks keep working.

- **Click the tray icon** to show the window
- **Right-click** for a menu: Show Trellis / Quit

---

## 21. Arduino Library Reference

### Initialization

```cpp
Trellis trellis("Device Name");
// or with custom port:
Trellis trellis("Device Name", 8080);
```

### WiFi Connection

```cpp
// Option A: hardcoded credentials
trellis.begin("SSID", "password");

// Option B: captive portal provisioning (recommended)
trellis.beginAutoConnect();
```

### Capabilities

```cpp
trellis.addSwitch("id", "Label", gpio_pin);
trellis.addSensor("id", "Label", "unit");
trellis.addSlider("id", "Label", min, max, gpio_pin);
trellis.addColor("id", "Label");
trellis.addText("id", "Label");
```

### Updating Values

```cpp
trellis.setSensor("id", float_value);
trellis.setSwitch("id", true_or_false);
trellis.setText("id", "new text");
trellis.setColor("id", "#ff6600");
```

### Reading Values

```cpp
float temp = trellis.getSensor("id");
bool state = trellis.getSwitch("id");
```

### Custom Command Handler

```cpp
void onCommand(const char* id, JsonVariant value) {
  if (strcmp(id, "pump") == 0) {
    bool on = value.as<bool>();
    // do something custom
  }
}

void setup() {
  trellis.onCommand(onCommand);
}
```

### Logging

```cpp
trellis.logInfo("Pump started");
trellis.logWarn("Water level low");
trellis.logError("Sensor disconnected");
```

Logs are sent via WebSocket to the desktop app and appear in the Device Detail > Logs section.

### Firmware Version

```cpp
trellis.setFirmwareVersion("1.2.0");
```

The desktop app displays this on device cards and in the OTA history.

### Main Loop

```cpp
void loop() {
  // update your sensors
  trellis.setSensor("temp", readTemp());

  // MUST call this — handles WebSocket, heartbeat, broadcasts
  trellis.loop();

  delay(2000); // sensor read interval
}
```

### Limits

- Maximum 16 capabilities per device
- String values (text, color) are limited to 31 characters
- Heartbeat is sent every 10 seconds
- Sensor values are broadcast every 5 seconds

---

## 22. Sinric Pro (Alexa / Google Home)

Control your Trellis devices with voice commands through Amazon Alexa and Google Home via [Sinric Pro](https://sinric.pro). Trellis connects to the Sinric Pro cloud over a persistent WebSocket — no port forwarding, no extra hardware.

### How it works

The Sinric Pro bridge runs inside the Trellis desktop app. When you say *"Alexa, turn on the light"*, the request flows:

```
Voice assistant → Sinric Pro cloud → Trellis desktop app → your device
```

State changes flow the other way too: if you toggle a switch from the Trellis dashboard, the Sinric cloud shadow updates so Alexa reports the correct state when asked.

### Supported capability types

| Trellis type | Sinric device type | Voice actions |
|---|---|---|
| Switch | Switch / Smart Plug | "Turn on/off the light" |
| Slider | Dimmer / Window Blinds | "Set brightness to 50" |
| Color | RGB Light | "Set the light to blue" |
| Sensor | Temperature Sensor | "What's the temperature?" |
| Text | — | Not supported (no standard Sinric device type) |

### Setup

#### 1. Create a Sinric Pro account

1. Go to [sinric.pro](https://sinric.pro) and sign up (free tier: 3 devices)
2. Note your **APP_KEY** and **APP_SECRET** from the Credentials page — you'll paste these into Trellis

#### 2. Create virtual devices on Sinric Pro

For each Trellis device (or capability) you want to control by voice:

1. Click **Add Device** in the Sinric Pro dashboard
2. Choose a device type that matches your Trellis capability (e.g., "Switch" for a relay, "Temperature Sensor" for a DHT22)
3. Pick a name Alexa/Google will recognise (e.g., "Desk Lamp", "Living Room Temp")
4. Copy the **Device ID** (a UUID) — you'll need it in the next step

#### 3. Configure the bridge in Trellis

1. Open Trellis → **Settings** → scroll to **Sinric Pro (Alexa / Google Home)**
2. Tick **Enable Sinric Pro bridge**
3. Paste your **APP_KEY** and **APP_SECRET**
4. Under **Device mappings**, click **+ Add mapping** for each virtual device:
   - **Sinric Device ID** — paste the UUID from the Sinric Pro dashboard
   - **Trellis Device** — select the physical device from the dropdown
   - **Capability** — pick a specific capability, or leave as **Auto (first match)** to let the bridge auto-discover the first capability of the matching type
5. Click **Save & apply**

The status indicator should turn green and show "Connected to `ws.sinric.pro`".

> **Tip — Test connection first.** Click **Test connection** before saving to verify your APP_KEY is accepted. The test opens a WebSocket, reads one server frame, and disconnects. If it fails, double-check the APP_KEY (it's case-sensitive).

#### 4. Link to Alexa or Google Home

1. In the **Alexa** app (or **Google Home** app), search for the **Sinric Pro** skill/action
2. Enable it and sign in with your Sinric Pro account
3. Alexa will discover the devices you created in step 2
4. Test: *"Alexa, turn on Desk Lamp"*

### Per-capability mapping

By default, the bridge auto-discovers the first capability of each type on a device. This works well for simple devices (one switch, one sensor). For devices with **multiple capabilities of the same type** (e.g., two switches or two sliders), pick the specific capability in the **Capability** dropdown.

The dropdown shows a type badge for each capability:

- **[SW]** — Switch
- **[SL]** — Slider
- **[SN]** — Sensor
- **[CL]** — Color

If you map a specific capability and the Sinric device sends an action for a different type (e.g., the Sinric device supports both power and dimmer, but you mapped a switch), the bridge will use your explicit mapping for the matching type and fall back to auto-discovery for the other types.

### Sensor naming convention

Temperature queries use a name-hint heuristic: the bridge looks for sensor capabilities whose ID contains `temp` (for temperature) or `humid` (for humidity). If your sensor IDs follow this convention, temperature reporting works automatically. If not, use the Capability dropdown to explicitly map the sensor you want to report.

### Monitoring

- **Desktop app**: The status indicator in Settings shows connection state, messages sent/received
- **Web dashboard** (`localhost:9090`): Settings tab shows Sinric connection dot and message counters (read-only)
- **REST API**: `GET /api/sinric/status` returns `{ enabled, connected, last_error, messages_sent, messages_received }`

### Limitations

- **Text capabilities** can't be mapped — Sinric Pro has no standard "text display" device type
- **Trellis must be running** — the bridge lives in the desktop app; if you close it, voice control stops (minimize to tray to keep it alive in the background)
- **One-directional humidity** — humidity is always auto-discovered (the Sinric temperature device reports both temp and humidity together); the explicit capability mapping applies to the primary temperature reading only
- **Sinric Pro free tier** allows 3 devices — paid plans support more

---

## 23. Troubleshooting

### Device doesn't appear in the app

- **Same network?** The device and your PC must be on the same subnet. mDNS doesn't cross subnets (e.g., 192.168.1.x can't see 192.168.2.x).
- **WiFi connected?** Check Serial Monitor at 115200 baud — you should see the IP address.
- **Firewall?** Trellis uses ports 8080 (HTTP), 8081 (WebSocket), and 9090 (REST API). Make sure they're not blocked.
- **Manual fallback**: Click "Add by IP" on the dashboard and enter the device's IP and port.

### Device shows "Offline" but it's running

- The health check runs every 30 seconds (configurable in Settings). Wait a cycle.
- Check if the device's WiFi disconnected (RSSI very low = unstable connection).
- Try power-cycling the device.

### OTA update fails

- OTA only works on ESP32 (Pico W support is planned).
- The device must be able to reach your PC's IP on the port shown in the logs. Check for firewalls.
- Make sure the `.bin` file is a valid compiled firmware for your board.

### Controls don't respond

- Check that the device is online (green badge).
- Open the Device Detail page and check the Logs section for errors.
- Make sure your `trellis.loop()` is being called in your Arduino `loop()`.

### Charts show no data

- Sensor data is only stored when the app is running and the device is connected.
- Check that you're calling `trellis.setSensor()` in your loop.
- Try a shorter time range (1 hour) if you just started.
