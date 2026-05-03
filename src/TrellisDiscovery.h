#ifndef TRELLIS_DISCOVERY_H
#define TRELLIS_DISCOVERY_H

#include <Arduino.h>
#include <vector>

#if defined(ESP32)
  #include <ESPmDNS.h>
  #include <freertos/FreeRTOS.h>
  #include <freertos/task.h>
  #include <freertos/semphr.h>
#elif defined(ARDUINO_ARCH_RP2040)
  #include <LEAmDNS.h>
#endif

struct TrellisPeer {
  String id;
  String name;
  String host;
  uint16_t port;
};

class TrellisDiscovery {
public:
  TrellisDiscovery();

  // Start advertising this device's mDNS service. Computes the same
  // MAC-derived id the WebServer publishes at /api/info and embeds it
  // as a TXT record so browsing peers can dedupe against self.
  void begin(const char* name, uint16_t port);

  // Spawn the periodic peer browser. ESP32 only — runs queryService every
  // 30s on a FreeRTOS task so the 3s-blocking lookup never stalls the
  // WebServer loop. Pico (LEAmDNS) is a no-op for this slot; this device
  // is still discoverable by other Trellis nodes via advertising.
  void beginBrowse();

  // Snapshot of the currently-discovered peers, excluding self. Cheap:
  // a copy under a short mutex hold. Returns empty on Pico.
  std::vector<TrellisPeer> getPeers();

  // Stable MAC-derived id of this device (e.g. "trellis-aabbccdd"). Empty
  // until begin() runs.
  const String& getSelfId() const { return _selfId; }

private:
  String _selfId;

#if defined(ESP32)
  static void browseTask(void* pv);
  void browseLoop();

  std::vector<TrellisPeer> _peers;
  SemaphoreHandle_t _mutex;
  TaskHandle_t _task;
  static const uint32_t BROWSE_INTERVAL_MS = 30000;
#endif
};

#endif
