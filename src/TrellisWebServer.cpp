#include "TrellisWebServer.h"
#include "Trellis.h"
#include "TrellisWebUI_html.h"

TrellisWebServer::TrellisWebServer(Trellis* trellis)
  : _trellis(trellis), _http(nullptr), _ws(nullptr), _webUIEnabled(true) {}

void TrellisWebServer::begin(uint16_t port) {
  _http = new WebServer(port);
  _ws = new WebSocketsServer(port + 1);

  _http->on("/api/info", HTTP_GET, [this]() { handleInfo(); });
  _http->on("/api/peers", HTTP_GET, [this]() { handlePeers(); });
  _http->on("/api/scenes",         HTTP_GET,  [this]() { handleScenesGet(); });
  _http->on("/api/scenes",         HTTP_POST, [this]() { handleScenesPost(); });
  _http->on("/api/scenes/recall",  HTTP_POST, [this]() { handleSceneRecall(); });
  _http->on("/api/scenes/delete",  HTTP_POST, [this]() { handleSceneDelete(); });
  _http->on("/api/schedules",        HTTP_GET,  [this]() { handleSchedulesGet(); });
  _http->on("/api/schedules",        HTTP_POST, [this]() { handleSchedulesPost(); });
  _http->on("/api/schedules/delete", HTTP_POST, [this]() { handleScheduleDelete(); });
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
  // Allow cross-origin reads so the embedded dashboard loaded from a peer
  // device can fetch this device's /api/info to render its capabilities.
  // The endpoint is read-only and intended to be discoverable LAN-wide.
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", json);
}

void TrellisWebServer::handlePeers() {
  String json = buildPeersJson();
  _http->sendHeader("Cache-Control", "no-store");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
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
#if defined(ESP32)
  sys["nvs_writes"] = _trellis->getNvsWrites();
#endif

  String output;
  serializeJson(doc, output);
  return output;
}

String TrellisWebServer::buildPeersJson() {
  JsonDocument doc;
  JsonArray arr = doc.to<JsonArray>();

  TrellisDiscovery* d = _trellis->getDiscovery();
  if (d) {
    std::vector<TrellisPeer> peers = d->getPeers();
    for (const TrellisPeer& p : peers) {
      JsonObject obj = arr.add<JsonObject>();
      obj["id"]   = p.id;
      obj["name"] = p.name;
      obj["host"] = p.host;
      obj["port"] = p.port;
    }
  }

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
    // Single canonical apply path — same code TrellisScenes::recallScene
    // calls so manual control and scene recall stay in lockstep.
    _trellis->applyCapabilityValue(id, doc["value"]);
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
#if defined(ESP32)
  sys["nvs_writes"] = _trellis->getNvsWrites();
#endif
  String json;
  serializeJson(doc, json);
  _ws->broadcastTXT(json);
}

String TrellisWebServer::readJsonBody() {
  // arg("plain") is how Arduino WebServer exposes the raw POST body when the
  // Content-Type isn't form-urlencoded. Returns "" if no body was sent.
  if (_http->hasArg("plain")) return _http->arg("plain");
  return String();
}

void TrellisWebServer::sendJsonError(int code, const char* message) {
  JsonDocument doc;
  doc["error"] = message;
  String out;
  serializeJson(doc, out);
  _http->sendHeader("Cache-Control", "no-store");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(code, "application/json", out);
}

String TrellisWebServer::buildScenesJson() {
  JsonDocument doc;
  JsonObject root = doc.to<JsonObject>();
  JsonArray arr = root["scenes"].to<JsonArray>();
  TrellisScenes* sc = _trellis->getScenes();
  if (sc) {
    std::vector<Scene> all = sc->listScenes();
    for (const Scene& s : all) {
      JsonObject obj = arr.add<JsonObject>();
      obj["id"]   = s.id;
      obj["name"] = s.name;
      JsonArray sps = obj["setpoints"].to<JsonArray>();
      for (const SceneSetpoint& sp : s.setpoints) {
        JsonObject spo = sps.add<JsonObject>();
        spo["capId"] = sp.capId;
        // Re-parse so we ship the typed value, not the JSON-as-string. The
        // client never has to know we store it as a string for round-trip.
        JsonDocument vd;
        if (deserializeJson(vd, sp.valueJson) == DeserializationError::Ok) {
          spo["value"] = vd.as<JsonVariant>();
        } else {
          spo["value"] = nullptr;
        }
      }
    }
  }
  root["max"]           = TrellisScenes::maxScenes();
  root["maxSetpoints"]  = TrellisScenes::maxSetpoints();
  String out;
  serializeJson(doc, out);
  return out;
}

