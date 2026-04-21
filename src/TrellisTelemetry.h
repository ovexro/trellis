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
  // Reason the chip last booted. On ESP32, populated once at startup via
  // esp_reset_reason() and held constant for the life of the session —
  // the desktop's power-supply-stability rule samples it per connect and
  // treats repeated brownouts as a failing PSU. Non-ESP32 platforms
  // report "unknown" since there's no equivalent API yet.
  const char* resetReason;
};

class TrellisTelemetry {
public:
  TrellisTelemetry();
  void update();
  TelemetryData getData() const;

private:
  unsigned long _startMillis;
  const char* _resetReason;
};

#endif
