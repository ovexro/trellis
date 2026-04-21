import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { invoke } from "@tauri-apps/api/core";
import { Download } from "lucide-react";

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
  time: number; // ms since epoch
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
  deviceName?: string;
  deviceOnline?: boolean;
  onAnnotationClick?: (ann: {
    timestamp: string;
    kind: string;
    label: string;
  }) => void;
}

const TIME_RANGES = [
  { label: "1h", hours: 1 },
  { label: "6h", hours: 6 },
  { label: "24h", hours: 24 },
  { label: "7d", hours: 168 },
];

const ANN_COLOR: Record<string, string> = {
  ota: "#3b82f6",
  online: "#10b981",
  offline: "#ef4444",
  error: "#f59e0b",
  warn: "#f59e0b",
  reset_brownout: "#dc2626",
  reset_fault: "#a855f7",
  reset: "#64748b",
};
const ANN_FALLBACK = "#6b7280";
const ANN_LABEL: Record<string, string> = {
  ota: "OTA",
  online: "Online",
  offline: "Offline",
  error: "Error",
  warn: "Warning",
  reset_brownout: "Brownout",
  reset_fault: "Fault reset",
  reset: "Reset",
};
const LEGEND_ORDER = [
  "ota",
  "online",
  "offline",
  "warn",
  "error",
  "reset_brownout",
  "reset_fault",
  "reset",
];

// Chart dimensions in viewBox units. CSS scales via width:100%.
const W = 400;
const H = 160;
const PAD = { top: 8, right: 10, bottom: 22, left: 42 } as const;
const PW = W - PAD.left - PAD.right;
const PH = H - PAD.top - PAD.bottom;

// Cap points to keep SVG responsive on long time ranges.
const MAX_POINTS = 200;

function annColor(kind: string): string {
  return ANN_COLOR[kind] ?? ANN_FALLBACK;
}
function annLabelText(kind: string): string {
  return ANN_LABEL[kind] ?? kind;
}

function parseUtcMs(ts: string): number {
  return new Date(ts + "Z").getTime();
}

function fmtVal(v: number): string {
  if (Math.abs(v) >= 1e6) return (v / 1e6).toFixed(1) + "M";
  if (Math.abs(v) >= 1e3) return (v / 1e3).toFixed(1) + "k";
  if (v === Math.floor(v)) return String(v);
  return v.toFixed(1);
}

// Downsample by averaging buckets — matches web_ui.html downsampleMetrics().
function downsample(points: ChartPoint[], maxPts: number): ChartPoint[] {
  if (points.length <= maxPts) return points;
  const step = points.length / maxPts;
  const result: ChartPoint[] = [];
  for (let i = 0; i < maxPts; i++) {
    const start = Math.floor(i * step);
    const end = Math.floor((i + 1) * step);
    let sum = 0;
    for (let j = start; j < end; j++) sum += points[j].value;
    const mid = Math.floor((start + end) / 2);
    result.push({
      value: sum / (end - start),
      timestamp: points[mid].timestamp,
      time: points[mid].time,
    });
  }
  return result;
}

interface Coord {
  x: number;
  y: number;
  value: number;
  time: number;
  timestamp: string;
}

