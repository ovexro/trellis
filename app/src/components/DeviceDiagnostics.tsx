import { useEffect, useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import {
  Stethoscope,
  CheckCircle2,
  AlertTriangle,
  XCircle,
  Info,
  RefreshCw,
  Download,
  Github,
} from "lucide-react";
import type {
  DiagnosticReport,
  DiagnosticFinding,
  DiagnosticLevel,
} from "@/lib/types";

interface Props {
  deviceId: string;
  deviceIp: string;
  devicePort: number;
}

interface SavedDeviceSlim {
  github_owner?: string | null;
  github_repo?: string | null;
}

export default function DeviceDiagnostics({ deviceId, deviceIp, devicePort }: Props) {
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

  const triggerFirmwareUpdate = async (finding: DiagnosticFinding) => {
    if (!finding.action || finding.action.action_type !== "firmware_update") return;
    const data = finding.action.data;
    try {
      await invoke("start_github_ota", {
        deviceId,
        ip: deviceIp,
        port: devicePort,
        downloadUrl: data.download_url,
        releaseTag: data.release_tag,
        assetName: data.asset_name,
      });
      // Re-run diagnostics after a moment so the report reflects the new firmware.
      setTimeout(run, 3000);
    } catch (e) {
      setError(`OTA failed: ${String(e)}`);
    }
  };

  return (
    <div className="space-y-3">
      <GithubRepoBinding deviceId={deviceId} onSaved={() => { if (report) run(); }} />
      {!report && !loading && !error && (
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
      )}
      {loading && (
        <div className="p-5 bg-zinc-900 rounded-xl border border-zinc-800">
          <p className="text-sm text-zinc-400 flex items-center gap-2">
            <RefreshCw size={14} className="animate-spin" />
            Running checks…
          </p>
        </div>
      )}
      {error && (
        <div className="p-5 bg-red-500/5 rounded-xl border border-red-500/20">
          <p className="text-sm text-red-300 mb-3">{error}</p>
          <button
            onClick={run}
            className="flex items-center gap-2 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded-lg text-sm font-medium transition-colors"
          >
            <RefreshCw size={14} />
            Try again
          </button>
        </div>
      )}
      {report && (
        <>
          <OverallBanner report={report} onRerun={run} />
          <div className="space-y-2">
            {report.findings.map((f) => (
              <FindingRow
                key={f.id}
                finding={f}
                onAction={triggerFirmwareUpdate}
              />
            ))}
          </div>
        </>
      )}
    </div>
  );
}

function GithubRepoBinding({ deviceId, onSaved }: { deviceId: string; onSaved: () => void }) {
  const [owner, setOwner] = useState("");
  const [repo, setRepo] = useState("");
  const [saving, setSaving] = useState(false);
  const [savedAt, setSavedAt] = useState<number | null>(null);
  const [expanded, setExpanded] = useState(false);

  useEffect(() => {
    (async () => {
      try {
        const sd = await invoke<SavedDeviceSlim | null>("get_saved_device", {
          deviceId,
        });
        if (sd) {
          setOwner(sd.github_owner ?? "");
          setRepo(sd.github_repo ?? "");
          if (sd.github_owner || sd.github_repo) setExpanded(true);
        }
      } catch {
        // non-fatal
      }
    })();
  }, [deviceId]);

  const save = async () => {
    setSaving(true);
    try {
      await invoke("set_device_github_repo", {
        deviceId,
        owner: owner.trim(),
        repo: repo.trim(),
      });
      setSavedAt(Date.now());
      onSaved();
    } finally {
      setSaving(false);
    }
  };

  const bound = owner.trim().length > 0 && repo.trim().length > 0;

  return (
    <div className="p-3 bg-zinc-900/60 rounded-xl border border-zinc-800/80">
      <button
        onClick={() => setExpanded((e) => !e)}
        className="flex items-center gap-2 text-xs text-zinc-400 hover:text-zinc-200 transition-colors"
      >
        <Github size={12} />
        Firmware source: {bound ? `${owner}/${repo}` : "not set"}
        <span className="text-zinc-600">{expanded ? "▲" : "▼"}</span>
      </button>
      {expanded && (
        <div className="mt-3 flex items-center gap-2">
          <input
            value={owner}
            onChange={(e) => setOwner(e.target.value)}
            placeholder="owner"
            className="flex-1 px-2.5 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-xs text-zinc-200 focus:outline-none focus:border-trellis-500"
          />
          <span className="text-zinc-600 text-xs">/</span>
          <input
            value={repo}
            onChange={(e) => setRepo(e.target.value)}
            placeholder="repo"
            className="flex-1 px-2.5 py-1.5 bg-zinc-800 border border-zinc-700 rounded-lg text-xs text-zinc-200 focus:outline-none focus:border-trellis-500"
          />
          <button
            onClick={save}
            disabled={saving}
            className="px-3 py-1.5 bg-zinc-800 hover:bg-zinc-700 text-zinc-200 rounded-lg text-xs font-medium transition-colors disabled:opacity-60"
          >
            {saving ? "…" : savedAt ? "Saved" : "Save"}
          </button>
        </div>
      )}
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

function FindingRow({
  finding,
  onAction,
}: {
  finding: DiagnosticFinding;
  onAction: (f: DiagnosticFinding) => void;
}) {
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
          {finding.action && (
            <button
              onClick={() => onAction(finding)}
              className="mt-3 flex items-center gap-2 px-3 py-1.5 bg-trellis-500 hover:bg-trellis-600 text-white rounded-lg text-xs font-medium transition-colors"
            >
              <Download size={12} />
              {finding.action.label}
            </button>
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
