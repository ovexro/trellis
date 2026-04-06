# Trellis Arduino Library

Drop-in library that makes your ESP32 or Pico W self-describing. The [Trellis desktop app](https://github.com/ovexro/trellis) discovers your device and auto-generates controls.

## Quick Start

```cpp
#include <Trellis.h>

Trellis trellis("My Device");

void setup() {
  trellis.addSwitch("relay", "Main Relay", 13);
  trellis.addSensor("temp", "Temperature", "C");
  trellis.begin("MyWiFi", "password");
}

void loop() {
  trellis.setSensor("temp", readTemp());
  trellis.loop();
}
```

## Supported Boards

- ESP32 (all variants)
- Raspberry Pi Pico W
- Raspberry Pi Pico 2 W

## Capability Types

| Type | Method | Description |
|------|--------|-------------|
| Switch | `addSwitch(id, label, gpio)` | Digital on/off control |
| Sensor | `addSensor(id, label, unit)` | Read-only value |
| Slider | `addSlider(id, label, min, max, gpio)` | Range control with PWM |
| Color | `addColor(id, label)` | RGB color picker |
| Text | `addText(id, label)` | Text display/input |

## Dependencies

- [ArduinoJson](https://github.com/bblanchon/ArduinoJson) ^7.0.0
- [WebSockets](https://github.com/Links2004/arduinoWebSockets) ^2.4.0

## License

MIT
