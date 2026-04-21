#ifndef TRELLIS_H
#define TRELLIS_H

#include <Arduino.h>
#include <ArduinoJson.h>

#if defined(ESP32)
  #include <WiFi.h>
  #include <ESPmDNS.h>
  #include <WebServer.h>
  #include <Update.h>
  #define TRELLIS_PLATFORM "esp32"
#elif defined(ARDUINO_ARCH_RP2040)
  #include <WiFi.h>
  #include <LEAmDNS.h>
  #include <WebServer.h>
  #define TRELLIS_PLATFORM "pico"
#else
  #error "Trellis: Unsupported platform. Use ESP32 or Raspberry Pi Pico W."
#endif

#include "Capability.h"
#include "TrellisWebServer.h"
#include "TrellisDiscovery.h"
#include "TrellisTelemetry.h"
#include "TrellisOTA.h"
#include "TrellisProvisioning.h"

#define TRELLIS_MAX_CAPABILITIES 16
#define TRELLIS_DEFAULT_PORT 8080
#define TRELLIS_VERSION "0.15.0"

class Trellis {
public:
  Trellis(const char* name, uint16_t port = TRELLIS_DEFAULT_PORT);

  // WiFi — choose one:
  bool begin(const char* ssid, const char* password, unsigned long timeout_ms = 15000);
  bool beginAutoConnect(unsigned long timeout_ms = 15000); // Uses stored creds or starts provisioning AP

  // Main loop — call in loop()
  void loop();

  // Capability registration
  void addSwitch(const char* id, const char* label, int gpio);
  void addSensor(const char* id, const char* label, const char* unit);
  void addSlider(const char* id, const char* label, float min, float max, int gpio);
  void addColor(const char* id, const char* label);
  void addText(const char* id, const char* label);

  // Update values
  void setSensor(const char* id, float value);
  void setSwitch(const char* id, bool value);
  void setSlider(const char* id, float value);
  void setText(const char* id, const char* value);
  void setColor(const char* id, const char* value);

  // Read values
  float getSensor(const char* id);
  bool getSwitch(const char* id);

  // Custom command handler
  typedef void (*CommandCallback)(const char* id, JsonVariant value);
  void onCommand(CommandCallback callback);

  // Firmware version
  void setFirmwareVersion(const char* version);

  // Embedded web dashboard (default: enabled). Call before begin() to disable
  // the on-device control panel served at GET /. Saves ~13 KB of flash when off.
  void enableWebUI(bool enabled = true);

  // Logging — sent to desktop app via WebSocket
  void log(const char* severity, const char* message);
  void logInfo(const char* message);
  void logWarn(const char* message);
  void logError(const char* message);

  // Accessors for internal use
  const char* getName() const { return _name; }
  const char* getFirmwareVersion() const { return _firmwareVersion; }
  uint16_t getPort() const { return _port; }
  Capability* getCapabilities() { return _capabilities; }
  uint8_t getCapabilityCount() const { return _capCount; }
  Capability* findCapability(const char* id);
  CommandCallback getCommandCallback() const { return _commandCallback; }
  TrellisTelemetry& getTelemetry() { return _telemetry; }

private:
  const char* _name;
  const char* _firmwareVersion;
  uint16_t _port;

  Capability _capabilities[TRELLIS_MAX_CAPABILITIES];
  uint8_t _capCount;

  CommandCallback _commandCallback;

  TrellisWebServer* _webServer;
  TrellisDiscovery* _discovery;
  TrellisTelemetry _telemetry;
  TrellisProvisioning* _provisioning;

  bool _webUIEnabled;

  unsigned long _lastBroadcast;
  unsigned long _lastHeartbeat;
  unsigned long _lastIdleWarn;
  static const unsigned long BROADCAST_INTERVAL_MS = 5000;
  static const unsigned long HEARTBEAT_INTERVAL_MS = 10000;
  static const unsigned long IDLE_WARN_INTERVAL_MS = 30000;

  uint8_t addCapability(const char* id, const char* label, CapabilityType type);
};

#endif
