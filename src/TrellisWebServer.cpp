#include "TrellisWebServer.h"
#include "Trellis.h"
#include "TrellisWebUI_html.h"

TrellisWebServer::TrellisWebServer(Trellis* trellis)
  : _trellis(trellis), _http(nullptr), _ws(nullptr), _webUIEnabled(true) {}

void TrellisWebServer::begin(uint16_t port) {
  _http = new WebServer(port);
  _ws = new WebSocketsServer(port + 1);

  _http->on("/api/info", HTTP_GET, [this]() { handleInfo(); });
  // Always register the route — handler checks _webUIEnabled at request time so
  // enableWebUI(false) works after begin() too.
  _http->on("/", HTTP_GET, [this]() { handleWebUI(); });

  // Register the request headers we want to read inside our handlers. The
  // Arduino WebServer library doesn't capture headers by default — anything
  // not in this list is silently dropped before the handler runs. Without
  // this call, _http->header("If-None-Match") would always return "" and
  // the conditional-GET path would never fire.
  static const char* RECOGNIZED_HEADERS[] = {"If-None-Match"};
  _http->collectHeaders(RECOGNIZED_HEADERS, sizeof(RECOGNIZED_HEADERS) / sizeof(RECOGNIZED_HEADERS[0]));

  _http->begin();

  _ws->begin();
  _ws->onEvent([this](uint8_t num, WStype_t type, uint8_t* payload, size_t length) {
    handleWebSocket(num, type, payload, length);
  });

  Serial.printf("[Trellis] HTTP on port %d, WebSocket on port %d\n", port, port + 1);
}

void TrellisWebServer::loop() {
  if (_http) _http->handleClient();
  if (_ws) _ws->loop();
}

void TrellisWebServer::handleInfo() {
  String json = buildInfoJson();
  _http->sendHeader("Cache-Control", "no-store");
  _http->send(200, "application/json", json);
}

void TrellisWebServer::handleWebUI() {
  if (!_webUIEnabled) {
    _http->send(404, PSTR("text/plain"), PSTR("Web UI disabled"));
    return;
  }
  // Build a content-tied ETag: <library version>-<sha256 prefix of HTML>.
  // The version prefix is for human inspection ("which firmware are you
  // running?"); the hash suffix is what makes the cache invalidate
  // automatically whenever the embedded HTML actually changes, even on
  // releases that forget to bump TRELLIS_VERSION.
  String etag = String("\"") + TRELLIS_VERSION + "-" + TRELLIS_WEB_UI_HTML_HASH + "\"";

  // RFC 7232 conditional GET: if the client's cached ETag matches what
  // we'd send, return 304 with no body and let the browser reuse its
  // cached copy. Saves ~25 KB per pageload over the lifetime of a firmware
  // version while still pulling fresh HTML the moment the version (or
  // content) changes.
  if (_http->header("If-None-Match") == etag) {
    _http->sendHeader("ETag", etag);
    _http->send(304, "text/plain", "");
    return;
  }

  // Cache-Control: no-cache forces the browser to revalidate every load
  // (sending If-None-Match), but it can still reuse its cached body when
  // we respond 304. This replaces the previous max-age=300 which made the
  // browser silently serve stale HTML for 5 minutes after a firmware bump.
  _http->sendHeader("Cache-Control", "no-cache, must-revalidate");
  _http->sendHeader("ETag", etag);
  // Serve the embedded dashboard from PROGMEM. send_P streams directly from
  // flash so we don't pull the whole HTML into RAM.
  _http->send_P(200, PSTR("text/html; charset=utf-8"), TRELLIS_WEB_UI_HTML);
}

String TrellisWebServer::buildInfoJson() {
  JsonDocument doc;

  doc["name"] = _trellis->getName();

  // Generate stable ID from MAC address
  uint8_t mac[6];
  WiFi.macAddress(mac);
  char macId[18];
  snprintf(macId, sizeof(macId), "trellis-%02x%02x%02x%02x",
    mac[2], mac[3], mac[4], mac[5]);
  doc["id"] = macId;

  doc["firmware"] = _trellis->getFirmwareVersion();
  doc["platform"] = TRELLIS_PLATFORM;

  JsonArray caps = doc["capabilities"].to<JsonArray>();
  for (uint8_t i = 0; i < _trellis->getCapabilityCount(); i++) {
    Capability* cap = &_trellis->getCapabilities()[i];
    JsonObject obj = caps.add<JsonObject>();
    obj["id"] = cap->id;
    obj["type"] = capabilityTypeToString(cap->type);
    obj["label"] = cap->label;

    switch (cap->type) {
      case CapabilityType::SWITCH:
        obj["value"] = cap->boolValue;
        break;
      case CapabilityType::SENSOR:
        obj["value"] = cap->floatValue;
        if (cap->unit) obj["unit"] = cap->unit;
        break;
      case CapabilityType::SLIDER:
        obj["value"] = cap->floatValue;
        obj["min"] = cap->minValue;
        obj["max"] = cap->maxValue;
        break;
      case CapabilityType::COLOR:
      case CapabilityType::TEXT:
        obj["value"] = cap->stringValue;
        break;
    }
  }

  TelemetryData telemetry = _trellis->getTelemetry().getData();
  JsonObject sys = doc["system"].to<JsonObject>();
  sys["rssi"] = telemetry.rssi;
  sys["heap_free"] = telemetry.heapFree;
  sys["uptime_s"] = telemetry.uptimeSeconds;
  sys["chip"] = telemetry.chip;
  sys["reset_reason"] = telemetry.resetReason;

  String output;
  serializeJson(doc, output);
  return output;
}

