import { useState, useRef, useCallback } from "react";

interface SliderProps {
  label: string;
  value: number;
  min: number;
  max: number;
  unit?: string;
  onChange: (value: number) => void;
}

export default function Slider({
  label,
  value,
  min,
  max,
  unit,
  onChange,
}: SliderProps) {
  const [localValue, setLocalValue] = useState(value);
  const timeoutRef = useRef<ReturnType<typeof setTimeout>>(undefined);

  // Debounce: update local value immediately for smooth UI,
  // but only send the command after 150ms of no movement
  const handleChange = useCallback(
    (newValue: number) => {
      setLocalValue(newValue);
      if (timeoutRef.current) clearTimeout(timeoutRef.current);
      timeoutRef.current = setTimeout(() => onChange(newValue), 150);
    },
    [onChange],
  );

  // Sync local value when external value changes (e.g., from WebSocket update)
  if (Math.abs(value - localValue) > 0.01 && !timeoutRef.current) {
    setLocalValue(value);
  }

  return (
    <div className="p-3 bg-zinc-800/50 rounded-lg">
      <div className="flex items-center justify-between mb-2">
        <span className="text-sm text-zinc-300">{label}</span>
        <span className="text-sm font-mono text-trellis-400">
          {localValue}
          {unit && <span className="text-zinc-500 ml-0.5">{unit}</span>}
        </span>
      </div>
      <input
        type="range"
        min={min}
        max={max}
        value={localValue}
        onChange={(e) => handleChange(Number(e.target.value))}
        className="w-full h-1.5 bg-zinc-700 rounded-full appearance-none cursor-pointer accent-trellis-500"
      />
    </div>
  );
}
