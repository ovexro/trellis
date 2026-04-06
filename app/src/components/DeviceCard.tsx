import { useNavigate } from "react-router-dom";
import { Wifi, WifiOff, Cpu } from "lucide-react";
import type { Device } from "@/lib/types";

interface DeviceCardProps {
  device: Device;
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

function rssiStrength(rssi: number): string {
  if (rssi >= -50) return "Excellent";
  if (rssi >= -60) return "Good";
  if (rssi >= -70) return "Fair";
  return "Weak";
}

export default function DeviceCard({ device }: DeviceCardProps) {
  const navigate = useNavigate();

  return (
    <button
      onClick={() => navigate(`/device/${device.id}`)}
      className="w-full text-left bg-zinc-900 border border-zinc-800 rounded-xl p-4 hover:border-zinc-700 hover:bg-zinc-900/80 transition-all group"
    >
      <div className="flex items-start justify-between mb-3">
        <div>
          <h3 className="font-semibold text-zinc-100 group-hover:text-trellis-400 transition-colors">
            {device.nickname || device.name}
          </h3>
          <p className="text-xs text-zinc-500 mt-0.5">{device.ip}:{device.port}</p>
        </div>
        <div
          className={`flex items-center gap-1.5 px-2 py-0.5 rounded-full text-xs ${
            device.online
              ? "bg-trellis-500/10 text-trellis-400"
              : "bg-red-500/10 text-red-400"
          }`}
        >
          {device.online ? <Wifi size={12} /> : <WifiOff size={12} />}
          {device.online ? "Online" : "Offline"}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-2 text-xs text-zinc-400">
        <div className="flex items-center gap-1.5">
          <Cpu size={12} className="text-zinc-600" />
          {device.system.chip}
        </div>
        <div>FW: {device.firmware}</div>
        <div>RSSI: {device.system.rssi} dBm ({rssiStrength(device.system.rssi)})</div>
        <div>Up: {formatUptime(device.system.uptime_s)}</div>
      </div>

      <div className="mt-3 pt-3 border-t border-zinc-800">
        {device.tags && (
          <div className="flex gap-1 flex-wrap mb-1.5">
            {device.tags.split(",").map((t) => t.trim()).filter(Boolean).map((tag) => (
              <span key={tag} className="px-1.5 py-0.5 bg-zinc-800 border border-zinc-700 rounded text-[10px] text-zinc-500">
                {tag}
              </span>
            ))}
          </div>
        )}
        <p className="text-xs text-zinc-500">
          {device.capabilities.length} control{device.capabilities.length !== 1 ? "s" : ""}
          {" — "}
          {device.capabilities.map((c) => c.label).join(", ")}
        </p>
      </div>
    </button>
  );
}