void TrellisWebServer::handleWebSocket(uint8_t num, WStype_t type, uint8_t* payload, size_t length) {
  switch (type) {
    case WStype_CONNECTED:
      Serial.printf("[Trellis] WS client #%u connected\n", num);
      break;
    case WStype_DISCONNECTED:
      Serial.printf("[Trellis] WS client #%u disconnected\n", num);
      break;
    case WStype_TEXT:
      processCommand(num, (const char*)payload);
      break;
    default:
      break;
  }
}

void TrellisWebServer::processCommand(uint8_t num, const char* json) {
  JsonDocument doc;
  DeserializationError err = deserializeJson(doc, json);
  if (err) {
    Serial.printf("[Trellis] JSON parse error: %s\n", err.c_str());
    return;
  }

  const char* command = doc["command"];
  if (!command) return;

  if (strcmp(command, "set") == 0) {
    const char* id = doc["id"];
    if (!id) return;

    Capability* cap = _trellis->findCapability(id);
    if (!cap) return;

    switch (cap->type) {
      case CapabilityType::SWITCH: {
        bool val = doc["value"].as<bool>();
        _trellis->setSwitch(id, val);
        // Persist to NVS so the value survives reboots (ESP32 only)
#if defined(ESP32)
        {
          Preferences prefs;
          prefs.begin("trellis_cap", false);
          prefs.putBool(id, val);
          prefs.end();
        }
#endif
        broadcastUpdate(id, val);
        break;
      }
      case CapabilityType::SLIDER: {
        float val = doc["value"].as<float>();
        _trellis->setSlider(id, val);
        // Persist to NVS so the value survives reboots (ESP32 only)
#if defined(ESP32)
        {
          Preferences prefs;
          prefs.begin("trellis_cap", false);
          prefs.putFloat(id, val);
          prefs.end();
        }
#endif
        broadcastUpdate(id, val);
        break;
      }
      case CapabilityType::COLOR: {
        const char* val = doc["value"].as<const char*>();
        if (val) _trellis->setColor(id, val);
        broadcastUpdate(id, val ? val : "#000000");
        break;
      }
      case CapabilityType::TEXT: {
        const char* val = doc["value"].as<const char*>();
        if (val) _trellis->setText(id, val);
        broadcastUpdate(id, val ? val : "");
        break;
      }
      default:
        break;
    }

    // Call user callback if registered
    if (_trellis->getCommandCallback()) {
      _trellis->getCommandCallback()(id, doc["value"]);
    }
  }
#if defined(ESP32)
  else if (strcmp(command, "ota") == 0) {
    const char* url = doc["url"];
    // Optional — desktop sends this on normal OTA paths starting v0.16.0
    // so the device can POST an apply confirmation after reboot; rollback
    // and pre-v0.16.0 desktops omit it, in which case we skip persistence.
    const char* ackUrl = doc["ack_url"] | (const char*)nullptr;
    if (url) {
      Serial.printf("[Trellis] OTA update from: %s\n", url);

      // Report start
      JsonDocument progress;
      progress["event"] = "ota_progress";
      progress["percent"] = 0;
      String startJson;
      serializeJson(progress, startJson);
      _ws->broadcastTXT(startJson);

      // Perform OTA with real-time progress broadcasting.
      WebSocketsServer* ws = _ws;
      bool ok = TrellisOTA::update(url, ackUrl, [ws](const String& json) {
        String copy = json;
        ws->broadcastTXT(copy);
      });

      if (ok) {
        // Firmware written — tell all clients before rebooting.
        JsonDocument delivered;
        delivered["event"] = "ota_delivered";
        String deliveredJson;
        serializeJson(delivered, deliveredJson);
        _ws->broadcastTXT(deliveredJson);
        delay(100);  // Let the WebSocket frame flush
        Serial.println("[Trellis] Rebooting...");
        ESP.restart();
      } else {
        progress["percent"] = -1;
        String failJson;
        serializeJson(progress, failJson);
        _ws->broadcastTXT(failJson);
      }
    }
  }
#endif
}

void TrellisWebServer::broadcastUpdate(const char* id, float value) {
  JsonDocument doc;
  doc["event"] = "update";
  doc["id"] = id;
  doc["value"] = value;
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}

void TrellisWebServer::broadcastUpdate(const char* id, bool value) {
  JsonDocument doc;
  doc["event"] = "update";
  doc["id"] = id;
  doc["value"] = value;
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}

void TrellisWebServer::broadcastUpdate(const char* id, const char* value) {
  JsonDocument doc;
  doc["event"] = "update";
  doc["id"] = id;
  doc["value"] = value;
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}

void TrellisWebServer::broadcastHeartbeat(const TelemetryData& telemetry) {
  JsonDocument doc;
  doc["event"] = "heartbeat";
  JsonObject sys = doc["system"].to<JsonObject>();
  sys["rssi"] = telemetry.rssi;
  sys["heap_free"] = telemetry.heapFree;
  sys["uptime_s"] = telemetry.uptimeSeconds;
  sys["chip"] = telemetry.chip;
  sys["reset_reason"] = telemetry.resetReason;
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}

void TrellisWebServer::broadcastLog(const char* severity, const char* message) {
  JsonDocument doc;
  doc["event"] = "log";
  doc["severity"] = severity;
  doc["message"] = message;
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}
