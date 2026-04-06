import { useState } from "react";

export default function Settings() {
  const [scanInterval, setScanInterval] = useState(30);

  return (
    <div className="max-w-xl">
      <h1 className="text-xl font-bold text-zinc-100 mb-6">Settings</h1>

      <div className="space-y-6">
        <div>
          <label className="block text-sm text-zinc-400 mb-1.5">
            Auto-scan interval (seconds)
          </label>
          <input
            type="number"
            min={5}
            max={300}
            value={scanInterval}
            onChange={(e) => setScanInterval(Number(e.target.value))}
            className="w-32 bg-zinc-800 border border-zinc-700 rounded-lg px-3 py-2 text-sm text-zinc-300"
          />
          <p className="text-xs text-zinc-600 mt-1">
            How often to scan for new devices on the network.
          </p>
        </div>

        <div className="pt-6 border-t border-zinc-800">
          <h2 className="text-sm font-semibold text-zinc-400 uppercase tracking-wide mb-3">
            About
          </h2>
          <div className="text-sm text-zinc-500 space-y-1">
            <p>Trellis v0.1.0</p>
            <p>The easiest way to deploy and control ESP32 and Pico W devices.</p>
            <p className="pt-2">
              <a
                href="https://github.com/ovexro/trellis"
                target="_blank"
                rel="noopener noreferrer"
                className="text-trellis-400 hover:text-trellis-300"
              >
                GitHub
              </a>
              {" · "}
              <a
                href="https://www.paypal.com/paypalme/ovexro"
                target="_blank"
                rel="noopener noreferrer"
                className="text-trellis-400 hover:text-trellis-300"
              >
                Donate
              </a>
            </p>
          </div>
        </div>
      </div>
    </div>
  );
}
