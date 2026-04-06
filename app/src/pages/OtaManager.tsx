import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open } from "@tauri-apps/plugin-dialog";
import { Upload, CheckCircle, AlertCircle, FileUp, History, RotateCcw, Trash2, HardDriveDownload } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

interface FirmwareRecord {
  id: number;
  device_id: string;
  version: string;
  file_path: string;
  file_size: number;
  uploaded_at: string;
}

function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

export default function OtaManager() {
  const { devices } = useDeviceStore();
  const [selectedDevice, setSelectedDevice] = useState("");
  const [firmwarePath, setFirmwarePath] = useState("");
  const [otaProgress, setOtaProgress] = useState(-1);
  const [status, setStatus] = useState<"idle" | "uploading" | "success" | "error">("idle");
  const [errorMsg, setErrorMsg] = useState("");
  const [firmwareHistory, setFirmwareHistory] = useState<FirmwareRecord[]>([]);
  const [historyLoading, setHistoryLoading] = useState(false);
  const [dragging, setDragging] = useState(false);

  const onlineDevices = devices.filter((d) => d.online);
  const selectedDeviceObj = devices.find((d) => d.id === selectedDevice);

  // Listen for Tauri drag-drop events (provides file paths)
  useEffect(() => {
    const unlistenDrop = listen<{ paths: string[] }>("tauri://drag-drop", (e) => {
      setDragging(false);
      const paths = e.payload.paths;
      if (paths && paths.length > 0) {
        const path = paths[0];
        if (path.endsWith(".bin")) {
          setFirmwarePath(path);
          setStatus("idle");
        }
      }
    });
    const unlistenEnter = listen("tauri://drag-enter", () => setDragging(true));
    const unlistenLeave = listen("tauri://drag-leave", () => setDragging(false));
    return () => {
      unlistenDrop.then((fn) => fn());
      unlistenEnter.then((fn) => fn());
      unlistenLeave.then((fn) => fn());
    };
  }, []);

  useEffect(() => {
    const unlisten = listen<{ device_id: string; event_type: string; payload: { percent?: number } }>(
      "device-event",
      (e) => {
        if (e.payload.event_type === "ota_progress" && e.payload.device_id === selectedDevice) {
          const pct = e.payload.payload.percent ?? -1;
          setOtaProgress(pct);
          if (pct === -1) setStatus("error");
          if (pct === 100) setStatus("success");
        }
      },
    );
    return () => { unlisten.then((fn) => fn()); };
  }, [selectedDevice]);

  const loadFirmwareHistory = async (deviceId: string) => {
    if (!deviceId) {
      setFirmwareHistory([]);
      return;
    }
    setHistoryLoading(true);
    try {
      const history = await invoke<FirmwareRecord[]>("get_firmware_history", { deviceId });
      setFirmwareHistory(history);
    } catch {
      setFirmwareHistory([]);
    } finally {
      setHistoryLoading(false);
    }
  };

  useEffect(() => {
    loadFirmwareHistory(selectedDevice);
  }, [selectedDevice]);

  const handleRollback = async (record: FirmwareRecord) => {
    const device = devices.find((d) => d.id === selectedDevice);
    if (!device) return;
    setStatus("uploading");
    setErrorMsg("");
    setOtaProgress(0);
    try {
      await invoke("rollback_firmware", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        firmwareRecordPath: record.file_path,
      });
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
    }
  };

  const handleDeleteRecord = async (record: FirmwareRecord) => {
    if (!confirm(`Delete stored firmware v${record.version} (${formatFileSize(record.file_size)})?`)) return;
    try {
      await invoke("delete_firmware_record", { id: record.id });
      setFirmwareHistory((prev) => prev.filter((r) => r.id !== record.id));
    } catch (err) {
      setErrorMsg(String(err));
    }
  };

  const handleUpload = async () => {
    if (!selectedDevice || !firmwarePath) return;

    const device = devices.find((d) => d.id === selectedDevice);
    if (!device) return;

    setStatus("uploading");
    setErrorMsg("");

    try {
      await invoke("start_ota", {
        deviceId: device.id,
        ip: device.ip,
        port: device.port,
        firmwarePath: firmwarePath,
      });
      // Don't set success here — the OTA command was SENT but the device
      // hasn't finished downloading yet. Status will update via ota_progress events.
      setOtaProgress(0);
      loadFirmwareHistory(selectedDevice);
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
    }
  };

  return (
    <div>
      <h1 className="text-xl font-bold text-zinc-100 mb-2">
        Firmware Update (OTA)
      </h1>
      <p className="text-sm text-zinc-500 mb-6">
        Upload a compiled .bin firmware file to a device over WiFi.
        The device will download the firmware from your PC and reboot.
      </p>

      <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-6 space-y-5">
        <div>
          <label className="block text-sm font-medium text-zinc-300 mb-2">
            Target Device
          </label>
          <select
            value={selectedDevice}
            onChange={(e) => {
              setSelectedDevice(e.target.value);
              setStatus("idle");
            }}
            className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
          >
            <option value="">Select a device...</option>
            {onlineDevices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name} ({d.ip}) — FW {d.firmware} — {d.platform}
              </option>
            ))}
          </select>
          {onlineDevices.length === 0 && (
            <p className="text-xs text-zinc-600 mt-1">
              No online devices found. Connect a device first.
            </p>
          )}
        </div>

        <div>
          <label className="block text-sm font-medium text-zinc-300 mb-2">
            Firmware File (.bin)
          </label>
          <div
            className={`border-2 border-dashed rounded-xl p-4 transition-colors ${
              dragging
                ? "border-trellis-500 bg-trellis-500/5"
                : firmwarePath
                  ? "border-zinc-700 bg-zinc-800/30"
                  : "border-zinc-700/50 bg-zinc-800/20"
            }`}
          >
            {firmwarePath ? (
              <div className="flex items-center gap-3">
                <HardDriveDownload size={20} className="text-trellis-400 flex-shrink-0" />
                <div className="flex-1 min-w-0">
                  <p className="text-sm text-zinc-300 font-mono truncate">{firmwarePath}</p>
                  <p className="text-xs text-zinc-600 mt-0.5">Ready to upload</p>
                </div>
                <button
                  className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-xs transition-colors"
                  onClick={async () => {
                    const file = await open({
                      multiple: false,
                      filters: [{ name: "Firmware", extensions: ["bin"] }],
                    });
                    if (file) {
                      setFirmwarePath(file);
                      setStatus("idle");
                    }
                  }}
                >
                  <FileUp size={12} />
                  Change
                </button>
              </div>
            ) : (
              <div className="text-center py-3">
                <FileUp size={24} className={`mx-auto mb-2 ${dragging ? "text-trellis-400" : "text-zinc-600"}`} />
                <p className="text-sm text-zinc-400">
                  {dragging ? "Drop .bin file here" : "Drag & drop a .bin file here"}
                </p>
                <p className="text-xs text-zinc-600 mt-1 mb-3">or</p>
                <button
                  className="flex items-center gap-1.5 px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors mx-auto"
                  onClick={async () => {
                    const file = await open({
                      multiple: false,
                      filters: [{ name: "Firmware", extensions: ["bin"] }],
                    });
                    if (file) {
                      setFirmwarePath(file);
                      setStatus("idle");
                    }
                  }}
                >
                  <FileUp size={14} />
                  Browse for file
                </button>
              </div>
            )}
          </div>
        </div>

        {selectedDeviceObj && selectedDeviceObj.platform !== "esp32" && (
          <div className="flex items-center gap-2 text-sm text-amber-400 bg-amber-500/10 p-3 rounded-lg">
            <AlertCircle size={16} />
            OTA is currently supported on ESP32 only. Pico W support is planned.
          </div>
        )}

        <button
          onClick={handleUpload}
          disabled={
            !selectedDevice ||
            !firmwarePath ||
            status === "uploading" ||
            (selectedDeviceObj?.platform !== "esp32")
          }
          className="flex items-center gap-2 px-4 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors disabled:opacity-50 w-full justify-center"
        >
          <Upload size={16} />
          {status === "uploading" ? "Sending to device..." : "Upload Firmware"}
        </button>

        {status === "uploading" && otaProgress >= 0 && (
          <div>
            <div className="flex justify-between text-xs text-zinc-400 mb-1">
              <span>Downloading firmware to device...</span>
              <span>{otaProgress}%</span>
            </div>
            <div className="w-full h-2 bg-zinc-800 rounded-full overflow-hidden">
              <div
                className="h-full bg-trellis-500 rounded-full transition-all duration-300"
                style={{ width: `${otaProgress}%` }}
              />
            </div>
          </div>
        )}

        {status === "success" && (
          <div className="flex items-center gap-2 text-sm text-trellis-400 bg-trellis-500/10 p-3 rounded-lg">
            <CheckCircle size={16} />
            Firmware sent. The device is downloading and will reboot automatically.
          </div>
        )}

        {status === "error" && (
          <div className="flex items-start gap-2 text-sm text-red-400 bg-red-500/10 p-3 rounded-lg">
            <AlertCircle size={16} className="mt-0.5 flex-shrink-0" />
            <div>
              <p>OTA update failed.</p>
              {errorMsg && <p className="text-xs mt-1 text-red-300">{errorMsg}</p>}
            </div>
          </div>
        )}
      </div>

      {/* Firmware History */}
      {selectedDevice && (
        <div className="mt-6 bg-zinc-900 border border-zinc-800 rounded-xl p-6">
          <div className="flex items-center gap-2 mb-4">
            <History size={16} className="text-zinc-400" />
            <h2 className="text-sm font-semibold text-zinc-200">Firmware History</h2>
          </div>

          {selectedDeviceObj && selectedDeviceObj.firmware && (
            <div className="flex items-center gap-2 text-xs text-zinc-500 mb-4">
              <span>Current firmware:</span>
              <span className="text-zinc-300 font-mono px-2 py-0.5 bg-trellis-500/10 border border-trellis-500/20 rounded">
                v{selectedDeviceObj.firmware}
              </span>
            </div>
          )}

          {historyLoading ? (
            <p className="text-xs text-zinc-600">Loading history...</p>
          ) : firmwareHistory.length === 0 ? (
            <p className="text-xs text-zinc-600">No firmware history yet. Upload a firmware to start tracking.</p>
          ) : (
            <div className="space-y-2">
              {firmwareHistory.map((record) => {
                const isCurrent = selectedDeviceObj?.firmware === record.version;
                return (
                <div
                  key={record.id}
                  className={`flex items-center justify-between rounded-lg px-4 py-3 ${
                    isCurrent
                      ? "bg-trellis-500/5 border border-trellis-500/20"
                      : "bg-zinc-800/50 border border-zinc-700/50"
                  }`}
                >
                  <div className="min-w-0 flex-1">
                    <p className="text-sm text-zinc-300 font-mono truncate">
                      v{record.version}
                      {isCurrent && (
                        <span className="text-[10px] text-trellis-400 ml-2 font-sans uppercase tracking-wide">
                          current
                        </span>
                      )}
                    </p>
                    <p className="text-xs text-zinc-500 mt-0.5">
                      {formatFileSize(record.file_size)} &middot; {new Date(record.uploaded_at + "Z").toLocaleString()}
                    </p>
                  </div>
                  <div className="flex items-center gap-2 ml-4 flex-shrink-0">
                    <button
                      onClick={() => handleRollback(record)}
                      disabled={status === "uploading" || !selectedDeviceObj?.online}
                      title="Rollback to this firmware"
                      className="flex items-center gap-1.5 px-3 py-1.5 text-xs bg-amber-500/10 text-amber-400 hover:bg-amber-500/20 rounded-md transition-colors disabled:opacity-40"
                    >
                      <RotateCcw size={12} />
                      Rollback
                    </button>
                    <button
                      onClick={() => handleDeleteRecord(record)}
                      title="Delete stored firmware"
                      className="flex items-center gap-1.5 px-2 py-1.5 text-xs text-zinc-500 hover:text-red-400 hover:bg-red-500/10 rounded-md transition-colors"
                    >
                      <Trash2 size={12} />
                    </button>
                  </div>
                </div>
                );
              })}
            </div>
          )}
        </div>
      )}
    </div>
  );
}
