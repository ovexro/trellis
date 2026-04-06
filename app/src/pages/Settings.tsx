import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { save, open as openDialog } from "@tauri-apps/plugin-dialog";
import { Download, Upload, Check } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function Settings() {
  const { devices } = useDeviceStore();
  const [exportStatus, setExportStatus] = useState("");
  const [importStatus, setImportStatus] = useState("");

  const exportConfig = async () => {
    try {
      const savedDevices = await invoke("get_saved_devices");
      const scenes = localStorage.getItem("trellis-scenes");

      const config = {
        version: "0.1.0",
        exported_at: new Date().toISOString(),
        devices: savedDevices,
        scenes: scenes ? JSON.parse(scenes) : [],
        device_count: devices.length,
      };

      const filePath = await save({
        defaultPath: "trellis-config.json",
        filters: [{ name: "JSON", extensions: ["json"] }],
      });

      if (filePath) {
        // Write via Tauri
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

        if (config.scenes) {
          localStorage.setItem("trellis-scenes", JSON.stringify(config.scenes));
        }

        // Restore saved devices (nicknames, tags)
        if (config.devices && Array.isArray(config.devices)) {
          for (const dev of config.devices) {
            if (dev.nickname) {
              await invoke("set_device_nickname", { deviceId: dev.id, nickname: dev.nickname });
            }
            if (dev.tags) {
              await invoke("set_device_tags", { deviceId: dev.id, tags: dev.tags });
            }
          }
        }

        setImportStatus(
          `Imported: ${config.scenes?.length || 0} scenes, ${config.devices?.length || 0} saved devices`,
        );
        setTimeout(() => setImportStatus(""), 3000);
      }
    } catch (err) {
      setImportStatus(`Import failed: ${err}`);
    }
  };

  return (
    <div className="max-w-xl">
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
            <p className="text-xs text-trellis-400 mt-2 flex items-center gap-1">
              <Check size={12} /> {exportStatus}
            </p>
          )}
          {importStatus && (
            <p className="text-xs text-trellis-400 mt-2 flex items-center gap-1">
              <Check size={12} /> {importStatus}
            </p>
          )}
          <p className="text-xs text-zinc-600 mt-2">
            Export saves device nicknames, tags, scenes, and alert rules.
            Import on a new PC to restore your setup.
          </p>
        </div>

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
            <p>Trellis v0.1.2</p>
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
