#include "Trellis.h"

Trellis::Trellis(const char* name, uint16_t port)
  : _name(name),
    _firmwareVersion("0.0.0"),
    _port(port),
    _capCount(0),
    _commandCallback(nullptr),
    _webServer(nullptr),
    _discovery(nullptr),
    _provisioning(nullptr),
    _webUIEnabled(true),
    _lastBroadcast(0),
    _lastHeartbeat(0) {
  memset(_capabilities, 0, sizeof(_capabilities));
}

bool Trellis::begin(const char* ssid, const char* password, unsigned long timeout_ms) {
  Serial.begin(115200);
  Serial.printf("[Trellis] Connecting to %s...\n", ssid);

  WiFi.begin(ssid, password);

  unsigned long start = millis();
  while (WiFi.status() != WL_CONNECTED) {
    if (millis() - start > timeout_ms) {
      Serial.println("[Trellis] WiFi connection timeout");
      return false;
    }
    delay(250);
    Serial.print(".");
  }

  Serial.printf("\n[Trellis] Connected! IP: %s\n", WiFi.localIP().toString().c_str());

  // Close the two-phase OTA loop if the previous firmware wrote an ack
  // URL to NVS before rebooting — single-shot, best-effort, silent if
  // nothing is pending. Runs before the web server comes up so a
  // transient POST stall (5s timeout) doesn't block command handling.
  TrellisOTA::sendPendingAck(_firmwareVersion);

  // Start mDNS
  _discovery = new TrellisDiscovery();
  _discovery->begin(_name, _port);

  // Start web server + WebSocket
  _webServer = new TrellisWebServer(this);
  _webServer->setWebUIEnabled(_webUIEnabled);
  _webServer->begin(_port);

  Serial.printf("[Trellis] %s ready at http://%s:%d\n",
    _name, WiFi.localIP().toString().c_str(), _port);
  if (_webUIEnabled) {
    Serial.printf("[Trellis] Open http://%s:%d/ in a browser to control this device\n",
      WiFi.localIP().toString().c_str(), _port);
  }

  return true;
}

bool Trellis::beginAutoConnect(unsigned long timeout_ms) {
  Serial.begin(115200);

  _provisioning = new TrellisProvisioning(_name);

  if (!_provisioning->autoConnect(timeout_ms)) {
    Serial.println("[Trellis] Auto-connect failed");
    return false;
  }

  // Two-phase OTA ack — see begin() for the rationale.
  TrellisOTA::sendPendingAck(_firmwareVersion);

  // Start mDNS
  _discovery = new TrellisDiscovery();
  _discovery->begin(_name, _port);

  // Start web server + WebSocket
  _webServer = new TrellisWebServer(this);
  _webServer->setWebUIEnabled(_webUIEnabled);
  _webServer->begin(_port);

  Serial.printf("[Trellis] %s ready at http://%s:%d\n",
    _name, WiFi.localIP().toString().c_str(), _port);
  if (_webUIEnabled) {
    Serial.printf("[Trellis] Open http://%s:%d/ in a browser to control this device\n",
      WiFi.localIP().toString().c_str(), _port);
  }

  return true;
}

void Trellis::enableWebUI(bool enabled) {
  _webUIEnabled = enabled;
  if (_webServer) {
    _webServer->setWebUIEnabled(enabled);
  }
}

void Trellis::loop() {
  // If begin() failed (WiFi timeout), _webServer was never instantiated.
  // Skip everything — there is nothing to service, no one to broadcast to,
  // and dereferencing the null _webServer in the broadcast block below would
  // panic with LoadProhibited.
  if (!_webServer) return;

  _webServer->loop();
  _telemetry.update();

  unsigned long now = millis();

  // Broadcast sensor values periodically
  if (now - _lastBroadcast >= BROADCAST_INTERVAL_MS) {
    _lastBroadcast = now;
    for (uint8_t i = 0; i < _capCount; i++) {
      Capability* cap = &_capabilities[i];
      switch (cap->type) {
        case CapabilityType::SENSOR:
          _webServer->broadcastUpdate(cap->id, cap->floatValue);
          break;
        case CapabilityType::SWITCH:
          _webServer->broadcastUpdate(cap->id, cap->boolValue);
          break;
        case CapabilityType::SLIDER:
          _webServer->broadcastUpdate(cap->id, cap->floatValue);
          break;
        case CapabilityType::COLOR:
        case CapabilityType::TEXT:
          _webServer->broadcastUpdate(cap->id, cap->stringValue);
          break;
      }
    }
  }

  // Broadcast heartbeat with system telemetry
  if (now - _lastHeartbeat >= HEARTBEAT_INTERVAL_MS) {
    _lastHeartbeat = now;
    _webServer->broadcastHeartbeat(_telemetry.getData());
  }
}

uint8_t Trellis::addCapability(const char* id, const char* label, CapabilityType type) {
  if (_capCount >= TRELLIS_MAX_CAPABILITIES) {
    Serial.println("[Trellis] Max capabilities reached");
    return 255;
  }
  uint8_t idx = _capCount++;
  _capabilities[idx].id = id;
  _capabilities[idx].label = label;
  _capabilities[idx].type = type;
  return idx;
}

