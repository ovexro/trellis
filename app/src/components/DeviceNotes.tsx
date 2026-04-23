import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pencil, Check, X } from "lucide-react";

interface DeviceNotesProps {
  deviceId: string;
}

interface SavedDeviceNotes {
  notes: string;
}

export default function DeviceNotes({ deviceId }: DeviceNotesProps) {
  const [editing, setEditing] = useState(false);
  const [notes, setNotes] = useState("");
  const [savedNotes, setSavedNotes] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const saved = await invoke<SavedDeviceNotes | null>("get_saved_device", { deviceId });
        const v = saved?.notes ?? "";
        setSavedNotes(v);
        setNotes(v);
      } catch (err) {
        console.error("Failed to load notes:", err);
      }
    })();
  }, [deviceId]);

  const save = async () => {
    try {
      await invoke("set_device_notes", { deviceId, notes });
      setSavedNotes(notes);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save notes:", err);
    }
  };

  const cancel = () => {
    setNotes(savedNotes);
    setEditing(false);
  };

  if (editing) {
    return (
      <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.target.value)}
          placeholder="Wiring, install date, calibration, breaker #, anything worth remembering…"
          rows={5}
          className="w-full bg-zinc-950 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-200 font-mono placeholder-zinc-600 focus:outline-none focus:border-trellis-500 resize-y"
          autoFocus
        />
        <div className="flex items-center justify-end gap-2 mt-2">
          <button
            onClick={cancel}
            className="flex items-center gap-1 px-3 py-1 text-xs text-zinc-400 hover:text-zinc-200 transition-colors"
          >
            <X size={14} /> Cancel
          </button>
          <button
            onClick={save}
            className="flex items-center gap-1 px-3 py-1 text-xs rounded bg-trellis-500/10 text-trellis-300 hover:bg-trellis-500/20 transition-colors"
          >
            <Check size={14} /> Save
          </button>
        </div>
      </div>
    );
  }

  if (savedNotes) {
    return (
      <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800 group">
        <div className="flex items-start justify-between gap-3">
          <pre className="text-sm text-zinc-300 whitespace-pre-wrap break-words font-sans flex-1 min-w-0">
            {savedNotes}
          </pre>
          <button
            onClick={() => setEditing(true)}
            className="p-1 text-zinc-600 hover:text-zinc-300 transition-colors opacity-0 group-hover:opacity-100"
            title="Edit notes"
          >
            <Pencil size={14} />
          </button>
        </div>
      </div>
    );
  }

  return (
    <button
      onClick={() => setEditing(true)}
      className="w-full p-4 bg-zinc-900 rounded-xl border border-dashed border-zinc-800 text-sm text-zinc-600 hover:text-zinc-400 hover:border-zinc-700 transition-colors text-left"
    >
      + Add notes — wiring, install date, breaker #, anything worth remembering
    </button>
  );
}
