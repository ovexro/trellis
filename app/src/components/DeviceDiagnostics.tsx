import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Stethoscope, CheckCircle2, AlertTriangle, XCircle, Info, RefreshCw } from "lucide-react";
import type { DiagnosticReport, DiagnosticFinding, DiagnosticLevel } from "@/lib/types";

interface Props {
  deviceId: string;
}

export default function DeviceDiagnostics({ deviceId }: Props) {
  const [report, setReport] = useState<DiagnosticReport | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const run = async () => {
    setLoading(true);
    setError(null);
    try {
      const r = await invoke<DiagnosticReport>("diagnose_device", { deviceId });
      setReport(r);
    } catch (e) {
      setError(String(e));
    } finally {
      setLoading(false);
    }
  };

  if (!report && !loading && !error) {
    return (
      <div className="p-5 bg-zinc-900 rounded-xl border border-zinc-800">
        <p className="text-sm text-zinc-400 mb-3">
          Run a full check on WiFi signal, free memory, uptime, reconnects, error rate, and firmware.
          Uses the last 24 hours of stored metrics and logs.
        </p>
        <button
          onClick={run}
          className="flex items-center gap-2 px-4 py-2 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-sm font-medium transition-colors"
        >
          <Stethoscope size={14} />
          Run diagnostics
        </button>
      </div>
    );
  }

  if (loading) {
    return (
      <div className="p-5 bg-zinc-900 rounded-xl border border-zinc-800">
        <p className="text-sm text-zinc-400 flex items-center gap-2">
          <RefreshCw size={14} className="animate-spin" />
          Running checks…
        </p>
      </div>
    );
  }

  if (error) {
    return (
      <div className="p-5 bg-red-500/5 rounded-xl border border-red-500/20">
        <p className="text-sm text-red-300 mb-3">Diagnostics failed: {error}</p>
        <button
          onClick={run}
          className="flex items-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded-lg text-sm font-medium transition-colors"
        >
          <RefreshCw size={14} />
          Try again
        </button>
      </div>
    );
  }

  if (!report) return null;

  return (
    <div className="space-y-3">
      <OverallBanner report={report} onRerun={run} />
      <div className="space-y-2">
        {report.findings.map((f) => (
          <FindingRow key={f.id} finding={f} />
        ))}
      </div>
    </div>
  );
}

function OverallBanner({ report, onRerun }: { report: DiagnosticReport; onRerun: () => void }) {
  const label =
    report.overall === "good"
      ? "All checks passed"
      : report.overall === "attention"
        ? "Needs attention"
        : "Unhealthy";
  const klass =
    report.overall === "good"
      ? "bg-trellis-500/10 border-trellis-500/20 text-trellis-400"
      : report.overall === "attention"
        ? "bg-amber-500/10 border-amber-500/20 text-amber-400"
        : "bg-red-500/10 border-red-500/20 text-red-400";
  const Icon =
    report.overall === "good"
      ? CheckCircle2
      : report.overall === "attention"
        ? AlertTriangle
        : XCircle;

  const failCount = report.findings.filter((f) => f.level === "fail").length;
  const warnCount = report.findings.filter((f) => f.level === "warn").length;

  return (
    <div className={`flex items-center justify-between p-4 rounded-xl border ${klass}`}>
      <div className="flex items-center gap-3">
        <Icon size={22} />
        <div>
          <p className="font-semibold">{label}</p>
          <p className="text-xs opacity-80 mt-0.5">
            {failCount} critical, {warnCount} warning(s), checked{" "}
            {new Date(report.generated_at).toLocaleTimeString()}
          </p>
        </div>
      </div>
      <button
        onClick={onRerun}
        className="p-2 rounded-lg hover:bg-zinc-800/30 transition-colors"
        title="Re-run diagnostics"
      >
        <RefreshCw size={14} />
      </button>
    </div>
  );
}

function FindingRow({ finding }: { finding: DiagnosticFinding }) {
  const { bg, border, text, Icon } = visuals(finding.level);
  return (
    <div className={`p-4 rounded-xl border ${bg} ${border}`}>
      <div className="flex items-start gap-3">
        <Icon size={16} className={`${text} mt-0.5 shrink-0`} />
        <div className="flex-1 min-w-0">
          <p className={`text-sm font-medium ${text}`}>{finding.title}</p>
          <p className="text-xs text-zinc-400 mt-1 leading-relaxed">{finding.detail}</p>
          {finding.suggestion && (
            <p className="text-xs text-zinc-500 mt-2 leading-relaxed italic">
              Suggestion: {finding.suggestion}
            </p>
          )}
        </div>
      </div>
    </div>
  );
}

function visuals(level: DiagnosticLevel) {
  switch (level) {
    case "ok":
      return {
        bg: "bg-trellis-500/5",
        border: "border-trellis-500/20",
        text: "text-trellis-400",
        Icon: CheckCircle2,
      };
    case "warn":
      return {
        bg: "bg-amber-500/5",
        border: "border-amber-500/20",
        text: "text-amber-400",
        Icon: AlertTriangle,
      };
    case "fail":
      return {
        bg: "bg-red-500/5",
        border: "border-red-500/20",
        text: "text-red-400",
        Icon: XCircle,
      };
    case "info":
    default:
      return {
        bg: "bg-zinc-800/40",
        border: "border-zinc-700/60",
        text: "text-zinc-300",
        Icon: Info,
      };
  }
}
