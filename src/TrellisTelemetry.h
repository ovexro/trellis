#ifndef TRELLIS_TELEMETRY_H
#define TRELLIS_TELEMETRY_H

#include <Arduino.h>

#if defined(ESP32)
  #include <WiFi.h>
  #include <esp_system.h>
#elif defined(ARDUINO_ARCH_RP2040)
  #include <WiFi.h>
#endif

struct TelemetryData {
  int32_t rssi;
  uint32_t heapFree;
  uint32_t uptimeSeconds;
  const char* chip;
};

class TrellisTelemetry {
public:
  TrellisTelemetry();
  void update();
  TelemetryData getData() const;

private:
  unsigned long _startMillis;
};

#endif