void Trellis::addSwitch(const char* id, const char* label, int gpio) {
  uint8_t idx = addCapability(id, label, CapabilityType::SWITCH);
  if (idx == 255) return;
  _capabilities[idx].gpio = gpio;
  _capabilities[idx].boolValue = false;
  pinMode(gpio, OUTPUT);
  digitalWrite(gpio, LOW);

  // Restore last-known value from NVS so the switch resumes across reboots.
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis_cap", true);
  if (prefs.isKey(id)) {
    bool stored = prefs.getBool(id, false);
    _capabilities[idx].boolValue = stored;
    digitalWrite(gpio, stored ? HIGH : LOW);
    Serial.printf("[Trellis] Restored switch '%s' = %s from NVS\n", id, stored ? "ON" : "OFF");
  }
  prefs.end();
#endif
}

void Trellis::addSensor(const char* id, const char* label, const char* unit) {
  uint8_t idx = addCapability(id, label, CapabilityType::SENSOR);
  if (idx == 255) return;
  _capabilities[idx].unit = unit;
  _capabilities[idx].floatValue = 0.0f;
}

void Trellis::addSlider(const char* id, const char* label, float min, float max, int gpio) {
  uint8_t idx = addCapability(id, label, CapabilityType::SLIDER);
  if (idx == 255) return;
  _capabilities[idx].minValue = min;
  _capabilities[idx].maxValue = max;
  _capabilities[idx].gpio = gpio;
  _capabilities[idx].floatValue = min;
  pinMode(gpio, OUTPUT);

  // Restore last-known value from NVS so the slider resumes across reboots.
  // PWM is applied immediately so hardware state matches before the first
  // client connects and reads /api/info.
#if defined(ESP32)
  Preferences prefs;
  prefs.begin("trellis_cap", true);
  if (prefs.isKey(id)) {
    float stored = prefs.getFloat(id, min);
    _capabilities[idx].floatValue = stored;
    int pwmVal = map((long)(stored), (long)(min), (long)(max), 0, 255);
    analogWrite(gpio, pwmVal);
    Serial.printf("[Trellis] Restored slider '%s' = %.1f from NVS\n", id, stored);
  }
  prefs.end();
#endif
}

void Trellis::addColor(const char* id, const char* label) {
  uint8_t idx = addCapability(id, label, CapabilityType::COLOR);
  if (idx == 255) return;
  strncpy(_capabilities[idx].stringValue, "#000000", sizeof(_capabilities[idx].stringValue));
}

void Trellis::addText(const char* id, const char* label) {
  uint8_t idx = addCapability(id, label, CapabilityType::TEXT);
  if (idx == 255) return;
  _capabilities[idx].stringValue[0] = '\0';
}

void Trellis::setSensor(const char* id, float value) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::SENSOR) {
    cap->floatValue = value;
  }
}

void Trellis::setSwitch(const char* id, bool value) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::SWITCH) {
    cap->boolValue = value;
    digitalWrite(cap->gpio, value ? HIGH : LOW);
  }
}

void Trellis::setSlider(const char* id, float value) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::SLIDER) {
    cap->floatValue = value;
    int pwmVal = map((long)(value), (long)(cap->minValue), (long)(cap->maxValue), 0, 255);
    analogWrite(cap->gpio, pwmVal);
  }
}

void Trellis::setText(const char* id, const char* value) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::TEXT) {
    strncpy(cap->stringValue, value, sizeof(cap->stringValue) - 1);
    cap->stringValue[sizeof(cap->stringValue) - 1] = '\0';
  }
}

void Trellis::setColor(const char* id, const char* value) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::COLOR) {
    strncpy(cap->stringValue, value, sizeof(cap->stringValue) - 1);
    cap->stringValue[sizeof(cap->stringValue) - 1] = '\0';
  }
}

float Trellis::getSensor(const char* id) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::SENSOR) return cap->floatValue;
  return 0.0f;
}

bool Trellis::getSwitch(const char* id) {
  Capability* cap = findCapability(id);
  if (cap && cap->type == CapabilityType::SWITCH) return cap->boolValue;
  return false;
}

void Trellis::onCommand(CommandCallback callback) {
  _commandCallback = callback;
}

void Trellis::setFirmwareVersion(const char* version) {
  _firmwareVersion = version;
}

void Trellis::log(const char* severity, const char* message) {
  Serial.printf("[Trellis] [%s] %s\n", severity, message);
  if (_webServer) {
    _webServer->broadcastLog(severity, message);
  }
}

void Trellis::logInfo(const char* message)  { log("info", message); }
void Trellis::logWarn(const char* message)  { log("warn", message); }
void Trellis::logError(const char* message) { log("error", message); }

Capability* Trellis::findCapability(const char* id) {
  for (uint8_t i = 0; i < _capCount; i++) {
    if (strcmp(_capabilities[i].id, id) == 0) {
      return &_capabilities[i];
    }
  }
  return nullptr;
}
