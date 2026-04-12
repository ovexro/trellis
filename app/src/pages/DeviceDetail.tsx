import { useCallback, useRef } from "react";
import { useParams, useNavigate } from "react-router-dom";
import { ArrowLeft, Wifi, Trash2, ExternalLink } from "lucide-react";
import { invoke } from "@tauri-apps/api/core";
import { useDeviceStore } from "@/stores/deviceStore";
import Switch from "@/components/controls/Switch";
import Slider from "@/components/controls/Slider";
import Sensor from "@/components/controls/Sensor";
import ColorPicker from "@/components/controls/ColorPicker";
import MetricChart from "@/components/charts/MetricChart";
import UptimeTimeline from "@/components/charts/UptimeTimeline";
import DeviceNickname from "@/components/DeviceNickname";
import DeviceLogs, { type DeviceLogsHandle } from "@/components/DeviceLogs";
import DeviceAlerts from "@/components/DeviceAlerts";
import type { Capability } from "@/lib/types";

function SectionHeader({ title }: { title: string }) {
  return (
    <div className="flex items-center gap-2.5 mt-10 mb-4">
      <div className="w-1 h-4 bg-trellis-500 rounded-full" />
      <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
        {title}
      </h2>
    </div>
  );
}

export default function DeviceDetail() {
  const { id } = useParams<{ id: string }>();
  const navigate = useNavigate();
  const { devices, updateCapability } = useDeviceStore();
  const device = devices.find((d) => d.id === id);
  const logsRef = useRef<DeviceLogsHandle>(null);

  const handleAnnotationClick = useCallback(
    (ann: { timestamp: string; kind: string; label: string }) => {
      if (ann.kind === "ota") return;
      logsRef.current?.scrollToLog(ann.timestamp, "events");
    },
    []
  );

  const handleSegmentClick = useCallback(
    (timestamp: string) => {
      logsRef.current?.scrollToLog(timestamp, "state");
    },
    []
  );

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
        deviceId: device.id,
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
              {(cap.value as string) || "\u2014"}
            </p>
          </div>
        );
      default:
        return null;
    }
  };

  const hasSensors = device.capabilities.filter((c) => c.type === "sensor").length > 0;

  return (
    <div>
      <button
        onClick={() => navigate("/")}
        className="flex items-center gap-2 text-sm text-zinc-400 hover:text-zinc-200 mb-6 transition-colors"
      >
        <ArrowLeft size={16} />
        Back to devices
      </button>

      {/* Header */}
      <div className="flex items-start justify-between mb-8">
        <div>
          <DeviceNickname deviceId={device.id} originalName={device.name} />
          <p className="text-sm text-zinc-500 mt-1">
            {device.ip}:{device.port}
            {(device.system.chip || device.platform) && (
              <> &middot; {device.system.chip || device.platform}</>
            )}
            {device.firmware && (
              <> &middot; FW {device.firmware}</>
            )}
            {device.online && (
              <>
                {" "}&middot;{" "}
                <a
                  href={`http://localhost:9090/proxy/${encodeURIComponent(device.id)}/`}
                  target="_blank"
                  rel="noopener noreferrer"
                  className="text-trellis-400 hover:text-trellis-300 inline-flex items-center gap-0.5 transition-colors"
                >
                  Device Dashboard <ExternalLink size={11} />
                </a>
              </>
            )}
          </p>
        </div>
        <div
          className={`flex items-center gap-1.5 px-3 py-1 rounded-full text-sm font-medium ${
            device.online
              ? "bg-trellis-500/10 text-trellis-400"
              : "bg-red-500/10 text-red-400"
          }`}
        >
          <Wifi size={14} />
          {device.online ? "Online" : "Offline"}
        </div>
      </div>

      {/* Two-column layout: Controls + System Stats */}
      <div className="grid grid-cols-1 lg:grid-cols-3 gap-6">
        {/* Controls — takes 2 cols */}
        <div className="lg:col-span-2">
          <h2 className="flex items-center gap-2.5 mb-4">
            <div className="w-1 h-4 bg-trellis-500 rounded-full" />
            <span className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
              Controls
            </span>
          </h2>
          <div className="space-y-2">
            {device.capabilities.map(renderControl)}
            {device.capabilities.length === 0 && (
              <p className="text-sm text-zinc-600 py-4">
                No capabilities reported by this device.
              </p>
            )}
          </div>
        </div>

        {/* System stats — right column */}
        <div className="space-y-3">
          <h2 className="flex items-center gap-2.5 mb-1">
            <div className="w-1 h-4 bg-zinc-600 rounded-full" />
            <span className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
              System
            </span>
          </h2>
          {device.online ? (
            <>
              <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
                <span className="text-[11px] text-zinc-500 uppercase tracking-wider">
                  RSSI
                </span>
                <p className="text-xl font-mono text-zinc-100 -mt-0.5">
                  {device.system.rssi}{" "}
                  <span className="text-sm text-zinc-500">dBm</span>
                </p>
              </div>
              <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
                <span className="text-[11px] text-zinc-500 uppercase tracking-wider">
                  Free Heap
                </span>
                <p className="text-xl font-mono text-zinc-100 -mt-0.5">
                  {(device.system.heap_free / 1024).toFixed(0)}{" "}
                  <span className="text-sm text-zinc-500">KB</span>
                </p>
              </div>
              <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
                <span className="text-[11px] text-zinc-500 uppercase tracking-wider">
                  Uptime
                </span>
                <p className="text-xl font-mono text-zinc-100 -mt-0.5">
                  {Math.floor(device.system.uptime_s / 3600)}h{" "}
                  {Math.floor((device.system.uptime_s % 3600) / 60)}m
                </p>
              </div>
            </>
          ) : (
            <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
              <p className="text-sm text-zinc-500">
                Device is offline. System stats will appear when the device reconnects.
              </p>
              {device.last_seen && (
                <p className="text-xs text-zinc-600 mt-2">
                  Last seen: {new Date(device.last_seen).toLocaleString()}
                </p>
              )}
            </div>
          )}
        </div>
      </div>

      {/* Uptime History */}
      <SectionHeader title="Uptime History" />
      <UptimeTimeline deviceId={device.id} onSegmentClick={handleSegmentClick} />

      {/* Charts */}
      {hasSensors && (
        <>
          <SectionHeader title="Sensor Charts" />
          <div className="space-y-3">
            {device.capabilities
              .filter((c) => c.type === "sensor")
              .map((cap) => (
                <MetricChart
                  key={cap.id}
                  deviceId={device.id}
                  metricId={cap.id}
                  label={cap.label}
                  unit={cap.unit}
                  onAnnotationClick={handleAnnotationClick}
                />
              ))}
          </div>
        </>
      )}

      {/* System Metrics Charts */}
      <SectionHeader title="System Metrics" />
      <div className="space-y-3">
        <MetricChart
          deviceId={device.id}
          metricId="_rssi"
          label="WiFi Signal"
          unit="dBm"
          color="#f59e0b"
          onAnnotationClick={handleAnnotationClick}
        />
        <MetricChart
          deviceId={device.id}
          metricId="_heap"
          label="Free Heap"
          unit="bytes"
          color="#3b82f6"
          onAnnotationClick={handleAnnotationClick}
        />
      </div>

      {/* Alerts */}
      <SectionHeader title="Alerts" />
      <DeviceAlerts
        deviceId={device.id}
        sensorIds={device.capabilities
          .filter((c) => c.type === "sensor")
          .map((c) => ({ id: c.id, label: c.label, unit: c.unit }))}
      />

      {/* Device Logs */}
      <SectionHeader title="Logs" />
      <DeviceLogs key={device.id} deviceId={device.id} ref={logsRef} />

      {/* Remove Device */}
      <div className="mt-12 pt-6 border-t border-zinc-800/50">
        <button
          onClick={async () => {
            if (
              confirm(
                `Remove ${device.name}? This deletes all saved data, metrics, and alerts.`,
              )
            ) {
              await useDeviceStore.getState().removeDevice(device.id);
              navigate("/");
            }
          }}
          className="flex items-center gap-2 text-sm text-red-400/70 hover:text-red-400 transition-colors"
        >
          <Trash2 size={14} />
          Remove device
        </button>
      </div>
    </div>
  );
}