function MetricChartImpl({
  deviceId,
  metricId,
  label,
  unit,
  color = "#22c55e",
  externalHours,
  deviceName,
  deviceOnline,
  onAnnotationClick,
}: MetricChartProps) {
  const [data, setData] = useState<ChartPoint[]>([]);
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [fetchedOnce, setFetchedOnce] = useState(false);
  const [internalHours, setInternalHours] = useState(1);
  const hours = externalHours ?? internalHours;

  // Refs for imperative hover updates — avoids re-renders on mousemove.
  const svgRef = useRef<SVGSVGElement>(null);
  const cursorRef = useRef<SVGLineElement>(null);
  const dotRef = useRef<SVGCircleElement>(null);
  const tipRef = useRef<HTMLDivElement>(null);
  // Store computed coords for the hover handler.
  const coordsRef = useRef<Coord[]>([]);

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
      setFetchedOnce(true);
    } catch (err) {
      console.error("Failed to load metrics:", err);
      setFetchedOnce(true);
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

  useEffect(() => {
    loadAnnotations();
  }, [loadAnnotations]);

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

  const formatTime = useCallback(
    (ms: number) => {
      const d = new Date(ms);
      if (isNaN(d.getTime())) return "";
      if (hours <= 1)
        return d.toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
          second: "2-digit",
        });
      if (hours <= 24)
        return d.toLocaleTimeString([], {
          hour: "2-digit",
          minute: "2-digit",
        });
      return d.toLocaleDateString([], {
        month: "short",
        day: "numeric",
        hour: "2-digit",
      });
    },
    [hours]
  );

  const formatTooltipTime = useCallback((ms: number) => {
    const d = new Date(ms);
    if (isNaN(d.getTime())) return "";
    const months = [
      "Jan", "Feb", "Mar", "Apr", "May", "Jun",
      "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    return (
      months[d.getMonth()] +
      " " +
      d.getDate() +
      " " +
      String(d.getHours()).padStart(2, "0") +
      ":" +
      String(d.getMinutes()).padStart(2, "0") +
      ":" +
      String(d.getSeconds()).padStart(2, "0")
    );
  }, []);

  // All derived chart geometry in one memo.
  const derived = useMemo(() => {
    if (data.length < 2) {
      coordsRef.current = [];
      return {
        coords: [] as Coord[],
        lineStr: "",
        fillStr: "",
        yTicks: [] as { val: number; y: number }[],
        xTicks: [] as { time: number; x: number }[],
        inWindow: [] as AnnotationWithTime[],
        legendKinds: [] as string[],
      };
    }

    const sampled = downsample(data, MAX_POINTS);
    const vals = sampled.map((p) => p.value);
    const rawMin = Math.min(...vals);
    const rawMax = Math.max(...vals);
    const ySpan = rawMax - rawMin || 1;
    const yMin = rawMin - ySpan * 0.1;
    const yMax = rawMax + ySpan * 0.1;
    const yRange = yMax - yMin;

    const firstTime = sampled[0].time;
    const lastTime = sampled[sampled.length - 1].time;
    const tRange = lastTime - firstTime || 1;

    // Compute pixel coords for each data point.
    const coords: Coord[] = sampled.map((p) => ({
      x: PAD.left + ((p.time - firstTime) / tRange) * PW,
      y: PAD.top + PH - ((p.value - yMin) / yRange) * PH,
      value: p.value,
      time: p.time,
      timestamp: p.timestamp,
    }));
    coordsRef.current = coords;

    const lineStr = coords
      .map((c) => c.x.toFixed(1) + "," + c.y.toFixed(1))
      .join(" ");
    const fillStr =
      coords[0].x.toFixed(1) +
      "," +
      (PAD.top + PH) +
      " " +
      lineStr +
      " " +
      coords[coords.length - 1].x.toFixed(1) +
      "," +
      (PAD.top + PH);

    // Y-axis grid (4 lines)
    const yTicks: { val: number; y: number }[] = [];
    for (let i = 0; i <= 3; i++) {
      const val = yMin + (yRange * i) / 3;
      const y = PAD.top + PH - (i / 3) * PH;
      yTicks.push({ val, y });
    }

    // X-axis labels (5 ticks, time-based)
    const xTicks: { time: number; x: number }[] = [];
    for (let i = 0; i <= 4; i++) {
      const t = firstTime + (tRange * i) / 4;
      const x = PAD.left + (i / 4) * PW;
      xTicks.push({ time: t, x });
    }

    // Annotations in the visible window.
    let inWindow: AnnotationWithTime[] = [];
    if (lastTime > firstTime) {
      inWindow = annotations
        .map((a) => ({ ...a, time: parseUtcMs(a.timestamp) }))
        .filter(
          (a) =>
            !isNaN(a.time) && a.time >= firstTime && a.time <= lastTime
        );
    }

    // Legend kinds from full set before any cap.
    const presentKinds = new Set(inWindow.map((a) => a.kind));
    const legendKinds: string[] = [];
    LEGEND_ORDER.forEach((k) => {
      if (presentKinds.has(k)) legendKinds.push(k);
    });
    inWindow.forEach((a) => {
      if (!legendKinds.includes(a.kind)) legendKinds.push(a.kind);
    });

    return { coords, lineStr, fillStr, yTicks, xTicks, inWindow, legendKinds };
  }, [data, annotations]);

  const { lineStr, fillStr, yTicks, xTicks, inWindow, legendKinds } = derived;

  // Hover handler — imperative DOM updates via refs, no state/re-render.
  const handlePointerMove = useCallback(
    (clientX: number) => {
      const svg = svgRef.current;
      const cursor = cursorRef.current;
      const dot = dotRef.current;
      const tip = tipRef.current;
      const coords = coordsRef.current;
      if (!svg || !cursor || !dot || !tip || coords.length === 0) return;

      const rect = svg.getBoundingClientRect();
      const scaleX = W / rect.width;
      const localX = (clientX - rect.left) * scaleX;

      // Find nearest data point.
      let nearest = 0;
      let minDist = Infinity;
      for (let i = 0; i < coords.length; i++) {
        const dist = Math.abs(coords[i].x - localX);
        if (dist < minDist) {
          minDist = dist;
          nearest = i;
        }
      }
      const c = coords[nearest];

      cursor.setAttribute("x1", c.x.toFixed(1));
      cursor.setAttribute("x2", c.x.toFixed(1));
      cursor.setAttribute("opacity", "1");
      dot.setAttribute("cx", c.x.toFixed(1));
      dot.setAttribute("cy", c.y.toFixed(1));
      dot.setAttribute("opacity", "1");

      const unitStr = unit ? " " + unit : "";
      tip.innerHTML =
        `<div style="font-weight:500">${fmtVal(c.value)}${unitStr}</div>` +
        `<div style="color:#71717a">${formatTooltipTime(c.time)}</div>`;
      tip.style.display = "";

      const tipX = (c.x / W) * rect.width;
      const tipY = (c.y / H) * rect.height;
      tip.style.left = Math.min(tipX + 8, rect.width - 120) + "px";
      tip.style.top = Math.max(0, tipY - 44) + "px";
    },
    [unit, formatTooltipTime]
  );

  const handlePointerLeave = useCallback(() => {
    cursorRef.current?.setAttribute("opacity", "0");
    dotRef.current?.setAttribute("opacity", "0");
    if (tipRef.current) tipRef.current.style.display = "none";
  }, []);

  // Compute annotation x-positions using the time range from sampled data.
  const annPositions = useMemo(() => {
    if (data.length < 2 || inWindow.length === 0) return [];
    const sampled = downsample(data, MAX_POINTS);
    const firstTime = sampled[0].time;
    const lastTime = sampled[sampled.length - 1].time;
    const tRange = lastTime - firstTime || 1;
    return inWindow.map((a) => ({
      ...a,
      ax: PAD.left + ((a.time - firstTime) / tRange) * PW,
    }));
  }, [data, inWindow]);

  const loading = !fetchedOnce;
  const hasData = data.length >= 2;

  return (
    <div
      className="bg-zinc-900 border border-zinc-800 rounded-xl p-4"
      style={{ contain: "layout paint" }}
    >
      {/* Header: label, range picker, CSV export */}
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-semibold text-zinc-300">
          {label}
          {unit && <span className="text-zinc-500 ml-1">({unit})</span>}
        </h3>
        <div className="flex gap-1 items-center">
          {externalHours == null &&
            TIME_RANGES.map((range) => (
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
                  const csv = await invoke<string>("export_metrics_csv", {
                    deviceId,
                    metricId,
                    hours,
                  });
                  const blob = new Blob([csv], { type: "text/csv" });
                  const url = URL.createObjectURL(blob);
                  const a = document.createElement("a");
                  a.href = url;
                  const safeName = deviceName
                    ? deviceName.replace(/[^a-zA-Z0-9_-]/g, "_") + "_"
                    : "";
                  a.download = `${safeName}${metricId}_${hours}h.csv`;
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

      {/* Chart area */}
      {!hasData ? (
        <div className="h-40 flex flex-col items-center justify-center text-sm text-zinc-500 gap-2">
          <div className="space-y-2 w-full px-8 opacity-30">
            <div className="h-px bg-zinc-700" />
            <div className="h-px bg-zinc-700" />
            <div className="h-px bg-zinc-700" />
          </div>
          <span className="text-xs">
            {loading
              ? "Loading\u2026"
              : deviceOnline === false
                ? "Device is offline \u2014 no data in this range"
                : "Waiting for data from device\u2026"}
          </span>
        </div>
      ) : (
        <>
          <div style={{ position: "relative" }}>
            <svg
              ref={svgRef}
              viewBox={`0 0 ${W} ${H}`}
              preserveAspectRatio="xMidYMid meet"
              style={{
                display: "block",
                width: "100%",
                touchAction: "pan-y pinch-zoom",
              }}
            >
              {/* Y-axis grid lines + labels */}
              {yTicks.map((t, i) => (
                <g key={`y${i}`}>
                  <line
                    x1={PAD.left}
                    y1={t.y}
                    x2={W - PAD.right}
                    y2={t.y}
                    stroke="#27272a"
                    strokeWidth="0.5"
                  />
                  <text
                    x={PAD.left - 5}
                    y={t.y + 3}
                    textAnchor="end"
                    fill="#52525b"
                    fontSize="8"
                    fontFamily="system-ui, sans-serif"
                  >
                    {fmtVal(t.val)}
                  </text>
                </g>
              ))}

              {/* X-axis labels */}
              {xTicks.map((t, i) => (
                <text
                  key={`x${i}`}
                  x={t.x}
                  y={H - 4}
                  textAnchor="middle"
                  fill="#52525b"
                  fontSize="8"
                  fontFamily="system-ui, sans-serif"
                >
                  {formatTime(t.time)}
                </text>
              ))}

              {/* Filled area under line */}
              <polyline
                points={fillStr}
                fill={color}
                opacity="0.1"
                stroke="none"
              />

              {/* Data line */}
              <polyline
                points={lineStr}
                fill="none"
                stroke={color}
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />

              {/* Annotation dashed lines (below hover rect) */}
              {annPositions.map((a, i) => (
                <line
                  key={`al${i}`}
                  x1={a.ax}
                  y1={PAD.top}
                  x2={a.ax}
                  y2={PAD.top + PH}
                  stroke={annColor(a.kind)}
                  strokeWidth="1"
                  strokeDasharray="2,2"
                  opacity="0.7"
                />
              ))}

              {/* Hover crosshair + dot (hidden by default) */}
              <line
                ref={cursorRef}
                x1="0"
                y1={PAD.top}
                x2="0"
                y2={PAD.top + PH}
                stroke="#71717a"
                strokeWidth="0.5"
                strokeDasharray="3,3"
                opacity="0"
              />
              <circle
                ref={dotRef}
                cx="0"
                cy="0"
                r="3"
                fill={color}
                opacity="0"
              />

              {/* Invisible hover target rect */}
              <rect
                x={PAD.left}
                y={PAD.top}
                width={PW}
                height={PH}
                fill="transparent"
                style={{ cursor: "crosshair" }}
                onMouseMove={(e) => handlePointerMove(e.clientX)}
                onMouseLeave={handlePointerLeave}
                onTouchMove={(e) => {
                  e.preventDefault();
                  handlePointerMove(e.touches[0].clientX);
                }}
                onTouchEnd={handlePointerLeave}
              />

              {/* Annotation markers (above hover rect for click priority) */}
              {annPositions.map((a, i) => (
                <g
                  key={`am${i}`}
                  className={onAnnotationClick ? "cursor-pointer" : ""}
                  onClick={
                    onAnnotationClick
                      ? (e) => {
                          e.stopPropagation();
                          onAnnotationClick({
                            timestamp: a.timestamp,
                            kind: a.kind,
                            label: a.label,
                          });
                        }
                      : undefined
                  }
                >
                  <title>
                    {annLabelText(a.kind)} — {a.label} ({formatTooltipTime(a.time)})
                  </title>
                  {/* Invisible 6px hit target for touch */}
                  <circle
                    cx={a.ax}
                    cy={PAD.top + 2}
                    r="6"
                    fill="transparent"
                  />
                  <circle
                    cx={a.ax}
                    cy={PAD.top + 2}
                    r="3"
                    fill={annColor(a.kind)}
                    stroke="#0a0a0a"
                    strokeWidth="1"
                  />
                </g>
              ))}
            </svg>

            {/* Tooltip (positioned absolutely over the SVG) */}
            <div
              ref={tipRef}
              style={{
                display: "none",
                position: "absolute",
                background: "#18181b",
                border: "1px solid #27272a",
                borderRadius: "8px",
                padding: "0.35rem 0.5rem",
                fontSize: "12px",
                pointerEvents: "none",
                zIndex: 10,
                whiteSpace: "nowrap",
              }}
            />
          </div>

          {/* Legend */}
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

const MetricChart = memo(MetricChartImpl);
export default MetricChart;
