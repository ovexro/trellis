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
        <span className="px-2 py-0.5 bg-zinc-800/80 rounded-full text-[11px] text-zinc-500">
          v0.1.2
        </span>
      </div>
    </aside>
  );
}
