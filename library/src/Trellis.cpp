#include "Trellis.h"

Trellis::Trellis(const char* name, uint16_t port)
  : _name(name),
    _firmwareVersion("0.0.0"),
    _port(port),
    _capCount(0),
    _commandCallback(nullptr),
    _webServer(nullptr),
    _discovery(nullptr),
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

  // Start mDNS
  _discovery = new TrellisDiscovery();
  _discovery->begin(_name, _port);

  // Start web server + WebSocket
  _webServer = new TrellisWebServer(this);
  _webServer->begin(_port);

  Serial.printf("[Trellis] %s ready at http://%s:%d\n",
    _name, WiFi.localIP().toString().c_str(), _port);

  return true;
}

void Trellis::loop() {
  if (_webServer) {
    _webServer->loop();
  }
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
    if (_webServer) {
      _webServer->broadcastHeartbeat(_telemetry.getData());
    }
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

Capability* Trellis::findCapability(const char* id) {
  for (uint8_t i = 0; i < _capCount; i++) {
    if (strcmp(_capabilities[i].id, id) == 0) {
      return &_capabilities[i];
    }
  }
  return nullptr;
}
