#include "TrellisScenes.h"
#include "Trellis.h"
#include <time.h>

TrellisScenes::TrellisScenes()
  : _trellis(nullptr), _lastTickMinute(0) {}

void TrellisScenes::begin(Trellis* trellis) {
  _trellis = trellis;

#if defined(ESP32)
  // Best-effort wall-clock sync. configTime returns immediately; the SNTP
  // task fills the clock asynchronously. Schedules quietly skip firing
  // until the clock has been set (year > 2020 sentinel inside tick()).
  configTime(0, 0, "pool.ntp.org", "time.nist.gov");
  loadFromNvs();
#endif
}

void TrellisScenes::tick() {
#if defined(ESP32)
  if (_schedules.empty()) return;

  time_t now = time(nullptr);
  // Skip until SNTP has filled the clock — pre-1970 epoch obviously, but
  // also guard against a half-set clock that's been forced to 1970 by the
  // RTC reset path. 2020 is the cheapest "is this a real wall-clock time"
  // sentinel that won't ever false-trigger after SNTP succeeds.
  if (now < 1577836800) return;  // 2020-01-01T00:00:00Z

  struct tm tmNow;
  if (!gmtime_r(&now, &tmNow)) return;

  uint32_t thisMinute = (uint32_t(tmNow.tm_yday) << 16)
                      | (uint32_t(tmNow.tm_hour) << 8)
                      | uint32_t(tmNow.tm_min);
  if (thisMinute == _lastTickMinute) return;  // already serviced this minute
  _lastTickMinute = thisMinute;

  uint8_t weekdayBit = uint8_t(1) << tmNow.tm_wday;  // 0=Sun … 6=Sat
  for (const Schedule& s : _schedules) {
    if (s.hour != tmNow.tm_hour || s.minute != tmNow.tm_min) continue;
    if ((s.daysOfWeekMask & weekdayBit) == 0) continue;
    String err;
    if (!recallScene(s.sceneId, &err)) {
      Serial.printf("[Trellis] Schedule %s: recall '%s' failed: %s\n",
        s.id.c_str(), s.sceneId.c_str(), err.c_str());
    } else {
      Serial.printf("[Trellis] Schedule %s fired -> scene '%s'\n",
        s.id.c_str(), s.sceneId.c_str());
    }
  }
#endif
}

String TrellisScenes::generateId(const char* prefix) {
  // 6 hex chars from millis + a wraparound counter is unique enough at
  // device scale (max 8 of each) and stable across the response we hand
  // back to the client without needing a separate retrieval round-trip.
  static uint16_t bump = 0;
  uint32_t now = millis();
  char buf[24];
  snprintf(buf, sizeof(buf), "%s_%06lx%02x",
    prefix, (unsigned long)(now & 0xFFFFFF), (unsigned)(bump++ & 0xFF));
  return String(buf);
}

bool TrellisScenes::createScene(const String& name,
                                const std::vector<SceneSetpoint>& setpoints,
                                String* outId, String* outErr) {
  if (_scenes.size() >= TRELLIS_SCENES_MAX) {
    if (outErr) *outErr = "scene limit reached";
    return false;
  }
  if (setpoints.size() > TRELLIS_SETPOINTS_MAX) {
    if (outErr) *outErr = "setpoint limit per scene reached";
    return false;
  }
  if (name.length() == 0) {
    if (outErr) *outErr = "scene name required";
    return false;
  }
  Scene s;
  s.id = generateId("scn");
  s.name = name;
  s.setpoints = setpoints;
  _scenes.push_back(s);
  if (outId) *outId = s.id;
#if defined(ESP32)
  saveScenesToNvs();
#endif
  return true;
}

bool TrellisScenes::deleteScene(const String& id) {
  for (auto it = _scenes.begin(); it != _scenes.end(); ++it) {
    if (it->id == id) {
      _scenes.erase(it);
      // Cascade: any schedule pointing at this scene becomes a dangling
      // reference. Remove them too so tick() can never recall a deleted
      // scene and the UI stays consistent.
      for (auto sit = _schedules.begin(); sit != _schedules.end();) {
        if (sit->sceneId == id) sit = _schedules.erase(sit);
        else ++sit;
      }
#if defined(ESP32)
      saveScenesToNvs();
      saveSchedulesToNvs();
#endif
      return true;
    }
  }
  return false;
}

bool TrellisScenes::recallScene(const String& id, String* outErr) {
  if (!_trellis) {
    if (outErr) *outErr = "scenes not initialised";
    return false;
  }
  const Scene* found = nullptr;
  for (const Scene& s : _scenes) {
    if (s.id == id) { found = &s; break; }
  }
  if (!found) {
    if (outErr) *outErr = "scene not found";
    return false;
  }
  for (const SceneSetpoint& sp : found->setpoints) {
    JsonDocument doc;
    DeserializationError err = deserializeJson(doc, sp.valueJson);
    if (err) {
      Serial.printf("[Trellis] Scene '%s' setpoint '%s' bad JSON: %s\n",
        id.c_str(), sp.capId.c_str(), err.c_str());
      continue;
    }
    _trellis->applyCapabilityValue(sp.capId.c_str(), doc.as<JsonVariant>());
  }
  return true;
}

