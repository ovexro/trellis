/*
 * Trellis — Temperature Sensor Example
 *
 * Reads an analog temperature sensor and reports it to the Trellis app.
 * Replace readTemperature() with your actual sensor reading logic.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

const char* WIFI_SSID = "YourWiFi";
const char* WIFI_PASS = "YourPassword";

const int SENSOR_PIN = 34;  // Analog input

Trellis trellis("Temp Sensor");

float readTemperature() {
  int raw = analogRead(SENSOR_PIN);
  // Simple conversion — replace with your sensor's formula
  float voltage = raw * (3.3 / 4095.0);
  float tempC = (voltage - 0.5) * 100.0;
  return tempC;
}

void setup() {
  trellis.setFirmwareVersion("1.0.0");
  trellis.addSensor("temp", "Temperature", "C");
  trellis.addSensor("raw", "Raw ADC", "");
  trellis.begin(WIFI_SSID, WIFI_PASS);
}

void loop() {
  trellis.setSensor("temp", readTemperature());
  trellis.setSensor("raw", analogRead(SENSOR_PIN));
  trellis.loop();
  delay(1000);  // Read every second
}
