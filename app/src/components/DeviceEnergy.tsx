import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Zap } from "lucide-react";

interface CapabilityEnergy {
  capability_id: string;
  nameplate_watts: number;
  on_time_seconds: number;
  wh: number;
  tracked_since: string | null;
}

interface DeviceEnergyReport {
  window_hours: number;
  total_wh: number;
  capabilities: CapabilityEnergy[];
}

interface CapabilityLabel {
  id: string;
  label: string;
}

interface DeviceEnergyProps {
  deviceId: string;
  capabilityLabels: CapabilityLabel[];
  costPerKwh: number | null;
  currency: string;
}

const RANGES = [
  { label: "24h", hours: 24 },
  { label: "7d", hours: 24 * 7 },
  { label: "30d", hours: 24 * 30 },
];

function fmtEnergy(wh: number): string {
  if (wh < 1) return `${wh.toFixed(2)} Wh`;
  if (wh < 1000) return `${wh.toFixed(1)} Wh`;
  return `${(wh / 1000).toFixed(2)} kWh`;
}

function fmtOnTime(s: number): string {
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m`;
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return m > 0 ? `${h}h ${m}m` : `${h}h`;
}

function fmtTrackedSince(ts: string | null): string | null {
  if (!ts) return null;
  const d = new Date(ts + "Z");
  if (isNaN(d.getTime())) return null;
  return d.toLocaleDateString([], {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

function fmtCost(wh: number, costPerKwh: number, currency: string): string {
  const kwh = wh / 1000;
  const cost = kwh * costPerKwh;
  return `≈ ${currency} ${cost.toFixed(cost < 1 ? 3 : 2)}`;
}

export default function DeviceEnergy({
  deviceId,
  capabilityLabels,
  costPerKwh,
  currency,
}: DeviceEnergyProps) {
  const [report, setReport] = useState<DeviceEnergyReport | null>(null);
  const [hours, setHours] = useState(24);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    invoke<DeviceEnergyReport>("get_device_energy", { deviceId, hours })
      .then((r) => {
        if (!cancelled) setReport(r);
      })
      .catch((err) => console.error("Failed to load energy report:", err))
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [deviceId, hours]);

  if (!report) {
    if (loading) {
      return (
        <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
          <p className="text-sm text-zinc-600">Loading energy report&hellip;</p>
        </div>
      );
    }
    return null;
  }

  // Hide the whole card when no switch has nameplate_watts set.
  if (report.capabilities.length === 0) return null;

  const labelFor = (capId: string) =>
    capabilityLabels.find((c) => c.id === capId)?.label ?? capId;

  const earliestTracked = report.capabilities
    .map((c) => c.tracked_since)
    .filter((t): t is string => !!t)
    .sort()[0];
  const trackedHint = fmtTrackedSince(earliestTracked ?? null);

  const showBreakdown = report.capabilities.length > 1;

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
      <div className="flex items-center justify-between mb-3">
        <div className="flex items-baseline gap-2">
          <Zap size={14} className="text-amber-400/80 self-center" />
          <span className="text-xl font-mono text-zinc-100 tabular-nums">
            {fmtEnergy(report.total_wh)}
          </span>
          {costPerKwh != null && costPerKwh > 0 && report.total_wh > 0 && (
            <span className="text-sm text-zinc-500 font-mono">
              {fmtCost(report.total_wh, costPerKwh, currency)}
            </span>
          )}
        </div>
        <div className="flex gap-1 shrink-0 ml-3">
          {RANGES.map((range) => (
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

      {showBreakdown && (
        <div className="space-y-1.5 mt-3 pt-3 border-t border-zinc-800/60">
          {report.capabilities.map((c) => (
            <div
              key={c.capability_id}
              className="flex items-center justify-between text-xs"
            >
              <span className="text-zinc-400">{labelFor(c.capability_id)}</span>
              <div className="flex items-center gap-3 font-mono tabular-nums">
                <span className="text-zinc-600">
                  {fmtOnTime(c.on_time_seconds)} on
                </span>
                <span className="text-zinc-300 min-w-[70px] text-right">
                  {fmtEnergy(c.wh)}
                </span>
              </div>
            </div>
          ))}
        </div>
      )}

      {trackedHint && (
        <p className="text-[11px] text-zinc-600 mt-3">
          Tracking since {trackedHint}
        </p>
      )}
    </div>
  );
}
