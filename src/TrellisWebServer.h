#ifndef TRELLIS_WEBSERVER_H
#define TRELLIS_WEBSERVER_H

#include <Arduino.h>
#include <WebServer.h>
#include <WebSocketsServer.h>
#include <ArduinoJson.h>
#include "TrellisTelemetry.h"

// Forward declaration
class Trellis;

class TrellisWebServer {
public:
  TrellisWebServer(Trellis* trellis);
  void begin(uint16_t port);
  void loop();
  void broadcastUpdate(const char* id, float value);
  void broadcastUpdate(const char* id, bool value);
  void broadcastUpdate(const char* id, const char* value);
  void broadcastHeartbeat(const TelemetryData& telemetry);
  void broadcastLog(const char* severity, const char* message);
  void setWebUIEnabled(bool enabled) { _webUIEnabled = enabled; }

private:
  Trellis* _trellis;
  WebServer* _http;
  WebSocketsServer* _ws;
  bool _webUIEnabled;

  void handleInfo();
  void handlePeers();
  void handleWebUI();
  void handleManifest();
  void handleServiceWorker();
  void handleIcon();
  void handleScenesGet();
  void handleScenesPost();
  void handleSceneRecall();
  void handleSceneDelete();
  void handleSchedulesGet();
  void handleSchedulesPost();
  void handleScheduleDelete();
  void handleWebSocket(uint8_t num, WStype_t type, uint8_t* payload, size_t length);
  void processCommand(uint8_t num, const char* json);
  String buildInfoJson();
  String buildPeersJson();
  String buildScenesJson();
  String buildSchedulesJson();
  // Read the request body — Arduino WebServer's plain() returns "" for empty
  // POSTs and the full payload otherwise. Centralised so error handling stays
  // consistent across the four POST routes that accept JSON bodies.
  String readJsonBody();
  void   sendJsonError(int code, const char* message);
};

#endif
