# What is Trellis?

**Trellis solves a specific problem nobody else solves cleanly.**

You have an ESP32 or Pico W. You flashed some code. It works on the bench. Now what?

- How do you control it from your PC without writing a custom dashboard?
- How do you monitor it without setting up Grafana?
- How do you update the firmware without unplugging it?
- How do you manage 5 of them without a spreadsheet?

That's Trellis.

## What it does in one sentence

**You plug in a device, it appears in the app, and you get controls, charts, logs, OTA updates, and automation — with zero configuration.**

## Who it's for

Hobbyists, makers, and engineers with 1–50 ESP32/Pico W devices who want something between "raw serial terminal" and "full Home Assistant installation."

- The person running a greenhouse with 3 sensors and a pump
- The maker with an LED strip, a relay, and a temperature logger
- The engineer prototyping IoT devices who needs monitoring during development
- The hobbyist who wants a clean dashboard without writing one

## Why it matters

| Pain | How Trellis fixes it |
|------|---------------------|
| "How do I get this on WiFi?" | Captive portal — no credentials in source code |
| "I need a dashboard" | Auto-generated from device capabilities — zero config |
| "Which device is offline?" | Continuous auto-discovery, live online/offline status |
| "What's the temperature been doing?" | Time-series charts with SQLite history |
| "I need to update firmware" | OTA from the app — pick file, click upload |
| "If temp is high, turn on the fan" | Conditional rules — if/then automation |
| "Turn everything off at night" | Scenes — one button, multiple devices |
| "Notify me if something breaks" | Alert rules + desktop notifications + webhooks |
| "I have to write code for every device" | Firmware generator — pick capabilities, copy sketch |
| "I manage 10 boards and it's chaos" | Nicknames, tags, search, device persistence |

## What makes it different

### No cloud

No account. No subscription. No vendor. Runs on your PC, talks to your devices on your LAN. Your data never leaves your network.

### No config

Devices describe themselves. The app renders controls automatically. No YAML, no JSON dashboards, no template files.

### No complexity

It's a desktop app you install with one command. Not a Docker container, not a VM, not a server. Just an app.

### Both sides of the stack

It's not just a dashboard or just a firmware library — it's both, designed to work together. The Arduino library is 15 lines to integrate. The desktop app discovers it automatically.

## How it compares

| | Trellis | ESPHome | Home Assistant | Arduino IoT Cloud | Blynk |
|---|---------|---------|----------------|-------------------|-------|
| Cloud required | No | No | No | Yes | Yes |
| Account required | No | No | No | Yes | Yes |
| Subscription | Free | Free | Free | Paid tiers | Paid tiers |
| Install complexity | One command | YAML + HA | Docker/VM | Cloud setup | Cloud setup |
| Auto-discovery | Yes (mDNS) | Via HA | Via integrations | Manual | Manual |
| Self-describing devices | Yes | No | No | No | No |
| Firmware library included | Yes | Generated | No | Yes | Yes |
| Desktop native | Yes (Tauri) | No | Web only | Web only | Web/mobile |
| WiFi provisioning | Built-in | Via HA | Via integrations | Via app | Via app |
| OTA updates | Built-in | Via HA | No | Yes | Yes |
| Serial monitor | Built-in | No | No | No | No |
| Firmware generator | Built-in | YAML-based | No | No | No |
| Automation rules | Built-in | Via HA | Yes (powerful) | Limited | Limited |
| Open source | MIT | Apache 2.0 | Apache 2.0 | No | No |

## The core idea

```cpp
#include <Trellis.h>

Trellis trellis("Greenhouse");

void setup() {
  trellis.addSwitch("pump", "Water Pump", 13);
  trellis.addSensor("temp", "Temperature", "C");
  trellis.addSlider("fan", "Fan Speed", 0, 100, 25);
  trellis.beginAutoConnect();  // WiFi provisioning built-in
}

void loop() {
  trellis.setSensor("temp", readDHT());
  trellis.loop();
}
```

Flash this. Open the Trellis desktop app. Your device appears automatically with a toggle for the pump, a gauge for temperature, and a slider for fan speed. No dashboard code. No server. No config file.

That's it. That's the product.

---

**Trellis makes ESP32 projects feel like products.**
