import { useState, useRef, useCallback } from "react";
import { useNavigate } from "react-router-dom";
import { Wifi, WifiOff, Cpu } from "lucide-react";
import type { Device, Capability } from "@/lib/types";

interface DeviceCardProps {
  device: Device;
  onCommand?: (deviceId: string, capId: string, value: unknown) => void;
}

function formatUptime(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400) return `${Math.floor(seconds / 3600)}h`;
  return `${Math.floor(seconds / 86400)}d`;
}

function formatLastSeen(iso: string): string {
  if (!iso) return "Unknown";
  const diff = Date.now() - new Date(iso).getTime();
  if (diff < 0 || isNaN(diff)) return "Unknown";
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

function formatSensorValue(value: unknown): string {
  if (typeof value === "number") {
    return Math.abs(value) < 10 ? value.toFixed(1) : Math.round(value).toString();
  }
  return String(value ?? "\u2014");
}

// Compact inline switch for card
function CardSwitch({
  cap,
  disabled,
  onToggle,
}: {
  cap: Capability;
  disabled: boolean;
  onToggle: (capId: string, value: boolean) => void;
}) {
  const checked = cap.value as boolean;
  return (
    <div className="flex items-center justify-between">
      <span className="text-xs text-zinc-400 truncate mr-2">{cap.label}</span>
      <button
        role="switch"
        aria-checked={checked}
        aria-label={cap.label}
        disabled={disabled}
        onClick={(e) => {
          e.stopPropagation();
          e.preventDefault();
          onToggle(cap.id, !checked);
        }}
        className={`relative w-9 h-5 rounded-full transition-colors duration-200 flex-shrink-0 ${
          disabled
            ? "bg-zinc-700 opacity-50 cursor-not-allowed"
            : checked
              ? "bg-trellis-500"
              : "bg-zinc-600"
        }`}
      >
        <span
          className={`absolute top-0.5 left-0.5 w-4 h-4 bg-white rounded-full transition-all duration-200 shadow-sm ${
            checked ? "translate-x-4" : "translate-x-0"
          }`}
        />
      </button>
    </div>
  );
}

// Compact inline slider for card
function CardSlider({
  cap,
  disabled,
  onChange,
}: {
  cap: Capability;
  disabled: boolean;
  onChange: (capId: string, value: number) => void;
}) {
  const [localValue, setLocalValue] = useState(cap.value as number);
  const timeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  const handleChange = useCallback(
    (newValue: number) => {
      setLocalValue(newValue);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      timeoutRef.current = setTimeout(() => onChange(cap.id, newValue), 150);
    },
    [onChange, cap.id],
  );

  // Sync external value
  if (Math.abs((cap.value as number) - localValue) > 0.01 && !timeoutRef.current) {
    setLocalValue(cap.value as number);
  }

  const min = cap.min ?? 0;
  const max = cap.max ?? 100;

  return (
    <div
      onClick={(e) => {
        e.stopPropagation();
        e.preventDefault();
      }}
    >
      <div className="flex items-center justify-between mb-0.5">
        <span className="text-xs text-zinc-400 truncate mr-2">{cap.label}</span>
        <span className="text-xs font-mono text-trellis-400 flex-shrink-0">
          {Math.round(localValue)}
          {cap.unit && <span className="text-zinc-500 ml-0.5">{cap.unit}</span>}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        value={localValue}
        disabled={disabled}
        onChange={(e) => handleChange(Number(e.target.value))}
        className="w-full h-1 bg-zinc-700 rounded-full appearance-none cursor-pointer accent-trellis-500 disabled:opacity-50 disabled:cursor-not-allowed"
      />
    </div>
  );
}

export default function DeviceCard({ device, onCommand }: DeviceCardProps) {
  const navigate = useNavigate();
  const canControl = device.online && !!onCommand;

  const handleCommand = (capId: string, value: unknown) => {
    if (onCommand) onCommand(device.id, capId, value);
  };

  const renderCapability = (cap: Capability) => {
    switch (cap.type) {
      case "switch":
        return (
          <CardSwitch
            key={cap.id}
            cap={cap}
            disabled={!canControl}
            onToggle={handleCommand}
          />
        );
      case "slider":
        return (
          <CardSlider
            key={cap.id}
            cap={cap}
            disabled={!canControl}
            onChange={handleCommand}
          />
        );
      case "sensor":
        return (
          <div key={cap.id} className="flex items-center justify-between">
            <span className="text-xs text-zinc-400 truncate mr-2">{cap.label}</span>
            <span className="text-xs font-mono text-zinc-200 flex-shrink-0">
              {formatSensorValue(cap.value)}
              {cap.unit && <span className="text-zinc-500 ml-0.5">{cap.unit}</span>}
            </span>
          </div>
        );
      case "color":
        return (
          <div key={cap.id} className="flex items-center justify-between">
            <span className="text-xs text-zinc-400 truncate mr-2">{cap.label}</span>
            <div className="flex items-center gap-1.5 flex-shrink-0">
              <span className="text-[10px] font-mono text-zinc-500">
                {(cap.value as string) || "#000000"}
              </span>
              <div
                className="w-5 h-5 rounded border border-zinc-600"
                style={{ backgroundColor: (cap.value as string) || "#000000" }}
              />
            </div>
          </div>
        );
      case "text":
        return (
          <div key={cap.id} className="flex items-center justify-between">
            <span className="text-xs text-zinc-400 truncate mr-2">{cap.label}</span>
            <span className="text-xs font-mono text-zinc-300 truncate max-w-[120px]">
              {(cap.value as string) || "\u2014"}
            </span>
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div
      onClick={() => navigate(`/device/${device.id}`)}
      role="button"
      tabIndex={0}
      onKeyDown={(e) => { if ((e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) navigate(`/device/${device.id}`); }}
      className="w-full text-left bg-zinc-900 border border-zinc-800 rounded-xl p-5 hover:border-zinc-700 hover:bg-zinc-900/80 transition-all duration-150 group focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-trellis-500/50 focus-visible:ring-offset-2 focus-visible:ring-offset-zinc-950 cursor-pointer"
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
        {device.system.chip ? (
          <div className="flex items-center gap-1.5">
            <Cpu size={12} className="text-zinc-600 flex-shrink-0" />
            <span className="text-zinc-300">{device.system.chip}</span>
          </div>
        ) : (
          <div className="flex items-center gap-1.5">
            <Cpu size={12} className="text-zinc-600 flex-shrink-0" />
            <span className="text-zinc-600">{device.platform || "\u2014"}</span>
          </div>
        )}
        <div>
          <span className="text-zinc-600">FW </span>
          <span className="text-zinc-300">{device.firmware || "\u2014"}</span>
        </div>
        {device.online ? (
          <>
            <div>
              <span className="text-zinc-600">RSSI </span>
              <span className="text-zinc-300">{device.system.rssi} dBm</span>
            </div>
            <div>
              <span className="text-zinc-600">Up </span>
              <span className="text-zinc-300">{formatUptime(device.system.uptime_s)}</span>
            </div>
          </>
        ) : (
          <div className="col-span-2">
            <span className="text-zinc-600">Last seen </span>
            <span className="text-zinc-400">{formatLastSeen(device.last_seen)}</span>
          </div>
        )}
      </div>

      {device.tags && (
        <div className="flex gap-1 flex-wrap mt-3 pt-3 border-t border-zinc-800/50">
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

      {device.capabilities.length > 0 && (
        <div
          className={`space-y-2 mt-3 ${!device.tags ? "pt-3 border-t border-zinc-800/50" : "pt-2"}`}
        >
          {device.capabilities.map(renderCapability)}
        </div>
      )}
    </div>
  );
}
