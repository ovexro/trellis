import { Wifi, WifiOff } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function Header() {
  const { devices } = useDeviceStore();
  const onlineCount = devices.filter((d) => d.online).length;

  return (
    <header className="h-14 bg-zinc-900 border-b border-zinc-800 flex items-center justify-between px-6">
      <div className="flex items-center gap-4">
        <span className="text-sm text-zinc-400">
          {devices.length === 0 ? (
            <span className="flex items-center gap-2">
              <span className="w-2 h-2 rounded-full bg-trellis-400 animate-pulse" />
              Scanning for devices...
            </span>
          ) : (
            <span className="flex items-center gap-2">
              {onlineCount > 0 ? (
                <Wifi size={14} className="text-trellis-400" />
              ) : (
                <WifiOff size={14} className="text-zinc-600" />
              )}
              {devices.length} device{devices.length !== 1 ? "s" : ""}
              <span className="text-trellis-400">
                ({onlineCount} online)
              </span>
            </span>
          )}
        </span>
      </div>

      <div className="flex items-center gap-2 text-xs text-zinc-600">
        <span className="w-1.5 h-1.5 rounded-full bg-trellis-500" />
        Auto-discovering
      </div>
    </header>
  );
}
