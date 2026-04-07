#ifndef TRELLIS_CAPABILITY_H
#define TRELLIS_CAPABILITY_H

#include <Arduino.h>

enum class CapabilityType {
  SWITCH,
  SENSOR,
  SLIDER,
  COLOR,
  TEXT,
};

struct Capability {
  const char* id;
  const char* label;
  CapabilityType type;

  // For switch
  int gpio;
  bool boolValue;

  // For sensor / slider
  float floatValue;
  float minValue;
  float maxValue;
  const char* unit;

  // For color / text
  char stringValue[32];
};

inline const char* capabilityTypeToString(CapabilityType type) {
  switch (type) {
    case CapabilityType::SWITCH: return "switch";
    case CapabilityType::SENSOR: return "sensor";
    case CapabilityType::SLIDER: return "slider";
    case CapabilityType::COLOR:  return "color";
    case CapabilityType::TEXT:   return "text";
    default: return "unknown";
  }
}

#endif
