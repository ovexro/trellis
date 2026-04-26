import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { ToggleLeft, Check, X, Pencil } from "lucide-react";

interface CapabilityBinarySensorProps {
  deviceId: string;
  capabilityId: string;
  binarySensor: boolean;
  deviceClass: string | null;
  onChange: (binarySensor: boolean, deviceClass: string | null) => void;
}

// Curated subset of HA binary_sensor device classes that map cleanly onto
// typical Trellis sensor capabilities. Keeping the list short avoids
// overwhelming users; "(none)" is the safe default for a generic
// binary_sensor with no specific icon.
const DEVICE_CLASSES: { value: string; label: string }[] = [
  { value: "", label: "(none)" },
  { value: "motion", label: "Motion" },
  { value: "occupancy", label: "Occupancy" },
  { value: "presence", label: "Presence" },
  { value: "door", label: "Door" },
  { value: "window", label: "Window" },
  { value: "opening", label: "Opening" },
  { value: "smoke", label: "Smoke" },
  { value: "gas", label: "Gas" },
  { value: "moisture", label: "Moisture" },
  { value: "sound", label: "Sound" },
  { value: "vibration", label: "Vibration" },
  { value: "tamper", label: "Tamper" },
  { value: "safety", label: "Safety" },
  { value: "problem", label: "Problem" },
  { value: "connectivity", label: "Connectivity" },
];

export default function CapabilityBinarySensor({
  deviceId,
  capabilityId,
  binarySensor,
  deviceClass,
  onChange,
}: CapabilityBinarySensorProps) {
  const [editing, setEditing] = useState(false);
  const [enabledInput, setEnabledInput] = useState(binarySensor);
  const [classInput, setClassInput] = useState(deviceClass ?? "");
  const [saving, setSaving] = useState(false);

  const startEdit = () => {
    setEnabledInput(binarySensor);
    setClassInput(deviceClass ?? "");
    setEditing(true);
  };

  const save = async () => {
    setSaving(true);
    try {
      const trimmed = classInput.trim();
      const sendClass = enabledInput && trimmed !== "" ? trimmed : null;
      await invoke("set_capability_binary_sensor", {
        deviceId,
        capabilityId,
        binarySensor: enabledInput,
        deviceClass: sendClass,
      });
      onChange(enabledInput, sendClass);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save binary_sensor meta:", err);
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
          <span>HA binary sensor</span>
        </label>
        {enabledInput && (
          <div className="flex items-center gap-1.5 pl-[18px]">
            <select
              value={classInput}
              onChange={(e) => setClassInput(e.target.value)}
              disabled={saving}
              className="bg-zinc-800 border border-zinc-700 rounded px-1 py-0 text-[11px] text-zinc-300 focus:outline-none focus:border-trellis-500/50"
            >
              {DEVICE_CLASSES.map((c) => (
                <option key={c.value} value={c.value}>
                  {c.label}
                </option>
              ))}
            </select>
          </div>
        )}
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
          Reports as Home Assistant binary_sensor (true/false). Sensor must
          publish "true"/"false" string values. Device class adds a typed
          icon in HA.
        </p>
      </div>
    );
  }

  return (
    <button
      onClick={startEdit}
      className="flex items-center gap-1 mt-1 ml-1 text-[11px] text-zinc-600 hover:text-trellis-400 transition-colors group"
      title={
        binarySensor
          ? "Edit HA binary_sensor settings"
          : "Advertise this sensor as a Home Assistant binary_sensor"
      }
    >
      <ToggleLeft
        size={10}
        className={binarySensor ? "text-trellis-400/70" : ""}
      />
      {binarySensor ? (
        <span className="font-mono tabular-nums">
          binary_sensor
          {deviceClass ? (
            <span className="text-zinc-600"> · {deviceClass}</span>
          ) : null}
        </span>
      ) : (
        <span>HA binary sensor</span>
      )}
      <Pencil
        size={9}
        className="opacity-0 group-hover:opacity-60 transition-opacity"
      />
    </button>
  );
}
