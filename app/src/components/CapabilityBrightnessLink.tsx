import { useState } from "react";
import { invoke } from "@tauri-apps/api/core";
import { Lightbulb, Check, X, Pencil } from "lucide-react";

interface SliderOption {
  id: string;
  label: string;
}

interface CapabilityBrightnessLinkProps {
  deviceId: string;
  colorCapabilityId: string;
  linkedSliderCapId: string | null;
  sliderOptions: SliderOption[];
  onChange: (linkedSliderCapId: string | null) => void;
}

export default function CapabilityBrightnessLink({
  deviceId,
  colorCapabilityId,
  linkedSliderCapId,
  sliderOptions,
  onChange,
}: CapabilityBrightnessLinkProps) {
  const [editing, setEditing] = useState(false);
  const [selected, setSelected] = useState<string>(linkedSliderCapId ?? "");
  const [saving, setSaving] = useState(false);

  const startEdit = () => {
    setSelected(linkedSliderCapId ?? "");
    setEditing(true);
  };

  const save = async () => {
    if (sliderOptions.length === 0) {
      setEditing(false);
      return;
    }
    setSaving(true);
    const prevLinked = linkedSliderCapId;
    const newLinked = selected || null;
    try {
      // The link is stored on the SLIDER row pointing at the color cap.
      // Clearing the previous slider's row first lets the user retarget the
      // link to a different slider in one save without leaving a stale row.
      if (prevLinked && prevLinked !== newLinked) {
        await invoke("set_capability_brightness_link", {
          deviceId,
          capabilityId: prevLinked,
          colorCapabilityId: null,
        });
      }
      if (newLinked) {
        await invoke("set_capability_brightness_link", {
          deviceId,
          capabilityId: newLinked,
          colorCapabilityId: colorCapabilityId,
        });
      }
      onChange(newLinked);
      setEditing(false);
    } catch (err) {
      console.error("Failed to save brightness link:", err);
    } finally {
      setSaving(false);
    }
  };

  const linkedLabel = linkedSliderCapId
    ? sliderOptions.find((s) => s.id === linkedSliderCapId)?.label ??
      linkedSliderCapId
    : null;

  if (editing) {
    return (
      <div className="mt-1 ml-1 space-y-1">
        <label className="flex items-center gap-1.5 text-[10.5px] text-zinc-500">
          <span>Brightness slider:</span>
          <select
            value={selected}
            onChange={(e) => setSelected(e.target.value)}
            disabled={saving || sliderOptions.length === 0}
            className="bg-zinc-900 text-zinc-200 text-[10.5px] rounded border border-zinc-700 px-1 py-0.5"
          >
            <option value="">— none —</option>
            {sliderOptions.map((s) => (
              <option key={s.id} value={s.id}>
                {s.label}
              </option>
            ))}
          </select>
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
          Combine this color cap and a slider into one Home Assistant{" "}
          <code className="text-zinc-500">light</code> entity carrying both
          rgb and brightness. The slider's standalone entity is hidden while
          linked.
        </p>
      </div>
    );
  }

  return (
    <button
      onClick={startEdit}
      disabled={sliderOptions.length === 0 && !linkedSliderCapId}
      className="flex items-center gap-1 mt-1 ml-1 text-[11px] text-zinc-600 hover:text-trellis-400 transition-colors group disabled:opacity-40 disabled:cursor-not-allowed"
      title={
        sliderOptions.length === 0 && !linkedSliderCapId
          ? "No sliders on this device to link as brightness"
          : linkedSliderCapId
          ? "Edit brightness linkage"
          : "Link a slider as the brightness channel for this light"
      }
    >
      <Lightbulb
        size={10}
        className={linkedSliderCapId ? "text-trellis-400/70" : ""}
      />
      {linkedLabel ? (
        <span>
          brightness: <span className="font-mono">{linkedLabel}</span>
        </span>
      ) : (
        <span>HA brightness link</span>
      )}
      <Pencil
        size={9}
        className="opacity-0 group-hover:opacity-60 transition-opacity"
      />
    </button>
  );
}
