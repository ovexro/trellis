import { useState } from "react";
import { useNavigate } from "react-router-dom";
import { Wifi, WifiOff } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";
import MetricChart from "@/components/charts/MetricChart";
import UptimeTimeline from "@/components/charts/UptimeTimeline";

const TIME_RANGES = [
  { label: "1h", hours: 1 },
  { label: "6h", hours: 6 },
  { label: "24h", hours: 24 },
  { label: "7d", hours: 168 },
];

export default function Metrics() {
  const { devices } = useDeviceStore();
  const navigate = useNavigate();
  const [hours, setHours] = useState(24);

  const sorted = [...devices].sort(
    (a, b) => (a.sort_order ?? 0) - (b.sort_order ?? 0)
  );
  const onlineCount = devices.filter((d) => d.online).length;

  return (
    <div>
      {/* Header */}
      <div className="flex items-center justify-between mb-6">
        <div>
          <h1 className="text-xl font-bold text-zinc-100">Metrics</h1>
          <p className="text-sm text-zinc-500 mt-0.5">
            {devices.length} device{devices.length !== 1 ? "s" : ""}
            {devices.length > 0 && (
              <span className="ml-1.5">
                &middot;{" "}
                <span className={onlineCount > 0 ? "text-emerald-500" : ""}>
                  {onlineCount} online
                </span>
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-1">
          {TIME_RANGES.map((range) => (
            <button
              key={range.hours}
              onClick={() => setHours(range.hours)}
              className={`px-3 py-1.5 rounded-lg text-sm transition-colors ${
                hours === range.hours
                  ? "bg-trellis-500/20 text-trellis-400 font-medium"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {range.label}
            </button>
          ))}
        </div>
      </div>

      {/* Device sections */}
      {sorted.length === 0 ? (
        <div className="flex flex-col items-center justify-center py-24 text-zinc-500">
          <p className="text-sm">No devices found.</p>
          <p className="text-xs mt-1">
            Devices will appear here once discovered.
          </p>
        </div>
      ) : (
        <div className="space-y-6">
          {sorted.map((device) => {
            const sensors = device.capabilities.filter(
              (c) => c.type === "sensor"
            );
            return (
              <div
                key={device.id}
                className="border border-zinc-800 rounded-xl bg-zinc-900/30 overflow-hidden"
              >
                {/* Device header */}
                <div className="flex items-center justify-between px-5 py-3 border-b border-zinc-800/50">
                  <div className="flex items-center gap-3">
                    {device.online ? (
                      <Wifi size={14} className="text-trellis-400" />
                    ) : (
                      <WifiOff size={14} className="text-zinc-600" />
                    )}
                    <span className="text-sm font-medium text-zinc-200">
                      {device.nickname || device.name}
                    </span>
                    {device.firmware && (
                      <span className="text-xs text-zinc-600">
                        v{device.firmware}
                      </span>
                    )}
                  </div>
                  <button
                    onClick={() => navigate(`/device/${device.id}`)}
                    className="text-xs text-trellis-400 hover:text-trellis-300 transition-colors"
                  >
                    Details
                  </button>
                </div>

                {/* Charts */}
                <div className="p-4 space-y-3">
                  {/* Uptime ribbon */}
                  <UptimeTimeline
                    deviceId={device.id}
                    externalHours={hours}
                  />

                  {/* 2-column chart grid */}
                  <div className="grid grid-cols-1 lg:grid-cols-2 gap-3">
                    <MetricChart
                      deviceId={device.id}
                      metricId="_rssi"
                      label="WiFi Signal"
                      unit="dBm"
                      color="#f59e0b"
                      externalHours={hours}
                      deviceName={device.nickname || device.name}
                      deviceOnline={device.online}
                    />
                    <MetricChart
                      deviceId={device.id}
                      metricId="_heap"
                      label="Free Heap"
                      unit="bytes"
                      color="#3b82f6"
                      externalHours={hours}
                      deviceName={device.nickname || device.name}
                      deviceOnline={device.online}
                    />
                    {sensors.map((cap) => (
                      <MetricChart
                        key={cap.id}
                        deviceId={device.id}
                        metricId={cap.id}
                        label={cap.label}
                        unit={cap.unit}
                        externalHours={hours}
                        deviceName={device.nickname || device.name}
                        deviceOnline={device.online}
                      />
                    ))}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
