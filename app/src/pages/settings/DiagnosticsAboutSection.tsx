import { Check } from "lucide-react";
import { useDeviceStore } from "@/stores/deviceStore";

export default function DiagnosticsAboutSection() {
  const { devices } = useDeviceStore();

  return (
    <>
      {/* Diagnostics */}
      <div>
        <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
          Diagnostics
        </h2>
        <div className="space-y-2">
          {devices.filter((d) => d.online).map((device) => {
            const warnings: string[] = [];

            if (device.system.rssi < -80) {
              warnings.push("Weak WiFi signal — consider moving the device closer to the router");
            }
            if (device.system.heap_free < 20000) {
              warnings.push("Low free heap — possible memory leak");
            }
            if (device.system.heap_free < 10000) {
              warnings.push("Critical: heap nearly exhausted — device may crash");
            }

            if (warnings.length === 0) return null;

            return (
              <div key={device.id} className="p-3 bg-amber-500/5 border border-amber-500/20 rounded-lg">
                <p className="text-sm font-medium text-amber-400 mb-1">{device.name}</p>
                {warnings.map((w, i) => (
                  <p key={i} className="text-xs text-amber-300/70">• {w}</p>
                ))}
              </div>
            );
          }).filter(Boolean)}

          {devices.filter((d) => d.online).every(
            (d) => d.system.rssi >= -80 && d.system.heap_free >= 20000,
          ) && (
            <p className="text-sm text-trellis-400 flex items-center gap-2">
              <Check size={14} />
              All devices healthy — no issues detected
            </p>
          )}
        </div>
      </div>

      {/* About */}
      <div className="pt-6 border-t border-zinc-800">
        <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
          About
        </h2>
        <div className="text-sm text-zinc-500 space-y-1">
          <p>Trellis v{__APP_VERSION__}</p>
          <p>The easiest way to deploy and control ESP32 and Pico W devices.</p>
          <p className="pt-2">
            <a href="https://github.com/ovexro/trellis" target="_blank" rel="noopener noreferrer" className="text-trellis-400 hover:text-trellis-300">
              GitHub
            </a>
            {" · "}
            <a href="https://www.paypal.com/paypalme/ovexro" target="_blank" rel="noopener noreferrer" className="text-trellis-400 hover:text-trellis-300">
              Donate
            </a>
          </p>
        </div>
      </div>
    </>
  );
}
