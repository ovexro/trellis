/*
 * Trellis — Auto-Connect Example
 *
 * No hardcoded WiFi credentials needed!
 *
 * First boot: device creates a WiFi hotspot "Trellis-My-Device".
 * Connect to it with your phone/laptop and enter your WiFi credentials.
 * Credentials are saved — next boot connects automatically.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

Trellis trellis("My Device");

void setup() {
  trellis.setFirmwareVersion("1.0.0");

  trellis.addSwitch("led", "Built-in LED", 2);
  trellis.addSensor("temp", "Temperature", "C");

  // No WiFi credentials needed — uses stored creds or starts provisioning AP
  if (!trellis.beginAutoConnect()) {
    Serial.println("Failed to connect. Restarting...");
    delay(3000);
    ESP.restart();
  }
}

void loop() {
  trellis.setSensor("temp", 20.0 + random(0, 100) / 10.0);
  trellis.loop();
  delay(2000);
}
