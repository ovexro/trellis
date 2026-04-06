import { useEffect, useState } from "react";
import { Radar, Plus, Wifi, Search } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import DeviceCard from "@/components/DeviceCard";

export default function Dashboard() {
  const { devices, initEventListeners, addDeviceByIp } = useDeviceStore();
  const [showAddDialog, setShowAddDialog] = useState(false);
  const [manualIp, setManualIp] = useState("");
  const [manualPort, setManualPort] = useState("8080");
  const [adding, setAdding] = useState(false);
  const [addError, setAddError] = useState("");
  const [searchQuery, setSearchQuery] = useState("");

  useEffect(() => {
    initEventListeners();
  }, [initEventListeners]);

  const onlineCount = devices.filter((d) => d.online).length;

  const handleAdd = async () => {
    if (!manualIp.trim()) return;
    setAdding(true);
    setAddError("");
    try {
      await addDeviceByIp(manualIp.trim(), parseInt(manualPort));
      setShowAddDialog(false);
      setManualIp("");
    } catch (err) {
      setAddError(String(err));
    } finally {
      setAdding(false);
    }
  };

  if (devices.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center">
        <Radar size={48} className="text-zinc-700 mb-4 animate-pulse" />
        <h2 className="text-lg font-semibold text-zinc-300 mb-2">
          Scanning for devices...
        </h2>
        <p className="text-sm text-zinc-500 max-w-sm mb-6">
          Trellis is automatically discovering devices on your network.
          You can also add a device manually by IP.
        </p>
        <button
          onClick={() => setShowAddDialog(true)}
          className="flex items-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors"
        >
          <Plus size={14} />
          Add by IP
        </button>

        {showAddDialog && (
          <AddDialog
            ip={manualIp}
            port={manualPort}
            adding={adding}
            error={addError}
            onIpChange={setManualIp}
            onPortChange={setManualPort}
            onAdd={handleAdd}
            onCancel={() => setShowAddDialog(false)}
          />
        )}
      </div>
    );
  }

  return (
    <div>
      <div className="flex items-center justify-between mb-4 gap-3">
        <div className="flex items-center gap-2 text-sm text-zinc-400">
          <Wifi size={14} className={onlineCount > 0 ? "text-trellis-400" : "text-zinc-600"} />
          {onlineCount} of {devices.length} online
        </div>

        <div className="flex items-center gap-2 flex-1 max-w-xs">
          <div className="relative flex-1">
            <Search size={14} className="absolute left-2.5 top-1/2 -translate-y-1/2 text-zinc-500" />
            <input
              type="text"
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              placeholder="Search devices..."
              className="w-full bg-zinc-800 border border-zinc-700 rounded-lg pl-8 pr-3 py-1.5 text-sm text-zinc-300 placeholder-zinc-600 focus:border-trellis-500 focus:outline-none"
            />
          </div>
        </div>

        <button
          onClick={() => setShowAddDialog(!showAddDialog)}
          className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm bg-zinc-800 hover:bg-zinc-700 text-zinc-300 transition-colors"
        >
          <Plus size={14} />
          Add by IP
        </button>
      </div>

      {showAddDialog && (
        <AddDialog
          ip={manualIp}
          port={manualPort}
          adding={adding}
          error={addError}
          onIpChange={setManualIp}
          onPortChange={setManualPort}
          onAdd={handleAdd}
          onCancel={() => setShowAddDialog(false)}
        />
      )}

      <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-4">
        {devices
          .filter((d) => {
            if (!searchQuery) return true;
            const q = searchQuery.toLowerCase();
            return (
              d.name.toLowerCase().includes(q) ||
              d.id.toLowerCase().includes(q) ||
              d.ip.includes(q) ||
              d.platform.toLowerCase().includes(q) ||
              d.system.chip.toLowerCase().includes(q)
            );
          })
          .map((device) => (
            <DeviceCard key={device.id} device={device} />
          ))}
      </div>
    </div>
  );
}

function AddDialog({
  ip, port, adding, error, onIpChange, onPortChange, onAdd, onCancel,
}: {
  ip: string;
  port: string;
  adding: boolean;
  error: string;
  onIpChange: (v: string) => void;
  onPortChange: (v: string) => void;
  onAdd: () => void;
  onCancel: () => void;
}) {
  return (
    <div className="mt-4 mb-4 p-4 bg-zinc-900 border border-zinc-800 rounded-xl max-w-sm w-full">
      <h3 className="text-sm font-semibold text-zinc-300 mb-3">Add Device by IP</h3>
      <div className="flex gap-2 mb-2">
        <input
          type="text"
          value={ip}
          onChange={(e) => onIpChange(e.target.value)}
          onKeyDown={(e) => e.key === "Enter" && onAdd()}
          placeholder="192.168.1.108"
          className="flex-1 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300 placeholder-zinc-600"
          autoFocus
        />
        <input
          type="number"
          value={port}
          onChange={(e) => onPortChange(e.target.value)}
          className="w-20 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
        />
      </div>
      {error && (
        <p className="text-xs text-red-400 mb-2">{error}</p>
      )}
      <div className="flex gap-2">
        <button
          onClick={onAdd}
          disabled={adding}
          className="flex-1 px-3 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm transition-colors disabled:opacity-50"
        >
          {adding ? "Connecting..." : "Connect"}
        </button>
        <button
          onClick={onCancel}
          className="px-3 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-400 rounded-lg text-sm transition-colors"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}
