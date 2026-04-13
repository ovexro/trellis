import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import {
  Wifi,
  WifiOff,
  Activity,
  ToggleLeft,
  Thermometer,
  Clock,
  ArrowRight,
  Radio,
  Star,
} from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import Slider from "@/components/controls/Slider";
import ColorPicker from "@/components/controls/ColorPicker";
import type { Capability, Device } from "@/lib/types";

interface ActivityEntry {
  device_id: string;
  severity: string;
  message: string;
  timestamp: string;
}

interface BridgeStatus {
  enabled: boolean;
  connected: boolean;
}

function formatTimeAgo(ts: string): string {
  const now = Date.now();
  const then = new Date(ts + "Z").getTime();
  const diff = Math.max(0, now - then);
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "just now";
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.floor(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  const days = Math.floor(hrs / 24);
  return `${days}d ago`;
}

function severityColor(sev: string): string {
  switch (sev) {
    case "error":
      return "text-red-400";
    case "warn":
      return "text-amber-400";
    case "state":
      return "text-trellis-400";
    default:
      return "text-zinc-400";
  }
}

function severityDot(sev: string): string {
  switch (sev) {
    case "error":
      return "bg-red-400";
    case "warn":
      return "bg-amber-400";
    case "state":
      return "bg-trellis-400";
    default:
      return "bg-zinc-500";
  }
}

// ─── Status Strip ──────────────────────────────────────────────────────

function StatusStrip({
  devices,
  mqtt,
  sinric,
}: {
  devices: Device[];
  mqtt: BridgeStatus | null;
  sinric: BridgeStatus | null;
}) {
  const online = devices.filter((d) => d.online).length;
  const offline = devices.length - online;

  return (
    <div className="flex items-center gap-4 flex-wrap text-sm">
      <div className="flex items-center gap-2">
        <Wifi size={14} className={online > 0 ? "text-emerald-400" : "text-zinc-600"} />
        <span className="text-zinc-200 font-medium">{online}</span>
        <span className="text-zinc-500">online</span>
      </div>
      {offline > 0 && (
        <div className="flex items-center gap-2">
          <WifiOff size={14} className="text-red-400" />
          <span className="text-red-400 font-medium">{offline}</span>
          <span className="text-zinc-500">offline</span>
        </div>
      )}
      <div className="h-4 w-px bg-zinc-800" />
      {mqtt && mqtt.enabled && (
        <div className="flex items-center gap-1.5">
          <span
            className={`w-2 h-2 rounded-full ${
              mqtt.connected ? "bg-emerald-400" : "bg-red-400"
            }`}
          />
          <span className="text-zinc-400">MQTT</span>
        </div>
      )}
      {sinric && sinric.enabled && (
        <div className="flex items-center gap-1.5">
          <span
            className={`w-2 h-2 rounded-full ${
              sinric.connected ? "bg-emerald-400" : "bg-red-400"
            }`}
          />
          <span className="text-zinc-400">Sinric</span>
        </div>
      )}
      <div className="flex-1" />
      <div className="flex items-center gap-1.5 text-zinc-500 text-xs">
        <Radio size={12} />
        {devices.length} device{devices.length !== 1 ? "s" : ""}
      </div>
    </div>
  );
}

// ─── Section wrapper ──────────────────────────────────────────────────

function Section({
  title,
  icon: Icon,
  children,
  action,
}: {
  title: string;
  icon: React.ComponentType<{ size?: number; className?: string }>;
  children: React.ReactNode;
  action?: React.ReactNode;
}) {
  return (
    <div>
      <div className="flex items-center gap-2 mb-3">
        <Icon size={15} className="text-trellis-400" />
        <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
          {title}
        </h2>
        {action && <div className="ml-auto">{action}</div>}
      </div>
      {children}
    </div>
  );
}

// ─── Star toggle ──────────────────────────────────────────────────

function FavStar({
  active,
  onToggle,
}: {
  active: boolean;
  onToggle: () => void;
}) {
  return (
    <button
      onClick={(e) => {
        e.stopPropagation();
        onToggle();
      }}
      className="p-0.5 -m-0.5 transition-colors"
      title={active ? "Remove from favorites" : "Add to favorites"}
    >
      <Star
        size={12}
        className={
          active
            ? "text-amber-400 fill-amber-400"
            : "text-zinc-700 hover:text-amber-400/60"
        }
      />
    </button>
  );
}

// ─── Sensor card (compact, with optional sparkline) ────────────────

function SensorCard({
  device,
  cap,
  isFav,
  onClick,
  onToggleFavorite,
}: {
  device: Device;
  cap: Capability;
  isFav: boolean;
  onClick: () => void;
  onToggleFavorite: () => void;
}) {
  const val = cap.value as number;
  return (
    <button
      onClick={onClick}
      className="p-4 bg-zinc-800/50 rounded-lg text-left hover:bg-zinc-800 transition-colors group w-full"
    >
      <div className="flex items-center gap-1 mb-1">
        <div className="text-[10px] text-zinc-600 group-hover:text-zinc-400 transition-colors truncate flex-1">
          {device.nickname || device.name}
        </div>
        <FavStar active={isFav} onToggle={onToggleFavorite} />
      </div>
      <div className="text-xs text-zinc-500 uppercase tracking-wide mb-1">
        {cap.label}
      </div>
      <div className="flex items-baseline gap-1">
        <span className="text-2xl font-mono font-bold text-zinc-100">
          {typeof val === "number" ? val.toFixed(1) : val}
        </span>
        {cap.unit && (
          <span className="text-sm text-zinc-500">{cap.unit}</span>
        )}
      </div>
    </button>
  );
}

// ─── Quick control card ────────────────────────────────────────────

function QuickControl({
  device,
  cap,
  isFav,
  onChangeVal,
  onToggleFavorite,
}: {
  device: Device;
  cap: Capability;
  isFav: boolean;
  onChangeVal: (deviceId: string, capId: string, value: unknown) => void;
  onToggleFavorite: () => void;
}) {
  const deviceLabel = device.nickname || device.name;

  const header = (
    <div className="px-3 pt-3 pb-1 flex items-center gap-1">
      <span className="text-[10px] text-zinc-600 truncate flex-1">
        {deviceLabel}
      </span>
      {!device.online && (
        <span className="text-[10px] text-red-400/70">offline</span>
      )}
      <FavStar active={isFav} onToggle={onToggleFavorite} />
    </div>
  );
  const disabledClass = device.online ? "" : "opacity-50 pointer-events-none";

  switch (cap.type) {
    case "switch":
      return (
        <div className="border border-zinc-800 rounded-lg">
          {header}
          <div className={`px-3 pb-3 ${disabledClass}`}>
            <div className="flex items-center justify-between">
              <span className="text-sm text-zinc-300">{cap.label}</span>
              <button
                role="switch"
                aria-checked={cap.value as boolean}
                onClick={() => onChangeVal(device.id, cap.id, !(cap.value as boolean))}
                className={`relative w-11 h-6 rounded-full transition-colors duration-200 ${
                  cap.value ? "bg-trellis-500" : "bg-zinc-600"
                }`}
              >
                <span
                  className={`absolute top-0.5 left-0.5 w-5 h-5 bg-white rounded-full transition-all duration-200 shadow-sm ${
                    cap.value ? "translate-x-5" : "translate-x-0"
                  }`}
                />
              </button>
            </div>
          </div>
        </div>
      );
    case "slider":
      return (
        <div className="border border-zinc-800 rounded-lg">
          {header}
          <div className={`px-3 pb-3 ${disabledClass}`}>
            <Slider
              label={cap.label}
              value={cap.value as number}
              min={cap.min ?? 0}
              max={cap.max ?? 100}
              unit={cap.unit}
              onChange={(v) => onChangeVal(device.id, cap.id, v)}
            />
          </div>
        </div>
      );
    case "color":
      return (
        <div className="border border-zinc-800 rounded-lg">
          {header}
          <div className={`px-3 pb-3 ${disabledClass}`}>
            <ColorPicker
              label={cap.label}
              value={cap.value as string}
              onChange={(v) => onChangeVal(device.id, cap.id, v)}
            />
          </div>
        </div>
      );
    default:
      return null;
  }
}

// ─── Activity item ──────────────────────────────────────────────────

function ActivityItem({
  entry,
  deviceName,
  onClick,
}: {
  entry: ActivityEntry;
  deviceName: string;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex items-start gap-2.5 py-2 px-2 -mx-2 rounded-lg hover:bg-zinc-800/50 transition-colors w-full text-left"
    >
      <span
        className={`w-2 h-2 rounded-full mt-1.5 flex-shrink-0 ${severityDot(
          entry.severity
        )}`}
      />
      <div className="flex-1 min-w-0">
        <div className="flex items-center gap-2">
          <span className="text-xs font-medium text-zinc-400 truncate">
            {deviceName}
          </span>
          <span className={`text-[10px] uppercase tracking-wide ${severityColor(entry.severity)}`}>
            {entry.severity}
          </span>
        </div>
        <p className="text-sm text-zinc-300 truncate">{entry.message}</p>
      </div>
      <span className="text-[11px] text-zinc-600 whitespace-nowrap mt-0.5">
        {formatTimeAgo(entry.timestamp)}
      </span>
    </button>
  );
}

// ─── Home page ─────────────────────────────────────────────────────

export default function Home() {
  const { devices, favoriteCapabilities, initEventListeners, updateCapability, toggleFavoriteCapability } = useDeviceStore();
  const navigate = useNavigate();
  const [activity, setActivity] = useState<ActivityEntry[]>([]);
  const [mqtt, setMqtt] = useState<BridgeStatus | null>(null);
  const [sinric, setSinric] = useState<BridgeStatus | null>(null);

  useEffect(() => {
    initEventListeners();
  }, [initEventListeners]);

  const loadActivity = useCallback(async () => {
    try {
      const entries = await invoke<ActivityEntry[]>("get_recent_activity", {
        limit: 30,
      });
      setActivity(entries);
    } catch (err) {
      console.error("Failed to load activity:", err);
    }
  }, []);

  useEffect(() => {
    loadActivity();
    // Refresh activity feed every 30 seconds
    const interval = setInterval(loadActivity, 30000);
    return () => clearInterval(interval);
  }, [loadActivity]);

  useEffect(() => {
    invoke<BridgeStatus>("get_mqtt_status")
      .then(setMqtt)
      .catch(() => {});
    invoke<BridgeStatus>("get_sinric_status")
      .then(setSinric)
      .catch(() => {});
  }, []);

  const handleControlChange = async (
    deviceId: string,
    capId: string,
    value: unknown
  ) => {
    const device = devices.find((d) => d.id === deviceId);
    if (!device) return;
    updateCapability(deviceId, capId, value);
    try {
      await invoke("send_command", {
        deviceId,
        ip: device.ip,
        port: device.port,
        command: { command: "set", id: capId, value },
      });
    } catch (err) {
      console.error("Failed to send command:", err);
    }
  };

  // Collect sensors and controls, split by capability-level favorite status
  const favSensors: { device: Device; cap: Capability }[] = [];
  const favControls: { device: Device; cap: Capability }[] = [];
  const sensors: { device: Device; cap: Capability }[] = [];
  const controls: { device: Device; cap: Capability }[] = [];

  for (const device of devices) {
    for (const cap of device.capabilities) {
      const isFav = favoriteCapabilities.has(`${device.id}:${cap.id}`);
      if (cap.type === "sensor") {
        (isFav ? favSensors : sensors).push({ device, cap });
      } else if (
        cap.type === "switch" ||
        cap.type === "slider" ||
        cap.type === "color"
      ) {
        (isFav ? favControls : controls).push({ device, cap });
      }
    }
  }

  const hasFavorites = favSensors.length > 0 || favControls.length > 0;

  // Map device IDs to display names for the activity feed
  const deviceNames = new Map(
    devices.map((d) => [d.id, d.nickname || d.name])
  );

  if (devices.length === 0) {
    return (
      <div className="flex flex-col items-center justify-center h-full text-center">
        <div className="border border-dashed border-zinc-800 rounded-2xl p-12 max-w-md">
          <Radio
            size={56}
            className="text-zinc-600 mb-5 mx-auto animate-pulse"
          />
          <h2 className="text-lg font-semibold text-zinc-200 mb-2">
            Welcome to Trellis
          </h2>
          <p className="text-sm text-zinc-500 mb-6">
            Connect your first device to see your home overview.
          </p>
          <button
            onClick={() => navigate("/get-started")}
            className="flex items-center gap-2 px-5 py-2.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors mx-auto"
          >
            Get Started
          </button>
        </div>
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* Status strip */}
      <div className="p-3 bg-zinc-900/50 border border-zinc-800/50 rounded-lg">
        <StatusStrip devices={devices} mqtt={mqtt} sinric={sinric} />
      </div>

      {/* Favorites section */}
      {hasFavorites && (
        <Section title="Favorites" icon={Star}>
          <div className="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-3">
            {favSensors.map(({ device, cap }) => (
              <SensorCard
                key={`fav-${device.id}-${cap.id}`}
                device={device}
                cap={cap}
                isFav={true}
                onClick={() => navigate(`/device/${device.id}`)}
                onToggleFavorite={() => toggleFavoriteCapability(device.id, cap.id)}
              />
            ))}
            {favControls.map(({ device, cap }) => (
              <QuickControl
                key={`fav-${device.id}-${cap.id}`}
                device={device}
                cap={cap}
                isFav={true}
                onChangeVal={handleControlChange}
                onToggleFavorite={() => toggleFavoriteCapability(device.id, cap.id)}
              />
            ))}
          </div>
        </Section>
      )}

      {/* Two-column layout: sensors + controls left, activity right */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Left column: sensors + controls */}
        <div className="lg:col-span-2 space-y-6">
          {/* Sensors */}
          {sensors.length > 0 && (
            <Section
              title="Live Readings"
              icon={Thermometer}
              action={
                <button
                  onClick={() => navigate("/metrics")}
                  className="flex items-center gap-1 text-xs text-zinc-500 hover:text-trellis-400 transition-colors"
                >
                  All metrics <ArrowRight size={12} />
                </button>
              }
            >
              <div className="grid grid-cols-2 sm:grid-cols-3 gap-3">
                {sensors.map(({ device, cap }) => (
                  <SensorCard
                    key={`${device.id}-${cap.id}`}
                    device={device}
                    cap={cap}
                    isFav={false}
                    onClick={() => navigate(`/device/${device.id}`)}
                    onToggleFavorite={() => toggleFavoriteCapability(device.id, cap.id)}
                  />
                ))}
              </div>
            </Section>
          )}

          {/* Controls */}
          {controls.length > 0 && (
            <Section title="Quick Controls" icon={ToggleLeft}>
              <div className="grid grid-cols-1 sm:grid-cols-2 gap-3">
                {controls.map(({ device, cap }) => (
                  <QuickControl
                    key={`${device.id}-${cap.id}`}
                    device={device}
                    cap={cap}
                    isFav={false}
                    onChangeVal={handleControlChange}
                    onToggleFavorite={() => toggleFavoriteCapability(device.id, cap.id)}
                  />
                ))}
              </div>
            </Section>
          )}
        </div>

        {/* Right column: activity feed */}
        <div>
          <Section
            title="Recent Activity"
            icon={Activity}
            action={
              activity.length > 0 ? (
                <span className="text-[11px] text-zinc-600">
                  {activity.length} events
                </span>
              ) : undefined
            }
          >
            {activity.length === 0 ? (
              <div className="p-4 text-center border border-dashed border-zinc-800 rounded-lg">
                <Clock size={20} className="text-zinc-700 mx-auto mb-2" />
                <p className="text-xs text-zinc-600">
                  No recent activity yet. Events will appear here as devices
                  come online and report state changes.
                </p>
              </div>
            ) : (
              <div className="divide-y divide-zinc-800/50">
                {activity.map((entry, i) => (
                  <ActivityItem
                    key={`${entry.timestamp}-${i}`}
                    entry={entry}
                    deviceName={
                      deviceNames.get(entry.device_id) || entry.device_id
                    }
                    onClick={() => navigate(`/device/${entry.device_id}`)}
                  />
                ))}
              </div>
            )}
          </Section>
        </div>
      </div>
    </div>
  );
}
