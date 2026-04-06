#include "TrellisOTA.h"

bool TrellisOTA::update(const char* url) {
#if defined(ESP32)
  Serial.printf("[Trellis] Starting OTA from %s\n", url);

  WiFiClient client;
  t_httpUpdate_return ret = httpUpdate.update(client, url);

  switch (ret) {
    case HTTP_UPDATE_FAILED:
      Serial.printf("[Trellis] OTA failed: %s\n", httpUpdate.getLastErrorString().c_str());
      return false;
    case HTTP_UPDATE_NO_UPDATES:
      Serial.println("[Trellis] OTA: no update available");
      return false;
    case HTTP_UPDATE_OK:
      Serial.println("[Trellis] OTA success, rebooting...");
      return true;
  }
#else
  Serial.println("[Trellis] OTA not supported on this platform");
  (void)url;
#endif
  return false;
}
