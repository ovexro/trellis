interface ColorPickerProps {
  label: string;
  value: string;
  onChange: (value: string) => void;
}

export default function ColorPicker({
  label,
  value,
  onChange,
}: ColorPickerProps) {
  return (
    <div className="flex items-center justify-between p-3 bg-zinc-800/50 rounded-lg">
      <span className="text-sm text-zinc-300">{label}</span>
      <div className="flex items-center gap-2.5">
        <span className="text-xs font-mono text-zinc-500">{value}</span>
        <div className="relative">
          <div
            className="w-9 h-9 rounded-lg border-2 border-zinc-600 shadow-inner cursor-pointer"
            style={{ backgroundColor: value }}
          />
          <input
            type="color"
            value={value}
            onChange={(e) => onChange(e.target.value)}
            className="absolute inset-0 opacity-0 cursor-pointer w-full h-full"
            aria-label={label}
          />
        </div>
      </div>
    </div>
  );
}
