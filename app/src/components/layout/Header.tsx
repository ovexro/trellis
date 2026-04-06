import { RefreshCw } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function Header() {
  const { devices, scanning, scan } = useDeviceStore();
  const onlineCount = devices.filter((d) => d.online).length;

  return (
    <header className="h-14 bg-zinc-900 border-b border-zinc-800 flex items-center justify-between px-6">
      <div className="flex items-center gap-4">
        <span className="text-sm text-zinc-400">
          {devices.length} device{devices.length !== 1 ? "s" : ""}
          {onlineCount > 0 && (
            <span className="text-trellis-400 ml-1">
              ({onlineCount} online)
            </span>
          )}
        </span>
      </div>

      <button
        onClick={scan}
        disabled={scanning}
        className="flex items-center gap-2 px-3 py-1.5 rounded-lg text-sm bg-zinc-800 hover:bg-zinc-700 text-zinc-300 transition-colors disabled:opacity-50"
      >
        <RefreshCw size={14} className={scanning ? "animate-spin" : ""} />
        {scanning ? "Scanning..." : "Scan"}
      </button>
    </header>
  );
}
