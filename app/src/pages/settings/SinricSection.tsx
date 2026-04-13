import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Mic } from "lucide-react";
import type { SinricConfig, SinricConfigPublic, SinricStatus, SinricDeviceMapping } from "./types";
import { DEFAULT_SINRIC_CONFIG } from "./types";
import type { Device } from "@/lib/types";

export default function SinricSection() {
  const [config, setConfig] = useState<SinricConfig>(DEFAULT_SINRIC_CONFIG);
  const [hasSecret, setHasSecret] = useState(false);
  const [status, setStatus] = useState<SinricStatus | null>(null);
  const [feedback, setFeedback] = useState("");
  const [busy, setBusy] = useState(false);
  const [devices, setDevices] = useState<Device[]>([]);

  useEffect(() => {
    invoke<SinricConfigPublic>("get_sinric_config")
      .then((cfg) => {
        const { has_secret, ...rest } = cfg;
        setConfig({ ...DEFAULT_SINRIC_CONFIG, ...rest, api_secret: "" });
        setHasSecret(has_secret);
      })
      .catch(() => {});

    invoke<Device[]>("get_devices")
      .then(setDevices)
      .catch(() => {});

    refreshStatus();
    const id = setInterval(refreshStatus, 5000);
    return () => clearInterval(id);
  }, []);

  const refreshStatus = async () => {
    try {
      const s = await invoke<SinricStatus>("get_sinric_status");
      setStatus(s);
    } catch {
      // Silent
    }
  };

  const save = async () => {
    setBusy(true);
    setFeedback("");
    try {
      const newStatus = await invoke<SinricStatus>("set_sinric_config", { config });
      setStatus(newStatus);
      if (config.api_secret.length > 0) {
        setHasSecret(true);
        setConfig({ ...config, api_secret: "" });
      }
      setFeedback(
        config.enabled
          ? "Settings saved — bridge starting"
          : "Settings saved — bridge stopped"
      );
      setTimeout(() => setFeedback(""), 4000);
    } catch (err) {
      setFeedback(`Save failed: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const testConnection = async () => {
    setBusy(true);
    setFeedback("Testing connection…");
    try {
      await invoke("test_sinric_connection", { config });
      setFeedback("Connection succeeded");
      setTimeout(() => setFeedback(""), 4000);
    } catch (err) {
      setFeedback(`Connection failed: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const clearSecret = async () => {
    if (!confirm("Clear the stored Sinric Pro secret? The bridge will restart without signing.")) return;
    setBusy(true);
    setFeedback("");
    try {
      const newStatus = await invoke<SinricStatus>("clear_sinric_secret");
      setStatus(newStatus);
      setHasSecret(false);
      setConfig({ ...config, api_secret: "" });
      setFeedback("Secret cleared");
      setTimeout(() => setFeedback(""), 4000);
    } catch (err) {
      setFeedback(`Clear failed: ${err}`);
    } finally {
      setBusy(false);
    }
  };

  const addMapping = () => {
    setConfig({
      ...config,
      device_mappings: [
        ...config.device_mappings,
        { sinric_device_id: "", trellis_device_id: "" },
      ],
    });
  };

  const updateMapping = (index: number, field: keyof SinricDeviceMapping, value: string) => {
    const updated = [...config.device_mappings];
    updated[index] = { ...updated[index], [field]: value };
    // Clear capability selection when device changes
    if (field === "trellis_device_id") {
      delete updated[index].trellis_capability_id;
    }
    // Treat empty capability as "auto" (omit from payload)
    if (field === "trellis_capability_id" && !value) {
      delete updated[index].trellis_capability_id;
    }
    setConfig({ ...config, device_mappings: updated });
  };

  /** Capabilities on a given Trellis device that Sinric can map to. */
  const getMappableCapabilities = (trellisDeviceId: string) => {
    const device = devices.find((d) => d.id === trellisDeviceId);
    if (!device) return [];
    return device.capabilities.filter((c) =>
      ["switch", "slider", "sensor", "color"].includes(c.type)
    );
  };

  const capTypeBadge: Record<string, string> = {
    switch: "SW",
    slider: "SL",
    sensor: "SN",
    color: "CL",
  };

  const removeMapping = (index: number) => {
    const updated = config.device_mappings.filter((_, i) => i !== index);
    setConfig({ ...config, device_mappings: updated });
  };

  return (
    <div>
      <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
        Sinric Pro (Alexa / Google Home)
      </h2>
      <div className="space-y-3">
        <div className="flex items-center gap-2 text-sm text-zinc-300">
          <Mic
            size={16}
            className={
              status?.connected
                ? "text-trellis-400"
                : status?.enabled
                ? "text-amber-400"
                : "text-zinc-500"
            }
          />
          {status?.connected ? (
            <span>
              Connected to <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-trellis-400 text-xs">ws.sinric.pro</code>
              {" — "}
              <span className="text-zinc-500">
                {status.messages_sent} sent / {status.messages_received} recv
              </span>
            </span>
          ) : status?.enabled ? (
            <span className="text-amber-400">Enabled but not connected{status.last_error ? ` — ${status.last_error}` : ""}</span>
          ) : (
            <span className="text-zinc-500">Disabled</span>
          )}
        </div>

        <label className="flex items-center gap-2 text-sm text-zinc-300">
          <input
            type="checkbox"
            checked={config.enabled}
            onChange={(e) => setConfig({ ...config, enabled: e.target.checked })}
            className="rounded border-zinc-700 bg-zinc-800"
          />
          Enable Sinric Pro bridge
        </label>

        <div className="grid grid-cols-2 gap-2">
          <div className="col-span-2">
            <label className="text-xs text-zinc-500 block mb-1">APP_KEY (UUID)</label>
            <input
              type="text"
              value={config.api_key}
              onChange={(e) => setConfig({ ...config, api_key: e.target.value })}
              className="w-full px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500 font-mono"
              placeholder="xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx"
            />
          </div>
          <div className="col-span-2">
            <label className="text-xs text-zinc-500 block mb-1">
              APP_SECRET
              {hasSecret && (
                <span className="ml-2 text-trellis-400">• stored</span>
              )}
            </label>
            <div className="flex gap-1">
              <input
                type="password"
                value={config.api_secret}
                onChange={(e) => setConfig({ ...config, api_secret: e.target.value })}
                className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 focus:outline-none focus:border-trellis-500 font-mono"
                placeholder={hasSecret ? "(unchanged — type to update)" : "(paste from Sinric Pro dashboard)"}
              />
              {hasSecret && (
                <button
                  type="button"
                  onClick={clearSecret}
                  disabled={busy}
                  className="px-2 py-2 bg-zinc-800 hover:bg-red-900/40 text-zinc-400 hover:text-red-300 rounded-lg text-xs transition-colors disabled:opacity-40"
                  title="Clear stored secret"
                >
                  Clear
                </button>
              )}
            </div>
          </div>
        </div>

        {/* Device mappings */}
        <div className="border border-zinc-800 rounded-lg p-3 space-y-2">
          <div className="flex items-center justify-between">
            <label className="text-xs text-zinc-500">Device mappings</label>
            <button
              type="button"
              onClick={addMapping}
              className="px-2 py-1 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded text-xs transition-colors"
            >
              + Add mapping
            </button>
          </div>
          {config.device_mappings.length === 0 && (
            <p className="text-xs text-zinc-600 italic">
              No mappings yet. Create devices on sinric.pro, then map them to your Trellis devices here.
            </p>
          )}
          {config.device_mappings.map((mapping, i) => {
            const caps = getMappableCapabilities(mapping.trellis_device_id);
            return (
              <div key={i} className="grid grid-cols-[1fr_1fr_1fr_auto] gap-2 items-end">
                <div>
                  <label className="text-xs text-zinc-600 block mb-1">Sinric Device ID</label>
                  <input
                    type="text"
                    value={mapping.sinric_device_id}
                    onChange={(e) => updateMapping(i, "sinric_device_id", e.target.value)}
                    className="w-full px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-200 focus:outline-none focus:border-trellis-500 font-mono"
                    placeholder="Sinric device ID"
                  />
                </div>
                <div>
                  <label className="text-xs text-zinc-600 block mb-1">Trellis Device</label>
                  <select
                    value={mapping.trellis_device_id}
                    onChange={(e) => updateMapping(i, "trellis_device_id", e.target.value)}
                    className="w-full px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-200 focus:outline-none focus:border-trellis-500"
                  >
                    <option value="">— select —</option>
                    {devices.map((d) => (
                      <option key={d.id} value={d.id}>
                        {d.name} ({d.id})
                      </option>
                    ))}
                  </select>
                </div>
                <div>
                  <label className="text-xs text-zinc-600 block mb-1">Capability</label>
                  <select
                    value={mapping.trellis_capability_id ?? ""}
                    onChange={(e) => updateMapping(i, "trellis_capability_id", e.target.value)}
                    disabled={!mapping.trellis_device_id}
                    className="w-full px-2 py-1.5 bg-zinc-800 border border-zinc-700 rounded text-xs text-zinc-200 focus:outline-none focus:border-trellis-500 disabled:opacity-40"
                  >
                    <option value="">Auto (first match)</option>
                    {caps.map((c) => (
                      <option key={c.id} value={c.id}>
                        [{capTypeBadge[c.type] ?? c.type}] {c.label}
                      </option>
                    ))}
                  </select>
                </div>
                <button
                  type="button"
                  onClick={() => removeMapping(i)}
                  className="px-2 py-1.5 bg-zinc-800 hover:bg-red-900/40 text-zinc-400 hover:text-red-300 rounded text-xs transition-colors"
                  title="Remove mapping"
                >
                  ×
                </button>
              </div>
            );
          })}
        </div>

        <div className="flex gap-2">
          <button
            onClick={save}
            disabled={busy}
            className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 disabled:bg-zinc-700 text-white rounded-lg text-sm transition-colors"
          >
            Save & apply
          </button>
          <button
            onClick={testConnection}
            disabled={busy}
            className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 disabled:bg-zinc-900 text-zinc-300 rounded-lg text-sm transition-colors"
          >
            Test connection
          </button>
        </div>

        {feedback && (
          <p className={`text-xs ${feedback.includes("failed") || feedback.includes("Failed") ? "text-red-400" : "text-trellis-400"}`}>
            {feedback}
          </p>
        )}

        <p className="text-xs text-zinc-600">
          Bridges your Trellis devices to Alexa and Google Home via Sinric Pro.
          Create devices on{" "}
          <a href="https://sinric.pro" target="_blank" rel="noopener" className="text-trellis-500 hover:underline">sinric.pro</a>,
          copy the APP_KEY and APP_SECRET from the dashboard, then map each Sinric device to a Trellis device above.
          Optionally pick a specific capability — otherwise the bridge auto-discovers the first match.
          Switches map to power on/off, sliders to dimmer range, color pickers to RGB light, and sensors report temperature readings.
        </p>
      </div>
    </div>
  );
}
