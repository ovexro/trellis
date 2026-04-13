import { memo, useCallback, useEffect, useMemo, useState } from "react";
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

interface AnnotationWithTime extends Annotation {
  time: number;
}

interface MetricChartProps {
  deviceId: string;
  metricId: string;
  label: string;
  unit?: string;
  color?: string;
  externalHours?: number;
  onAnnotationClick?: (ann: { timestamp: string; kind: string; label: string }) => void;
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

// Visual cap on markers rendered per chart. 200 × 6 charts × 2 (line + dot) =
// 2400 Recharts children, which is enough to block the main thread on every
// render and cause scroll jank (observed on Greenhouse). 40 per chart is
// plenty for visual density on a 400px-wide plot and drops the total to a
// manageable 480. If the backend returns more than the cap, we evenly
// subsample across the window so the marker distribution still reflects the
// full event stream. The legend still shows every kind that was present in
// the full un-subsampled window.
const MARKER_CAP_PER_CHART = 40;

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

// Evenly pick up to `cap` items from `src`, preserving the first and last
// element so the visual window boundaries are always marked.
function subsample<T>(src: T[], cap: number): T[] {
  if (src.length <= cap) return src;
  const out: T[] = [];
  const step = (src.length - 1) / (cap - 1);
  for (let i = 0; i < cap; i++) {
    out.push(src[Math.round(i * step)]);
  }
  return out;
}

function MetricChartImpl({
  deviceId,
  metricId,
  label,
  unit,
  color = "#22c55e",
  externalHours,
  onAnnotationClick,
}: MetricChartProps) {
  const [data, setData] = useState<ChartPoint[]>([]);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [internalHours, setInternalHours] = useState(1);
  const hours = externalHours ?? internalHours;

  // Metrics refresh is hot — new sensor readings arrive every ~5s from live
  // WS broadcasts, so we re-fetch the line data on an interval. Annotations
  // are cold — they only change when a new event is logged (OTA / state
  // change / error / warn), so they're loaded once per deps change and never
  // on the interval. Splitting these two halves the per-tick Tauri work and,
  // more importantly, stops the expensive annotation re-render (up to 40
  // ReferenceLine + 40 ReferenceDot per chart) from firing every 10s.
  const loadMetrics = useCallback(async () => {
    try {
      const points = await invoke<MetricPoint[]>("get_metrics", {
        deviceId,
        metricId,
        hours,
      });
      setData(
        points.map((p) => ({
          ...p,
          time: parseUtcMs(p.timestamp),
        }))
      );
    } catch (err) {
      console.error("Failed to load metrics:", err);
    }
  }, [deviceId, metricId, hours]);

  const loadAnnotations = useCallback(async () => {
    try {
      const anns = await invoke<Annotation[]>("get_device_annotations", {
        deviceId,
        hours,
      });
      setAnnotations(anns);
    } catch (err) {
      console.error("Failed to load annotations:", err);
    }
  }, [deviceId, hours]);

  // Annotations: load once per deps change, never on the interval.
  useEffect(() => {
    loadAnnotations();
  }, [loadAnnotations]);

  // Metrics: load immediately, then refresh every 10s. Each chart gets a
  // random 0-10s offset on its first interval tick so six charts on a
  // multi-sensor device (e.g. Greenhouse) don't all fire on the same
  // millisecond — that was the "half-second stall every 10s" the user
  // observed after the P1(a) perf pass. Spreading the ticks across the
  // window keeps peak per-tick work at 1 chart's worth instead of 6x.
  useEffect(() => {
    let cancelled = false;
    let interval: ReturnType<typeof setInterval> | null = null;
    loadMetrics();
    const initialOffset = Math.random() * 10000;
    const firstTick = setTimeout(() => {
      if (cancelled) return;
      loadMetrics();
      interval = setInterval(loadMetrics, 10000);
    }, initialOffset);
    return () => {
      cancelled = true;
      clearTimeout(firstTick);
      if (interval !== null) clearInterval(interval);
    };
  }, [loadMetrics]);

  // Loading-state is only used by the empty-state placeholder below. Since
  // we now never flip it to true on a live refresh (only the initial load
  // matters for the placeholder), we can track it as a plain boolean that
  // flips false once the first successful load lands.
  const loading = data.length === 0;

  const formatTime = useCallback(
    (ms: number) => {
      const d = new Date(ms);
      if (isNaN(d.getTime())) return "";
      if (hours <= 1) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
      if (hours <= 24) return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
      return d.toLocaleDateString([], { month: "short", day: "numeric", hour: "2-digit" });
    },
    [hours]
  );

  // All derived state in one memo so it recomputes only when data or
  // annotations change. Scroll and parent re-renders no longer re-run the
  // O(n) filter/sort/set operations.
  const derived = useMemo(() => {
    if (data.length === 0) {
      return {
        yMin: 0,
        yMax: 1,
        markerY: 0.9,
        inWindow: [] as AnnotationWithTime[],
        legendKinds: [] as string[],
      };
    }
    const vals = data.map((p) => p.value);
    const rawMin = Math.min(...vals);
    const rawMax = Math.max(...vals);
    const ySpan = (rawMax - rawMin) || 1;
    const yMin = rawMin - ySpan * 0.1;
    const yMax = rawMax + ySpan * 0.18;
    const markerY = rawMax + ySpan * 0.14;

    const firstTime = data[0].time;
    const lastTime = data[data.length - 1].time;

    // Filter annotations to those inside the visible window.
    let inWindow: AnnotationWithTime[] = [];
    if (data.length >= 2) {
      inWindow = annotations
        .map((a) => ({ ...a, time: parseUtcMs(a.timestamp) }))
        .filter((a) => !isNaN(a.time) && a.time >= firstTime && a.time <= lastTime);
    }

    // Build the legend row from the FULL in-window set, before capping,
    // so "kinds present in window" is accurate even when we subsample the
    // visible markers for performance.
    const presentKinds = new Set(inWindow.map((a) => a.kind));
    const legendKinds: string[] = [];
    LEGEND_ORDER.forEach((k) => {
      if (presentKinds.has(k)) legendKinds.push(k);
    });
    inWindow.forEach((a) => {
      if (!legendKinds.includes(a.kind)) legendKinds.push(a.kind);
    });

    // Cap visible markers so multi-chart pages (e.g. Greenhouse with 4
    // sensors + 2 system metrics = 6 charts) don't paint thousands of
    // SVG elements. See `MARKER_CAP_PER_CHART` comment for rationale.
    const capped = subsample(inWindow, MARKER_CAP_PER_CHART);

    return { yMin, yMax, markerY, inWindow: capped, legendKinds };
  }, [data, annotations]);

  const { yMin, yMax, markerY, inWindow, legendKinds } = derived;

  return (
    <div
      className="bg-zinc-900 border border-zinc-800 rounded-xl p-4"
      // Paint containment: the chart's SVG and its own hover state don't
      // leak paint invalidations into the surrounding page. Critical when
      // multiple charts are stacked and the user scrolls — without this,
      // the browser repaints all charts on every scroll tick.
      style={{ contain: "layout paint" }}
    >
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-semibold text-zinc-300">
          {label}
          {unit && <span className="text-zinc-500 ml-1">({unit})</span>}
        </h3>
        <div className="flex gap-1 items-center">
          {externalHours == null && TIME_RANGES.map((range) => (
            <button
              key={range.hours}
              onClick={() => setInternalHours(range.hours)}
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
              {/* Annotations: one <ReferenceLine> + one <ReferenceDot> per
                  in-window event. Visible markers are capped via subsample()
                  so multi-chart pages stay responsive. Uses Recharts' built-in
                  dot (no custom shape closure) — drops the previous native
                  <title> hover tooltip as a minor trade-off for render speed. */}
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
                    className={onAnnotationClick ? "cursor-pointer" : ""}
                    onClick={
                      onAnnotationClick
                        ? () =>
                            onAnnotationClick({
                              timestamp: a.timestamp,
                              kind: a.kind,
                              label: a.label,
                            })
                        : undefined
                    }
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

// Memoized so DeviceDetail re-renders (e.g. device-store updates from live
// WS events) don't cascade into all 6 charts on pages like Greenhouse. Props
// are all primitives so the default shallow compare is correct.
const MetricChart = memo(MetricChartImpl);
export default MetricChart;
