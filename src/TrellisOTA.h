#ifndef TRELLIS_OTA_H
#define TRELLIS_OTA_H

#include <Arduino.h>
#include <functional>

#if defined(ESP32)
  #include <HTTPUpdate.h>
  #include <WiFi.h>
#endif

class TrellisOTA {
public:
  /// Perform OTA update from the given URL.
  /// If broadcaster is provided, it receives JSON strings for progress and
  /// delivery events to forward over WebSocket.
  /// On success, returns true WITHOUT rebooting — caller must send
  /// ota_delivered and call ESP.restart().
  static bool update(const char* url, std::function<void(const String&)> broadcaster = nullptr);
};

#endif
