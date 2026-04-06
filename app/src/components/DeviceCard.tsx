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

export default function DeviceCard({ device }: DeviceCardProps) {
  const navigate = useNavigate();

  return (
    <button
      onClick={() => navigate(`/device/${device.id}`)}
      className="w-full text-left bg-zinc-900 border border-zinc-800 rounded-xl p-5 hover:border-zinc-700 hover:bg-zinc-900/80 transition-all duration-150 group focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-trellis-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-zinc-950"
    >
      <div className="flex items-start justify-between mb-3">
        <div>
          <h3 className="font-semibold text-zinc-100 group-hover:text-trellis-400 transition-colors">
            {device.nickname || device.name}
          </h3>
          <p className="text-xs text-zinc-500 mt-0.5">
            {device.ip}:{device.port}
            {device.nickname && (
              <span className="text-zinc-600 ml-1.5">({device.name})</span>
            )}
          </p>
        </div>
        <div
          className={`flex items-center gap-1.5 px-2.5 py-1 rounded-full text-xs font-medium ${
            device.online
              ? "bg-trellis-500/10 text-trellis-400"
              : "bg-red-500/10 text-red-400"
          }`}
        >
          {device.online ? <Wifi size={12} /> : <WifiOff size={12} />}
          {device.online ? "Online" : "Offline"}
        </div>
      </div>

      <div className="grid grid-cols-2 gap-x-4 gap-y-1.5 text-xs">
        <div className="flex items-center gap-1.5">
          <Cpu size={12} className="text-zinc-600 flex-shrink-0" />
          <span className="text-zinc-300">{device.system.chip}</span>
        </div>
        <div>
          <span className="text-zinc-600">FW </span>
          <span className="text-zinc-300">{device.firmware}</span>
        </div>
        <div>
          <span className="text-zinc-600">RSSI </span>
          <span className="text-zinc-300">
            {device.system.rssi} dBm
          </span>
        </div>
        <div>
          <span className="text-zinc-600">Up </span>
          <span className="text-zinc-300">{formatUptime(device.system.uptime_s)}</span>
        </div>
      </div>

      <div className="mt-3 pt-3 border-t border-zinc-800/50">
        {device.tags && (
          <div className="flex gap-1 flex-wrap mb-2">
            {device.tags
              .split(",")
              .map((t) => t.trim())
              .filter(Boolean)
              .map((tag) => (
                <span
                  key={tag}
                  className="px-2 py-0.5 bg-zinc-800 border border-zinc-700/50 rounded-full text-[11px] text-zinc-400"
                >
                  {tag}
                </span>
              ))}
          </div>
        )}
        <p className="text-xs text-zinc-500">
          {device.capabilities.length} control
          {device.capabilities.length !== 1 ? "s" : ""}
          {device.capabilities.length > 0 && (
            <span className="text-zinc-600">
              {" — "}
              {device.capabilities.map((c) => c.label).join(", ")}
            </span>
          )}
        </p>
      </div>
    </button>
  );
}
