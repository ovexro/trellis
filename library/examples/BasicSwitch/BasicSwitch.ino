/*
 * Trellis — Basic Switch Example
 *
 * Controls an LED or relay on GPIO 2 (built-in LED on most ESP32 boards).
 * The switch appears automatically in the Trellis desktop app.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

// Change these to match your network
const char* WIFI_SSID = "YourWiFi";
const char* WIFI_PASS = "YourPassword";

Trellis trellis("Basic Switch");

void setup() {
  trellis.setFirmwareVersion("1.0.0");
  trellis.addSwitch("led", "Built-in LED", 2);
  trellis.begin(WIFI_SSID, WIFI_PASS);
}

void loop() {
  trellis.loop();
}
