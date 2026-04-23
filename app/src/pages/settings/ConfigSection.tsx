import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { Download, Upload, Check } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function ConfigSection() {
  const { devices } = useDeviceStore();
  const [exportStatus, setExportStatus] = useState("");
  const [importStatus, setImportStatus] = useState("");
  const [scanInterval, setScanInterval] = useState("30");
  const [dataRetention, setDataRetention] = useState("30");
  const [costPerKwh, setCostPerKwh] = useState("");
  const [currency, setCurrency] = useState("USD");

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "scan_interval" }).then((val) => {
      if (val) setScanInterval(val);
    }).catch(() => {});
    invoke<string | null>("get_setting", { key: "data_retention_days" }).then((val) => {
      if (val) setDataRetention(val);
    }).catch(() => {});
    invoke<string | null>("get_setting", { key: "cost_per_kwh" }).then((val) => {
      if (val) setCostPerKwh(val);
    }).catch(() => {});
    invoke<string | null>("get_setting", { key: "currency" }).then((val) => {
      if (val) setCurrency(val);
    }).catch(() => {});
  }, []);

  const exportConfig = async () => {
    try {
      const [savedDevices, schedules, rules, webhooks, templates, groups, scenes, floorPlans, favorites] = await Promise.all([
        invoke("get_saved_devices"),
        invoke("get_schedules"),
        invoke("get_rules"),
        invoke("get_webhooks"),
        invoke("get_templates"),
        invoke("get_groups"),
        invoke("get_scenes"),
        invoke("get_floor_plans"),
        invoke("get_favorite_capabilities"),
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

      // Collect device positions for each floor
      const allPositions: unknown[] = [];
      if (Array.isArray(floorPlans)) {
        for (const fp of floorPlans as { id: number }[]) {
          try {
            const positions = await invoke("get_device_positions", { floorId: fp.id });
            if (Array.isArray(positions)) allPositions.push(...positions);
          } catch {
            // Floor may have no positions
          }
        }
      }

      const config = {
        version: 2,
        exported_at: new Date().toISOString(),
        devices: savedDevices,
        scenes,
        schedules,
        rules,
        webhooks,
        alerts: allAlerts,
        templates,
        groups,
        floor_plans: floorPlans,
        device_positions: allPositions,
        favorites,
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

        // Restore saved devices (nicknames, tags, notes, install_date, group assignment)
        if (config.devices && Array.isArray(config.devices)) {
          for (const dev of config.devices) {
            if (dev.nickname) {
              await invoke("set_device_nickname", { deviceId: dev.id, nickname: dev.nickname });
            }
            if (dev.tags) {
              await invoke("set_device_tags", { deviceId: dev.id, tags: dev.tags });
            }
            if (dev.notes) {
              await invoke("set_device_notes", { deviceId: dev.id, notes: dev.notes });
            }
            if (dev.install_date) {
              await invoke("set_device_install_date", { deviceId: dev.id, installDate: dev.install_date });
            }
            if (dev.group_id != null && groupIdMap.has(dev.group_id)) {
              await invoke("set_device_group", { deviceId: dev.id, groupId: groupIdMap.get(dev.group_id) });
            }
          }
          imported.push(`${config.devices.length} devices`);
        }

        // Restore scenes (must be before schedules so scene_id references are valid)
        const sceneIdMap = new Map<number, number>(); // old id → new id
        if (config.scenes && Array.isArray(config.scenes)) {
          for (const s of config.scenes) {
            try {
              const actions = Array.isArray(s.actions)
                ? s.actions.map((a: { device_id: string; capability_id: string; value: string }) => ({
                    device_id: a.device_id,
                    capability_id: a.capability_id,
                    value: a.value,
                  }))
                : [];
              if (actions.length > 0) {
                const newId = await invoke<number>("create_scene", {
                  name: s.name, actions,
                });
                sceneIdMap.set(s.id, newId);
              }
            } catch (err) {
              console.error("Failed to import scene:", err);
            }
          }
          imported.push(`${config.scenes.length} scenes`);
        }

        // Restore floor plans and device positions
        const floorIdMap = new Map<number, number>(); // old id → new id
        if (config.floor_plans && Array.isArray(config.floor_plans)) {
          for (const fp of config.floor_plans) {
            try {
              const newId = await invoke<number>("create_floor_plan", { name: fp.name });
              floorIdMap.set(fp.id, newId);
              if (fp.background) {
                await invoke("update_floor_plan", { id: newId, name: null, background: fp.background });
              }
            } catch (err) {
              console.error("Failed to import floor plan:", err);
            }
          }
          imported.push(`${config.floor_plans.length} floor plans`);
        }
        if (config.device_positions && Array.isArray(config.device_positions)) {
          for (const pos of config.device_positions) {
            const newFloorId = floorIdMap.get(pos.floor_id) ?? pos.floor_id;
            try {
              await invoke("set_device_position", {
                deviceId: pos.device_id, floorId: newFloorId, x: pos.x, y: pos.y,
              });
            } catch (err) {
              console.error("Failed to import device position:", err);
            }
          }
          imported.push(`${config.device_positions.length} positions`);
        }

        // Restore favorites
        if (config.favorites && Array.isArray(config.favorites)) {
          for (const [deviceId, capabilityId] of config.favorites) {
            try {
              await invoke("toggle_favorite_capability", { deviceId, capabilityId });
            } catch (err) {
              console.error("Failed to import favorite:", err);
            }
          }
          imported.push(`${config.favorites.length} favorites`);
        }

        // Restore schedules (after scenes so scene_id remapping works)
        if (config.schedules && Array.isArray(config.schedules)) {
          for (const s of config.schedules) {
            try {
              const sceneId = s.scene_id != null ? (sceneIdMap.get(s.scene_id) ?? s.scene_id) : null;
              await invoke("create_schedule", {
                deviceId: s.device_id, capabilityId: s.capability_id,
                value: s.value, cron: s.cron, label: s.label, sceneId,
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
                logic: r.logic || null, conditions: r.conditions || null,
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
    <>
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
          Export saves devices, scenes, floor plans, positions, favorites, schedules, rules, webhooks, alerts, and templates.
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

      {/* Data Retention */}
      <div>
        <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
          Data Retention
        </h2>
        <div className="flex items-center gap-3">
          <label className="text-sm text-zinc-300">Keep metrics &amp; logs for</label>
          <select
            value={dataRetention}
            onChange={async (e) => {
              const val = e.target.value;
              setDataRetention(val);
              try {
                await invoke("set_setting", { key: "data_retention_days", value: val });
              } catch (err) {
                console.error("Failed to save data retention:", err);
              }
            }}
            className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300"
          >
            <option value="7">7 days</option>
            <option value="30">30 days (default)</option>
            <option value="90">90 days</option>
            <option value="365">1 year</option>
            <option value="0">Forever</option>
          </select>
        </div>
        <p className="text-xs text-zinc-600 mt-2">
          Metrics and device logs older than this are automatically deleted. Choosing &ldquo;Forever&rdquo; disables cleanup but the database will grow over time.
        </p>
      </div>

      {/* Energy tariff (optional — only surfaces if a device has nameplate watts set) */}
      <div>
        <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
          Energy Tariff
        </h2>
        <div className="flex flex-wrap items-center gap-3">
          <label className="text-sm text-zinc-300">Cost per kWh</label>
          <input
            type="number"
            min="0"
            step="0.001"
            value={costPerKwh}
            onChange={(e) => setCostPerKwh(e.target.value)}
            onBlur={async () => {
              const trimmed = costPerKwh.trim();
              try {
                if (trimmed === "") {
                  await invoke("delete_setting", { key: "cost_per_kwh" });
                } else {
                  await invoke("set_setting", {
                    key: "cost_per_kwh",
                    value: trimmed,
                  });
                }
              } catch (err) {
                console.error("Failed to save cost per kWh:", err);
              }
            }}
            placeholder="0.00"
            className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300 w-28 font-mono"
          />
          <select
            value={currency}
            onChange={async (e) => {
              const val = e.target.value;
              setCurrency(val);
              try {
                await invoke("set_setting", { key: "currency", value: val });
              } catch (err) {
                console.error("Failed to save currency:", err);
              }
            }}
            className="bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-1.5 text-sm text-zinc-300"
          >
            <option value="USD">USD</option>
            <option value="EUR">EUR</option>
            <option value="GBP">GBP</option>
            <option value="CAD">CAD</option>
            <option value="AUD">AUD</option>
            <option value="JPY">JPY</option>
            <option value="RON">RON</option>
          </select>
        </div>
        <p className="text-xs text-zinc-600 mt-2">
          Optional. When set, device Energy cards estimate a cost alongside the measured Wh. Leave blank to show energy only.
        </p>
      </div>
    </>
  );
}
