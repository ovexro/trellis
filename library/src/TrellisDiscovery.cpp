#include "TrellisDiscovery.h"

void TrellisDiscovery::begin(const char* name, uint16_t port) {
  // Convert device name to valid mDNS hostname (lowercase, alphanumeric + hyphens)
  String hostname = String(name);
  hostname.toLowerCase();
  hostname.replace(" ", "-");

  if (!MDNS.begin(hostname.c_str())) {
    Serial.println("[Trellis] mDNS failed to start");
    return;
  }

  MDNS.addService("trellis", "tcp", port);
  MDNS.addServiceTxt("trellis", "tcp", "name", name);

  Serial.printf("[Trellis] mDNS: %s._trellis._tcp.local:%d\n", hostname.c_str(), port);
}
