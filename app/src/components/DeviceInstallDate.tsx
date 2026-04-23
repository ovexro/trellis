import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Pencil, Check, X } from "lucide-react";

interface DeviceInstallDateProps {
  deviceId: string;
}

interface SavedDeviceInstallDate {
  install_date: string;
}

function formatDisplay(iso: string): string {
  if (!iso) return "";
  const d = new Date(iso + "T00:00:00");
  if (Number.isNaN(d.getTime())) return iso;
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export default function DeviceInstallDate({ deviceId }: DeviceInstallDateProps) {
  const [editing, setEditing] = useState(false);
  const [value, setValue] = useState("");
  const [savedValue, setSavedValue] = useState("");

  useEffect(() => {
    (async () => {
      try {
        const saved = await invoke<SavedDeviceInstallDate | null>("get_saved_device", { deviceId });
        const v = saved?.install_date ?? "";
        setSavedValue(v);
        setValue(v);
      } catch (err) {
        console.error("Failed to load install date:", err);
      }
    })();
  }, [deviceId]);

  const save = async () => {
    try {
      await invoke("set_device_install_date", { deviceId, installDate: value });
      setSavedValue(value);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save install date:", err);
    }
  };

  const clear = async () => {
    try {
      await invoke("set_device_install_date", { deviceId, installDate: "" });
      setSavedValue("");
      setValue("");
      setEditing(false);
    } catch (err) {
      console.error("Failed to clear install date:", err);
    }
  };

  const cancel = () => {
    setValue(savedValue);
    setEditing(false);
  };

  if (editing) {
    return (
      <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800">
        <input
          type="date"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          className="bg-zinc-950 border border-zinc-700 rounded px-3 py-2 text-sm text-zinc-200 focus:outline-none focus:border-trellis-500"
          autoFocus
        />
        <div className="flex items-center justify-end gap-2 mt-2">
          {savedValue && (
            <button
              onClick={clear}
              className="flex items-center gap-1 px-3 py-1 text-xs text-zinc-500 hover:text-red-400 transition-colors"
            >
              Clear
            </button>
          )}
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

  if (savedValue) {
    return (
      <div className="p-4 bg-zinc-900 rounded-xl border border-zinc-800 group">
        <div className="flex items-center justify-between gap-3">
          <div className="text-sm text-zinc-300">
            Installed <span className="text-zinc-400">{formatDisplay(savedValue)}</span>
          </div>
          <button
            onClick={() => setEditing(true)}
            className="p-1 text-zinc-600 hover:text-zinc-300 transition-colors opacity-0 group-hover:opacity-100"
            title="Edit install date"
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
      + Set install date — when this device was put into service
    </button>
  );
}
