#include "TrellisOTA.h"
#include <ArduinoJson.h>

bool TrellisOTA::update(const char* url, std::function<void(const String&)> broadcaster) {
#if defined(ESP32)
  Serial.printf("[Trellis] Starting OTA from %s\n", url);

  // Don't auto-reboot — caller sends ota_delivered first, then restarts.
  httpUpdate.rebootOnUpdate(false);

  // Stream real progress percentages over WebSocket.
  if (broadcaster) {
    int lastPct = 0;  // Start at 0: caller already broadcasts the initial 0% event
    httpUpdate.onProgress([broadcaster, lastPct](int current, int total) mutable {
      if (total <= 0) return;
      int pct = (current * 100) / total;
      // Throttle: send every 5% to avoid flooding the WebSocket.
      if (pct == lastPct || (pct % 5 != 0 && pct != 100)) return;
      lastPct = pct;
      JsonDocument doc;
      doc["event"] = "ota_progress";
      doc["percent"] = pct;
      String json;
      serializeJson(doc, json);
      broadcaster(json);
    });
  }

  WiFiClient client;
  t_httpUpdate_return ret = httpUpdate.update(client, url);

  // Clear callbacks so they don't outlive the broadcaster capture.
  httpUpdate.onProgress(nullptr);

  switch (ret) {
    case HTTP_UPDATE_FAILED:
      Serial.printf("[Trellis] OTA failed: %s\n", httpUpdate.getLastErrorString().c_str());
      return false;
    case HTTP_UPDATE_NO_UPDATES:
      Serial.println("[Trellis] OTA: no update available");
      return false;
    case HTTP_UPDATE_OK:
      Serial.println("[Trellis] OTA written successfully");
      return true;
  }
#else
  Serial.println("[Trellis] OTA not supported on this platform");
  (void)url;
  (void)broadcaster;
#endif
  return false;
}
