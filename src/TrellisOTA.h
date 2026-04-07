#ifndef TRELLIS_OTA_H
#define TRELLIS_OTA_H

#include <Arduino.h>

#if defined(ESP32)
  #include <HTTPUpdate.h>
  #include <WiFi.h>
#endif

class TrellisOTA {
public:
  static bool update(const char* url);
};

#endif
