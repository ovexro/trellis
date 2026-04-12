/*
 * Trellis — Auto-Connect Example
 *
 * No hardcoded WiFi credentials needed!
 *
 * First boot: device creates a WiFi hotspot "Trellis-My-Device".
 * Connect to it with your phone/laptop and enter your WiFi credentials.
 * Credentials are saved — next boot connects automatically.
 *
 * After connecting, the device prints its IP to Serial. Open that IP in
 * your browser to control the device from any phone or laptop on the
 * network — no desktop app needed.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

Trellis trellis("My Device");

void setup() {
  trellis.setFirmwareVersion("1.0.0");

  trellis.addSwitch("led", "Built-in LED", 2);
  trellis.addSlider("brightness", "LED Brightness", 0, 100, 4);
  trellis.addSensor("temp", "Temperature", "C");

  // No WiFi credentials needed — uses stored creds or starts provisioning AP
  if (!trellis.beginAutoConnect()) {
    Serial.println("Failed to connect. Restarting...");
    delay(3000);
#if defined(ESP32)
    ESP.restart();
#elif defined(ARDUINO_ARCH_RP2040)
    rp2040.reboot();
#endif
  }
}

void loop() {
  trellis.setSensor("temp", 20.0 + random(0, 100) / 10.0);
  trellis.loop();
  delay(2000);
}
