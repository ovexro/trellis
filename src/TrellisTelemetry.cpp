#include "TrellisTelemetry.h"

#if defined(ESP32)
static const char* resetReasonToString(esp_reset_reason_t reason) {
  switch (reason) {
    case ESP_RST_POWERON:    return "poweron";
    case ESP_RST_EXT:        return "external";
    case ESP_RST_SW:         return "software";
    case ESP_RST_PANIC:      return "panic";
    case ESP_RST_INT_WDT:    return "interrupt_watchdog";
    case ESP_RST_TASK_WDT:   return "task_watchdog";
    case ESP_RST_WDT:        return "watchdog";
    case ESP_RST_DEEPSLEEP:  return "deepsleep";
    case ESP_RST_BROWNOUT:   return "brownout";
    case ESP_RST_SDIO:       return "sdio";
    case ESP_RST_UNKNOWN:
    default:                 return "unknown";
  }
}
#endif

TrellisTelemetry::TrellisTelemetry()
  : _startMillis(millis()),
#if defined(ESP32)
    // esp_reset_reason() is stable from boot onward — capture once so
    // the string survives even if a later call gets masked by runtime
    // reset-source clobbering (seen on some IDF versions).
    _resetReason(resetReasonToString(esp_reset_reason()))
#else
    _resetReason("unknown")
#endif
{}

void TrellisTelemetry::update() {
  // Telemetry is read on-demand via getData(), no periodic work needed yet
}

TelemetryData TrellisTelemetry::getData() const {
  TelemetryData data;
  data.rssi = WiFi.RSSI();
  data.uptimeSeconds = (millis() - _startMillis) / 1000;
  data.resetReason = _resetReason;

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
