/*
 * Trellis — RGB LED Example
 *
 * Controls an RGB LED using the color picker in the Trellis app.
 * Adjust GPIO pins for your wiring.
 *
 * Works on: ESP32, Pico W, Pico 2 W
 */

#include <Trellis.h>

const char* WIFI_SSID = "YourWiFi";
const char* WIFI_PASS = "YourPassword";

const int RED_PIN = 16;
const int GREEN_PIN = 17;
const int BLUE_PIN = 18;

Trellis trellis("RGB Light");

void onCommand(const char* id, JsonVariant value) {
  if (strcmp(id, "color") == 0) {
    const char* hex = value.as<const char*>();
    if (!hex || strlen(hex) < 7) return;

    // Parse #RRGGBB
    long color = strtol(hex + 1, NULL, 16);
    int r = (color >> 16) & 0xFF;
    int g = (color >> 8) & 0xFF;
    int b = color & 0xFF;

    analogWrite(RED_PIN, r);
    analogWrite(GREEN_PIN, g);
    analogWrite(BLUE_PIN, b);

    Serial.printf("Color: R=%d G=%d B=%d\n", r, g, b);
  }
}

void setup() {
  pinMode(RED_PIN, OUTPUT);
  pinMode(GREEN_PIN, OUTPUT);
  pinMode(BLUE_PIN, OUTPUT);

  trellis.setFirmwareVersion("1.0.0");
  trellis.addColor("color", "LED Color");
  trellis.addSwitch("power", "Power", 2);
  trellis.onCommand(onCommand);
  trellis.begin(WIFI_SSID, WIFI_PASS);
}

void loop() {
  trellis.loop();
}
