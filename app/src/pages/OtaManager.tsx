import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { open } from "@tauri-apps/plugin-dialog";
import { Upload, CheckCircle, AlertCircle, FileUp } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function OtaManager() {
  const { devices } = useDeviceStore();
  const [selectedDevice, setSelectedDevice] = useState("");
  const [firmwarePath, setFirmwarePath] = useState("");
  const [status, setStatus] = useState<"idle" | "uploading" | "success" | "error">("idle");
  const [errorMsg, setErrorMsg] = useState("");

  const onlineDevices = devices.filter((d) => d.online);
  const selectedDeviceObj = devices.find((d) => d.id === selectedDevice);

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
      setStatus("success");
    } catch (err) {
      setStatus("error");
      setErrorMsg(String(err));
    }
  };

  return (
    <div className="max-w-xl">
      <h1 className="text-xl font-bold text-zinc-100 mb-2">
        Firmware Update (OTA)
      </h1>
      <p className="text-sm text-zinc-500 mb-6">
        Upload a compiled .bin firmware file to a device over WiFi.
        The device will download the firmware from your PC and reboot.
      </p>

      <div className="space-y-4">
        <div>
          <label className="block text-sm text-zinc-400 mb-1.5">
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
          <label className="block text-sm text-zinc-400 mb-1.5">
            Firmware File (.bin)
          </label>
          <div className="flex gap-2">
            <input
              type="text"
              value={firmwarePath}
              onChange={(e) => {
                setFirmwarePath(e.target.value);
                setStatus("idle");
              }}
              placeholder="/path/to/firmware.bin"
              className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600 font-mono"
            />
            <button
              className="flex items-center gap-1.5 px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
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
              Browse
            </button>
          </div>
          <p className="text-xs text-zinc-600 mt-1">
            Compile your sketch first, then enter the path to the .bin file.
          </p>
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
    </div>
  );
}
