import { useState } from "react";
import { NavLink } from "react-router-dom";
import {
  LayoutDashboard,
  Terminal,
  TerminalSquare,
  Upload,
  Cpu,
  Zap,
  GitBranch,
  Settings,
  Radio,
  X,
  ExternalLink,
} from "lucide-react";

const navItems = [
  { to: "/", icon: LayoutDashboard, label: "Devices" },
  { to: "/new-device", icon: Cpu, label: "New Device" },
  { to: "/scenes", icon: Zap, label: "Scenes" },
  { to: "/automation", icon: GitBranch, label: "Automation" },
  { to: "/serial", icon: Terminal, label: "Serial" },
  { to: "/terminal", icon: TerminalSquare, label: "Terminal" },
  { to: "/ota", icon: Upload, label: "OTA" },
  { to: "/settings", icon: Settings, label: "Settings" },
];

export default function Sidebar() {
  const [showAbout, setShowAbout] = useState(false);

  return (
    <aside className="w-56 bg-zinc-900/50 border-r border-zinc-800/50 flex flex-col">
      <div className="p-5 border-b border-zinc-800/50">
        <div className="flex items-center gap-2.5">
          <div className="w-8 h-8 bg-trellis-500/15 rounded-lg flex items-center justify-center">
            <Radio size={16} className="text-trellis-400" />
          </div>
          <div>
            <h1 className="text-base font-bold text-zinc-100 tracking-tight">
              Trellis
            </h1>
            <p className="text-[11px] text-zinc-500 -mt-0.5">
              Device Control Center
            </p>
          </div>
        </div>
      </div>

      <nav className="flex-1 p-2 space-y-0.5 mt-1">
        {navItems.map(({ to, icon: Icon, label }) => (
          <NavLink
            key={to}
            to={to}
            end={to === "/"}
            className={({ isActive }) =>
              `flex items-center gap-3 px-3 py-2.5 rounded-lg text-sm transition-all duration-150 ${
                isActive
                  ? "bg-trellis-500/10 text-trellis-400 font-medium"
                  : "text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800/50"
              }`
            }
          >
            <Icon size={18} />
            {label}
          </NavLink>
        ))}
      </nav>

      <div className="p-4 border-t border-zinc-800/50">
        <button
          onClick={() => setShowAbout(true)}
          className="px-2 py-0.5 bg-zinc-800/80 hover:bg-zinc-700/80 rounded-full text-[11px] text-zinc-500 hover:text-zinc-400 transition-colors"
        >
          v0.1.4
        </button>
      </div>

      {showAbout && (
        <div className="fixed inset-0 bg-black/50 backdrop-blur-sm flex items-center justify-center z-50" onClick={() => setShowAbout(false)}>
          <div onClick={(e) => e.stopPropagation()} className="bg-zinc-900 border border-zinc-800 rounded-xl p-6 w-[340px] shadow-2xl">
            <div className="flex items-center justify-between mb-4">
              <div className="flex items-center gap-2.5">
                <div className="w-10 h-10 bg-trellis-500/15 rounded-xl flex items-center justify-center">
                  <Radio size={20} className="text-trellis-400" />
                </div>
                <div>
                  <h2 className="text-lg font-bold text-zinc-100">Trellis</h2>
                  <p className="text-xs text-zinc-500">v0.1.4</p>
                </div>
              </div>
              <button onClick={() => setShowAbout(false)} className="text-zinc-500 hover:text-zinc-300 transition-colors">
                <X size={16} />
              </button>
            </div>
            <p className="text-sm text-zinc-400 mb-4">
              The easiest way to deploy and control ESP32 and Pico W devices on your local network.
            </p>
            <div className="space-y-2 text-sm">
              <a
                href="https://github.com/ovexro/trellis"
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-2 text-trellis-400 hover:text-trellis-300 transition-colors"
              >
                <ExternalLink size={12} />
                GitHub Repository
              </a>
              <a
                href="https://www.paypal.com/paypalme/ovexro"
                target="_blank"
                rel="noopener noreferrer"
                className="flex items-center gap-2 text-trellis-400 hover:text-trellis-300 transition-colors"
              >
                <ExternalLink size={12} />
                Support Development
              </a>
            </div>
            <p className="text-[11px] text-zinc-600 mt-4 pt-3 border-t border-zinc-800">
              MIT License &middot; Made by Ovidiu
            </p>
          </div>
        </div>
      )}
    </aside>
  );
}
