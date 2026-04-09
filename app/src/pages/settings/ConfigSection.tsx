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

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "scan_interval" }).then((val) => {
      if (val) setScanInterval(val);
    }).catch(() => {});
  }, []);

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
    </>
  );
}
