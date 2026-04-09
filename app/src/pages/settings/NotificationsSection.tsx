import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Bell, Check } from "lucide-react";

export default function NotificationsSection() {
  const [ntfyTopic, setNtfyTopic] = useState("");
  const [ntfySavedTopic, setNtfySavedTopic] = useState<string | null>(null);
  const [ntfyStatus, setNtfyStatus] = useState("");

  useEffect(() => {
    invoke<string | null>("get_setting", { key: "ntfy_topic" }).then((topic) => {
      if (topic) {
        setNtfyTopic(topic);
        setNtfySavedTopic(topic);
      }
    }).catch(() => {});
  }, []);

  const saveNtfyTopic = async () => {
    const trimmed = ntfyTopic.trim();
    if (!trimmed) {
      setNtfyStatus("Topic name cannot be empty");
      setTimeout(() => setNtfyStatus(""), 3000);
      return;
    }
    try {
      await invoke("set_setting", { key: "ntfy_topic", value: trimmed });
      setNtfySavedTopic(trimmed);
      setNtfyStatus("Topic saved — push notifications enabled");
      setTimeout(() => setNtfyStatus(""), 3000);
    } catch (err) {
      setNtfyStatus(`Failed to save: ${err}`);
    }
  };

  const testNtfy = async () => {
    const topic = ntfySavedTopic || ntfyTopic.trim();
    if (!topic) {
      setNtfyStatus("Save a topic first");
      setTimeout(() => setNtfyStatus(""), 3000);
      return;
    }
    try {
      await invoke("test_ntfy", { topic });
      setNtfyStatus("Test notification sent — check your phone");
      setTimeout(() => setNtfyStatus(""), 5000);
    } catch (err) {
      setNtfyStatus(`Test failed: ${err}`);
    }
  };

  const clearNtfyTopic = async () => {
    try {
      await invoke("delete_setting", { key: "ntfy_topic" });
      setNtfyTopic("");
      setNtfySavedTopic(null);
      setNtfyStatus("Push notifications disabled");
      setTimeout(() => setNtfyStatus(""), 3000);
    } catch (err) {
      setNtfyStatus(`Failed to clear: ${err}`);
    }
  };

  return (
    <div>
      <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
        Push Notifications
      </h2>
      <div className="space-y-3">
        <div className="flex items-center gap-2 text-sm text-zinc-300">
          <Bell size={16} className={ntfySavedTopic ? "text-trellis-400" : "text-zinc-500"} />
          {ntfySavedTopic ? (
            <span>Enabled — sending to <code className="px-1.5 py-0.5 bg-zinc-800 rounded text-trellis-400 text-xs">{ntfySavedTopic}</code></span>
          ) : (
            <span className="text-zinc-500">Disabled — no topic configured</span>
          )}
        </div>
        <div className="flex gap-2">
          <input
            type="text"
            value={ntfyTopic}
            onChange={(e) => setNtfyTopic(e.target.value)}
            placeholder="Enter ntfy topic name"
            className="flex-1 px-3 py-2 bg-zinc-800 border border-zinc-700 rounded-lg text-sm text-zinc-200 placeholder-zinc-500 focus:outline-none focus:border-trellis-500"
          />
          <button
            onClick={saveNtfyTopic}
            className="px-4 py-2 bg-trellis-600 hover:bg-trellis-500 text-white rounded-lg text-sm transition-colors"
          >
            Save
          </button>
        </div>
        <div className="flex gap-2">
          <button
            onClick={testNtfy}
            disabled={!ntfySavedTopic}
            className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Test
          </button>
          <button
            onClick={clearNtfyTopic}
            disabled={!ntfySavedTopic}
            className="px-4 py-2 bg-zinc-800 hover:bg-zinc-700 text-zinc-300 rounded-lg text-sm transition-colors disabled:opacity-40 disabled:cursor-not-allowed"
          >
            Clear
          </button>
        </div>
        {ntfyStatus && (
          <p className={`text-xs flex items-center gap-1 ${ntfyStatus.includes("failed") || ntfyStatus.includes("Failed") || ntfyStatus.includes("cannot") ? "text-red-400" : "text-trellis-400"}`}>
            <Check size={12} /> {ntfyStatus}
          </p>
        )}
        <p className="text-xs text-zinc-600">
          Install the ntfy app on your phone, subscribe to your topic name, and Trellis will send push alerts when sensors trigger alerts or devices go offline.
        </p>
      </div>
    </div>
  );
}