String TrellisWebServer::buildSchedulesJson() {
  JsonDocument doc;
  JsonObject root = doc.to<JsonObject>();
  JsonArray arr = root["schedules"].to<JsonArray>();
  TrellisScenes* sc = _trellis->getScenes();
  if (sc) {
    std::vector<Schedule> all = sc->listSchedules();
    for (const Schedule& s : all) {
      JsonObject obj = arr.add<JsonObject>();
      obj["id"]      = s.id;
      obj["sceneId"] = s.sceneId;
      obj["hour"]    = s.hour;
      obj["minute"]  = s.minute;
      obj["dow"]     = s.daysOfWeekMask;
    }
  }
  root["max"] = TrellisScenes::maxSchedules();
  String out;
  serializeJson(doc, out);
  return out;
}

void TrellisWebServer::handleScenesGet() {
  String out = buildScenesJson();
  _http->sendHeader("Cache-Control", "no-store");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", out);
}

void TrellisWebServer::handleSchedulesGet() {
  String out = buildSchedulesJson();
  _http->sendHeader("Cache-Control", "no-store");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", out);
}

void TrellisWebServer::handleScenesPost() {
  TrellisScenes* sc = _trellis->getScenes();
  if (!sc) return sendJsonError(503, "scenes not initialised");
  String body = readJsonBody();
  if (body.length() == 0) return sendJsonError(400, "empty body");

  JsonDocument doc;
  if (deserializeJson(doc, body)) return sendJsonError(400, "invalid json");

  const char* name = doc["name"] | (const char*)nullptr;
  if (!name || !*name) return sendJsonError(400, "name required");

  std::vector<SceneSetpoint> setpoints;
  // Two accepted shapes:
  //   1) {name, setpoints:[{capId, value}, ...]} — explicit
  //   2) {name, capture:true} — snapshot all current cap values now
  // Shape 2 is what the embedded UI uses for "Capture current state". Shape 1
  // is for explicit programmatic creation (future external tooling).
  if (doc["capture"] | false) {
    Capability* caps = _trellis->getCapabilities();
    uint8_t n = _trellis->getCapabilityCount();
    for (uint8_t i = 0; i < n && setpoints.size() < TrellisScenes::maxSetpoints(); i++) {
      Capability* cap = &caps[i];
      // Sensors are read-only; capturing them is allowed but doesn't recall
      // (applyCapabilityValue is silent on SENSOR). Keep them out of the
      // capture so the scene reads cleanly to the user as "what I set".
      if (cap->type == CapabilityType::SENSOR) continue;
      SceneSetpoint sp;
      sp.capId = cap->id;
      JsonDocument vd;
      switch (cap->type) {
        case CapabilityType::SWITCH: vd.set(cap->boolValue);   break;
        case CapabilityType::SLIDER: vd.set(cap->floatValue);  break;
        case CapabilityType::COLOR:
        case CapabilityType::TEXT:   vd.set((const char*)cap->stringValue); break;
        default: continue;
      }
      String vjson;
      serializeJson(vd, vjson);
      sp.valueJson = vjson;
      setpoints.push_back(sp);
    }
  } else {
    JsonArray arr = doc["setpoints"].as<JsonArray>();
    if (!arr) return sendJsonError(400, "setpoints required");
    for (JsonObject sp : arr) {
      const char* capId = sp["capId"] | (const char*)nullptr;
      if (!capId) continue;
      // Validate the capability exists at submit time so a typo is loud
      // instead of mysteriously no-oping at recall.
      if (!_trellis->findCapability(capId)) {
        return sendJsonError(400, "unknown capId in setpoints");
      }
      SceneSetpoint s;
      s.capId = capId;
      String vjson;
      // Re-serialize the value field so we store byte-clean canonical JSON
      // regardless of how the client encoded it.
      serializeJson(sp["value"], vjson);
      s.valueJson = vjson;
      setpoints.push_back(s);
      if (setpoints.size() >= TrellisScenes::maxSetpoints()) break;
    }
  }

  String id, err;
  if (!sc->createScene(name, setpoints, &id, &err)) {
    int code = err.indexOf("limit") >= 0 ? 409 : 400;
    return sendJsonError(code, err.c_str());
  }
  JsonDocument out;
  out["ok"] = true;
  out["id"] = id;
  String body2;
  serializeJson(out, body2);
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", body2);
}

