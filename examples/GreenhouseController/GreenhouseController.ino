/*
 * Trellis — Greenhouse Controller Example
 *
 * A more complete example showing multiple capability types:
 * - Switch: water pump relay
 * - Sensor: temperature reading
 * - Sensor: humidity reading
 * - Slider: fan speed (PWM)
 *
 * After flashing, open http://<device-ip>/ from any device on the same
 * network — the embedded dashboard renders all four controls in a
 * mobile-friendly layout. Useful for headless monitoring without the
 * desktop app running.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

const char* WIFI_SSID = "YourWiFi";
const char* WIFI_PASS = "YourPassword";

const int PUMP_PIN = 13;
const int FAN_PIN = 25;
const int TEMP_PIN = 34;

Trellis trellis("Greenhouse");

// Custom command handler for advanced logic
void onCommand(const char* id, JsonVariant value) {
  Serial.printf("Command received: %s\n", id);

  // You can add custom logic here, e.g.:
  // if pump turns on, start a 30-minute timer
  // if fan speed > 80%, also turn on the pump
}

float readTemperature() {
  int raw = analogRead(TEMP_PIN);
  float voltage = raw * (3.3 / 4095.0);
  return (voltage - 0.5) * 100.0;
}

float readHumidity() {
  // Replace with your humidity sensor logic
  return 65.0 + random(-5, 5);
}

void setup() {
  trellis.setFirmwareVersion("1.0.0");

  trellis.addSwitch("pump", "Water Pump", PUMP_PIN);
  trellis.addSensor("temp", "Temperature", "C");
  trellis.addSensor("humidity", "Humidity", "%");
  trellis.addSlider("fan", "Fan Speed", 0, 100, FAN_PIN);

  trellis.onCommand(onCommand);
  trellis.begin(WIFI_SSID, WIFI_PASS);
}

void loop() {
  trellis.setSensor("temp", readTemperature());
  trellis.setSensor("humidity", readHumidity());
  trellis.loop();
  delay(2000);
}
