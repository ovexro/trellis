import { useState, useEffect, useRef } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ScrollText } from "lucide-react";

interface LogEntry {
  severity: string;
  message: string;
  timestamp: string;
}

interface DeviceLogsProps {
  deviceId: string;
}

// Chip row mirrors the :9090 dashboard's Recent Logs filter set. `events`
// is a composite that matches the chart annotation severities
// (`state,error,warn`), so clicking it shows only rows that can appear as
// chart markers.
const CHIPS: ReadonlyArray<{ key: string; label: string; severity: string | null }> = [
  { key: "all", label: "All", severity: null },
  { key: "events", label: "Events", severity: "state,error,warn" },
  { key: "state", label: "State", severity: "state" },
  { key: "error", label: "Error", severity: "error" },
  { key: "warn", label: "Warn", severity: "warn" },
  { key: "info", label: "Info", severity: "info" },
  { key: "debug", label: "Debug", severity: "debug" },
];

// Does a live-log severity belong in the currently-filtered view? Used to
// drop WS-pushed logs that don't match the active chip so the visible list
// stays consistent with the server-side filter until the next chip switch.
function liveLogMatchesChip(chipKey: string, severity: string): boolean {
  if (chipKey === "all") return true;
  if (chipKey === "events") {
    return severity === "state" || severity === "error" || severity === "warn";
  }
  return chipKey === severity;
}

export default function DeviceLogs({ deviceId }: DeviceLogsProps) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<string>("all");
  // Stale-fetch guard: increment before each fetch, capture locally, drop
  // the result if another fetch started mid-await. Mirrors the
  // `currentLogDeviceId` pattern in web_ui.html.
  const fetchGenRef = useRef(0);
  // Lets the WS listener closure read the latest filter without
  // re-subscribing on every chip click.
  const filterRef = useRef(filter);
  useEffect(() => {
    filterRef.current = filter;
  }, [filter]);

  // Fetch on deviceId / filter change.
  useEffect(() => {
    const gen = ++fetchGenRef.current;
    const chip = CHIPS.find((c) => c.key === filter) ?? CHIPS[0];
    (async () => {
      try {
        const entries = await invoke<LogEntry[]>("get_device_logs", {
          deviceId,
          limit: 200,
          severity: chip.severity,
        });
        if (fetchGenRef.current !== gen) return;
        setLogs(entries);
      } catch (err) {
        console.error("Failed to load logs:", err);
      }
    })();
  }, [deviceId, filter]);

  // Live log listener — resubscribes only on device change.
  useEffect(() => {
    const unlisten = listen<{ device_id: string; event_type: string; payload: { severity?: string; message?: string } }>(
      "device-event",
      (e) => {
        if (e.payload.device_id !== deviceId) return;
        if (e.payload.event_type !== "log") return;

        const severity = e.payload.payload.severity || "info";
        const message = e.payload.payload.message || "";

        // Always persist — a filtered view shouldn't hide writes to disk.
        invoke("store_log_entry", {
          deviceId,
          severity,
          message,
        }).catch((err: unknown) => console.error("Failed to store log:", err));

        // Only append to the visible list if it matches the active chip.
        if (!liveLogMatchesChip(filterRef.current, severity)) return;

        const entry: LogEntry = {
          severity,
          message,
          timestamp: new Date().toISOString(),
        };
        setLogs((prev) => [...prev.slice(-499), entry]);
      },
    );

    return () => { unlisten.then((fn) => fn()); };
  }, [deviceId]);

  const severityColor = (s: string) => {
    switch (s) {
      case "error": return "text-red-400";
      case "warn": return "text-amber-400";
      case "info": return "text-blue-400";
      case "state": return "text-emerald-400";
      case "debug": return "text-zinc-500";
      default: return "text-zinc-400";
    }
  };

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
      <div className="mb-3">
        <h3 className="text-sm font-semibold text-zinc-300 flex items-center gap-2 mb-2">
          <ScrollText size={14} />
          Device Logs
        </h3>
        <div className="flex flex-wrap gap-1">
          {CHIPS.map((chip) => (
            <button
              key={chip.key}
              onClick={() => setFilter(chip.key)}
              className={`px-2.5 py-1 rounded-md text-xs min-w-[32px] text-center transition-colors ${
                filter === chip.key
                  ? "bg-trellis-500/20 text-trellis-400"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {chip.label}
            </button>
          ))}
        </div>
      </div>

      <div className="max-h-72 overflow-auto font-mono text-xs space-y-0.5">
        {logs.length === 0 ? (
          <p className="text-zinc-600 text-center py-4">
            {filter === "all"
              ? 'No logs yet. Use trellis.logInfo("message") in your firmware.'
              : "No matching log entries in the last 200."}
          </p>
        ) : (
          logs.map((log, i) => (
            <div key={i} className="flex gap-2">
              <span className="text-zinc-600 flex-shrink-0">
                {new Date(log.timestamp + "Z").toLocaleTimeString()}
              </span>
              <span className={`flex-shrink-0 uppercase w-10 ${severityColor(log.severity)}`}>
                {log.severity}
              </span>
              <span className="text-zinc-300">{log.message}</span>
            </div>
          ))
        )}
      </div>
    </div>
  );
}
