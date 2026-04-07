#ifndef TRELLIS_DISCOVERY_H
#define TRELLIS_DISCOVERY_H

#include <Arduino.h>

#if defined(ESP32)
  #include <ESPmDNS.h>
#elif defined(ARDUINO_ARCH_RP2040)
  #include <LEAmDNS.h>
#endif

class TrellisDiscovery {
public:
  void begin(const char* name, uint16_t port);
};

#endif
