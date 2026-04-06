# Trellis Protocol Specification

Version: 0.1.0

## Overview

Trellis uses a simple HTTP + WebSocket protocol for device discovery and control. Devices serve a JSON capability declaration over HTTP, and communicate in real-time over WebSocket.

## Discovery

Devices advertise themselves via mDNS:

- Service type: `_trellis._tcp`
- Default port: `8080`
- TXT record: `name=<device name>`

## HTTP API

### GET /api/info

Returns the device's capability declaration.

```json
{
  "name": "Greenhouse Controller",
  "id": "trellis-a1b2c3d4",
  "firmware": "1.0.0",
  "platform": "esp32",
  "capabilities": [
    {
      "id": "pump",
      "type": "switch",
      "label": "Water Pump",
      "value": false
    },
    {
      "id": "temp",
      "type": "sensor",
      "label": "Temperature",
      "unit": "C",
      "value": 23.5
    },
    {
      "id": "fan",
      "type": "slider",
      "label": "Fan Speed",
      "min": 0,
      "max": 100,
      "value": 50
    },
    {
      "id": "led",
      "type": "color",
      "label": "Status LED",
      "value": "#00ff00"
    },
    {
      "id": "status",
      "type": "text",
      "label": "Status Message",
      "value": "Running"
    }
  ],
  "system": {
    "rssi": -45,
    "heap_free": 180000,
    "uptime_s": 3600,
    "chip": "esp32s3"
  }
}
```

### Capability Types

| Type | Fields | Description |
|------|--------|-------------|
| `switch` | `value: boolean` | Digital on/off |
| `sensor` | `value: number`, `unit?: string` | Read-only measurement |
| `slider` | `value: number`, `min: number`, `max: number` | Range control |
| `color` | `value: string` (#RRGGBB) | RGB color |
| `text` | `value: string` | Text display/input |

### Device ID

Generated from MAC address: `trellis-XXYYZZ` (last 4 bytes of MAC). Stable across reboots.

## WebSocket

Connect to: `ws://<device-ip>:<port+1>/`

WebSocket port is HTTP port + 1 (default: 8081).

### App → Device (Commands)

#### Set a capability value

```json
{
  "command": "set",
  "id": "pump",
  "value": true
}
```

#### Trigger OTA update (ESP32 only)

```json
{
  "command": "ota",
  "url": "http://192.168.1.100:9000/firmware.bin"
}
```

### Device → App (Events)

#### Capability value update

```json
{
  "event": "update",
  "id": "temp",
  "value": 24.1
}
```

#### OTA progress

```json
{
  "event": "ota_progress",
  "percent": 45
}
```

#### Heartbeat

```json
{
  "event": "heartbeat"
}
```

## Platform Support

| Feature | ESP32 | Pico W / Pico 2 W |
|---------|-------|-------------------|
| mDNS | Yes | Yes |
| HTTP API | Yes | Yes |
| WebSocket | Yes | Yes |
| OTA | Yes | Not yet |
| Telemetry | Full | Partial (no chip temp) |
