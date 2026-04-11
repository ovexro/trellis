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
  ReferenceLine,
  ReferenceDot,
} from "recharts";

interface MetricPoint {
  value: number;
  timestamp: string;
}

interface Annotation {
  timestamp: string;
  kind: string;
  label: string;
  severity: string;
}

interface ChartPoint {
  value: number;
  timestamp: string;
  time: number; // ms since epoch for numeric XAxis
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

// Kind → color and human label. Mirrors `annColor()` / `annLabel()` in
// `app/src-tauri/src/web_ui.html` so the React overlay matches the hand-rolled
// :9090 dashboard exactly (same hex, same label strings).
const ANN_COLOR: Record<string, string> = {
  ota: "#3b82f6",     // blue — firmware upload
  online: "#10b981",  // green — device came back
  offline: "#ef4444", // red — device dropped
  error: "#f59e0b",   // amber — device-reported error
  warn: "#f59e0b",    // amber — device-reported warning
};
const ANN_FALLBACK = "#6b7280";
const ANN_LABEL: Record<string, string> = {
  ota: "OTA",
  online: "Online",
  offline: "Offline",
  error: "Error",
  warn: "Warning",
};
// Stable legend order matches the :9090 dashboard.
const LEGEND_ORDER = ["ota", "online", "offline", "warn", "error"];

function annColor(kind: string): string {
  return ANN_COLOR[kind] ?? ANN_FALLBACK;
}
function annLabelText(kind: string): string {
  return ANN_LABEL[kind] ?? kind;
}

// SQLite timestamps are UTC but lack the `Z` suffix — append it so the
// browser parses them correctly regardless of local TZ.
function parseUtcMs(ts: string): number {
  return new Date(ts + "Z").getTime();
}

export default function MetricChart({
  deviceId,
  metricId,
  label,
  unit,
  color = "#22c55e",
}: MetricChartProps) {
  const [data, setData] = useState<ChartPoint[]>([]);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
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
      // Fetch metrics + annotations in parallel — single render pass, no
      // flicker when annotations land a tick after the line.
      const [points, anns] = await Promise.all([
        invoke<MetricPoint[]>("get_metrics", { deviceId, metricId, hours }),
        invoke<Annotation[]>("get_device_annotations", { deviceId, hours }),
      ]);
      setData(
        points.map((p) => ({
          ...p,
          time: parseUtcMs(p.timestamp),
        }))
      );
      setAnnotations(anns);
    } catch (err) {
      console.error("Failed to load metrics:", err);
    } finally {
      setLoading(false);
    }
  };

  const formatTime = (ms: number) => {
    const d = new Date(ms);
    if (isNaN(d.getTime())) return "";
    if (hours <= 1) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
    if (hours <= 24) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
    return d.toLocaleDateString([], { month: "short", day: "numeric", hour: "2-digit" });
  };

  const formatTooltipTime = (ms: number) => {
    const d = new Date(ms);
    if (isNaN(d.getTime())) return "";
    const months = ["Jan","Feb","Mar","Apr","May","Jun","Jul","Aug","Sep","Oct","Nov","Dec"];
    const pad = (n: number) => String(n).padStart(2, "0");
    return `${months[d.getMonth()]} ${d.getDate()} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
  };

  // Derive the plot window + y-axis domain from the data points themselves.
  // We need explicit y bounds (with headroom at the top) so ReferenceDot
  // markers can anchor at a predictable "near top of plot" y value instead
  // of being clipped by the auto-domain.
  const vals = data.map((p) => p.value);
  const rawMin = vals.length ? Math.min(...vals) : 0;
  const rawMax = vals.length ? Math.max(...vals) : 1;
  const ySpan = (rawMax - rawMin) || 1;
  const yMin = rawMin - ySpan * 0.1;
  const yMax = rawMax + ySpan * 0.18; // extra headroom for the marker row
  const markerY = rawMax + ySpan * 0.14;

  const firstTime = data.length ? data[0].time : 0;
  const lastTime = data.length ? data[data.length - 1].time : 0;

  // Filter annotations to those inside the visible window. Also drop any
  // whose timestamp fails to parse. `inWindow` is what feeds the legend
  // row — only kinds that actually landed on the chart are shown.
  const inWindow = data.length >= 2
    ? annotations
        .map((a) => ({ ...a, time: parseUtcMs(a.timestamp) }))
        .filter((a) => !isNaN(a.time) && a.time >= firstTime && a.time <= lastTime)
    : [];

  // Build the legend row — stable order, unknown kinds appended at the end.
  const presentKinds = new Set(inWindow.map((a) => a.kind));
  const legendKinds: string[] = [];
  LEGEND_ORDER.forEach((k) => { if (presentKinds.has(k)) legendKinds.push(k); });
  inWindow.forEach((a) => {
    if (!legendKinds.includes(a.kind)) legendKinds.push(a.kind);
  });

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
        <>
          <ResponsiveContainer width="100%" height={160}>
            <LineChart data={data}>
              <CartesianGrid strokeDasharray="3 3" stroke="#27272a" />
              <XAxis
                dataKey="time"
                type="number"
                domain={["dataMin", "dataMax"]}
                scale="time"
                tickFormatter={formatTime}
                stroke="#52525b"
                tick={{ fontSize: 10 }}
                interval="preserveStartEnd"
              />
              <YAxis
                domain={[yMin, yMax]}
                stroke="#52525b"
                tick={{ fontSize: 10 }}
                width={40}
                allowDataOverflow={false}
              />
              <Tooltip
                contentStyle={{
                  backgroundColor: "#18181b",
                  border: "1px solid #27272a",
                  borderRadius: "8px",
                  fontSize: "12px",
                }}
                labelFormatter={(l) => formatTime(Number(l))}
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
                isAnimationActive={false}
              />
              {/* Annotations: vertical dashed line (decorative) + circle marker
                  at the top of the plot area. One <ReferenceLine> and one
                  <ReferenceDot> per in-window annotation. Colors match the
                  :9090 palette and the native SVG <title> element provides
                  the hover tooltip text without wiring Recharts Tooltip. */}
              {inWindow.map((a, i) => {
                const c = annColor(a.kind);
                return (
                  <ReferenceLine
                    key={`ann-line-${i}`}
                    x={a.time}
                    stroke={c}
                    strokeDasharray="2 2"
                    strokeOpacity={0.7}
                    ifOverflow="discard"
                  />
                );
              })}
              {inWindow.map((a, i) => {
                const c = annColor(a.kind);
                const tipText = `${annLabelText(a.kind)} — ${a.label || ""} (${formatTooltipTime(a.time)})`;
                return (
                  <ReferenceDot
                    key={`ann-dot-${i}`}
                    x={a.time}
                    y={markerY}
                    r={3}
                    fill={c}
                    stroke="#0a0a0a"
                    strokeWidth={1}
                    ifOverflow="extendDomain"
                    shape={(props: any) => (
                      <g className="chart-annotation" style={{ cursor: "pointer" }}>
                        <title>{tipText}</title>
                        {/* 6px transparent hit target for touch */}
                        <circle cx={props.cx} cy={props.cy} r={6} fill="transparent" />
                        <circle
                          cx={props.cx}
                          cy={props.cy}
                          r={3}
                          fill={c}
                          stroke="#0a0a0a"
                          strokeWidth={1}
                        />
                      </g>
                    )}
                  />
                );
              })}
            </LineChart>
          </ResponsiveContainer>

          {legendKinds.length > 0 && (
            <div className="flex flex-wrap gap-3 mt-2 text-[10px] text-zinc-500">
              {legendKinds.map((k) => (
                <span key={k} className="inline-flex items-center gap-1.5">
                  <span
                    className="inline-block w-2 h-2 rounded-full"
                    style={{ background: annColor(k) }}
                  />
                  {annLabelText(k)}
                </span>
              ))}
            </div>
          )}
        </>
      )}
    </div>
  );
}
