import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ToggleLeft, Check, X, Pencil } from "lucide-react";

interface CapabilityCoverProps {
  deviceId: string;
  capabilityId: string;
  coverPosition: boolean;
  onChange: (coverPosition: boolean) => void;
}

export default function CapabilityCover({
  deviceId,
  capabilityId,
  coverPosition,
  onChange,
}: CapabilityCoverProps) {
  const [editing, setEditing] = useState(false);
  const [enabledInput, setEnabledInput] = useState(coverPosition);
  const [saving, setSaving] = useState(false);

  const startEdit = () => {
    setEnabledInput(coverPosition);
    setEditing(true);
  };

  const save = async () => {
    setSaving(true);
    try {
      await invoke("set_capability_cover", {
        deviceId,
        capabilityId,
        coverPosition: enabledInput,
      });
      onChange(enabledInput);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save cover_position meta:", err);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="mt-1 ml-1 space-y-1">
        <label className="flex items-center gap-1.5 text-[10.5px] text-zinc-500 cursor-pointer select-none">
          <input
            type="checkbox"
            checked={enabledInput}
            onChange={(e) => setEnabledInput(e.target.checked)}
            disabled={saving}
            className="accent-trellis-500 h-3 w-3"
          />
          <span>HA cover (position 0..max)</span>
        </label>
        <div className="flex items-center gap-1 pl-[18px]">
          <button
            onClick={save}
            disabled={saving}
            className="p-0.5 text-trellis-400 hover:text-trellis-300 disabled:opacity-50"
            aria-label="Save"
          >
            <Check size={11} />
          </button>
          <button
            onClick={() => setEditing(false)}
            disabled={saving}
            className="p-0.5 text-zinc-500 hover:text-zinc-300 disabled:opacity-50"
            aria-label="Cancel"
          >
            <X size={11} />
          </button>
        </div>
        <p className="text-[10px] text-zinc-600 pl-[18px] max-w-xs leading-snug">
          Reports as Home Assistant cover with position 0..slider-max.
          Useful for blinds, shades, garage doors fronted by a Trellis
          slider.
        </p>
      </div>
    );
  }

  return (
    <button
      onClick={startEdit}
      className="flex items-center gap-1 mt-1 ml-1 text-[11px] text-zinc-600 hover:text-trellis-400 transition-colors group"
      title={
        coverPosition
          ? "Edit HA cover settings"
          : "Advertise this slider as a Home Assistant cover"
      }
    >
      <ToggleLeft
        size={10}
        className={coverPosition ? "text-trellis-400/70" : ""}
      />
      {coverPosition ? (
        <span className="font-mono tabular-nums">cover</span>
      ) : (
        <span>HA cover</span>
      )}
      <Pencil
        size={9}
        className="opacity-0 group-hover:opacity-60 transition-opacity"
      />
    </button>
  );
}
