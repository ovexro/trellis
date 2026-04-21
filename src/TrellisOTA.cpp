#include "TrellisOTA.h"
#include <ArduinoJson.h>

#if defined(ESP32)
  #include <HTTPClient.h>
  #include <Preferences.h>
  static const char* TRELLIS_OTA_NVS_NAMESPACE = "trellis_ota";
  static const char* TRELLIS_OTA_NVS_ACK_KEY   = "ack_url";
#endif

bool TrellisOTA::update(
    const char* url,
    const char* ackUrl,
    std::function<void(const String&)> broadcaster) {
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
      // Persist the ack URL BEFORE returning — caller restarts immediately
      // after, so any write that happens after this point is discarded. A
      // failure to save NVS just means the desktop's ota_success_rate will
      // undercount this device; the firmware still boots.
      if (ackUrl && ackUrl[0] != '\0') {
        Preferences prefs;
        if (prefs.begin(TRELLIS_OTA_NVS_NAMESPACE, false)) {
          prefs.putString(TRELLIS_OTA_NVS_ACK_KEY, ackUrl);
          prefs.end();
          Serial.println("[Trellis] OTA ack pending — will POST on next boot");
        } else {
          Serial.println("[Trellis] Warning: could not open NVS to persist ack URL");
        }
      }
      Serial.println("[Trellis] OTA written successfully");
      return true;
  }
#else
  Serial.println("[Trellis] OTA not supported on this platform");
  (void)url;
  (void)ackUrl;
  (void)broadcaster;
#endif
  return false;
}

void TrellisOTA::sendPendingAck(const char* firmwareVersion) {
#if defined(ESP32)
  Preferences prefs;
  if (!prefs.begin(TRELLIS_OTA_NVS_NAMESPACE, false)) {
    return;
  }
  String ackUrl = prefs.getString(TRELLIS_OTA_NVS_ACK_KEY, "");
  if (ackUrl.length() == 0) {
    prefs.end();
    return;
  }
  // Clear before the POST so a transient network glitch can't trap the
  // device in a boot-retry cycle. The desktop endpoint is already
  // idempotent, so even if a future boot re-tries (which won't happen
  // with this implementation) there's no double-count.
  prefs.remove(TRELLIS_OTA_NVS_ACK_KEY);
  prefs.end();

  Serial.printf("[Trellis] Posting OTA apply ack to %s\n", ackUrl.c_str());

  HTTPClient http;
  if (!http.begin(ackUrl)) {
    Serial.println("[Trellis] Ack POST failed: http.begin() rejected URL");
    return;
  }
  http.setTimeout(5000);
  http.addHeader("Content-Type", "application/json");
  JsonDocument doc;
  doc["version"] = firmwareVersion ? firmwareVersion : "";
  String body;
  serializeJson(doc, body);
  int status = http.POST(body);
  if (status > 0) {
    Serial.printf("[Trellis] Ack POST returned %d\n", status);
  } else {
    Serial.printf("[Trellis] Ack POST transport error: %s\n",
      http.errorToString(status).c_str());
  }
  http.end();
#else
  (void)firmwareVersion;
#endif
}
