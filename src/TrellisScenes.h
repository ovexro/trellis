#ifndef TRELLIS_SCENES_H
#define TRELLIS_SCENES_H

#include <Arduino.h>
#include <ArduinoJson.h>
#include <vector>

#if defined(ESP32)
  #include <Preferences.h>
#endif

class Trellis;

// On-device scenes + simple time-based schedules. ESP32-only persistence
// and tick. On Pico, all methods are safe no-ops so the rest of the library
// can call them unconditionally — matches TrellisDiscovery's
// advertise-on-Pico, browse-on-ESP32 shape.
//
// Bounded by design (TRELLIS_SCENES_MAX / TRELLIS_SCHEDULES_MAX). NVS budget
// at the cap is roughly 2 KB which leaves the trellis_scn namespace well
// inside the partition's quota even after years of edits.

#define TRELLIS_SCENES_MAX     8
#define TRELLIS_SETPOINTS_MAX  5
#define TRELLIS_SCHEDULES_MAX  8

struct SceneSetpoint {
  String capId;
  // Stored as a JSON-encoded scalar so we can round-trip any cap type
  // (bool / float / string) through the same path. Recall parses this
  // back into a JsonDocument and dispatches via Trellis::applyCapabilityValue.
  String valueJson;
};

struct Scene {
  String id;
  String name;
  std::vector<SceneSetpoint> setpoints;
};

struct Schedule {
  String id;
  String sceneId;
  uint8_t hour;            // 0-23, wall-clock UTC for v0.33.0
  uint8_t minute;          // 0-59
  uint8_t daysOfWeekMask;  // bit 0=Sun … bit 6=Sat; 0x7F = every day
};

class TrellisScenes {
public:
  TrellisScenes();

  // Wire the helper to its parent. Call once from Trellis::begin/beginAutoConnect
  // after WiFi is up (NTP needs a connected radio). Loads persisted scenes
  // and schedules from NVS into RAM.
  void begin(Trellis* trellis);

  // Drive the schedule tick. Cheap when no schedule fires; gated so a single
  // minute can't fire the same schedule twice even if loop() runs many times
  // during that minute. Safe to call every loop iteration.
  void tick();

  // Snapshots — return a copy so callers can serialize without holding any
  // lock-equivalent. Cheap (<256B per scene).
  std::vector<Scene>    listScenes()    const { return _scenes; }
  std::vector<Schedule> listSchedules() const { return _schedules; }

  // CRUD. Bounded by TRELLIS_SCENES_MAX / TRELLIS_SCHEDULES_MAX — both
  // return false on overflow so the REST surface can translate to HTTP 409.
  // create*() generates the id; the caller hands back the id via outId.
  bool createScene(const String& name,
                   const std::vector<SceneSetpoint>& setpoints,
                   String* outId, String* outErr);
  bool deleteScene(const String& id);
  bool recallScene(const String& id, String* outErr);

  bool createSchedule(const String& sceneId, uint8_t hour, uint8_t minute,
                      uint8_t daysOfWeekMask, String* outId, String* outErr);
  bool deleteSchedule(const String& id);

  // Capacity reporting for the REST GET — clients render "X / max" hints.
  static uint8_t maxScenes()    { return TRELLIS_SCENES_MAX; }
  static uint8_t maxSetpoints() { return TRELLIS_SETPOINTS_MAX; }
  static uint8_t maxSchedules() { return TRELLIS_SCHEDULES_MAX; }

private:
  Trellis* _trellis;
  std::vector<Scene>    _scenes;
  std::vector<Schedule> _schedules;

  // Last wall-clock minute we serviced, packed as (yday<<16)|(hour<<8)|min.
  // Prevents double-firing a schedule inside the same minute when tick()
  // is called many times between minute boundaries.
  uint32_t _lastTickMinute;

  String generateId(const char* prefix);

#if defined(ESP32)
  void loadFromNvs();
  void saveScenesToNvs();
  void saveSchedulesToNvs();
#endif
};

#endif
