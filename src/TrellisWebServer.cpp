#include "TrellisWebServer.h"
#include "Trellis.h"

TrellisWebServer::TrellisWebServer(Trellis* trellis)
  : _trellis(trellis), _http(nullptr), _ws(nullptr) {}

void TrellisWebServer::begin(uint16_t port) {
  _http = new WebServer(port);
  _ws = new WebSocketsServer(port + 1);

  _http->on("/api/info", HTTP_GET, [this]() { handleInfo(); });

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
  _http->send(200, "application/json", json);
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
        broadcastUpdate(id, val);
        break;
      }
      case CapabilityType::SLIDER: {
        float val = doc["value"].as<float>();
        cap->floatValue = val;
        // Apply PWM
        int pwmVal = map((long)(val), (long)(cap->minValue), (long)(cap->maxValue), 0, 255);
        analogWrite(cap->gpio, pwmVal);
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
    if (url) {
      Serial.printf("[Trellis] OTA update from: %s\n", url);

      // Report start
      JsonDocument progress;
      progress["event"] = "ota_progress";
      progress["percent"] = 0;
      String startJson;
      serializeJson(progress, startJson);
      _ws->broadcastTXT(startJson);

      // Perform OTA
      TrellisOTA::update(url);
      // If update() returns, it failed (success reboots)
      progress["percent"] = -1;
      String failJson;
      serializeJson(progress, failJson);
      _ws->broadcastTXT(failJson);
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
