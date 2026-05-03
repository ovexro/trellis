#include "TrellisDiscovery.h"
#include <WiFi.h>

TrellisDiscovery::TrellisDiscovery()
#if defined(ESP32)
  : _mutex(nullptr), _task(nullptr)
#endif
{
#if defined(ESP32)
  _mutex = xSemaphoreCreateMutex();
#endif
}

void TrellisDiscovery::begin(const char* name, uint16_t port) {
  String hostname = String(name);
  hostname.toLowerCase();
  hostname.replace(" ", "-");

  if (!MDNS.begin(hostname.c_str())) {
    Serial.println("[Trellis] mDNS failed to start");
    return;
  }

  // Same algorithm the WebServer uses at /api/info — using the last 4 MAC
  // bytes keeps the id stable across reboots and short enough to read in
  // logs. Both sides must compute it identically or self-exclusion in the
  // peer cache breaks.
  uint8_t mac[6];
  WiFi.macAddress(mac);
  char id[18];
  snprintf(id, sizeof(id), "trellis-%02x%02x%02x%02x",
    mac[2], mac[3], mac[4], mac[5]);
  _selfId = id;

  MDNS.addService("trellis", "tcp", port);
  MDNS.addServiceTxt("trellis", "tcp", "name", name);
  MDNS.addServiceTxt("trellis", "tcp", "id", _selfId.c_str());

  Serial.printf("[Trellis] mDNS: %s._trellis._tcp.local:%d (id=%s)\n",
    hostname.c_str(), port, _selfId.c_str());
}

#if defined(ESP32)

void TrellisDiscovery::beginBrowse() {
  if (_task != nullptr) return;
  xTaskCreatePinnedToCore(
    &TrellisDiscovery::browseTask,
    "trellis-browse",
    4096,
    this,
    1,
    &_task,
    APP_CPU_NUM);
}

void TrellisDiscovery::browseTask(void* pv) {
  static_cast<TrellisDiscovery*>(pv)->browseLoop();
  vTaskDelete(nullptr);
}

void TrellisDiscovery::browseLoop() {
  for (;;) {
    if (WiFi.status() == WL_CONNECTED) {
      int n = MDNS.queryService("trellis", "tcp");
      std::vector<TrellisPeer> next;
      if (n > 0) next.reserve(n);

      for (int i = 0; i < n; i++) {
        TrellisPeer p;
        // ESP32 core 3.x renamed MDNS.IP() to MDNS.address(); older 2.x cores
        // exposed only IP(). The 3.x name is what current Trellis users will
        // be on (esp32:esp32 3.3.7 ships in CI).
        p.host = MDNS.address(i).toString();
        p.port = MDNS.port(i);
        p.name = MDNS.txt(i, "name");
        p.id   = MDNS.txt(i, "id");

        // Skip self. Also skip pre-this-slot firmware whose announcements
        // carry no id TXT — without an id we can't tell self from peer,
        // and false-positive self-renders in the embedded UI would be
        // worse than a missing peer card.
        if (p.id.length() == 0) continue;
        if (p.id == _selfId)    continue;

        next.push_back(p);
      }

      if (xSemaphoreTake(_mutex, portMAX_DELAY) == pdTRUE) {
        _peers = std::move(next);
        xSemaphoreGive(_mutex);
      }
    }

    vTaskDelay(pdMS_TO_TICKS(BROWSE_INTERVAL_MS));
  }
}

std::vector<TrellisPeer> TrellisDiscovery::getPeers() {
  std::vector<TrellisPeer> snapshot;
  if (_mutex && xSemaphoreTake(_mutex, pdMS_TO_TICKS(50)) == pdTRUE) {
    snapshot = _peers;
    xSemaphoreGive(_mutex);
  }
  return snapshot;
}

#else  // ARDUINO_ARCH_RP2040 — advertise-only

void TrellisDiscovery::beginBrowse() {}

std::vector<TrellisPeer> TrellisDiscovery::getPeers() {
  return std::vector<TrellisPeer>();
}

#endif
