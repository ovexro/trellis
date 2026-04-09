import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import { Radio } from "lucide-react";
import type { MqttConfig, MqttConfigPublic, MqttStatus } from "./types";
import { DEFAULT_MQTT_CONFIG } from "./types";

export default function MqttSection() {
  // `mqttConfig.password` is always empty on initial load — the backend
  // redacts it from get_mqtt_config to avoid leaking it over the LAN-exposed
  // REST API. `mqttHasPassword` reflects whether a password is currently
  // stored server-side so the input placeholder and Clear button can render
  // meaningfully. When the user saves without re-typing, the backend
  // interprets empty `password` as "preserve existing".
  const [mqttConfig, setMqttConfig] = useState<MqttConfig>(DEFAULT_MQTT_CONFIG);
  const [mqttHasPassword, setMqttHasPassword] = useState(false);
  const [mqttStatus, setMqttStatus] = useState<MqttStatus | null>(null);
  const [mqttFeedback, setMqttFeedback] = useState("");
  const [mqttBusy, setMqttBusy] = useState(false);

  useEffect(() => {
    invoke<MqttConfigPublic>("get_mqtt_config")
      .then((cfg) => {
        const { has_password, ...rest } = cfg;
        setMqttConfig({ ...DEFAULT_MQTT_CONFIG, ...rest, password: "" });
        setMqttHasPassword(has_password);
      })
      .catch(() => {});

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
      const newStatus = await invoke<MqttStatus>("set_mqtt_config", { config: mqttConfig });
      setMqttStatus(newStatus);
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

  return (
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

        {/* TLS subsection */}
        <div className="border border-zinc-800 rounded-lg p-3 space-y-2">
          <label className="flex items-center gap-2 text-sm text-zinc-300">
            <input
              type="checkbox"
              checked={mqttConfig.tls_enabled}
              onChange={(e) => {
                const next = { ...mqttConfig, tls_enabled: e.target.checked };
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
          {mqttConfig.tls_enabled && (
            <div>
              <label className="flex items-center gap-2 text-sm text-zinc-300">
                <input
                  type="checkbox"
                  checked={mqttConfig.tls_skip_verify}
                  onChange={(e) => setMqttConfig({ ...mqttConfig, tls_skip_verify: e.target.checked })}
                  className="rounded border-zinc-700 bg-zinc-800"
                />
                Skip certificate verification
              </label>
              {mqttConfig.tls_skip_verify && (
                <div className="mt-2 px-3 py-2 bg-amber-900/30 border border-amber-700/50 rounded-lg">
                  <p className="text-xs text-amber-300">
                    Certificate verification is disabled. The connection is encrypted but the broker's identity is not checked — expired, self-signed, and wrong-hostname certificates will all be accepted. Only use this on a trusted network.
                  </p>
                </div>
              )}
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
  );
}
