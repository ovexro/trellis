import { useState } from "react";
import { Upload, CheckCircle, AlertCircle } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function OtaManager() {
  const { devices } = useDeviceStore();
  const [selectedDevice, setSelectedDevice] = useState("");
  const [firmwarePath, setFirmwarePath] = useState("");
  const [status, setStatus] = useState<"idle" | "uploading" | "success" | "error">("idle");

  const onlineDevices = devices.filter((d) => d.online);

  const handleUpload = async () => {
    if (!selectedDevice || !firmwarePath) return;
    setStatus("uploading");
    // TODO: implement OTA via Tauri command
    setTimeout(() => setStatus("success"), 2000);
  };

  return (
    <div className="max-w-xl">
      <h1 className="text-xl font-bold text-zinc-100 mb-6">
        Firmware Update (OTA)
      </h1>

      <div className="space-y-4">
        <div>
          <label className="block text-sm text-zinc-400 mb-1.5">
            Target Device
          </label>
          <select
            value={selectedDevice}
            onChange={(e) => setSelectedDevice(e.target.value)}
            className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
          >
            <option value="">Select a device...</option>
            {onlineDevices.map((d) => (
              <option key={d.id} value={d.id}>
                {d.name} ({d.ip}) — FW {d.firmware}
              </option>
            ))}
          </select>
        </div>

        <div>
          <label className="block text-sm text-zinc-400 mb-1.5">
            Firmware File (.bin)
          </label>
          <div className="flex gap-2">
            <input
              type="text"
              value={firmwarePath}
              onChange={(e) => setFirmwarePath(e.target.value)}
              placeholder="Drag & drop or enter path..."
              className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600"
            />
          </div>
        </div>

        <button
          onClick={handleUpload}
          disabled={!selectedDevice || !firmwarePath || status === "uploading"}
          className="flex items-center gap-2 px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        >
          <Upload size={16} />
          {status === "uploading" ? "Uploading..." : "Upload Firmware"}
        </button>

        {status === "success" && (
          <div className="flex items-center gap-2 text-sm text-trellis-400">
            <CheckCircle size={16} />
            Firmware updated successfully. Device is rebooting.
          </div>
        )}

        {status === "error" && (
          <div className="flex items-center gap-2 text-sm text-red-400">
            <AlertCircle size={16} />
            OTA update failed. Check device connection and try again.
          </div>
        )}
      </div>
    </div>
  );
}
