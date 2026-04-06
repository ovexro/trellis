import { useEffect, useState } from "react";
import { Radar, Plus } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useDeviceStore } from "@/stores/deviceStore";
import DeviceCard from "@/components/DeviceCard";
import type { Device } from "@/lib/types";

export default function Dashboard() {
  const { devices, scanning, scan } = useDeviceStore();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [manualIp, setManualIp] = useState("");
  const [manualPort, setManualPort] = useState("8080");
  const [adding, setAdding] = useState(false);

  useEffect(() => {
    scan();
  }, [scan]);

  const addByIp = async () => {
    if (!manualIp.trim()) return;
    setAdding(true);
    try {
      await invoke<Device>("add_device_by_ip", {
        ip: manualIp.trim(),
        port: parseInt(manualPort),
      });
      // Refresh the device list from store
      const updatedDevices = await invoke<Device[]>("get_devices");
      useDeviceStore.setState({ devices: updatedDevices });
      setShowAddDialog(false);
      setManualIp("");
    } catch (err) {
      console.error("Failed to add device:", err);
      alert(`Failed to connect: ${err}`);
    } finally {
      setAdding(false);
    }
  };

  if (devices.length === 0 && !scanning) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center">
        <Radar size={48} className="text-zinc-700 mb-4" />
        <h2 className="text-lg font-semibold text-zinc-300 mb-2">
          No devices found
        </h2>
        <p className="text-sm text-zinc-500 max-w-sm mb-6">
          Make sure your ESP32 or Pico W is running the Trellis library
          and connected to the same network.
        </p>
        <div className="flex gap-3">
          <button
            onClick={scan}
            className="px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors"
          >
            Scan Network
          </button>
          <button
            onClick={() => setShowAddDialog(true)}
            className="flex items-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
          >
            <Plus size={14} />
            Add by IP
          </button>
        </div>

        {showAddDialog && (
          <div className="mt-6 p-4 bg-zinc-900 border border-zinc-800 rounded-xl text-left w-80">
            <h3 className="text-sm font-semibold text-zinc-300 mb-3">Add Device by IP</h3>
            <input
              type="text"
              value={manualIp}
              onChange={(e) => setManualIp(e.target.value)}
              placeholder="192.168.1.108"
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600 mb-2"
              autoFocus
            />
            <input
              type="number"
              value={manualPort}
              onChange={(e) => setManualPort(e.target.value)}
              placeholder="8080"
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600 mb-3"
            />
            <div className="flex gap-2">
              <button
                onClick={addByIp}
                disabled={adding}
                className="flex-1 px-3 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
              >
                {adding ? "Connecting..." : "Connect"}
              </button>
              <button
                onClick={() => setShowAddDialog(false)}
                className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-sm transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="flex justify-end mb-4">
        <button
          onClick={() => setShowAddDialog(!showAddDialog)}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm bg-zinc-800 hover:bg-zinc-700 text-zinc-300 transition-colors"
        >
          <Plus size={14} />
          Add by IP
        </button>
      </div>

      {showAddDialog && (
        <div className="mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl max-w-sm">
          <div className="flex gap-2">
            <input
              type="text"
              value={manualIp}
              onChange={(e) => setManualIp(e.target.value)}
              placeholder="IP address"
              className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600"
              autoFocus
            />
            <input
              type="number"
              value={manualPort}
              onChange={(e) => setManualPort(e.target.value)}
              className="w-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
            />
            <button
              onClick={addByIp}
              disabled={adding}
              className="px-3 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
            >
              {adding ? "..." : "Add"}
            </button>
          </div>
        </div>
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {devices.map((device) => (
          <DeviceCard key={device.id} device={device} />
        ))}
      </div>
    </div>
  );
}
