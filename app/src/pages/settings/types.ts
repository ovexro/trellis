export type ApiToken = {
  id: number;
  name: string;
  created_at: string;
  last_used_at: string | null;
  expires_at: string | null;
};

export type CreatedApiToken = {
  id: number;
  name: string;
  token: string;
  expires_at: string | null;
};

// Result of a single reachability probe — mirrors the Rust struct in
// commands.rs. `category` is one of: "success", "auth_failed",
// "not_trellis", "tunnel_down", "unexpected", "network_error", "timeout".
export type RemoteProbeResult = {
  ok: boolean;
  status: number;
  latency_ms: number;
  category: string;
  message: string;
};

export type MqttConfig = {
  enabled: boolean;
  broker_host: string;
  broker_port: number;
  username: string;
  password: string;
  base_topic: string;
  ha_discovery_prefix: string;
  ha_discovery_enabled: boolean;
  client_id: string;
  tls_enabled: boolean;
  tls_ca_cert_path: string | null;
};

// Network-safe view returned by get_mqtt_config — the password field is
// omitted on the wire so it can't be leaked over the LAN-exposed REST API.
// `has_password` tells the UI whether a password is currently stored so the
// input placeholder can show "(unchanged — type to update)".
export type MqttConfigPublic = Omit<MqttConfig, "password"> & { has_password: boolean };

export type MqttStatus = {
  enabled: boolean;
  connected: boolean;
  last_error: string | null;
  messages_published: number;
  messages_received: number;
};

export const DEFAULT_MQTT_CONFIG: MqttConfig = {
  enabled: false,
  broker_host: "localhost",
  broker_port: 1883,
  username: "",
  password: "",
  base_topic: "trellis",
  ha_discovery_prefix: "homeassistant",
  ha_discovery_enabled: true,
  client_id: "trellis-bridge",
  tls_enabled: false,
  tls_ca_cert_path: null,
};

export function formatTimestamp(ts: string | null): string {
  if (!ts) return "Never";
  try {
    // SQLite returns "2026-04-08 12:34:56" — append Z to parse as UTC
    return new Date(ts.replace(" ", "T") + "Z").toLocaleString();
  } catch {
    return ts;
  }
}
