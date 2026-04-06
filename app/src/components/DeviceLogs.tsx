import { useState, useEffect } from "react";
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

export default function DeviceLogs({ deviceId }: DeviceLogsProps) {
  const [logs, setLogs] = useState<LogEntry[]>([]);
  const [filter, setFilter] = useState<string>("all");

  useEffect(() => {
    loadLogs();

    // Listen for live log events
    const unlisten = listen<{ device_id: string; event_type: string; payload: { severity?: string; message?: string } }>(
      "device-event",
      (e) => {
        if (e.payload.device_id !== deviceId) return;
        if (e.payload.event_type !== "log") return;

        const entry: LogEntry = {
          severity: e.payload.payload.severity || "info",
          message: e.payload.payload.message || "",
          timestamp: new Date().toISOString(),
        };

        // Store in DB
        invoke("store_log_entry", {
          deviceId,
          severity: entry.severity,
          message: entry.message,
        }).catch(() => {});

        setLogs((prev) => [...prev.slice(-499), entry]);
      },
    );

    return () => { unlisten.then((fn) => fn()); };
  }, [deviceId]);

  const loadLogs = async () => {
    try {
      const entries = await invoke<LogEntry[]>("get_device_logs", {
        deviceId,
        limit: 200,
      });
      setLogs(entries);
    } catch {}
  };

  const filteredLogs = filter === "all"
    ? logs
    : logs.filter((l) => l.severity === filter);

  const severityColor = (s: string) => {
    switch (s) {
      case "error": return "text-red-400";
      case "warn": return "text-amber-400";
      case "info": return "text-blue-400";
      default: return "text-zinc-400";
    }
  };

  return (
    <div className="bg-zinc-900 border border-zinc-800 rounded-xl p-4">
      <div className="flex items-center justify-between mb-3">
        <h3 className="text-sm font-semibold text-zinc-300 flex items-center gap-2">
          <ScrollText size={14} />
          Device Logs
        </h3>
        <div className="flex gap-1">
          {["all", "error", "warn", "info"].map((f) => (
            <button
              key={f}
              onClick={() => setFilter(f)}
              className={`px-2 py-0.5 rounded text-xs transition-colors ${
                filter === f
                  ? "bg-trellis-500/20 text-trellis-400"
                  : "text-zinc-500 hover:text-zinc-300"
              }`}
            >
              {f}
            </button>
          ))}
        </div>
      </div>

      <div className="max-h-48 overflow-auto font-mono text-xs space-y-0.5">
        {filteredLogs.length === 0 ? (
          <p className="text-zinc-600 text-center py-4">
            No logs yet. Use trellis.logInfo("message") in your firmware.
          </p>
        ) : (
          filteredLogs.map((log, i) => (
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
