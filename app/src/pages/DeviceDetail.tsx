import { useParams, useNavigate } from "react-router-dom";
import { ArrowLeft, Wifi } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useDeviceStore } from "@/stores/deviceStore";
import Switch from "@/components/controls/Switch";
import Slider from "@/components/controls/Slider";
import Sensor from "@/components/controls/Sensor";
import ColorPicker from "@/components/controls/ColorPicker";
import type { Capability } from "@/lib/types";

export default function DeviceDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { devices, updateCapability } = useDeviceStore();
  const device = devices.find((d) => d.id === id);

  if (!device) {
    return (
      <div className="flex flex-col items-center justify-center h-full">
        <p className="text-zinc-500">Device not found</p>
        <button
          onClick={() => navigate("/")}
          className="mt-4 text-sm text-trellis-400 hover:text-trellis-300"
        >
          Back to dashboard
        </button>
      </div>
    );
  }

  const handleChange = async (capId: string, value: unknown) => {
    updateCapability(device.id, capId, value);
    try {
      await invoke("send_command", {
        ip: device.ip,
        port: device.port,
        command: { command: "set", id: capId, value },
      });
    } catch (err) {
      console.error("Failed to send command:", err);
    }
  };

  const renderControl = (cap: Capability) => {
    switch (cap.type) {
      case "switch":
        return (
          <Switch
            key={cap.id}
            label={cap.label}
            value={cap.value as boolean}
            onChange={(v) => handleChange(cap.id, v)}
          />
        );
      case "slider":
        return (
          <Slider
            key={cap.id}
            label={cap.label}
            value={cap.value as number}
            min={cap.min ?? 0}
            max={cap.max ?? 100}
            unit={cap.unit}
            onChange={(v) => handleChange(cap.id, v)}
          />
        );
      case "sensor":
        return (
          <Sensor
            key={cap.id}
            label={cap.label}
            value={cap.value as number}
            unit={cap.unit}
          />
        );
      case "color":
        return (
          <ColorPicker
            key={cap.id}
            label={cap.label}
            value={cap.value as string}
            onChange={(v) => handleChange(cap.id, v)}
          />
        );
      case "text":
        return (
          <div key={cap.id} className="p-3 bg-zinc-800/50 rounded-lg">
            <span className="text-xs text-zinc-500 uppercase tracking-wide">
              {cap.label}
            </span>
            <p className="mt-1 text-sm text-zinc-200 font-mono">
              {cap.value as string || "—"}
            </p>
          </div>
        );
      default:
        return null;
    }
  };

  return (
    <div className="max-w-2xl">
      <button
        onClick={() => navigate("/")}
        className="flex items-center gap-2 text-sm text-zinc-400 hover:text-zinc-200 mb-6 transition-colors"
      >
        <ArrowLeft size={16} />
        Back to devices
      </button>

      <div className="flex items-start justify-between mb-6">
        <div>
          <h1 className="text-2xl font-bold text-zinc-100">{device.name}</h1>
          <p className="text-sm text-zinc-500 mt-1">
            {device.ip}:{device.port} &middot; {device.system.chip} &middot; FW {device.firmware}
          </p>
        </div>
        <div
          className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-sm ${
            device.online
              ? "bg-trellis-500/10 text-trellis-400"
              : "bg-red-500/10 text-red-400"
          }`}
        >
          <Wifi size={14} />
          {device.online ? "Online" : "Offline"}
        </div>
      </div>

      <div className="grid grid-cols-3 gap-3 mb-6">
        <div className="p-3 bg-zinc-900 rounded-lg border border-zinc-800">
          <span className="text-xs text-zinc-500">RSSI</span>
          <p className="text-lg font-mono text-zinc-100">{device.system.rssi} dBm</p>
        </div>
        <div className="p-3 bg-zinc-900 rounded-lg border border-zinc-800">
          <span className="text-xs text-zinc-500">Free Heap</span>
          <p className="text-lg font-mono text-zinc-100">
            {(device.system.heap_free / 1024).toFixed(0)} KB
          </p>
        </div>
        <div className="p-3 bg-zinc-900 rounded-lg border border-zinc-800">
          <span className="text-xs text-zinc-500">Uptime</span>
          <p className="text-lg font-mono text-zinc-100">
            {Math.floor(device.system.uptime_s / 3600)}h{" "}
            {Math.floor((device.system.uptime_s % 3600) / 60)}m
          </p>
        </div>
      </div>

      <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
        Controls
      </h2>
      <div className="space-y-2">
        {device.capabilities.map(renderControl)}
      </div>
    </div>
  );
}
