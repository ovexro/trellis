#ifndef TRELLIS_OTA_H
#define TRELLIS_OTA_H

#include <Arduino.h>
#include <functional>

#if defined(ESP32)
  #include <HTTPUpdate.h>
  #include <WiFi.h>
#endif

class TrellisOTA {
public:
  /// Perform OTA update from the given URL.
  /// If `ackUrl` is non-null and non-empty, it is persisted to NVS before
  /// the caller reboots; the new firmware image's `sendPendingAck` call at
  /// boot reads it back and POSTs, closing the two-phase OTA loop
  /// (v0.16.0). Pass nullptr for rollback / legacy paths.
  /// If broadcaster is provided, it receives JSON strings for progress and
  /// delivery events to forward over WebSocket.
  /// On success, returns true WITHOUT rebooting — caller must send
  /// ota_delivered and call ESP.restart().
  static bool update(
    const char* url,
    const char* ackUrl = nullptr,
    std::function<void(const String&)> broadcaster = nullptr);

  /// Fire-and-forget boot-time hook. If NVS has a pending ack URL (set by a
  /// preceding `update()` call that succeeded before reboot), POST to it
  /// exactly once and clear the stored URL regardless of outcome — the
  /// desktop's ack handler is idempotent and "unknown nonce" means the
  /// row is gone anyway, so retrying across many boots would be noise.
  /// Safe to call any number of times; does nothing if no ack is pending.
  /// `firmwareVersion` is included in the POST body for observability.
  static void sendPendingAck(const char* firmwareVersion);
};

#endif
