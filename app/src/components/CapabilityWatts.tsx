import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Zap, Check, X, Pencil } from "lucide-react";

interface CapabilityWattsProps {
  deviceId: string;
  capabilityId: string;
  capType: "switch" | "slider";
  watts: number | null;
  linearPower: boolean;
  sliderMax: number | null;
  onChange: (watts: number | null) => void;
  onLinearPowerChange: (linearPower: boolean) => void;
}

export default function CapabilityWatts({
  deviceId,
  capabilityId,
  capType,
  watts,
  linearPower,
  sliderMax,
  onChange,
  onLinearPowerChange,
}: CapabilityWattsProps) {
  const [editing, setEditing] = useState(false);
  const [input, setInput] = useState(watts != null ? String(watts) : "");
  const [linearInput, setLinearInput] = useState(linearPower);
  const [saving, setSaving] = useState(false);

  const startEdit = () => {
    setInput(watts != null ? String(watts) : "");
    setLinearInput(linearPower);
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
      if (capType === "slider" && linearInput !== linearPower) {
        await invoke("set_capability_linear_power", {
          deviceId,
          capabilityId,
          linearPower: linearInput,
          sliderMax: linearInput ? sliderMax : null,
        });
        onLinearPowerChange(linearInput);
      }
      setEditing(false);
    } catch (err) {
      console.error("Failed to save capability watts:", err);
    } finally {
      setSaving(false);
    }
  };

  if (editing) {
    return (
      <div className="mt-1 ml-1 space-y-1">
        <div className="flex items-center gap-1.5">
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
        {capType === "slider" && (
          <div className="pl-[14px]">
            <label className="flex items-center gap-1.5 text-[10.5px] text-zinc-500 cursor-pointer select-none">
              <input
                type="checkbox"
                checked={linearInput}
                onChange={(e) => setLinearInput(e.target.checked)}
                disabled={saving}
                className="accent-amber-400/80 h-3 w-3"
              />
              <span>Linear power</span>
            </label>
            <p className="text-[10px] text-zinc-600 mt-0.5 max-w-xs leading-snug">
              Assumes power scales linearly with slider value. Accurate for
              resistive loads (heaters, incandescent); rough for LEDs and
              motors.
            </p>
          </div>
        )}
      </div>
    );
  }

  return (
    <button
      onClick={startEdit}
      className="flex items-center gap-1 mt-1 ml-1 text-[11px] text-zinc-600 hover:text-amber-400/80 transition-colors group"
      title={
        watts != null
          ? capType === "slider" && linearPower
            ? "Edit nameplate watts (linear power tracking on)"
            : "Edit nameplate watts"
          : "Set nameplate watts to track estimated energy"
      }
    >
      <Zap size={10} className={watts != null ? "text-amber-400/60" : ""} />
      {watts != null ? (
        <span className="font-mono tabular-nums">
          {watts} W
          {capType === "slider" && linearPower ? (
            <span className="text-zinc-600"> · linear</span>
          ) : null}
        </span>
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