void TrellisWebServer::handleSceneRecall() {
  TrellisScenes* sc = _trellis->getScenes();
  if (!sc) return sendJsonError(503, "scenes not initialised");
  String body = readJsonBody();
  if (body.length() == 0) return sendJsonError(400, "empty body");
  JsonDocument doc;
  if (deserializeJson(doc, body)) return sendJsonError(400, "invalid json");
  const char* id = doc["id"] | (const char*)nullptr;
  if (!id) return sendJsonError(400, "id required");
  String err;
  if (!sc->recallScene(id, &err)) {
    int code = err == "scene not found" ? 404 : 400;
    return sendJsonError(code, err.c_str());
  }
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", "{\"ok\":true}");
}

void TrellisWebServer::handleSceneDelete() {
  TrellisScenes* sc = _trellis->getScenes();
  if (!sc) return sendJsonError(503, "scenes not initialised");
  String body = readJsonBody();
  if (body.length() == 0) return sendJsonError(400, "empty body");
  JsonDocument doc;
  if (deserializeJson(doc, body)) return sendJsonError(400, "invalid json");
  const char* id = doc["id"] | (const char*)nullptr;
  if (!id) return sendJsonError(400, "id required");
  if (!sc->deleteScene(id)) return sendJsonError(404, "scene not found");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", "{\"ok\":true}");
}

void TrellisWebServer::handleSchedulesPost() {
  TrellisScenes* sc = _trellis->getScenes();
  if (!sc) return sendJsonError(503, "scenes not initialised");
  String body = readJsonBody();
  if (body.length() == 0) return sendJsonError(400, "empty body");
  JsonDocument doc;
  if (deserializeJson(doc, body)) return sendJsonError(400, "invalid json");
  const char* sceneId = doc["sceneId"] | (const char*)nullptr;
  if (!sceneId) return sendJsonError(400, "sceneId required");
  int hour   = doc["hour"]   | -1;
  int minute = doc["minute"] | -1;
  if (hour < 0 || hour > 23 || minute < 0 || minute > 59) {
    return sendJsonError(400, "hour/minute out of range");
  }
  // Default to "every day" if the client omits dow — common convenience for
  // a "once per day at HH:MM" schedule that's the most likely first use.
  uint8_t dow = doc["dow"] | 0x7F;
  String id, err;
  if (!sc->createSchedule(sceneId, (uint8_t)hour, (uint8_t)minute, dow, &id, &err)) {
    int code = err.indexOf("limit") >= 0 ? 409 :
               (err == "scene not found" ? 404 : 400);
    return sendJsonError(code, err.c_str());
  }
  JsonDocument out;
  out["ok"] = true;
  out["id"] = id;
  String body2;
  serializeJson(out, body2);
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", body2);
}

void TrellisWebServer::handleScheduleDelete() {
  TrellisScenes* sc = _trellis->getScenes();
  if (!sc) return sendJsonError(503, "scenes not initialised");
  String body = readJsonBody();
  if (body.length() == 0) return sendJsonError(400, "empty body");
  JsonDocument doc;
  if (deserializeJson(doc, body)) return sendJsonError(400, "invalid json");
  const char* id = doc["id"] | (const char*)nullptr;
  if (!id) return sendJsonError(400, "id required");
  if (!sc->deleteSchedule(id)) return sendJsonError(404, "schedule not found");
  _http->sendHeader("Access-Control-Allow-Origin", "*");
  _http->send(200, "application/json", "{\"ok\":true}");
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