bool TrellisScenes::createSchedule(const String& sceneId, uint8_t hour, uint8_t minute,
                                   uint8_t daysOfWeekMask, String* outId, String* outErr) {
  if (_schedules.size() >= TRELLIS_SCHEDULES_MAX) {
    if (outErr) *outErr = "schedule limit reached";
    return false;
  }
  if (hour > 23 || minute > 59) {
    if (outErr) *outErr = "time out of range";
    return false;
  }
  if ((daysOfWeekMask & 0x7F) == 0) {
    if (outErr) *outErr = "at least one day of week required";
    return false;
  }
  bool sceneExists = false;
  for (const Scene& s : _scenes) {
    if (s.id == sceneId) { sceneExists = true; break; }
  }
  if (!sceneExists) {
    if (outErr) *outErr = "scene not found";
    return false;
  }
  Schedule s;
  s.id = generateId("sch");
  s.sceneId = sceneId;
  s.hour = hour;
  s.minute = minute;
  s.daysOfWeekMask = daysOfWeekMask & 0x7F;
  _schedules.push_back(s);
  if (outId) *outId = s.id;
#if defined(ESP32)
  saveSchedulesToNvs();
#endif
  return true;
}

bool TrellisScenes::deleteSchedule(const String& id) {
  for (auto it = _schedules.begin(); it != _schedules.end(); ++it) {
    if (it->id == id) {
      _schedules.erase(it);
#if defined(ESP32)
      saveSchedulesToNvs();
#endif
      return true;
    }
  }
  return false;
}

#if defined(ESP32)

// NVS schema: one JSON blob per top-level collection ("scenes", "schedules")
// so we don't have to manage per-record key sprawl. JSON's small-document
// shape (handful of bytes per scene) keeps this comfortably under the 1976-byte
// per-key limit even at TRELLIS_SCENES_MAX with full setpoints.

void TrellisScenes::loadFromNvs() {
  Preferences prefs;
  if (!prefs.begin("trellis_scn", true)) return;

  if (prefs.isKey("scenes")) {
    String blob = prefs.getString("scenes", "[]");
    JsonDocument doc;
    if (deserializeJson(doc, blob) == DeserializationError::Ok && doc.is<JsonArray>()) {
      for (JsonObject obj : doc.as<JsonArray>()) {
        Scene s;
        s.id   = obj["id"]   | "";
        s.name = obj["name"] | "";
        if (s.id.length() == 0) continue;
        JsonArray sps = obj["setpoints"].as<JsonArray>();
        if (sps) {
          for (JsonObject spo : sps) {
            SceneSetpoint sp;
            sp.capId     = spo["capId"]     | "";
            sp.valueJson = spo["valueJson"] | "null";
            if (sp.capId.length() == 0) continue;
            s.setpoints.push_back(sp);
            if (s.setpoints.size() >= TRELLIS_SETPOINTS_MAX) break;
          }
        }
        _scenes.push_back(s);
        if (_scenes.size() >= TRELLIS_SCENES_MAX) break;
      }
    }
  }

  if (prefs.isKey("scheds")) {
    String blob = prefs.getString("scheds", "[]");
    JsonDocument doc;
    if (deserializeJson(doc, blob) == DeserializationError::Ok && doc.is<JsonArray>()) {
      for (JsonObject obj : doc.as<JsonArray>()) {
        Schedule s;
        s.id      = obj["id"]      | "";
        s.sceneId = obj["sceneId"] | "";
        s.hour    = obj["hour"]    | 0;
        s.minute  = obj["minute"]  | 0;
        s.daysOfWeekMask = obj["dow"] | 0x7F;
        if (s.id.length() == 0 || s.sceneId.length() == 0) continue;
        if (s.hour > 23 || s.minute > 59) continue;
        if ((s.daysOfWeekMask & 0x7F) == 0) continue;
        _schedules.push_back(s);
        if (_schedules.size() >= TRELLIS_SCHEDULES_MAX) break;
      }
    }
  }

  prefs.end();
  if (!_scenes.empty() || !_schedules.empty()) {
    Serial.printf("[Trellis] Loaded %u scenes, %u schedules from NVS\n",
      (unsigned)_scenes.size(), (unsigned)_schedules.size());
  }
}

void TrellisScenes::saveScenesToNvs() {
  JsonDocument doc;
  JsonArray arr = doc.to<JsonArray>();
  for (const Scene& s : _scenes) {
    JsonObject obj = arr.add<JsonObject>();
    obj["id"]   = s.id;
    obj["name"] = s.name;
    JsonArray sps = obj["setpoints"].to<JsonArray>();
    for (const SceneSetpoint& sp : s.setpoints) {
      JsonObject spo = sps.add<JsonObject>();
      spo["capId"]     = sp.capId;
      spo["valueJson"] = sp.valueJson;
    }
  }
  String out;
  serializeJson(doc, out);
  Preferences prefs;
  if (prefs.begin("trellis_scn", false)) {
    prefs.putString("scenes", out);
    prefs.end();
    if (_trellis) _trellis->incrementNvsWrites();
  }
}

void TrellisScenes::saveSchedulesToNvs() {
  JsonDocument doc;
  JsonArray arr = doc.to<JsonArray>();
  for (const Schedule& s : _schedules) {
    JsonObject obj = arr.add<JsonObject>();
    obj["id"]      = s.id;
    obj["sceneId"] = s.sceneId;
    obj["hour"]    = s.hour;
    obj["minute"]  = s.minute;
    obj["dow"]     = s.daysOfWeekMask;
  }
  String out;
  serializeJson(doc, out);
  Preferences prefs;
  if (prefs.begin("trellis_scn", false)) {
    prefs.putString("scheds", out);
    prefs.end();
    if (_trellis) _trellis->incrementNvsWrites();
  }
}

#endif  // ESP32
