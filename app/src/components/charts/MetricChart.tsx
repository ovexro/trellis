import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Download } from "lucide-react";
import {
  LineChart,
  Line,
  XAxis,
  YAxis,
  CartesianGrid,
  Tooltip,
  ResponsiveContainer,
} from "recharts";

interface MetricPoint {
  value: number;
  timestamp: string;
}

interface MetricChartProps {
  deviceId: string;
  metricId: string;
  label: string;
  unit?: string;
  color?: string;
}

const TIME_RANGES = [
  { label: "1h", hours: 1 },
  { label: "6h", hours: 6 },
  { label: "24h", hours: 24 },
  { label: "7d", hours: 168 },
];

export default function MetricChart({
  deviceId,
  metricId,
  label,
  unit,
  color = "#22c55e",
}: MetricChartProps) {
  const [data, setData] = useState<MetricPoint[]>([]);
  const [hours, setHours] = useState(1);
  const [loading, setLoading] = useState(false);

  useEffect(() => {
    loadData();
    const interval = setInterval(loadData, 10000); // Refresh every 10s
    return () => clearInterval(interval);
  }, [deviceId, metricId, hours]);

  const loadData = async () => {
    setLoading(true);
    try {
      const points = await invoke<MetricPoint[]>("get_metrics", {
        deviceId,
        metricId,
        hours,
      });
      setData(points);
    } catch (err) {
      console.error("Failed to load metrics:", err);
    } finally {
      setLoading(false);
    }
  };

  const formatTime = (timestamp: string) => {
    const d = new Date(timestamp + "Z");
    if (hours <= 1) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    if (hours <= 24) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return d.toLocaleDateString([], { month: "short", day: "numeric", hour: "2-digit" });
  };

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-semibold text-zinc-300">
          {label}
          {unit && <span className="text-zinc-500 ml-1">({unit})</span>}
        </h3>
        <div className="flex gap-1 items-center">
          {TIME_RANGES.map((range) => (
            <button
              key={range.hours}
              onClick={() => setHours(range.hours)}
              className={`px-2.5 py-1 rounded-md text-xs min-w-[32px] text-center transition-colors ${
                hours === range.hours
                  ? "bg-trellis-500/20 text-trellis-400"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {range.label}
            </button>
          ))}
          {data.length > 0 && (
            <button
              onClick={async () => {
                try {
                  const csv = await invoke<string>("export_metrics_csv", { deviceId, metricId, hours });
                  const blob = new Blob([csv], { type: "text/csv" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  a.download = `${metricId}_${hours}h.csv`;
                  a.click();
                  URL.revokeObjectURL(url);
                } catch (err) {
                  console.error("Failed to export CSV:", err);
                }
              }}
              className="p-1 rounded text-zinc-600 hover:text-zinc-400 transition-colors ml-1"
              title="Export CSV"
            >
              <Download size={12} />
            </button>
          )}
        </div>
      </div>

      {data.length === 0 ? (
        <div className="h-40 flex flex-col items-center justify-center text-sm text-zinc-500 gap-2">
          <div className="space-y-2 w-full px-8 opacity-30">
            <div className="h-px bg-zinc-700" />
            <div className="h-px bg-zinc-700" />
            <div className="h-px bg-zinc-700" />
          </div>
          <span className="text-xs">{loading ? "Loading..." : "Waiting for data from device..."}</span>
        </div>
      ) : (
        <ResponsiveContainer width="100%" height={160}>
          <LineChart data={data}>
            <CartesianGrid strokeDasharray="3 3" stroke="#27272a" />
            <XAxis
              dataKey="timestamp"
              tickFormatter={formatTime}
              stroke="#52525b"
              tick={{ fontSize: 10 }}
              interval="preserveStartEnd"
            />
            <YAxis stroke="#52525b" tick={{ fontSize: 10 }} width={40} />
            <Tooltip
              contentStyle={{
                backgroundColor: "#18181b",
                border: "1px solid #27272a",
                borderRadius: "8px",
                fontSize: "12px",
              }}
              labelFormatter={(l) => formatTime(String(l))}
              formatter={(value) => [
                `${Number(value).toFixed(1)}${unit ? ` ${unit}` : ""}`,
                label,
              ]}
            />
            <Line
              type="monotone"
              dataKey="value"
              stroke={color}
              strokeWidth={2}
              dot={false}
              activeDot={{ r: 3 }}
            />
          </LineChart>
        </ResponsiveContainer>
      )}
    </div>
  );
}
