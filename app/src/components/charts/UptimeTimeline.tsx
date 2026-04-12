import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { invoke } from "@tauri-apps/api/core";

interface Annotation {
  timestamp: string;
  kind: string;
  label: string;
  severity: string;
}

interface Segment {
  kind: string;
  start: number;
  end: number;
  inferred: boolean;
}

interface UptimeTimelineProps {
  deviceId: string;
}

const TIME_RANGES = [
  { label: "1h", hours: 1 },
  { label: "6h", hours: 6 },
  { label: "24h", hours: 24 },
  { label: "7d", hours: 168 },
];

// SVG coordinate system — PAD_L matches MetricChart's YAxis width (40) so the
// ribbon aligns horizontally with the chart data area in the sections below.
const W = 400;
const STRIP_H = 18;
const TOTAL_H = 30;
const PAD_L = 42;
const PAD_R = 10;
const DATA_W = W - PAD_L - PAD_R;

const LEGEND_ORDER = ["online", "offline", "unknown"];

function parseUtcMs(ts: string): number {
  return new Date(ts + "Z").getTime();
}

function segColor(kind: string, inferred: boolean): string {
  if (inferred || kind === "unknown") return "#6b7280";
  if (kind === "online") return "#10b981";
  if (kind === "offline") return "#ef4444";
  return "#6b7280";
}

function kindLabel(kind: string): string {
  if (kind === "online") return "Online";
  if (kind === "offline") return "Offline";
  if (kind === "unknown") return "Unknown";
  return kind;
}

function fmtDuration(seconds: number): string {
  if (seconds < 60) return `${seconds}s`;
  if (seconds < 3600) return `${Math.floor(seconds / 60)}m`;
  if (seconds < 86400)
    return `${Math.floor(seconds / 3600)}h ${Math.floor((seconds % 3600) / 60)}m`;
  return `${Math.floor(seconds / 86400)}d ${Math.floor((seconds % 86400) / 3600)}h`;
}

function fmtAxisLabel(ms: number, hours: number): string {
  const d = new Date(ms);
  if (isNaN(d.getTime())) return "";
  if (hours <= 24)
    return d.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
  return d.toLocaleDateString([], {
    month: "short",
    day: "numeric",
    hour: "2-digit",
  });
}

function segTooltip(seg: Segment): string {
  const durSec = Math.max(0, Math.floor((seg.end - seg.start) / 1000));
  const human = fmtDuration(durSec);
  if (seg.inferred) {
    return `Unknown \u2014 no state transition recorded before this point (${human})`;
  }
  const startStr = new Date(seg.start).toLocaleString();
  const endStr =
    Date.now() - seg.end < 2000 ? "now" : new Date(seg.end).toLocaleString();
  const word = seg.kind === "online" ? "Online" : "Offline";
  return `${word} for ${human} (${startStr} \u2192 ${endStr})`;
}

