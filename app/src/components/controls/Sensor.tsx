interface SensorProps {
  label: string;
  value: number;
  unit?: string;
}

export default function Sensor({ label, value, unit }: SensorProps) {
  return (
    <div className="p-3 bg-zinc-800/50 rounded-lg">
      <span className="text-xs text-zinc-500 uppercase tracking-wide">
        {label}
      </span>
      <div className="mt-1">
        <span className="text-2xl font-mono font-bold text-zinc-100">
          {typeof value === "number" ? value.toFixed(1) : value}
        </span>
        {unit && (
          <span className="text-sm text-zinc-500 ml-1">{unit}</span>
        )}
      </div>
    </div>
  );
}
