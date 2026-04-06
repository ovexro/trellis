import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bell, Plus, Trash2, ToggleLeft, ToggleRight } from "lucide-react";

interface AlertRule {
  id: number;
  device_id: string;
  metric_id: string;
  condition: string;
  threshold: number;
  label: string;
  enabled: boolean;
}

interface DeviceAlertsProps {
  deviceId: string;
  sensorIds: Array<{ id: string; label: string; unit?: string }>;
}

export default function DeviceAlerts({ deviceId, sensorIds }: DeviceAlertsProps) {
  const [alerts, setAlerts] = useState<AlertRule[]>([]);
  const [adding, setAdding] = useState(false);
  const [newMetric, setNewMetric] = useState("");
  const [newCondition, setNewCondition] = useState("above");
  const [newThreshold, setNewThreshold] = useState("");
  const [newLabel, setNewLabel] = useState("");

  useEffect(() => {
    loadAlerts();
  }, [deviceId]);

  const loadAlerts = async () => {
    try {
      const result = await invoke<AlertRule[]>("get_alerts", { deviceId });
      setAlerts(result);
    } catch {}
  };

  const createAlert = async () => {
    if (!newMetric || !newThreshold || !newLabel) return;
    try {
      await invoke("create_alert", {
        deviceId,
        metricId: newMetric,
        condition: newCondition,
        threshold: parseFloat(newThreshold),
        label: newLabel,
      });
      setAdding(false);
      setNewMetric("");
      setNewThreshold("");
      setNewLabel("");
      loadAlerts();
    } catch (err) {
      console.error("Failed to create alert:", err);
    }
  };

  const deleteAlert = async (alertId: number) => {
    try {
      await invoke("delete_alert", { alertId });
      loadAlerts();
    } catch {}
  };

  const toggleAlert = async (alertId: number, enabled: boolean) => {
    try {
      await invoke("toggle_alert", { alertId, enabled });
      loadAlerts();
    } catch {}
  };

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-semibold text-zinc-300 flex items-center gap-2">
          <Bell size={14} />
          Alerts
        </h3>
        {sensorIds.length > 0 && (
          <button
            onClick={() => {
              setAdding(!adding);
              if (!newMetric && sensorIds.length > 0) setNewMetric(sensorIds[0].id);
            }}
            className="flex items-center gap-1 text-xs text-trellis-400 hover:text-trellis-300"
          >
            <Plus size={12} />
            Add rule
          </button>
        )}
      </div>

      {adding && (
        <div className="mb-4 p-4 bg-zinc-800/50 rounded-xl border border-zinc-700/30 space-y-3">
          <div>
            <label className="block text-[11px] text-zinc-500 uppercase tracking-wider mb-1">Alert name</label>
            <input
              type="text"
              value={newLabel}
              onChange={(e) => setNewLabel(e.target.value)}
              placeholder="e.g., High temperature warning"
              className="w-full bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600"
              autoFocus
            />
          </div>
          <div>
            <label className="block text-[11px] text-zinc-500 uppercase tracking-wider mb-1">Condition</label>
            <div className="flex gap-2">
              <select
                value={newMetric}
                onChange={(e) => setNewMetric(e.target.value)}
                className="flex-1 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
              >
                {sensorIds.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.label}
                  </option>
                ))}
              </select>
              <select
                value={newCondition}
                onChange={(e) => setNewCondition(e.target.value)}
                className="w-24 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
              >
                <option value="above">above</option>
                <option value="below">below</option>
              </select>
              <input
                type="number"
                value={newThreshold}
                onChange={(e) => setNewThreshold(e.target.value)}
                placeholder="value"
                className="w-24 bg-zinc-900 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
              />
            </div>
          </div>
          <div className="flex gap-2 pt-1">
            <button
              onClick={createAlert}
              className="px-4 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium transition-colors"
            >
              Create alert
            </button>
            <button
              onClick={() => setAdding(false)}
              className="px-4 py-1.5 text-zinc-400 hover:text-zinc-300 border border-zinc-700 rounded-lg text-xs transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {alerts.length === 0 && !adding ? (
        <p className="text-xs text-zinc-600">
          No alerts. Add a rule to get notified when sensor values cross a threshold.
        </p>
      ) : (
        <div className="space-y-1.5">
          {alerts.map((alert) => (
            <div
              key={alert.id}
              className={`flex items-center justify-between p-2 rounded-lg text-xs ${
                alert.enabled ? "bg-zinc-800/50" : "bg-zinc-800/20 opacity-50"
              }`}
            >
              <div>
                <span className="text-zinc-300">{alert.label}</span>
                <span className="text-zinc-500 ml-2">
                  {alert.metric_id} {alert.condition} {alert.threshold}
                </span>
              </div>
              <div className="flex items-center gap-1.5">
                <button
                  onClick={() => toggleAlert(alert.id, !alert.enabled)}
                  className="text-zinc-500 hover:text-zinc-300"
                >
                  {alert.enabled ? <ToggleRight size={16} className="text-trellis-400" /> : <ToggleLeft size={16} />}
                </button>
                <button
                  onClick={() => deleteAlert(alert.id)}
                  className="text-zinc-500 hover:text-red-400"
                >
                  <Trash2 size={12} />
                </button>
              </div>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