function UptimeTimelineImpl({ deviceId }: UptimeTimelineProps) {
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [hours, setHours] = useState(1);

  const loadAnnotations = useCallback(async () => {
    try {
      const anns = await invoke<Annotation[]>("get_device_annotations", {
        deviceId,
        hours,
      });
      setAnnotations(anns);
    } catch (err) {
      console.error("Failed to load uptime annotations:", err);
    }
  }, [deviceId, hours]);

  useEffect(() => {
    loadAnnotations();
  }, [loadAnnotations]);

  const derived = useMemo(() => {
    const now = Date.now();
    const windowStart = now - hours * 3600_000;

    const states = annotations
      .filter((a) => a.kind === "online" || a.kind === "offline")
      .map((a) => ({ ...a, ts: parseUtcMs(a.timestamp) }))
      .filter((a) => !isNaN(a.ts) && a.ts >= windowStart && a.ts <= now);

    if (states.length === 0) {
      return {
        segments: [] as Segment[],
        statLine: null as null | {
          pct: string;
          tracked: string;
          trans: string;
        },
        legendKinds: [] as string[],
        now,
        windowStart,
      };
    }

    const segs: Segment[] = [];
    const firstTs = states[0].ts;

    if (firstTs > windowStart) {
      segs.push({
        kind: "unknown",
        start: windowStart,
        end: firstTs,
        inferred: true,
      });
    }

    for (let i = 0; i < states.length; i++) {
      const nextTs = i + 1 < states.length ? states[i + 1].ts : now;
      if (nextTs <= states[i].ts) continue;
      segs.push({
        kind: states[i].kind,
        start: states[i].ts,
        end: nextTs,
        inferred: false,
      });
    }

    let knownMs = 0;
    let onlineMs = 0;
    let transitions = 0;
    for (const s of segs) {
      if (s.inferred) continue;
      const dur = Math.max(0, s.end - s.start);
      knownMs += dur;
      if (s.kind === "online") onlineMs += dur;
      transitions++;
    }

    let statLine: { pct: string; tracked: string; trans: string } | null =
      null;
    if (knownMs > 0) {
      const pct = (onlineMs / knownMs) * 100;
      let pctStr: string;
      if (pct >= 99.95) pctStr = "100%";
      else if (pct > 0 && pct < 0.05) pctStr = "<0.1%";
      else pctStr = pct.toFixed(1) + "%";
      statLine = {
        pct: pctStr,
        tracked: fmtDuration(Math.floor(knownMs / 1000)),
        trans: `${transitions} transition${transitions === 1 ? "" : "s"}`,
      };
    }

    const present = new Set(
      segs.map((s) => (s.inferred ? "unknown" : s.kind))
    );
    const legendKinds = LEGEND_ORDER.filter((k) => present.has(k));

    return { segments: segs, statLine, legendKinds, now, windowStart };
  }, [annotations, hours]);

  const { segments, statLine, legendKinds, now, windowStart } = derived;
  const windowMs = now - windowStart;

  const xPos = (t: number): number => {
    const clamped = Math.max(windowStart, Math.min(now, t));
    return PAD_L + ((clamped - windowStart) / windowMs) * DATA_W;
  };

  return (
    <div
      className="bg-zinc-900 border border-zinc-800 rounded-xl p-4"
      style={{ contain: "layout paint" }}
    >
      <div className="flex items-center justify-between mb-3">
        {statLine ? (
          <div className="flex flex-wrap items-center gap-2 text-xs text-zinc-500 leading-tight">
            <span className="font-semibold text-zinc-200">
              {statLine.pct} online
            </span>
            <span className="opacity-50">&middot;</span>
            <span>{statLine.tracked} tracked</span>
            <span className="opacity-50">&middot;</span>
            <span>{statLine.trans}</span>
          </div>
        ) : (
          <div className="text-xs text-zinc-500 italic">
            No tracked uptime in this window
          </div>
        )}
        <div className="flex gap-1 shrink-0 ml-3">
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
        </div>
      </div>

      <svg
        viewBox={`0 0 ${W} ${TOTAL_H}`}
        preserveAspectRatio="xMidYMid meet"
        className="w-full block"
      >
        {/* Background track */}
        <rect
          x={PAD_L}
          y={0}
          width={DATA_W}
          height={STRIP_H}
          fill="#27272a"
          opacity={segments.length === 0 ? 0.4 : 0.3}
        />

        {/* Colored segments */}
        {segments.map((seg, i) => {
          const x1 = xPos(seg.start);
          const x2 = xPos(seg.end);
          const w = Math.max(0.5, x2 - x1);
          return (
            <g key={i}>
              <title>{segTooltip(seg)}</title>
              <rect
                x={x1}
                y={0}
                width={w}
                height={STRIP_H}
                fill={segColor(seg.kind, seg.inferred)}
              />
            </g>
          );
        })}

        {/* Empty-state label */}
        {segments.length === 0 && (
          <text
            x={PAD_L + DATA_W / 2}
            y={STRIP_H / 2 + 3}
            textAnchor="middle"
            fill="#71717a"
            style={{ fontFamily: "system-ui, sans-serif", fontSize: 9 }}
          >
            No state transitions in this window
          </text>
        )}

        {/* Border */}
        <rect
          x={PAD_L}
          y={0}
          width={DATA_W}
          height={STRIP_H}
          fill="none"
          stroke="#27272a"
          strokeWidth={0.5}
        />

        {/* Axis labels */}
        <text
          x={PAD_L}
          y={STRIP_H + 10}
          textAnchor="start"
          fill="#71717a"
          style={{ fontFamily: "system-ui, sans-serif", fontSize: 8 }}
        >
          {fmtAxisLabel(windowStart, hours)}
        </text>
        <text
          x={W - PAD_R}
          y={STRIP_H + 10}
          textAnchor="end"
          fill="#71717a"
          style={{ fontFamily: "system-ui, sans-serif", fontSize: 8 }}
        >
          now
        </text>
      </svg>

      {/* Legend */}
      {legendKinds.length > 0 && (
        <div className="flex flex-wrap gap-3 mt-2 text-[10px] text-zinc-500">
          {legendKinds.map((k) => (
            <span key={k} className="inline-flex items-center gap-1.5">
              <span
                className="inline-block w-2 h-2 rounded-full"
                style={{ background: segColor(k, k === "unknown") }}
              />
              {kindLabel(k)}
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

const UptimeTimeline = memo(UptimeTimelineImpl);
export default UptimeTimeline;
