#include "TrellisTelemetry.h"

TrellisTelemetry::TrellisTelemetry() : _startMillis(millis()) {}

void TrellisTelemetry::update() {
  // Telemetry is read on-demand via getData(), no periodic work needed yet
}

TelemetryData TrellisTelemetry::getData() const {
  TelemetryData data;
  data.rssi = WiFi.RSSI();
  data.uptimeSeconds = (millis() - _startMillis) / 1000;

#if defined(ESP32)
  data.heapFree = esp_get_free_heap_size();
  data.chip = CONFIG_IDF_TARGET;  // "esp32", "esp32s3", etc.
#elif defined(ARDUINO_ARCH_RP2040)
  data.heapFree = rp2040.getFreeHeap();
  #if defined(PICO_RP2350)
    data.chip = "rp2350";
  #else
    data.chip = "rp2040";
  #endif
#else
  data.heapFree = 0;
  data.chip = "unknown";
#endif

  return data;
}
