import { useEffect, useState, useCallback } from "react";
import { invoke } from "@tauri-apps/api/core";
import { useNavigate } from "react-router-dom";
import {
  CheckCircle2,
  AlertTriangle,
  XCircle,
  RefreshCw,
  ArrowRight,
} from "lucide-react";
import type { FleetReport, FleetOverall } from "@/lib/types";

type Filter = FleetOverall | null;

const TILE_LIMIT = 6;

export default function FleetHealth() {
  const navigate = useNavigate();
  const [report, setReport] = useState<FleetReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [filter, setFilter] = useState<Filter>(null);

  const run = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await invoke<FleetReport>("diagnose_fleet");
      setReport(r);
      // Default the filter to the most urgent bucket that has anything in it.
      if (r.unhealthy > 0) setFilter("unhealthy");
      else if (r.attention > 0) setFilter("attention");
      else setFilter(null);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    run();
  }, [run]);

  if (loading && !report) {
    return (
      <div className="p-4 bg-zinc-900/50 border border-zinc-800/50 rounded-lg">
        <p className="text-sm text-zinc-500 flex items-center gap-2">
          <RefreshCw size={14} className="animate-spin" />
          Running fleet health checks…
        </p>
      </div>
    );
  }

  if (error && !report) {
    return (
      <div className="p-4 bg-red-500/5 border border-red-500/20 rounded-lg flex items-center justify-between">
        <p className="text-sm text-red-300">Fleet diagnostics failed: {error}</p>
        <button
          onClick={run}
          className="flex items-center gap-1.5 px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded-md text-xs transition-colors"
        >
          <RefreshCw size={12} />
          Retry
        </button>
      </div>
    );
  }

  if (!report) return null;

  if (report.total === 0) return null;

  const visibleDevices =
    filter === null
      ? []
      : report.devices.filter((d) => d.overall === filter).slice(0, TILE_LIMIT);

  const hiddenCount =
    filter === null
      ? 0
      : report.devices.filter((d) => d.overall === filter).length -
        visibleDevices.length;

  return (
    <div className="bg-zinc-900/50 border border-zinc-800/50 rounded-lg p-4 space-y-3">
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-semibold text-zinc-300 uppercase tracking-wide">
          Fleet Health
        </h2>
        <button
          onClick={run}
          disabled={loading}
          className="p-1.5 rounded-md hover:bg-zinc-800 text-zinc-500 hover:text-zinc-300 transition-colors disabled:opacity-50"
          title="Re-run fleet diagnostics"
        >
          <RefreshCw size={13} className={loading ? "animate-spin" : ""} />
        </button>
      </div>

      <div className="grid grid-cols-3 gap-3">
        <HealthTile
          level="good"
          count={report.good}
          active={filter === "good"}
          onClick={() => setFilter(filter === "good" ? null : "good")}
        />
        <HealthTile
          level="attention"
          count={report.attention}
          active={filter === "attention"}
          onClick={() => setFilter(filter === "attention" ? null : "attention")}
        />
        <HealthTile
          level="unhealthy"
          count={report.unhealthy}
          active={filter === "unhealthy"}
          onClick={() => setFilter(filter === "unhealthy" ? null : "unhealthy")}
        />
      </div>

      {visibleDevices.length > 0 && (
        <div className="pt-2 border-t border-zinc-800/50">
          <ul className="divide-y divide-zinc-800/50">
            {visibleDevices.map((d) => (
              <li key={d.device_id}>
                <button
                  onClick={() => navigate(`/device/${d.device_id}`)}
                  className="w-full flex items-center justify-between py-2 px-1 -mx-1 rounded hover:bg-zinc-800/50 text-left transition-colors"
                >
                  <div className="flex items-center gap-2 min-w-0 flex-1">
                    <DotIcon level={d.overall} />
                    <span className="text-sm text-zinc-200 truncate">
                      {d.name}
                    </span>
                    {!d.online && (
                      <span className="text-[10px] uppercase tracking-wide text-red-400/70">
                        offline
                      </span>
                    )}
                  </div>
                  <div className="flex items-center gap-3 min-w-0 max-w-[55%]">
                    {d.top_finding ? (
                      <span
                        className={[
                          "text-xs truncate",
                          d.top_finding.level === "fail"
                            ? "text-red-400"
                            : "text-amber-400",
                        ].join(" ")}
                        title={`${d.top_finding.title}: ${d.top_finding.detail}`}
                      >
                        {d.top_finding.title}
                        <span className="text-zinc-600"> · </span>
                        <span className="text-zinc-400">
                          {d.top_finding.detail}
                        </span>
                      </span>
                    ) : (
                      <span className="text-xs text-zinc-600">
                        all checks passed
                      </span>
                    )}
                    <ArrowRight size={12} className="text-zinc-600 shrink-0" />
                  </div>
                </button>
              </li>
            ))}
          </ul>
          {hiddenCount > 0 && (
            <p className="text-[11px] text-zinc-600 mt-2">
              +{hiddenCount} more — click a device to drill in.
            </p>
          )}
        </div>
      )}
    </div>
  );
}

function HealthTile({
  level,
  count,
  active,
  onClick,
}: {
  level: FleetOverall;
  count: number;
  active: boolean;
  onClick: () => void;
}) {
  const { label, Icon, text, bg, ring } = tileStyles(level);
  const disabled = count === 0;
  return (
    <button
      onClick={onClick}
      disabled={disabled}
      className={[
        "relative flex flex-col items-start gap-1 p-3 rounded-lg border transition-all text-left",
        bg,
        active && !disabled ? ring : "border-transparent",
        disabled ? "opacity-40 cursor-default" : "hover:brightness-110",
      ].join(" ")}
    >
      <div className="flex items-center gap-1.5">
        <Icon size={14} className={text} />
        <span className={`text-[11px] uppercase tracking-wide ${text}`}>
          {label}
        </span>
      </div>
      <span className={`text-2xl font-mono font-semibold ${text}`}>{count}</span>
    </button>
  );
}

function DotIcon({ level }: { level: FleetOverall }) {
  const { text, Icon } = tileStyles(level);
  return <Icon size={13} className={`${text} shrink-0`} />;
}

function tileStyles(level: FleetOverall) {
  switch (level) {
    case "good":
      return {
        label: "Healthy",
        Icon: CheckCircle2,
        text: "text-emerald-400",
        bg: "bg-emerald-500/10",
        ring: "border-emerald-500/40",
      };
    case "attention":
      return {
        label: "Attention",
        Icon: AlertTriangle,
        text: "text-amber-400",
        bg: "bg-amber-500/10",
        ring: "border-amber-500/40",
      };
    case "unhealthy":
      return {
        label: "Unhealthy",
        Icon: XCircle,
        text: "text-red-400",
        bg: "bg-red-500/10",
        ring: "border-red-500/40",
      };
  }
}
