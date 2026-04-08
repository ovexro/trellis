import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { Download, Upload, Check, Bell, Radio, Key, Copy, Trash2, AlertTriangle } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

type ApiToken = {
  id: number;
  name: string;
  created_at: string;
  last_used_at: string | null;
};

type CreatedApiToken = {
  id: number;
  name: string;
  token: string;
};

type MqttConfig = {
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
type MqttConfigPublic = Omit<MqttConfig, "password"> & { has_password: boolean };

type MqttStatus = {
  enabled: boolean;
  connected: boolean;
  last_error: string | null;
  messages_published: number;
  messages_received: number;
};

const DEFAULT_MQTT_CONFIG: MqttConfig = {
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

export default function Settings() {
  const { devices } = useDeviceStore();
  const [exportStatus, setExportStatus] = useState("");
  const [importStatus, setImportStatus] = useState("");

  // Scan interval state
  const [scanInterval, setScanInterval] = useState("30");

  // ntfy.sh push notification state
  const [ntfyTopic, setNtfyTopic] = useState("");
  const [ntfySavedTopic, setNtfySavedTopic] = useState<string | null>(null);
  const [ntfyStatus, setNtfyStatus] = useState("");

  // MQTT bridge state. `mqttConfig.password` is always empty on initial
  // load — the backend redacts it from get_mqtt_config to avoid leaking it
  // over the LAN-exposed REST API. `mqttHasPassword` reflects whether a
  // password is currently stored server-side so the input placeholder and
  // Clear button can render meaningfully. When the user saves without
  // re-typing, the backend interprets empty `password` as "preserve existing".
  const [mqttConfig, setMqttConfig] = useState<MqttConfig>(DEFAULT_MQTT_CONFIG);
  const [mqttHasPassword, setMqttHasPassword] = useState(false);
  const [mqttStatus, setMqttStatus] = useState<MqttStatus | null>(null);
  const [mqttFeedback, setMqttFeedback] = useState("");
  const [mqttBusy, setMqttBusy] = useState(false);

  // API tokens state. `apiTokens` is the list shown in the table;
  // `newTokenName` drives the create modal; `createdToken` holds the
  // freshly-minted plaintext that's surfaced exactly once. After dismiss,
  // the plaintext is gone forever — only the SHA-256 digest remains in
  // SQLite. `requireAuthLocalhost` mirrors the backend setting.
  const [apiTokens, setApiTokens] = useState<ApiToken[]>([]);
  const [newTokenName, setNewTokenName] = useState("");
  const [createdToken, setCreatedToken] = useState<CreatedApiToken | null>(null);
  const [tokenCopied, setTokenCopied] = useState(false);
  const [tokenFeedback, setTokenFeedback] = useState("");
  const [tokenBusy, setTokenBusy] = useState(false);
  const [requireAuthLocalhost, setRequireAuthLocalhost] = useState(false);

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "ntfy_topic" }).then((topic) => {
      if (topic) {
        setNtfyTopic(topic);
        setNtfySavedTopic(topic);
      }
    }).catch(() => {});

    invoke<string | null>("get_setting", { key: "scan_interval" }).then((val) => {
      if (val) setScanInterval(val);
    }).catch(() => {});

    invoke<MqttConfigPublic>("get_mqtt_config")
      .then((cfg) => {
        // The wire shape has no `password` field; merge into local state with
        // an empty password and remember whether one is stored.
        const { has_password, ...rest } = cfg;
        setMqttConfig({ ...DEFAULT_MQTT_CONFIG, ...rest, password: "" });
        setMqttHasPassword(has_password);
      })
      .catch(() => {});

    refreshApiTokens();
    invoke<string | null>("get_setting", { key: "require_auth_localhost" }).then((val) => {
      setRequireAuthLocalhost(val === "true" || val === "1");
    }).catch(() => {});

    refreshMqttStatus();
    const id = setInterval(refreshMqttStatus, 5000);
    return () => clearInterval(id);
  }, []);

  const refreshMqttStatus = async () => {
    try {
      const s = await invoke<MqttStatus>("get_mqtt_status");
      setMqttStatus(s);
    } catch {
      // Silent — bridge not initialized yet during early startup
    }
  };

  const saveMqttConfig = async () => {
    setMqttBusy(true);
    setMqttFeedback("");
    try {
      // Empty password in the request body means "preserve existing" on the
      // backend. After save, refresh has_password from the new config so the
      // placeholder reflects the post-save state.
      const newStatus = await invoke<MqttStatus>("set_mqtt_config", { config: mqttConfig });
      setMqttStatus(newStatus);
      // If the user typed a new password, the backend now has it stored.
      // If they didn't type anything, has_password is unchanged from before.
      if (mqttConfig.password.length > 0) {
        setMqttHasPassword(true);
        setMqttConfig({ ...mqttConfig, password: "" });
      }
      setMqttFeedback(
        mqttConfig.enabled
          ? "Settings saved — bridge starting"
          : "Settings saved — bridge stopped"
      );
      setTimeout(() => setMqttFeedback(""), 4000);
    } catch (err) {
      setMqttFeedback(`Save failed: ${err}`);
    } finally {
      setMqttBusy(false);
    }
  };

  const testMqttConnection = async () => {
    setMqttBusy(true);
    setMqttFeedback("Testing connection…");
    try {
      await invoke("test_mqtt_connection", { config: mqttConfig });
      setMqttFeedback("Connection succeeded");
      setTimeout(() => setMqttFeedback(""), 4000);
    } catch (err) {
      setMqttFeedback(`Connection failed: ${err}`);
    } finally {
      setMqttBusy(false);
    }
  };

  const clearMqttPassword = async () => {
    if (!confirm("Clear the stored MQTT broker password? The bridge will restart with no auth.")) return;
    setMqttBusy(true);
    setMqttFeedback("");
    try {
      const newStatus = await invoke<MqttStatus>("clear_mqtt_password");
      setMqttStatus(newStatus);
      setMqttHasPassword(false);
      setMqttConfig({ ...mqttConfig, password: "" });
      setMqttFeedback("Password cleared");
      setTimeout(() => setMqttFeedback(""), 4000);
    } catch (err) {
      setMqttFeedback(`Clear failed: ${err}`);
    } finally {
      setMqttBusy(false);
    }
  };

  const refreshApiTokens = async () => {
    try {
      const tokens = await invoke<ApiToken[]>("list_api_tokens");
      setApiTokens(tokens);
    } catch (err) {
      console.error("Failed to load API tokens:", err);
    }
  };

  const createApiToken = async () => {
    const name = newTokenName.trim();
    if (!name) {
      setTokenFeedback("Token name is required");
      setTimeout(() => setTokenFeedback(""), 3000);
      return;
    }
    setTokenBusy(true);
    setTokenFeedback("");
    try {
      const created = await invoke<CreatedApiToken>("create_api_token", { name });
      // The plaintext is in `created.token` — show it once. After the user
      // closes the modal we drop it from state and only the digest remains
      // in SQLite. There is no way to recover it, by design.
      setCreatedToken(created);
      setNewTokenName("");
      setTokenCopied(false);
      await refreshApiTokens();
    } catch (err) {
      setTokenFeedback(`Create failed: ${err}`);
    } finally {
      setTokenBusy(false);
    }
  };

  const copyCreatedToken = async () => {
    if (!createdToken) return;
    try {
      await navigator.clipboard.writeText(createdToken.token);
      setTokenCopied(true);
      setTimeout(() => setTokenCopied(false), 2000);
    } catch (err) {
      setTokenFeedback(`Copy failed: ${err}`);
    }
  };

  const dismissCreatedToken = () => {
    setCreatedToken(null);
    setTokenCopied(false);
  };

  const revokeApiToken = async (id: number, name: string) => {
    if (!confirm(`Revoke API token "${name}"? Any client using it will immediately get 401.`)) return;
    try {
      await invoke("revoke_api_token", { id });
      await refreshApiTokens();
      setTokenFeedback("Token revoked");
      setTimeout(() => setTokenFeedback(""), 3000);
    } catch (err) {
      setTokenFeedback(`Revoke failed: ${err}`);
    }
  };

  const toggleRequireAuthLocalhost = async (val: boolean) => {
    setRequireAuthLocalhost(val);
    try {
      await invoke("set_setting", { key: "require_auth_localhost", value: val ? "true" : "false" });
    } catch (err) {
      console.error("Failed to save require_auth_localhost:", err);
      setRequireAuthLocalhost(!val); // revert on failure
    }
  };

  const formatTimestamp = (ts: string | null) => {
    if (!ts) return "Never";
    try {
      // SQLite returns "2026-04-08 12:34:56" — append Z to parse as UTC
      return new Date(ts.replace(" ", "T") + "Z").toLocaleString();
    } catch {
      return ts;
    }
  };

  const saveNtfyTopic = async () => {
    const trimmed = ntfyTopic.trim();
    if (!trimmed) {
      setNtfyStatus("Topic name cannot be empty");
      setTimeout(() => setNtfyStatus(""), 3000);
      return;
    }
    try {
      await invoke("set_setting", { key: "ntfy_topic", value: trimmed });
      setNtfySavedTopic(trimmed);
      setNtfyStatus("Topic saved — push notifications enabled");
      setTimeout(() => setNtfyStatus(""), 3000);
    } catch (err) {
      setNtfyStatus(`Failed to save: ${err}`);
    }
  };

  const testNtfy = async () => {
    const topic = ntfySavedTopic || ntfyTopic.trim();
    if (!topic) {
      setNtfyStatus("Save a topic first");
      setTimeout(() => setNtfyStatus(""), 3000);
      return;
    }
    try {
      await invoke("test_ntfy", { topic });
      setNtfyStatus("Test notification sent — check your phone");
      setTimeout(() => setNtfyStatus(""), 5000);
    } catch (err) {
      setNtfyStatus(`Test failed: ${err}`);
    }
  };

  const clearNtfyTopic = async () => {
    try {
      await invoke("delete_setting", { key: "ntfy_topic" });
      setNtfyTopic("");
      setNtfySavedTopic(null);
      setNtfyStatus("Push notifications disabled");
      setTimeout(() => setNtfyStatus(""), 3000);
    } catch (err) {
      setNtfyStatus(`Failed to clear: ${err}`);
    }
  };

  const exportConfig = async () => {
    try {
      const [savedDevices, schedules, rules, webhooks, templates, groups] = await Promise.all([
        invoke("get_saved_devices"),
        invoke("get_schedules"),
        invoke("get_rules"),
        invoke("get_webhooks"),
        invoke("get_templates"),
        invoke("get_groups"),
      ]);

      // Collect alerts for all known devices
      const allAlerts: unknown[] = [];
      for (const d of devices) {
        try {
          const alerts = await invoke("get_alerts", { deviceId: d.id });
          if (Array.isArray(alerts)) allAlerts.push(...alerts);
        } catch {
          // Device may not have alerts
        }
      }

      const scenes = localStorage.getItem("trellis-scenes");

      const config = {
        version: "0.1.5",
        exported_at: new Date().toISOString(),
        devices: savedDevices,
        scenes: scenes ? JSON.parse(scenes) : [],
        schedules,
        rules,
        webhooks,
        alerts: allAlerts,
        templates,
        groups,
        device_count: devices.length,
      };

      const filePath = await save({
        defaultPath: "trellis-config.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });

      if (filePath) {
        const { writeTextFile } = await import("@tauri-apps/plugin-fs");
        await writeTextFile(filePath, JSON.stringify(config, null, 2));
        setExportStatus("Configuration exported successfully");
        setTimeout(() => setExportStatus(""), 3000);
      }
    } catch (err) {
      setExportStatus(`Export failed: ${err}`);
    }
  };

  const importConfig = async () => {
    try {
      const filePath = await openDialog({
        multiple: false,
        filters: [{ name: "JSON", extensions: ["json"] }],
      });

      if (filePath) {
        const { readTextFile } = await import("@tauri-apps/plugin-fs");
        const content = await readTextFile(filePath);
        const config = JSON.parse(content);

        let imported = [];

        if (config.scenes) {
          localStorage.setItem("trellis-scenes", JSON.stringify(config.scenes));
          imported.push(`${config.scenes.length} scenes`);
        }

        // Restore groups (must be before devices so group_id references are valid)
        const groupIdMap = new Map<number, number>(); // old id → new id
        if (config.groups && Array.isArray(config.groups)) {
          for (const g of config.groups) {
            try {
              const newId = await invoke<number>("create_group", {
                name: g.name, color: g.color || "#6366f1",
              });
              groupIdMap.set(g.id, newId);
            } catch (err) {
              console.error("Failed to import group:", err);
            }
          }
          imported.push(`${config.groups.length} groups`);
        }

        // Restore saved devices (nicknames, tags, group assignment)
        if (config.devices && Array.isArray(config.devices)) {
          for (const dev of config.devices) {
            if (dev.nickname) {
              await invoke("set_device_nickname", { deviceId: dev.id, nickname: dev.nickname });
            }
            if (dev.tags) {
              await invoke("set_device_tags", { deviceId: dev.id, tags: dev.tags });
            }
            if (dev.group_id != null && groupIdMap.has(dev.group_id)) {
              await invoke("set_device_group", { deviceId: dev.id, groupId: groupIdMap.get(dev.group_id) });
            }
          }
          imported.push(`${config.devices.length} devices`);
        }

        // Restore schedules
        if (config.schedules && Array.isArray(config.schedules)) {
          for (const s of config.schedules) {
            try {
              await invoke("create_schedule", {
                deviceId: s.device_id, capabilityId: s.capability_id,
                value: s.value, cron: s.cron, label: s.label,
              });
            } catch (err) {
              console.error("Failed to import schedule:", err);
            }
          }
          imported.push(`${config.schedules.length} schedules`);
        }

        // Restore rules
        if (config.rules && Array.isArray(config.rules)) {
          for (const r of config.rules) {
            try {
              await invoke("create_rule", {
                sourceDeviceId: r.source_device_id, sourceMetricId: r.source_metric_id,
                condition: r.condition, threshold: r.threshold,
                targetDeviceId: r.target_device_id, targetCapabilityId: r.target_capability_id,
                targetValue: r.target_value, label: r.label,
              });
            } catch (err) {
              console.error("Failed to import rule:", err);
            }
          }
          imported.push(`${config.rules.length} rules`);
        }

        // Restore webhooks
        if (config.webhooks && Array.isArray(config.webhooks)) {
          for (const w of config.webhooks) {
            try {
              await invoke("create_webhook", {
                eventType: w.event_type, deviceId: w.device_id || null,
                url: w.url, label: w.label,
              });
            } catch (err) {
              console.error("Failed to import webhook:", err);
            }
          }
          imported.push(`${config.webhooks.length} webhooks`);
        }

        // Restore alerts
        if (config.alerts && Array.isArray(config.alerts)) {
          for (const a of config.alerts) {
            try {
              await invoke("create_alert", {
                deviceId: a.device_id, metricId: a.metric_id,
                condition: a.condition, threshold: a.threshold, label: a.label,
              });
            } catch (err) {
              console.error("Failed to import alert:", err);
            }
          }
          imported.push(`${config.alerts.length} alerts`);
        }

        // Restore templates
        if (config.templates && Array.isArray(config.templates)) {
          for (const t of config.templates) {
            try {
              await invoke("create_template", {
                name: t.name, description: t.description, capabilities: t.capabilities,
              });
            } catch (err) {
              console.error("Failed to import template:", err);
            }
          }
          imported.push(`${config.templates.length} templates`);
        }

        setImportStatus(`Imported: ${imported.join(", ")}`);
        setTimeout(() => setImportStatus(""), 5000);
      }
    } catch (err) {
      setImportStatus(`Import failed: ${err}`);
    }
  };

  return (
    <div>
      <h1 className="text-xl font-bold text-zinc-100 mb-6">Settings</h1>

      <div className="space-y-8">
        {/* Import/Export */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            Configuration
          </h2>
          <div className="flex gap-3">
            <button
              onClick={exportConfig}
              className="flex items-center gap-2 px-4 py-2.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
            >
              <Download size={16} />
              Export Config
            </button>
            <button
              onClick={importConfig}
              className="flex items-center gap-2 px-4 py-2.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
            >
              <Upload size={16} />
              Import Config
            </button>
          </div>
          {exportStatus && (
            <p className={`text-xs mt-2 flex items-center gap-1 ${exportStatus.startsWith("Export failed") ? "text-red-400" : "text-trellis-400"}`}>
              <Check size={12} /> {exportStatus}
            </p>
          )}
          {importStatus && (
            <p className={`text-xs mt-2 flex items-center gap-1 ${importStatus.startsWith("Import failed") ? "text-red-400" : "text-trellis-400"}`}>
              <Check size={12} /> {importStatus}
            </p>
          )}
          <p className="text-xs text-zinc-600 mt-2">
            Export saves device nicknames, tags, scenes, schedules, rules, webhooks, alerts, and templates.
            Import on a new PC to restore your setup.
          </p>
        </div>

        {/* Discovery */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            Discovery
          </h2>
          <div className="flex items-center gap-3">
            <label className="text-sm text-zinc-300">Health check interval</label>
            <select
              value={scanInterval}
              onChange={async (e) => {
                const val = e.target.value;
                setScanInterval(val);
                try {
                  await invoke("set_setting", { key: "scan_interval", value: val });
                } catch (err) {
                  console.error("Failed to save scan interval:", err);
                }
              }}
              className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300"
            >
              <option value="10">10 seconds</option>
              <option value="30">30 seconds (default)</option>
              <option value="60">1 minute</option>
              <option value="120">2 minutes</option>
            </select>
          </div>
          <p className="text-xs text-zinc-600 mt-2">
            How often Trellis checks if devices are still online. Lower values detect changes faster but use more network traffic.
          </p>
        </div>

        {/* Push Notifications */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            Push Notifications
          </h2>
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-sm text-zinc-300">
              <Bell size={16} className={ntfySavedTopic ? "text-trellis-400" : "text-zinc-500"} />
              {ntfySavedTopic ? (
                <span>Enabled — sending to <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-trellis-400 text-xs">{ntfySavedTopic}</code></span>
              ) : (
                <span className="text-zinc-500">Disabled — no topic configured</span>
              )}
            </div>
            <div className="flex gap-2">
              <input
                type="text"
                value={ntfyTopic}
                onChange={(e) => setNtfyTopic(e.target.value)}
                placeholder="Enter ntfy topic name"
                className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500"
              />
              <button
                onClick={saveNtfyTopic}
                className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 text-white rounded-lg text-sm transition-colors"
              >
                Save
              </button>
            </div>
            <div className="flex gap-2">
              <button
                onClick={testNtfy}
                disabled={!ntfySavedTopic}
                className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Test
              </button>
              <button
                onClick={clearNtfyTopic}
                disabled={!ntfySavedTopic}
                className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
              >
                Clear
              </button>
            </div>
            {ntfyStatus && (
              <p className={`text-xs flex items-center gap-1 ${ntfyStatus.includes("failed") || ntfyStatus.includes("Failed") || ntfyStatus.includes("cannot") ? "text-red-400" : "text-trellis-400"}`}>
                <Check size={12} /> {ntfyStatus}
              </p>
            )}
            <p className="text-xs text-zinc-600">
              Install the ntfy app on your phone, subscribe to your topic name, and Trellis will send push alerts when sensors trigger alerts or devices go offline.
            </p>
          </div>
        </div>

        {/* MQTT bridge */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            MQTT bridge
          </h2>
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-sm text-zinc-300">
              <Radio
                size={16}
                className={
                  mqttStatus?.connected
                    ? "text-trellis-400"
                    : mqttStatus?.enabled
                    ? "text-amber-400"
                    : "text-zinc-500"
                }
              />
              {mqttStatus?.connected ? (
                <span>
                  Connected to <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-trellis-400 text-xs">{mqttConfig.broker_host}:{mqttConfig.broker_port}</code>
                  {" — "}
                  <span className="text-zinc-500">
                    {mqttStatus.messages_published} pub / {mqttStatus.messages_received} sub
                  </span>
                </span>
              ) : mqttStatus?.enabled ? (
                <span className="text-amber-400">Enabled but not connected{mqttStatus.last_error ? ` — ${mqttStatus.last_error}` : ""}</span>
              ) : (
                <span className="text-zinc-500">Disabled</span>
              )}
            </div>

            <label className="flex items-center gap-2 text-sm text-zinc-300">
              <input
                type="checkbox"
                checked={mqttConfig.enabled}
                onChange={(e) => setMqttConfig({ ...mqttConfig, enabled: e.target.checked })}
                className="rounded border-zinc-700 bg-zinc-800"
              />
              Enable MQTT bridge
            </label>

            <div className="grid grid-cols-2 gap-2">
              <div>
                <label className="text-xs text-zinc-500 block mb-1">Broker host</label>
                <input
                  type="text"
                  value={mqttConfig.broker_host}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, broker_host: e.target.value })}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                  placeholder="localhost"
                />
              </div>
              <div>
                <label className="text-xs text-zinc-500 block mb-1">Port</label>
                <input
                  type="number"
                  value={mqttConfig.broker_port}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, broker_port: parseInt(e.target.value) || 1883 })}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                  placeholder="1883"
                />
              </div>
              <div>
                <label className="text-xs text-zinc-500 block mb-1">Username (optional)</label>
                <input
                  type="text"
                  value={mqttConfig.username}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, username: e.target.value })}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                  placeholder="(none)"
                />
              </div>
              <div>
                <label className="text-xs text-zinc-500 block mb-1">
                  Password (optional)
                  {mqttHasPassword && (
                    <span className="ml-2 text-trellis-400">• stored</span>
                  )}
                </label>
                <div className="flex gap-1">
                  <input
                    type="password"
                    value={mqttConfig.password}
                    onChange={(e) => setMqttConfig({ ...mqttConfig, password: e.target.value })}
                    className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                    placeholder={mqttHasPassword ? "(unchanged — type to update)" : "(none)"}
                  />
                  {mqttHasPassword && (
                    <button
                      type="button"
                      onClick={clearMqttPassword}
                      disabled={mqttBusy}
                      className="px-2 py-2 bg-zinc-800 hover:bg-red-900/40 text-zinc-400 hover:text-red-300 rounded-lg text-xs transition-colors disabled:opacity-40"
                      title="Clear stored password"
                    >
                      Clear
                    </button>
                  )}
                </div>
              </div>
              <div>
                <label className="text-xs text-zinc-500 block mb-1">Base topic</label>
                <input
                  type="text"
                  value={mqttConfig.base_topic}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, base_topic: e.target.value })}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                  placeholder="trellis"
                />
              </div>
              <div>
                <label className="text-xs text-zinc-500 block mb-1">HA discovery prefix</label>
                <input
                  type="text"
                  value={mqttConfig.ha_discovery_prefix}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, ha_discovery_prefix: e.target.value })}
                  className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
                  placeholder="homeassistant"
                />
              </div>
            </div>

            {/* TLS subsection. Collapsed unless enabled. Toggling enables
                TLS and (if the port is still the plaintext default) auto-
                suggests 8883. */}
            <div className="border border-zinc-800 rounded-lg p-3 space-y-2">
              <label className="flex items-center gap-2 text-sm text-zinc-300">
                <input
                  type="checkbox"
                  checked={mqttConfig.tls_enabled}
                  onChange={(e) => {
                    const next = { ...mqttConfig, tls_enabled: e.target.checked };
                    // UI nicety: when the user enables TLS and the port is
                    // still on the plaintext default (1883), bump it to the
                    // standard MQTTS port (8883). The user can override
                    // afterwards. We don't auto-revert when disabling TLS —
                    // they may have a non-standard port intentionally.
                    if (e.target.checked && mqttConfig.broker_port === 1883) {
                      next.broker_port = 8883;
                    }
                    setMqttConfig(next);
                  }}
                  className="rounded border-zinc-700 bg-zinc-800"
                />
                Use TLS (mqtts://)
              </label>
              {mqttConfig.tls_enabled && (
                <div>
                  <label className="text-xs text-zinc-500 block mb-1">
                    CA certificate (PEM, optional)
                  </label>
                  <div className="flex gap-1">
                    <input
                      type="text"
                      value={mqttConfig.tls_ca_cert_path || ""}
                      onChange={(e) => setMqttConfig({
                        ...mqttConfig,
                        tls_ca_cert_path: e.target.value || null,
                      })}
                      className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500 font-mono"
                      placeholder="(blank — use system trust roots)"
                    />
                    <button
                      type="button"
                      onClick={async () => {
                        try {
                          const path = await openDialog({
                            multiple: false,
                            filters: [
                              { name: "PEM certificate", extensions: ["pem", "crt", "cer"] },
                              { name: "All files", extensions: ["*"] },
                            ],
                          });
                          if (typeof path === "string") {
                            setMqttConfig({ ...mqttConfig, tls_ca_cert_path: path });
                          }
                        } catch (err) {
                          console.error("CA file picker failed:", err);
                        }
                      }}
                      className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
                    >
                      Browse…
                    </button>
                    {mqttConfig.tls_ca_cert_path && (
                      <button
                        type="button"
                        onClick={() => setMqttConfig({ ...mqttConfig, tls_ca_cert_path: null })}
                        className="px-2 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 hover:text-zinc-200 rounded-lg text-xs transition-colors"
                        title="Clear CA cert path"
                      >
                        Clear
                      </button>
                    )}
                  </div>
                  <p className="text-xs text-zinc-600 mt-1">
                    Leave blank for public brokers (uses your OS trust store, same as your browser).
                    For self-signed brokers, point this at the broker's certificate file or its CA.
                  </p>
                </div>
              )}
            </div>

            <label className="flex items-center gap-2 text-sm text-zinc-300">
              <input
                type="checkbox"
                checked={mqttConfig.ha_discovery_enabled}
                onChange={(e) => setMqttConfig({ ...mqttConfig, ha_discovery_enabled: e.target.checked })}
                className="rounded border-zinc-700 bg-zinc-800"
              />
              Publish Home Assistant discovery configs
            </label>

            <div className="flex gap-2">
              <button
                onClick={saveMqttConfig}
                disabled={mqttBusy}
                className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 disabled:bg-zinc-700 text-white rounded-lg text-sm transition-colors"
              >
                Save & apply
              </button>
              <button
                onClick={testMqttConnection}
                disabled={mqttBusy}
                className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 disabled:bg-zinc-900 text-zinc-300 rounded-lg text-sm transition-colors"
              >
                Test connection
              </button>
            </div>

            {mqttFeedback && (
              <p className={`text-xs ${mqttFeedback.includes("failed") ? "text-red-400" : "text-trellis-400"}`}>
                {mqttFeedback}
              </p>
            )}

            <p className="text-xs text-zinc-600">
              Bridges your Trellis devices to an MQTT broker so Home Assistant, Node-RED, and other tools can read sensor values and send commands.
              When HA discovery is on, devices auto-appear as entities — no YAML needed.
            </p>
          </div>
        </div>

        {/* API Tokens */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            API Tokens
          </h2>
          <div className="space-y-3">
            <div className="flex items-center gap-2 text-sm text-zinc-300">
              <Key
                size={16}
                className={apiTokens.length > 0 ? "text-trellis-400" : "text-zinc-500"}
              />
              {apiTokens.length === 0 ? (
                <span className="text-zinc-500">
                  No tokens — REST API on <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-xs">:9090</code> rejects all non-loopback requests
                </span>
              ) : (
                <span>
                  {apiTokens.length} token{apiTokens.length === 1 ? "" : "s"} active —
                  <span className="text-zinc-500"> any non-loopback request must include <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-xs">Authorization: Bearer trls_…</code></span>
                </span>
              )}
            </div>

            {/* Token list */}
            {apiTokens.length > 0 && (
              <div className="border border-zinc-800 rounded-lg overflow-hidden">
                <table className="w-full text-sm">
                  <thead className="bg-zinc-900/40 text-zinc-500">
                    <tr>
                      <th className="text-left font-normal px-3 py-2">Name</th>
                      <th className="text-left font-normal px-3 py-2">Created</th>
                      <th className="text-left font-normal px-3 py-2">Last used</th>
                      <th className="px-3 py-2"></th>
                    </tr>
                  </thead>
                  <tbody>
                    {apiTokens.map((t) => (
                      <tr key={t.id} className="border-t border-zinc-800">
                        <td className="px-3 py-2 text-zinc-200">{t.name}</td>
                        <td className="px-3 py-2 text-zinc-500 text-xs">{formatTimestamp(t.created_at)}</td>
                        <td className="px-3 py-2 text-zinc-500 text-xs">{formatTimestamp(t.last_used_at)}</td>
                        <td className="px-3 py-2 text-right">
                          <button
                            onClick={() => revokeApiToken(t.id, t.name)}
                            className="p-1.5 text-zinc-500 hover:text-red-400 hover:bg-red-500/10 rounded transition-colors"
                            title="Revoke token"
                          >
                            <Trash2 size={14} />
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}

            {/* Create form */}
            <div className="flex gap-2">
              <input
                type="text"
                value={newTokenName}
                onChange={(e) => setNewTokenName(e.target.value)}
                placeholder="Token name (e.g. homeassistant, phone, ci)"
                className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500"
                onKeyDown={(e) => { if (e.key === "Enter") createApiToken(); }}
                disabled={tokenBusy}
              />
              <button
                onClick={createApiToken}
                disabled={tokenBusy || !newTokenName.trim()}
                className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 disabled:bg-zinc-700 disabled:opacity-50 text-white rounded-lg text-sm transition-colors"
              >
                Create token
              </button>
            </div>

            {tokenFeedback && (
              <p className={`text-xs ${tokenFeedback.toLowerCase().includes("fail") ? "text-red-400" : "text-trellis-400"}`}>
                {tokenFeedback}
              </p>
            )}

            {/* Strict-loopback toggle */}
            <label className="flex items-start gap-2 text-sm text-zinc-300 pt-2">
              <input
                type="checkbox"
                checked={requireAuthLocalhost}
                onChange={(e) => toggleRequireAuthLocalhost(e.target.checked)}
                className="mt-0.5 rounded border-zinc-700 bg-zinc-800"
              />
              <span>
                Require token even for localhost requests
                <span className="block text-xs text-zinc-600 mt-0.5">
                  Default off — the desktop app's embedded dashboard talks to the API over loopback and skipping auth there keeps it friction-free.
                  Turn on for defense in depth against malicious local processes.
                </span>
              </span>
            </label>

            <p className="text-xs text-zinc-600">
              Tokens gate the REST API on port 9090. Loopback requests are allowed without a token by default; every other source IP must
              present a valid Bearer token. Tokens are shown exactly once at creation — only the SHA-256 digest is stored, so a stolen
              database can't be used to authenticate.
            </p>
          </div>
        </div>

        {/* Created-token modal — surfaces the plaintext exactly once */}
        {createdToken && (
          <div
            className="fixed inset-0 bg-black/70 backdrop-blur-sm flex items-center justify-center z-50 p-4"
            onClick={dismissCreatedToken}
          >
            <div
              className="bg-zinc-900 border border-zinc-700 rounded-xl max-w-lg w-full p-6 space-y-4"
              onClick={(e) => e.stopPropagation()}
            >
              <div className="flex items-start gap-3">
                <div className="p-2 bg-amber-500/10 border border-amber-500/30 rounded-lg">
                  <AlertTriangle size={20} className="text-amber-400" />
                </div>
                <div className="flex-1">
                  <h3 className="text-lg font-semibold text-zinc-100">Token created — copy it now</h3>
                  <p className="text-sm text-zinc-400 mt-1">
                    This is the only time the token will be shown. After you close this dialog,
                    only the digest is kept in the database — there is no way to recover the
                    plaintext.
                  </p>
                </div>
              </div>

              <div>
                <label className="text-xs text-zinc-500 block mb-1">{createdToken.name}</label>
                <div className="flex gap-2">
                  <code className="flex-1 px-3 py-2 bg-zinc-950 border border-zinc-700 rounded-lg text-sm text-amber-300 font-mono break-all">
                    {createdToken.token}
                  </code>
                  <button
                    onClick={copyCreatedToken}
                    className="px-3 py-2 bg-trellis-600 hover:bg-trellis-500 text-white rounded-lg text-sm transition-colors flex items-center gap-1"
                  >
                    {tokenCopied ? <Check size={14} /> : <Copy size={14} />}
                    {tokenCopied ? "Copied" : "Copy"}
                  </button>
                </div>
              </div>

              <div className="text-xs text-zinc-500 bg-zinc-800/40 border border-zinc-800 rounded-lg p-3 font-mono break-all">
                curl -H "Authorization: Bearer {createdToken.token}" \<br/>
                &nbsp;&nbsp;http://&lt;host&gt;:9090/api/devices
              </div>

              <div className="flex justify-end">
                <button
                  onClick={dismissCreatedToken}
                  className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
                >
                  I've saved it
                </button>
              </div>
            </div>
          </div>
        )}

        {/* Diagnostics */}
        <div>
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            Diagnostics
          </h2>
          <div className="space-y-2">
            {devices.filter((d) => d.online).map((device) => {
              const warnings: string[] = [];

              if (device.system.rssi < -80) {
                warnings.push("Weak WiFi signal — consider moving the device closer to the router");
              }
              if (device.system.heap_free < 20000) {
                warnings.push("Low free heap — possible memory leak");
              }
              if (device.system.heap_free < 10000) {
                warnings.push("Critical: heap nearly exhausted — device may crash");
              }

              if (warnings.length === 0) return null;

              return (
                <div key={device.id} className="p-3 bg-amber-500/5 border border-amber-500/20 rounded-lg">
                  <p className="text-sm font-medium text-amber-400 mb-1">{device.name}</p>
                  {warnings.map((w, i) => (
                    <p key={i} className="text-xs text-amber-300/70">• {w}</p>
                  ))}
                </div>
              );
            }).filter(Boolean)}

            {devices.filter((d) => d.online).every(
              (d) => d.system.rssi >= -80 && d.system.heap_free >= 20000,
            ) && (
              <p className="text-sm text-trellis-400 flex items-center gap-2">
                <Check size={14} />
                All devices healthy — no issues detected
              </p>
            )}
          </div>
        </div>

        {/* About */}
        <div className="pt-6 border-t border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            About
          </h2>
          <div className="text-sm text-zinc-500 space-y-1">
            <p>Trellis v0.2.0</p>
            <p>The easiest way to deploy and control ESP32 and Pico W devices.</p>
            <p className="pt-2">
              <a href="https://github.com/ovexro/trellis" target="_blank" rel="noopener noreferrer" className="text-trellis-400 hover:text-trellis-300">
                GitHub
              </a>
              {" · "}
              <a href="https://www.paypal.com/paypalme/ovexro" target="_blank" rel="noopener noreferrer" className="text-trellis-400 hover:text-trellis-300">
                Donate
              </a>
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
