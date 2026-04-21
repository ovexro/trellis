import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Zap, Check, X, Pencil } from "lucide-react";

interface CapabilityWattsProps {
  deviceId: string;
  capabilityId: string;
  watts: number | null;
  onChange: (watts: number | null) => void;
}

export default function CapabilityWatts({
  deviceId,
  capabilityId,
  watts,
  onChange,
}: CapabilityWattsProps) {
  const [editing, setEditing] = useState(false);
  const [input, setInput] = useState(watts != null ? String(watts) : "");
  const [saving, setSaving] = useState(false);

  const startEdit = () => {
    setInput(watts != null ? String(watts) : "");
    setEditing(true);
  };

  const save = async () => {
    const trimmed = input.trim();
    const parsed = trimmed === "" ? null : Number(trimmed);
    if (parsed != null && (!Number.isFinite(parsed) || parsed < 0)) return;
    setSaving(true);
    try {
      await invoke("set_capability_watts", {
        deviceId,
        capabilityId,
        nameplateWatts: parsed,
      });
      onChange(parsed);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save capability watts:", err);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="flex items-center gap-1.5 mt-1 ml-1">
        <Zap size={10} className="text-amber-400/70" />
        <input
          type="number"
          min="0"
          step="0.1"
          value={input}
          onChange={(e) => setInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") save();
            if (e.key === "Escape") setEditing(false);
          }}
          placeholder="watts"
          disabled={saving}
          className="bg-zinc-800 border border-zinc-700 rounded px-1.5 py-0 text-[11px] text-zinc-300 w-16 font-mono focus:outline-none focus:border-amber-500/40"
          autoFocus
        />
        <span className="text-[11px] text-zinc-600">W</span>
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
    );
  }

  return (
    <button
      onClick={startEdit}
      className="flex items-center gap-1 mt-1 ml-1 text-[11px] text-zinc-600 hover:text-amber-400/80 transition-colors group"
      title={
        watts != null
          ? "Edit nameplate watts"
          : "Set nameplate watts to track estimated energy"
      }
    >
      <Zap size={10} className={watts != null ? "text-amber-400/60" : ""} />
      {watts != null ? (
        <span className="font-mono tabular-nums">{watts} W</span>
      ) : (
        <span>set watts</span>
      )}
      <Pencil
        size={9}
        className="opacity-0 group-hover:opacity-60 transition-opacity"
      />
    </button>
  );
}
