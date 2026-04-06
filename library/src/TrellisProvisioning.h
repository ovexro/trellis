#ifndef TRELLIS_PROVISIONING_H
#define TRELLIS_PROVISIONING_H

#include <Arduino.h>
#include <WebServer.h>

#if defined(ESP32)
  #include <WiFi.h>
  #include <Preferences.h>
#elif defined(ARDUINO_ARCH_RP2040)
  #include <WiFi.h>
  #include <LittleFS.h>
#endif

class TrellisProvisioning {
public:
  TrellisProvisioning(const char* deviceName);

  /// Check if WiFi credentials are stored
  bool hasCredentials();

  /// Get stored SSID
  String getSSID();

  /// Get stored password
  String getPassword();

  /// Save credentials
  void saveCredentials(const String& ssid, const String& password);

  /// Clear stored credentials
  void clearCredentials();

  /// Start AP mode for provisioning. Returns true when credentials are received.
  /// Blocks until the user submits WiFi credentials via the captive portal.
  bool startProvisioningAP(uint16_t port = 80);

  /// Try to connect with stored credentials, fall back to provisioning AP
  bool autoConnect(unsigned long timeout_ms = 15000);

private:
  const char* _deviceName;
  WebServer* _server;

  void handleRoot();
  void handleConfigure();
  void handleScan();

  String _receivedSSID;
  String _receivedPassword;
  bool _configured;
};

#endif
